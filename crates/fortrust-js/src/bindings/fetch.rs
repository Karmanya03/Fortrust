use std::cell::RefCell;
use std::rc::Rc;

use std::borrow::Cow;

use boa_engine::{
    Context, JsError as BoaError, JsNativeError, JsResult, JsString, JsValue, NativeFunction,
    js_string, object::FunctionObjectBuilder, object::ObjectInitializer, property::Attribute,
    property::PropertyKey,
};
use fortrust_core::{RequestContext, ResourceType};
use fortrust_net::NetworkClient;
use tracing::{debug, warn};
use url::Url;

use crate::event_loop::EventLoop;

thread_local! {
    static FETCH_CLIENT: RefCell<Option<Rc<RefCell<NetworkClient>>>> = const { RefCell::new(None) };
}

pub fn register(
    context: &mut Context,
    origin: String,
    _event_loop: &mut EventLoop,
    network: Option<NetworkClient>,
) -> JsResult<()> {
    if let Some(client) = network {
        FETCH_CLIENT.with(|fc| {
            *fc.borrow_mut() = Some(Rc::new(RefCell::new(client)));
        });
    }

    let origin_clone = origin.clone();
    let fetch_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| do_fetch(args, ctx, &origin_clone))
    };

    let global = context.global_object();
    let fetch_val: JsValue = FunctionObjectBuilder::new(context.realm(), fetch_fn)
        .build()
        .into();
    global.set(js_string!("fetch"), fetch_val, false, context)?;

    let resp_ctor = build_response_constructor(context)?;
    global.set(js_string!("Response"), resp_ctor, false, context)?;

    let headers_ctor = build_headers_constructor(context)?;
    global.set(js_string!("Headers"), headers_ctor, false, context)?;

    debug!("Fetch Web API registered");
    Ok(())
}

fn do_fetch(args: &[JsValue], ctx: &mut Context, base_origin: &str) -> JsResult<JsValue> {
    let input = args.first().ok_or_else(|| {
        BoaError::from(JsNativeError::typ().with_message("fetch() requires 1 argument"))
    })?;

    let url_str = if let Some(s) = input.as_string() {
        s.to_std_string_escaped()
    } else {
        input.to_string(ctx)?.to_std_string_escaped()
    };

    let resolved = resolve_url(&url_str, base_origin)
        .map_err(|e| BoaError::from(JsNativeError::typ().with_message(Cow::Owned(e))))?;
    let _method = parse_method(args.get(1))
        .map_err(|e| BoaError::from(JsNativeError::typ().with_message(Cow::Owned(e))))?;
    let (_req_headers, _body) = parse_options(args.get(1), ctx)
        .map_err(|e| BoaError::from(JsNativeError::typ().with_message(Cow::Owned(e))))?;

    let response = FETCH_CLIENT.with(|fc| {
        let mut guard = fc.borrow_mut();
        let rc = guard.as_mut()?;
        let client = rc.borrow_mut();
        Some(perform_fetch(client, &resolved))
    });

    match response {
        Some(Ok(net_resp)) => {
            let resp_obj = build_response_object(ctx, &net_resp)?;
            wrap_in_promise(ctx, resp_obj)
        }
        Some(Err(err)) => {
            warn!("fetch error for {url_str}: {err:?}");
            let err_val = JsValue::from(JsString::from(format!("{err:?}")));
            wrap_rejected_promise(ctx, err_val)
        }
        None => {
            let err = JsValue::from(JsString::from("Network client not available"));
            wrap_rejected_promise(ctx, err)
        }
    }
}

fn resolve_url(input: &str, base: &str) -> Result<String, String> {
    if let Ok(parsed) = Url::parse(input) {
        Ok(parsed.to_string())
    } else if let Ok(base_url) = Url::parse(base) {
        base_url
            .join(input)
            .map(|u| u.to_string())
            .map_err(|e| e.to_string())
    } else {
        Err(format!("Cannot resolve URL: {input}"))
    }
}

fn parse_method(options: Option<&JsValue>) -> Result<String, String> {
    let Some(opts) = options else {
        return Ok("GET".into());
    };
    let obj = opts.as_object().ok_or("options must be an object")?;
    let mut ctx = Context::default();
    let method_val = obj
        .get(js_string!("method"), &mut ctx)
        .map_err(|_| "cannot read method".to_string())?;
    if method_val.is_undefined() || method_val.is_null() {
        return Ok("GET".into());
    }
    let s = method_val
        .to_string(&mut ctx)
        .map_err(|_| "method must be a string".to_string())?;
    Ok(s.to_std_string_escaped().to_uppercase())
}

type FetchOptions = (Vec<(String, String)>, Option<String>);

fn parse_options(options: Option<&JsValue>, ctx: &mut Context) -> Result<FetchOptions, String> {
    let Some(opts) = options else {
        return Ok((Vec::new(), None));
    };
    let obj = opts.as_object().ok_or("options must be an object")?;

    let mut headers = Vec::new();
    if let Ok(headers_val) = obj.get(js_string!("headers"), ctx)
        && let Some(hdr_obj) = headers_val.as_object()
    {
        let keys = hdr_obj
            .own_property_keys(ctx)
            .map_err(|_| "cannot enumerate headers".to_string())?;
        for key in keys {
            let key_str = match &key {
                PropertyKey::String(s) => s.to_std_string_escaped(),
                PropertyKey::Index(i) => i.get().to_string(),
                PropertyKey::Symbol(_) => continue,
            };
            let val = hdr_obj
                .get(key, ctx)
                .ok()
                .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
                .unwrap_or_default();
            headers.push((key_str, val));
        }
    }

    let body = if let Ok(body_val) = obj.get(js_string!("body"), ctx) {
        if body_val.is_undefined() || body_val.is_null() {
            None
        } else {
            Some(
                body_val
                    .to_string(ctx)
                    .map_err(|_| "body must be stringable".to_string())?
                    .to_std_string_escaped(),
            )
        }
    } else {
        None
    };

    Ok((headers, body))
}

fn perform_fetch(
    mut client: std::cell::RefMut<'_, NetworkClient>,
    url: &str,
) -> Result<fortrust_net::NetworkResponse, String> {
    let request = RequestContext {
        url: url.to_string(),
        top_level_url: None,
        resource_type: ResourceType::Xhr,
        referrer_policy: None,
    };

    let handle = tokio::runtime::Handle::current();
    handle.block_on(async move { client.fetch(request).await.map_err(|e| format!("{e:?}")) })
}

fn build_response_object(
    ctx: &mut Context,
    net_resp: &fortrust_net::NetworkResponse,
) -> JsResult<JsValue> {
    let status = net_resp.status;
    let ok = (200..300).contains(&status);
    let status_text = if ok { "OK" } else { "Error" };
    let url_str = net_resp.url.to_string();
    let body_bytes = net_resp.body.clone();
    let body_string = String::from_utf8_lossy(&body_bytes).to_string();
    let headers_list: Vec<(String, String)> = net_resp
        .headers
        .iter()
        .map(|(name, val)| (name.to_string(), val.to_str().unwrap_or("").to_string()))
        .collect();

    let headers_obj = build_headers_object(ctx, &headers_list)?;
    let headers_val = JsValue::from(headers_obj);

    let body_string_clone = body_string.clone();
    let text_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let s = JsString::from(body_string_clone.as_str());
            wrap_in_promise(ctx, JsValue::from(s))
        })
    };

    let body_string_for_json = body_string.clone();
    let json_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let code = format!(
                "try {{ JSON.parse({s}) }} catch(e) {{ null }}",
                s = serde_json::to_string(&body_string_for_json).unwrap_or_default()
            );
            let source = boa_engine::Source::from_bytes(code.as_bytes());
            let result = ctx.eval(source).unwrap_or(JsValue::null());
            wrap_in_promise(ctx, result)
        })
    };

    let obj = ObjectInitializer::new(ctx)
        .property(js_string!("status"), status as i32, Attribute::all())
        .property(js_string!("ok"), ok, Attribute::all())
        .property(
            js_string!("statusText"),
            js_string!(status_text),
            Attribute::all(),
        )
        .property(
            js_string!("url"),
            js_string!(url_str.as_str()),
            Attribute::all(),
        )
        .property(js_string!("headers"), headers_val, Attribute::all())
        .property(js_string!("bodyUsed"), false, Attribute::all())
        .property(js_string!("redirected"), false, Attribute::all())
        .property(js_string!("type"), js_string!("basic"), Attribute::all())
        .function(text_fn, js_string!("text"), 0)
        .function(json_fn, js_string!("json"), 0)
        .build();

    Ok(JsValue::from(obj))
}

fn build_response_constructor(ctx: &mut Context) -> JsResult<JsValue> {
    let ctor_fn =
        unsafe { NativeFunction::from_closure(|_this, _args, _ctx| Ok(JsValue::undefined())) };
    Ok(FunctionObjectBuilder::new(ctx.realm(), ctor_fn)
        .build()
        .into())
}

fn build_headers_constructor(ctx: &mut Context) -> JsResult<JsValue> {
    let ctor_fn =
        unsafe { NativeFunction::from_closure(|_this, _args, _ctx| Ok(JsValue::undefined())) };
    Ok(FunctionObjectBuilder::new(ctx.realm(), ctor_fn)
        .build()
        .into())
}

fn build_headers_object(
    ctx: &mut Context,
    entries: &[(String, String)],
) -> JsResult<boa_engine::object::JsObject> {
    let entries_clone = entries.to_vec();

    let get_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let key = args
                .first()
                .map(|v| {
                    v.to_string(&mut Context::default())
                        .map(|s| s.to_std_string_escaped().to_lowercase())
                })
                .unwrap_or(Ok(String::new()))?;
            let val = entries_clone
                .iter()
                .find(|(k, _)| k.to_lowercase() == key)
                .map(|(_, v)| JsValue::from(JsString::from(v.as_str())))
                .unwrap_or(JsValue::null());
            Ok(val)
        })
    };

    let entries_clone2 = entries.to_vec();
    let has_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let key = args
                .first()
                .map(|v| {
                    v.to_string(&mut Context::default())
                        .map(|s| s.to_std_string_escaped().to_lowercase())
                })
                .unwrap_or(Ok(String::new()))?;
            let found = entries_clone2.iter().any(|(k, _)| k.to_lowercase() == key);
            Ok(JsValue::from(found))
        })
    };

    let entries_clone3 = entries.to_vec();
    let for_each_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(callback) = args.first().and_then(|v| v.as_callable()) {
                for (key, val) in &entries_clone3 {
                    let key_val = JsValue::from(JsString::from(key.as_str()));
                    let val_val = JsValue::from(JsString::from(val.as_str()));
                    let _ = callback.call(
                        &JsValue::undefined(),
                        &[val_val, key_val, _this.clone()],
                        ctx,
                    );
                }
            }
            Ok(JsValue::undefined())
        })
    };

    let obj = ObjectInitializer::new(ctx)
        .function(get_fn, js_string!("get"), 1)
        .function(has_fn, js_string!("has"), 1)
        .function(for_each_fn, js_string!("forEach"), 1)
        .build();

    Ok(obj)
}

fn wrap_in_promise(ctx: &mut Context, value: JsValue) -> JsResult<JsValue> {
    let global = ctx.global_object();
    let promise_ctor = global
        .get(js_string!("Promise"), ctx)
        .map_err(|_| BoaError::from(JsNativeError::typ().with_message("Promise not available")))?;
    let promise_obj = promise_ctor
        .as_object()
        .ok_or_else(|| {
            BoaError::from(JsNativeError::typ().with_message("Promise is not an object"))
        })?
        .clone();
    let resolve_fn_val = promise_obj.get(js_string!("resolve"), ctx)?;
    let resolve_fn = resolve_fn_val
        .as_object()
        .ok_or_else(|| {
            BoaError::from(JsNativeError::typ().with_message("Promise.resolve is not callable"))
        })?
        .clone();
    resolve_fn.call(&promise_ctor, &[value], ctx)
}

fn wrap_rejected_promise(ctx: &mut Context, reason: JsValue) -> JsResult<JsValue> {
    let global = ctx.global_object();
    let promise_ctor = global.get(js_string!("Promise"), ctx)?;
    let promise_obj = promise_ctor
        .as_object()
        .ok_or_else(|| {
            BoaError::from(JsNativeError::typ().with_message("Promise is not an object"))
        })?
        .clone();
    let reject_fn_val = promise_obj.get(js_string!("reject"), ctx)?;
    let reject_fn = reject_fn_val
        .as_object()
        .ok_or_else(|| {
            BoaError::from(JsNativeError::typ().with_message("Promise.reject is not callable"))
        })?
        .clone();
    reject_fn.call(&promise_ctor, &[reason], ctx)
}
