//! XMLHttpRequest — legacy-compatible HTTP request API.
//!
//! Implements the full XMLHttpRequest state machine:
//! UNSENT(0) → OPENED(1) → HEADERS_RECEIVED(2) → LOADING(3) → DONE(4)
//!
//! Event callbacks: onreadystatechange, onload, onerror, onabort, ontimeout,
//!                   onloadstart, onprogress, onloadend

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::FunctionObjectBuilder, object::ObjectInitializer, property::Attribute,
};
use fortrust_core::{RequestContext, ResourceType};
use fortrust_net::NetworkClient;
use tracing::debug;
use url::Url;

use crate::event_loop::EventLoop;

static NEXT_XHR_ID: AtomicU64 = AtomicU64::new(1);

/// XHR ready states per W3C spec.
const UNSENT: u8 = 0;
const OPENED: u8 = 1;
const HEADERS_RECEIVED: u8 = 2;
const LOADING: u8 = 3;
const DONE: u8 = 4;

thread_local! {
    static XHR_CLIENT: RefCell<Option<std::rc::Rc<RefCell<NetworkClient>>>> = const { RefCell::new(None) };
}

pub fn register(
    context: &mut Context,
    origin: String,
    _event_loop: &mut EventLoop,
    network: Option<NetworkClient>,
) -> JsResult<()> {
    if let Some(client) = network {
        XHR_CLIENT.with(|c| {
            *c.borrow_mut() = Some(std::rc::Rc::new(RefCell::new(client)));
        });
    }

    let origin_clone = origin.clone();
    let xhr_ctor = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            build_xhr_instance(ctx, &origin_clone)
        })
    };

    let global = context.global_object();
    let xhr_val: JsValue = FunctionObjectBuilder::new(context.realm(), xhr_ctor)
        .build()
        .into();

    // Register ready-state constants on the constructor
    if let Some(obj) = xhr_val.as_object() {
        let _ = obj.set(js_string!("UNSENT"), JsValue::from(UNSENT as i32), false, context);
        let _ = obj.set(js_string!("OPENED"), JsValue::from(OPENED as i32), false, context);
        let _ = obj.set(js_string!("HEADERS_RECEIVED"), JsValue::from(HEADERS_RECEIVED as i32), false, context);
        let _ = obj.set(js_string!("LOADING"), JsValue::from(LOADING as i32), false, context);
        let _ = obj.set(js_string!("DONE"), JsValue::from(DONE as i32), false, context);
    }

    global.set(js_string!("XMLHttpRequest"), xhr_val, false, context)?;

    debug!("XMLHttpRequest Web API registered");
    Ok(())
}

fn build_xhr_instance(ctx: &mut Context, base_origin: &str) -> JsResult<JsValue> {
    let _id = NEXT_XHR_ID.fetch_add(1, Ordering::Relaxed);
    let origin = base_origin.to_owned();

    // State storage in thread-local per XHR instance
    let state = std::rc::Rc::new(RefCell::new(XhrStateInner::new()));

    let state_open = state.clone();
    let origin_open = origin.clone();
    let open_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let method = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok("GET".to_owned()))?
                .to_uppercase();

            let url_str = args.get(1)
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;

            let async_flag = args.get(2)
                .and_then(|v| v.as_boolean())
                .unwrap_or(true);

            if url_str.is_empty() {
                return Err(JsNativeError::typ()
                    .with_message("XMLHttpRequest.open: URL is required")
                    .into());
            }

            let resolved = resolve_url(&url_str, &origin_open)
                .map_err(|e| JsNativeError::typ().with_message(e))?;

            let mut s = state_open.borrow_mut();
            s.method = method;
            s.url = resolved;
            s.r#async = async_flag;
            s.ready_state = OPENED;
            s.response_headers.clear();
            s.response_body.clear();
            s.status = 0;
            s.status_text.clear();
            s.sent = false;
            drop(s);

            fire_ready_state_change(ctx, &state_open, _this);
            Ok(JsValue::undefined())
        })
    };

    let state_set = state.clone();
    let set_request_header_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let value = args.get(1)
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;

            let s = state_set.borrow();
            if s.ready_state != OPENED {
                return Err(JsNativeError::typ()
                    .with_message("setRequestHeader: state must be OPENED")
                    .into());
            }
            if s.sent {
                return Err(JsNativeError::typ()
                    .with_message("setRequestHeader: request already sent")
                    .into());
            }
            drop(s);

            state_set.borrow_mut().request_headers.push((name, value));
            Ok(JsValue::undefined())
        })
    };

    let state_send = state.clone();
    let _origin_send = origin.clone();
    let send_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let body = args.first()
                .filter(|v| !v.is_null() && !v.is_undefined())
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .transpose()?;

            let mut s = state_send.borrow_mut();
            if s.ready_state != OPENED {
                return Err(JsNativeError::typ()
                    .with_message("send: state must be OPENED")
                    .into());
            }
            if s.sent {
                return Err(JsNativeError::typ()
                    .with_message("send: request already sent")
                    .into());
            }
            s.sent = true;
            let method = s.method.clone();
            let url = s.url.clone();
            let headers = s.request_headers.clone();
            drop(s);

            // Fire loadstart
            fire_event(ctx, _this, js_string!("onloadstart"));

            // Perform the request synchronously (blocking)
            let result = perform_xhr_request(&method, &url, &headers, body.as_deref());

            match result {
                Ok(response) => {
                    // HEADERS_RECEIVED
                    {
                        let mut s = state_send.borrow_mut();
                        s.ready_state = HEADERS_RECEIVED;
                        s.status = response.status;
                        s.status_text = status_text_for(response.status);
                        s.response_headers = response.headers;
                    }
                    fire_ready_state_change(ctx, &state_send, _this);

                    // LOADING
                    {
                        let mut s = state_send.borrow_mut();
                        s.ready_state = LOADING;
                    }
                    fire_ready_state_change(ctx, &state_send, _this);

                    // Fire progress
                    {
                        let s = state_send.borrow();
                        let loaded = s.response_body.len();
                        let ev = ObjectInitializer::new(ctx)
                            .property(js_string!("loaded"), JsValue::from(loaded as f64), Attribute::READONLY)
                            .property(js_string!("total"), JsValue::from(loaded as f64), Attribute::READONLY)
                            .property(js_string!("lengthComputable"), false, Attribute::READONLY)
                            .build();
                        let handler = _this.as_object()
                            .and_then(|o| o.get(js_string!("onprogress"), ctx).ok())
                            .filter(|v| v.is_callable());
                        if let Some(h) = handler.and_then(|v| v.as_object().cloned()) {
                            let _ = h.call(&JsValue::undefined(), &[JsValue::from(ev)], ctx);
                        }
                    }

                    // DONE
                    {
                        let mut s = state_send.borrow_mut();
                        s.ready_state = DONE;
                        s.response_body = response.body;
                    }
                    fire_ready_state_change(ctx, &state_send, _this);

                    // Fire load
                    fire_event(ctx, _this, js_string!("onload"));
                    // Fire loadend
                    fire_event(ctx, _this, js_string!("onloadend"));
                }
                Err(err_msg) => {
                    {
                        let mut s = state_send.borrow_mut();
                        s.ready_state = DONE;
                        s.error_message = err_msg;
                    }
                    fire_ready_state_change(ctx, &state_send, _this);
                    fire_event(ctx, _this, js_string!("onerror"));
                    fire_event(ctx, _this, js_string!("onloadend"));
                }
            }

            Ok(JsValue::undefined())
        })
    };

    let state_abort = state.clone();
    let abort_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut s = state_abort.borrow_mut();
            s.ready_state = DONE;
            s.sent = false;
            s.status = 0;
            s.status_text.clear();
            s.response_body.clear();
            drop(s);
            fire_event(_ctx, _this, js_string!("onabort"));
            fire_event(_ctx, _this, js_string!("onloadend"));
            Ok(JsValue::undefined())
        })
    };

    let state_get_all = state.clone();
    let get_all_response_headers_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let s = state_get_all.borrow();
            let mut result = String::new();
            for (name, value) in &s.response_headers {
                result.push_str(name);
                result.push_str(": ");
                result.push_str(value);
                result.push_str("\r\n");
            }
            Ok(JsValue::from(JsString::from(result.as_str())))
        })
    };

    let state_get_hdr = state.clone();
    let get_response_header_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped().to_lowercase()))
                .unwrap_or(Ok(String::new()))?;
            let s = state_get_hdr.borrow();
            let val = s.response_headers.iter()
                .find(|(k, _)| k.to_lowercase() == name)
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            Ok(JsValue::from(JsString::from(val)))
        })
    };

    let state_resp_text = state.clone();
    let get_response_text_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let s = state_resp_text.borrow();
            if s.ready_state == LOADING || s.ready_state == DONE {
                let text = String::from_utf8_lossy(&s.response_body).to_string();
                Ok(JsValue::from(JsString::from(text.as_str())))
            } else {
                Ok(JsValue::from(JsString::from("")))
            }
        })
    };

    let state_ready = state.clone();
    let get_ready_state_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let s = state_ready.borrow();
            Ok(JsValue::from(s.ready_state as i32))
        })
    };

    let state_status = state.clone();
    let get_status_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let s = state_status.borrow();
            Ok(JsValue::from(s.status as i32))
        })
    };

    let state_status_text = state.clone();
    let get_status_text_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let s = state_status_text.borrow();
            Ok(JsValue::from(JsString::from(s.status_text.as_str())))
        })
    };

    let state_resp_url = state.clone();
    let get_response_url_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let s = state_resp_url.borrow();
            Ok(JsValue::from(JsString::from(s.url.as_str())))
        })
    };

    let obj = ObjectInitializer::new(ctx)
        .property(js_string!("UNSENT"), JsValue::from(UNSENT as i32), Attribute::READONLY)
        .property(js_string!("OPENED"), JsValue::from(OPENED as i32), Attribute::READONLY)
        .property(js_string!("HEADERS_RECEIVED"), JsValue::from(HEADERS_RECEIVED as i32), Attribute::READONLY)
        .property(js_string!("LOADING"), JsValue::from(LOADING as i32), Attribute::READONLY)
        .property(js_string!("DONE"), JsValue::from(DONE as i32), Attribute::READONLY)
        .property(js_string!("readyState"), JsValue::from(UNSENT as i32), Attribute::READONLY)
        .property(js_string!("status"), JsValue::from(0), Attribute::READONLY)
        .property(js_string!("statusText"), js_string!(""), Attribute::READONLY)
        .property(js_string!("responseText"), js_string!(""), Attribute::READONLY)
        .property(js_string!("responseType"), js_string!(""), Attribute::all())
        .property(js_string!("responseURL"), js_string!(""), Attribute::READONLY)
        .property(js_string!("timeout"), JsValue::from(0), Attribute::all())
        .property(js_string!("withCredentials"), false, Attribute::all())
        .property(js_string!("onreadystatechange"), JsValue::null(), Attribute::WRITABLE)
        .property(js_string!("onload"), JsValue::null(), Attribute::WRITABLE)
        .property(js_string!("onerror"), JsValue::null(), Attribute::WRITABLE)
        .property(js_string!("onabort"), JsValue::null(), Attribute::WRITABLE)
        .property(js_string!("ontimeout"), JsValue::null(), Attribute::WRITABLE)
        .property(js_string!("onloadstart"), JsValue::null(), Attribute::WRITABLE)
        .property(js_string!("onprogress"), JsValue::null(), Attribute::WRITABLE)
        .property(js_string!("onloadend"), JsValue::null(), Attribute::WRITABLE)
        .function(open_fn, js_string!("open"), 2)
        .function(send_fn, js_string!("send"), 0)
        .function(abort_fn, js_string!("abort"), 0)
        .function(set_request_header_fn, js_string!("setRequestHeader"), 2)
        .function(get_all_response_headers_fn, js_string!("getAllResponseHeaders"), 0)
        .function(get_response_header_fn, js_string!("getResponseHeader"), 1)
        .function(get_response_text_fn, js_string!("getResponseText"), 0)
        .function(get_ready_state_fn, js_string!("getReadyState"), 0)
        .function(get_status_fn, js_string!("getStatus"), 0)
        .function(get_status_text_fn, js_string!("getStatusText"), 0)
        .function(get_response_url_fn, js_string!("getResponseURL"), 0)
        .build();

    Ok(JsValue::from(obj))
}

/// Internal state for a single XHR instance.
#[derive(Debug, Clone)]
struct XhrStateInner {
    ready_state: u8,
    method: String,
    url: String,
    r#async: bool,
    sent: bool,
    request_headers: Vec<(String, String)>,
    response_headers: Vec<(String, String)>,
    response_body: Vec<u8>,
    status: u16,
    status_text: String,
    error_message: String,
}

type XhrState = std::rc::Rc<RefCell<XhrStateInner>>;

impl XhrStateInner {
    fn new() -> Self {
        Self {
            ready_state: UNSENT,
            method: String::new(),
            url: String::new(),
            r#async: true,
            sent: false,
            request_headers: Vec::new(),
            response_headers: Vec::new(),
            response_body: Vec::new(),
            status: 0,
            status_text: String::new(),
            error_message: String::new(),
        }
    }
}

fn resolve_url(input: &str, base: &str) -> Result<String, String> {
    if let Ok(parsed) = Url::parse(input) {
        Ok(parsed.to_string())
    } else if let Ok(base_url) = Url::parse(base) {
        base_url.join(input).map(|u| u.to_string()).map_err(|e| e.to_string())
    } else {
        Err(format!("Cannot resolve URL: {input}"))
    }
}

fn fire_ready_state_change(ctx: &mut Context, state: &XhrState, this: &JsValue) {
    let s = state.borrow();
    let rs = s.ready_state;
    drop(s);

    // Update the readyState property on the JS object
    if let Some(obj) = this.as_object() {
        let _ = obj.set(js_string!("readyState"), JsValue::from(rs as i32), false, ctx);
        // Also update status and responseText for convenience
        let s = state.borrow();
        let _ = obj.set(js_string!("status"), JsValue::from(s.status as i32), false, ctx);
        let _ = obj.set(js_string!("statusText"), JsValue::from(JsString::from(s.status_text.as_str())), false, ctx);
        let text = String::from_utf8_lossy(&s.response_body).to_string();
        let _ = obj.set(js_string!("responseText"), JsValue::from(JsString::from(text.as_str())), false, ctx);
        let _ = obj.set(js_string!("responseURL"), JsValue::from(JsString::from(s.url.as_str())), false, ctx);
    }

    fire_event(ctx, this, js_string!("onreadystatechange"));
}

fn fire_event(ctx: &mut Context, this: &JsValue, handler_name: boa_engine::JsString) {
    let handler = this.as_object()
        .and_then(|o| o.get(handler_name.clone(), ctx).ok())
        .filter(|v| v.is_callable());

    if let Some(h) = handler.and_then(|v| v.as_object().cloned()) {
        let _ = h.call(&JsValue::undefined(), &[], ctx);
    }
}

struct XhrResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn perform_xhr_request(
    _method: &str,
    url: &str,
    _headers: &[(String, String)],
    _body: Option<&str>,
) -> Result<XhrResponse, String> {
    let client_rc = XHR_CLIENT.with(|c| c.borrow().clone());
    let Some(client_rc) = client_rc else {
        return Err("Network client not available".into());
    };

    let mut client = client_rc.borrow_mut();

    let request = RequestContext {
        url: url.to_string(),
        top_level_url: None,
        resource_type: ResourceType::Xhr,
        referrer_policy: None,
    };

    let handle = tokio::runtime::Handle::current();
    let response = handle.block_on(async {
        client.fetch(request).await.map_err(|e| format!("{e:?}"))
    })?;

    let resp_headers: Vec<(String, String)> = response.headers.iter()
        .map(|(name, val)| (name.to_string(), val.to_str().unwrap_or("").to_string()))
        .collect();

    Ok(XhrResponse {
        status: response.status,
        headers: resp_headers,
        body: response.body.to_vec(),
    })
}

fn status_text_for(status: u16) -> String {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text_known_codes() {
        assert_eq!(status_text_for(200), "OK");
        assert_eq!(status_text_for(404), "Not Found");
        assert_eq!(status_text_for(500), "Internal Server Error");
        assert_eq!(status_text_for(999), "");
    }

    #[test]
    fn xhr_state_initial() {
        let state = XhrStateInner::new();
        assert_eq!(state.ready_state, UNSENT);
        assert_eq!(state.method, "");
        assert!(state.response_body.is_empty());
    }

    #[test]
    fn resolve_url_absolute() {
        let result = resolve_url("https://example.com/api", "https://origin.com");
        assert_eq!(result.unwrap(), "https://example.com/api");
    }

    #[test]
    fn resolve_url_relative() {
        let result = resolve_url("/api/data", "https://example.com/page");
        assert_eq!(result.unwrap(), "https://example.com/api/data");
    }
}
