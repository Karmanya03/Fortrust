use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::FunctionObjectBuilder, object::ObjectInitializer, property::Attribute,
};
use fortrust_net::websocket::{WebSocketClient, WebSocketEvent, WebSocketMessage};
use tracing::debug;

const CONNECTING: u8 = 0;
const OPEN: u8 = 1;
const CLOSING: u8 = 2;
const CLOSED: u8 = 3;

static NEXT_WS_ID: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    static ref WS_CLIENTS: Mutex<HashMap<u64, WebSocketClient>> =
        Mutex::new(HashMap::new());
}

thread_local! {
    static WS_OBJECTS: RefCell<HashMap<u64, JsValue>> = RefCell::new(HashMap::new());
    static WS_EVENT_LISTENERS: RefCell<HashMap<u64, HashMap<String, Vec<JsValue>>>> =
        RefCell::new(HashMap::new());
}

pub fn poll_websocket_events(ctx: &mut Context) {
    let mut event_batches: Vec<(u64, WebSocketEvent)> = Vec::new();
    let mut disconnected = Vec::new();

    {
        let clients = WS_CLIENTS.lock().unwrap();
        for (&id, client) in clients.iter() {
            while let Some(event) = futures_executor::block_on(client.try_recv()) {
                let is_close = matches!(&event, WebSocketEvent::Close(_, _));
                event_batches.push((id, event));
                if is_close {
                    disconnected.push(id);
                }
            }
        }
    }

    if event_batches.is_empty() {
        return;
    }

    WS_OBJECTS.with(|objects_cell| {
        let objects = objects_cell.borrow();
        WS_EVENT_LISTENERS.with(|listeners_cell| {
            let listeners = listeners_cell.borrow();

            for (id, event) in &event_batches {
                let obj_val = match objects.get(id) {
                    Some(v) => v.clone(),
                    None => continue,
                };
                let obj = match obj_val.as_object() {
                    Some(o) => o.clone(),
                    None => continue,
                };

                let (handler_name, event_type) = match &event {
                    WebSocketEvent::Open => (js_string!("onopen"), "open"),
                    WebSocketEvent::Message(_) => (js_string!("onmessage"), "message"),
                    WebSocketEvent::Close(_, _) => (js_string!("onclose"), "close"),
                    WebSocketEvent::Error(_) => (js_string!("onerror"), "error"),
                };

                let event_arg = build_event_object(ctx, event);

                // 1. Update readyState on the JS object
                match &event {
                    WebSocketEvent::Open => {
                        let _ = obj.set(js_string!("readyState"), JsValue::from(f64::from(OPEN)), false, ctx);
                    }
                    WebSocketEvent::Close(_, _) => {
                        let _ = obj.set(js_string!("readyState"), JsValue::from(f64::from(CLOSED)), false, ctx);
                    }
                    _ => {}
                }

                // 2. Call on* handler
                if let Ok(handler) = obj.get(handler_name, ctx)
                    && !handler.is_undefined()
                    && let Some(h) = handler.as_object()
                {
                    let _ = h.call(&JsValue::undefined(), &event_arg, ctx);
                }

                // 3. Call addEventListener listeners
                if let Some(event_map) = listeners.get(id)
                    && let Some(callbacks) = event_map.get(event_type)
                {
                    for cb in callbacks {
                        if let Some(h) = cb.as_object() {
                            let _ = h.call(&JsValue::undefined(), &event_arg, ctx);
                        }
                    }
                }
            }
        });
    });

    if !disconnected.is_empty() {
        let mut clients = WS_CLIENTS.lock().unwrap();
        for id in &disconnected {
            clients.remove(id);
        }
        WS_OBJECTS.with(|o| {
            let mut o = o.borrow_mut();
            for id in &disconnected { o.remove(id); }
        });
        WS_EVENT_LISTENERS.with(|l| {
            let mut l = l.borrow_mut();
            for id in &disconnected { l.remove(id); }
        });
    }
}

fn build_event_object(ctx: &mut Context, event: &WebSocketEvent) -> Vec<JsValue> {
    match event {
        WebSocketEvent::Open => vec![JsValue::undefined()],
        WebSocketEvent::Message(msg) => {
            let data = match msg {
                WebSocketMessage::Text(t) => JsValue::from(JsString::from(t.as_str())),
                WebSocketMessage::Binary(b) => JsValue::from(JsString::from(
                    String::from_utf8_lossy(b).as_ref(),
                )),
                _ => JsValue::undefined(),
            };
            let ev = ObjectInitializer::new(ctx)
                .property(js_string!("data"), data, Attribute::READONLY)
                .build();
            vec![JsValue::Object(ev)]
        }
        WebSocketEvent::Close(code, reason) => {
            let ev = ObjectInitializer::new(ctx)
                .property(js_string!("code"), JsValue::from(f64::from(code.unwrap_or(1005))), Attribute::READONLY)
                .property(js_string!("reason"), JsString::from(reason.as_str()), Attribute::READONLY)
                .property(js_string!("wasClean"), JsValue::from(true), Attribute::READONLY)
                .build();
            vec![JsValue::Object(ev)]
        }
        WebSocketEvent::Error(msg) => {
            let ev = ObjectInitializer::new(ctx)
                .property(js_string!("message"), JsString::from(msg.as_str()), Attribute::READONLY)
                .build();
            vec![JsValue::Object(ev)]
        }
    }
}

pub fn register(context: &mut Context) -> JsResult<()> {
    let ws_ctor = unsafe {
        NativeFunction::from_closure(|_this, args, ctx| {
            let url_str = args
                .first()
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            if url_str.is_empty() {
                return Err(JsNativeError::typ()
                    .with_message("Failed to construct 'WebSocket': 1 argument required")
                    .into());
            }

            let protocols = args.get(1)
                .and_then(|v| v.as_string())
                .map(|s| s.to_std_string_escaped())
                .unwrap_or_default();

            let client = WebSocketClient::new(&url_str).map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("Failed to construct 'WebSocket': {e:?}"))
            })?;

            let id = NEXT_WS_ID.fetch_add(1, Ordering::Relaxed);

            WS_CLIENTS.lock().unwrap().insert(id, client);

            // Create send function
            let id_send = id;
            let send_fn = NativeFunction::from_closure(move |_t, a, c| {
                let msg = a.first().map(|val| {
                    if let Some(s) = val.as_string() {
                        WebSocketMessage::Text(s.to_std_string_escaped())
                    } else {
                        WebSocketMessage::Text(
                            val.to_string(c).map(|s| s.to_std_string_escaped()).unwrap_or_default()
                        )
                    }
                }).unwrap_or(WebSocketMessage::Text(String::new()));
                let clients = WS_CLIENTS.lock().unwrap();
                if let Some(c) = clients.get(&id_send) {
                    let _ = futures_executor::block_on(c.send(msg));
                }
                Ok(JsValue::undefined())
            });

            // Create close function
            let id_close = id;
            let close_fn = NativeFunction::from_closure(move |_t, a, ctx| {
                let code = a.first().and_then(|v| v.as_number()).map(|n| n as u16);
                let reason = a.get(1).and_then(|v| v.as_string()).map(|s| s.to_std_string_escaped());
                // Update readyState to CLOSING on the JS object
                WS_OBJECTS.with(|o| {
                    if let Some(obj_val) = o.borrow().get(&id_close)
                        && let Some(obj) = obj_val.as_object()
                    {
                        let _ = obj.set(js_string!("readyState"), JsValue::from(f64::from(CLOSING)), false, ctx);
                    }
                });
                let clients = WS_CLIENTS.lock().unwrap();
                if let Some(c) = clients.get(&id_close) {
                    let _ = futures_executor::block_on(c.close(code, reason));
                }
                Ok(JsValue::undefined())
            });

            // Create addEventListener
            let id_ael = id;
            let add_event_listener_fn = NativeFunction::from_closure(move |_t, a, _c| {
                let event_type = a.first()
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_default();
                let callback = a.get(1).cloned().unwrap_or(JsValue::undefined());
                if !event_type.is_empty() && callback.is_callable() {
                    WS_EVENT_LISTENERS.with(|l| {
                        let mut l = l.borrow_mut();
                        l.entry(id_ael).or_default()
                            .entry(event_type).or_default()
                            .push(callback);
                    });
                }
                Ok(JsValue::undefined())
            });

            // Create removeEventListener
            let id_rel = id;
            let remove_event_listener_fn = NativeFunction::from_closure(move |_t, a, _c| {
                let event_type = a.first()
                    .and_then(|v| v.as_string())
                    .map(|s| s.to_std_string_escaped())
                    .unwrap_or_default();
                let callback = a.get(1).cloned().unwrap_or(JsValue::undefined());
                if !event_type.is_empty() {
                    WS_EVENT_LISTENERS.with(|l| {
                        let mut l = l.borrow_mut();
                        if let Some(event_map) = l.get_mut(&id_rel)
                            && let Some(callbacks) = event_map.get_mut(&event_type)
                        {
                            callbacks.retain(|cb| !JsValue::strict_equals(cb, &callback));
                        }
                    });
                }
                Ok(JsValue::undefined())
            });

            let send_val: JsValue = FunctionObjectBuilder::new(ctx.realm(), send_fn).build().into();
            let close_val: JsValue = FunctionObjectBuilder::new(ctx.realm(), close_fn).build().into();
            let ael_val: JsValue = FunctionObjectBuilder::new(ctx.realm(), add_event_listener_fn).build().into();
            let rel_val: JsValue = FunctionObjectBuilder::new(ctx.realm(), remove_event_listener_fn).build().into();

            let instance = ObjectInitializer::new(ctx)
                .property(js_string!("url"), JsString::from(url_str.as_str()), Attribute::READONLY)
                .property(js_string!("readyState"), JsValue::from(f64::from(CONNECTING)), Attribute::WRITABLE)
                .property(js_string!("protocol"), JsString::from(protocols.as_str()), Attribute::READONLY)
                .property(js_string!("binaryType"), JsString::from("blob"), Attribute::WRITABLE)
                .property(js_string!("bufferedAmount"), JsValue::from(0), Attribute::READONLY)
                .property(js_string!("CONNECTING"), JsValue::from(0), Attribute::READONLY)
                .property(js_string!("OPEN"), JsValue::from(1), Attribute::READONLY)
                .property(js_string!("CLOSING"), JsValue::from(2), Attribute::READONLY)
                .property(js_string!("CLOSED"), JsValue::from(3), Attribute::READONLY)
                .property(js_string!("send"), send_val, Attribute::WRITABLE)
                .property(js_string!("close"), close_val, Attribute::WRITABLE)
                .property(js_string!("addEventListener"), ael_val, Attribute::WRITABLE)
                .property(js_string!("removeEventListener"), rel_val, Attribute::WRITABLE)
                .property(js_string!("onopen"), JsValue::undefined(), Attribute::WRITABLE)
                .property(js_string!("onmessage"), JsValue::undefined(), Attribute::WRITABLE)
                .property(js_string!("onclose"), JsValue::undefined(), Attribute::WRITABLE)
                .property(js_string!("onerror"), JsValue::undefined(), Attribute::WRITABLE)
                .build();

            // Store the JS object reference for event dispatching
            WS_OBJECTS.with(|o| {
                o.borrow_mut().insert(id, JsValue::Object(instance.clone()));
            });
            WS_EVENT_LISTENERS.with(|l| {
                l.borrow_mut().insert(id, HashMap::new());
            });

            Ok(JsValue::Object(instance))
        })
    };

    let global = context.global_object();
    let ws_fn = FunctionObjectBuilder::new(context.realm(), ws_ctor)
        .constructor(true)
        .build();
    // Set static constants on the constructor function
    ws_fn.set(js_string!("CONNECTING"), JsValue::from(0), false, context)?;
    ws_fn.set(js_string!("OPEN"), JsValue::from(1), false, context)?;
    ws_fn.set(js_string!("CLOSING"), JsValue::from(2), false, context)?;
    ws_fn.set(js_string!("CLOSED"), JsValue::from(3), false, context)?;
    let ws_val: JsValue = ws_fn.into();
    global.set(js_string!("WebSocket"), ws_val, false, context)?;

    debug!("WebSocket Web API registered");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a tokio runtime and run a closure inside it.
    fn with_runtime<F: FnOnce(&mut Context)>(f: F) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let mut ctx = Context::default();
        register(&mut ctx).unwrap();
        f(&mut ctx);
    }

    #[test]
    fn test_register_websocket_exists() {
        let mut ctx = Context::default();
        register(&mut ctx).unwrap();
        let global = ctx.global_object();
        let ws = global.get(js_string!("WebSocket"), &mut ctx).unwrap();
        assert!(!ws.is_undefined(), "WebSocket constructor should exist");
    }

    #[test]
    fn test_websocket_constants() {
        let mut ctx = Context::default();
        register(&mut ctx).unwrap();
        let result = ctx.eval(boa_engine::Source::from_bytes(
            b"JSON.stringify({ \
              connecting: WebSocket.CONNECTING, \
              open: WebSocket.OPEN, \
              closing: WebSocket.CLOSING, \
              closed: WebSocket.CLOSED \
            })"
        ));
        let json = result.unwrap().to_string(&mut ctx).unwrap().to_std_string_escaped();
        assert!(json.contains("\"connecting\":0"), "CONNECTING should be 0");
        assert!(json.contains("\"open\":1"), "OPEN should be 1");
        assert!(json.contains("\"closing\":2"), "CLOSING should be 2");
        assert!(json.contains("\"closed\":3"), "CLOSED should be 3");
    }

    #[test]
    fn test_websocket_constructor_creates_object_with_properties() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"var ws = new WebSocket('wss://echo.example/ws');\
                  JSON.stringify({ \
                    hasUrl: typeof ws.url === 'string', \
                    hasReadyState: typeof ws.readyState === 'number', \
                    hasProtocol: typeof ws.protocol === 'string', \
                    hasSend: typeof ws.send === 'function', \
                    hasClose: typeof ws.close === 'function', \
                    hasAEL: typeof ws.addEventListener === 'function', \
                    hasREL: typeof ws.removeEventListener === 'function', \
                    readyState: ws.readyState, \
                    binaryType: ws.binaryType \
                  })"
            ));
            let json = result.unwrap().to_string(ctx).unwrap().to_std_string_escaped();
            assert!(json.contains("\"hasUrl\":true"), "url property");
            assert!(json.contains("\"hasReadyState\":true"), "readyState property");
            assert!(json.contains("\"hasSend\":true"), "send function");
            assert!(json.contains("\"hasClose\":true"), "close function");
            assert!(json.contains("\"hasAEL\":true"), "addEventListener function");
            assert!(json.contains("\"hasREL\":true"), "removeEventListener function");
            assert!(json.contains("\"readyState\":0"), "initial readyState should be CONNECTING(0)");
            assert!(json.contains("\"binaryType\":\"blob\""), "default binaryType should be 'blob'");
        });
    }

    #[test]
    fn test_websocket_protocol_from_constructor() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"new WebSocket('wss://echo.example/ws', 'chat').protocol"
            ));
            assert_eq!(result.unwrap().to_string(ctx).unwrap().to_std_string_escaped(), "chat",
                "protocol should match constructor arg");
        });
    }

    #[test]
    fn test_websocket_binary_type_settable() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"var ws = new WebSocket('wss://echo.example/ws'); ws.binaryType = 'arraybuffer'; ws.binaryType"
            ));
            assert_eq!(result.unwrap().to_string(ctx).unwrap().to_std_string_escaped(), "arraybuffer");
        });
    }

    #[test]
    fn test_add_event_listener_stores_callback() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"var ws = new WebSocket('wss://echo.example/ws');\
                  ws.addEventListener('open', function() {});\
                  typeof ws.addEventListener === 'function'"
            ));
            assert!(result.unwrap().as_boolean().unwrap(),
                "addEventListener should be a function");
        });
    }

    #[test]
    fn test_remove_event_listener_removes_callback() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"var ws = new WebSocket('wss://echo.example/ws');\
                  var fn = function() {};\
                  ws.addEventListener('open', fn);\
                  ws.removeEventListener('open', fn);\
                  typeof ws.removeEventListener === 'function'"
            ));
            assert!(result.unwrap().as_boolean().unwrap(),
                "removeEventListener should be a function");
        });
    }

    #[test]
    fn test_onopen_handler_settable() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"var ws = new WebSocket('wss://echo.example/ws');\
                  ws.onopen = function() { return 'opened'; };\
                  typeof ws.onopen === 'function'"
            ));
            assert!(result.unwrap().as_boolean().unwrap(),
                "onopen should be settable and callable");
        });
    }

    #[test]
    fn test_poll_websocket_events_no_crash() {
        let mut ctx = Context::default();
        register(&mut ctx).unwrap();
        poll_websocket_events(&mut ctx);
    }

    #[test]
    fn test_multiple_websocket_instances() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"var ws1 = new WebSocket('wss://echo.example/ws1');\
                  var ws2 = new WebSocket('wss://echo.example/ws2');\
                  ws1 !== ws2 && ws1.readyState === 0 && ws2.readyState === 0"
            ));
            assert!(result.unwrap().as_boolean().unwrap(),
                "Multiple WebSocket instances should work independently");
        });
    }

    #[test]
    fn test_close_updates_closing_state() {
        with_runtime(|ctx| {
            let result = ctx.eval(boa_engine::Source::from_bytes(
                b"var ws = new WebSocket('wss://echo.example/ws');\
                  var before = ws.readyState;\
                  ws.close();\
                  var after = ws.readyState;\
                  before === 0 && after === 2"
            ));
            assert!(result.unwrap().as_boolean().unwrap(),
                "readyState should be CONNECTING(0) before close and CLOSING(2) after");
        });
    }
}
