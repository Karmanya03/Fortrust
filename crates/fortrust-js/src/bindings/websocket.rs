use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use boa_engine::{
    Context, JsNativeError, JsResult, JsString, JsValue, NativeFunction, js_string,
    object::FunctionObjectBuilder, object::ObjectInitializer, property::Attribute,
};
use fortrust_net::websocket::{WebSocketClient, WebSocketEvent, WebSocketMessage};
use tracing::debug;

static NEXT_WS_ID: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    static ref WS_CLIENTS: std::sync::Mutex<HashMap<u64, WebSocketClient>> =
        std::sync::Mutex::new(HashMap::new());
}

thread_local! {
    static WS_OBJECTS: RefCell<HashMap<u64, JsValue>> = RefCell::new(HashMap::new());
}

pub fn poll_websocket_events(ctx: &mut Context) {
    let mut events_to_fire: Vec<WebSocketEvent> = Vec::new();
    let mut ids_with_events: Vec<u64> = Vec::new();
    let mut disconnected = Vec::new();

    {
        let clients = WS_CLIENTS.lock().unwrap();
        for (&id, client) in clients.iter() {
            while let Some(event) = futures_executor::block_on(client.try_recv()) {
                let is_close = matches!(event, WebSocketEvent::Close(_, _));
                events_to_fire.push(event);
                ids_with_events.push(id);
                if is_close {
                    disconnected.push(id);
                }
            }
        }
    }

    if events_to_fire.is_empty() {
        return;
    }

    WS_OBJECTS.with(|objects| {
        let objects = objects.borrow();

        for (i, event) in events_to_fire.into_iter().enumerate() {
            let id = ids_with_events[i];
            let obj_val = match objects.get(&id) {
                Some(v) => v.clone(),
                None => continue,
            };
            let obj = match obj_val.as_object() {
                Some(o) => o.clone(),
                None => continue,
            };

            let handler_name = match &event {
                WebSocketEvent::Open => js_string!("onopen"),
                WebSocketEvent::Message(_) => js_string!("onmessage"),
                WebSocketEvent::Close(_, _) => js_string!("onclose"),
                WebSocketEvent::Error(_) => js_string!("onerror"),
            };

            let handler = match obj.get(handler_name, ctx) {
                Ok(v) if !v.is_undefined() && v.is_callable() => v,
                _ => continue,
            };

            let event_arg = match &event {
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
                        .property(
                            js_string!("code"),
                            JsValue::from(f64::from(code.unwrap_or(1005))),
                            Attribute::READONLY,
                        )
                        .property(
                            js_string!("reason"),
                            JsString::from(reason.as_str()),
                            Attribute::READONLY,
                        )
                        .property(
                            js_string!("wasClean"),
                            JsValue::from(true),
                            Attribute::READONLY,
                        )
                        .build();
                    vec![JsValue::Object(ev)]
                }
                WebSocketEvent::Error(msg) => {
                    let ev = ObjectInitializer::new(ctx)
                        .property(
                            js_string!("message"),
                            JsString::from(msg.as_str()),
                            Attribute::READONLY,
                        )
                        .build();
                    vec![JsValue::Object(ev)]
                }
            };

            if let Some(h) = handler.as_object() {
                let _ = h.call(&JsValue::undefined(), &event_arg, ctx);
            }
        }
    });

    if !disconnected.is_empty() {
        let mut clients = WS_CLIENTS.lock().unwrap();
        for id in &disconnected {
            clients.remove(id);
        }
        WS_OBJECTS.with(|o| {
            let mut o = o.borrow_mut();
            for id in &disconnected {
                o.remove(id);
            }
        });
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

            let client = WebSocketClient::new(&url_str).map_err(|e| {
                JsNativeError::typ()
                    .with_message(format!("Failed to construct 'WebSocket': {e:?}"))
            })?;

            let id = NEXT_WS_ID.fetch_add(1, Ordering::Relaxed);

            WS_CLIENTS.lock().unwrap().insert(id, client);

            let id_send = id;
            let send_fn = {
                NativeFunction::from_closure(move |_t, a, c| {
                    let data = a
                        .first()
                        .map(|v| {
                            if let Some(s) = v.as_string() {
                                WebSocketMessage::Text(s.to_std_string_escaped())
                            } else {
                                WebSocketMessage::Text(
                                    v.to_string(c)
                                        .map(|s| s.to_std_string_escaped())
                                        .unwrap_or_default(),
                                )
                            }
                        })
                        .unwrap_or(WebSocketMessage::Text(String::new()));
                    let clients = WS_CLIENTS.lock().unwrap();
                    if let Some(c) = clients.get(&id_send) {
                        let _ = futures_executor::block_on(c.send(data));
                    }
                    Ok(JsValue::undefined())
                })
            };

            let id_close = id;
            let close_fn = {
                NativeFunction::from_closure(move |_t, a, _c| {
                    let code = a.first().and_then(|v| v.as_number()).map(|n| n as u16);
                    let reason = a
                        .get(1)
                        .and_then(|v| v.as_string())
                        .map(|s| s.to_std_string_escaped());
                    let clients = WS_CLIENTS.lock().unwrap();
                    if let Some(c) = clients.get(&id_close) {
                        let _ = futures_executor::block_on(c.close(code, reason));
                    }
                    Ok(JsValue::undefined())
                })
            };

            let send_val: JsValue = FunctionObjectBuilder::new(ctx.realm(), send_fn)
                .build()
                .into();
            let close_val: JsValue = FunctionObjectBuilder::new(ctx.realm(), close_fn)
                .build()
                .into();

            let instance = ObjectInitializer::new(ctx)
                .property(
                    js_string!("url"),
                    JsString::from(url_str.as_str()),
                    Attribute::READONLY,
                )
                .property(
                    js_string!("readyState"),
                    JsValue::from(0),
                    Attribute::READONLY,
                )
                .property(
                    js_string!("bufferedAmount"),
                    JsValue::from(0),
                    Attribute::READONLY,
                )
                .property(
                    js_string!("CONNECTING"),
                    JsValue::from(0),
                    Attribute::READONLY,
                )
                .property(js_string!("OPEN"), JsValue::from(1), Attribute::READONLY)
                .property(
                    js_string!("CLOSING"),
                    JsValue::from(2),
                    Attribute::READONLY,
                )
                .property(js_string!("CLOSED"), JsValue::from(3), Attribute::READONLY)
                .property(js_string!("send"), send_val, Attribute::WRITABLE)
                .property(js_string!("close"), close_val, Attribute::WRITABLE)
                .property(
                    js_string!("onopen"),
                    JsValue::undefined(),
                    Attribute::WRITABLE,
                )
                .property(
                    js_string!("onmessage"),
                    JsValue::undefined(),
                    Attribute::WRITABLE,
                )
                .property(
                    js_string!("onclose"),
                    JsValue::undefined(),
                    Attribute::WRITABLE,
                )
                .property(
                    js_string!("onerror"),
                    JsValue::undefined(),
                    Attribute::WRITABLE,
                )
                .build();

            WS_OBJECTS.with(|o| {
                o.borrow_mut().insert(id, JsValue::Object(instance.clone()));
            });

            Ok(JsValue::Object(instance))
        })
    };

    let global = context.global_object();
    let ws_val: JsValue = FunctionObjectBuilder::new(context.realm(), ws_ctor)
        .build()
        .into();
    global.set(js_string!("WebSocket"), ws_val, false, context)?;

    debug!("WebSocket Web API registered");
    Ok(())
}
