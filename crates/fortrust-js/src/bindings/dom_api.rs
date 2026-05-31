use boa_engine::{
    Context, JsError as BoaError, JsNativeError, JsResult, JsString, JsValue, NativeFunction,
    js_string, object::FunctionObjectBuilder, object::ObjectInitializer, property::Attribute,
};
use fortrust_dom::Document;
use tracing::debug;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::hooks;

lazy_static::lazy_static! {
    // Map document pointer address -> title string
    static ref DOCUMENT_TITLES: Mutex<HashMap<usize, String>> = Mutex::new(HashMap::new());
    // Per-node attribute overrides created by JS `setAttribute` calls.
    static ref DOCUMENT_ATTR_OVERRIDES: Mutex<HashMap<usize, HashMap<String, String>>> = Mutex::new(HashMap::new());
    // Map of node_ptr -> list of event names registered by JS via addEventListener
    static ref DOCUMENT_EVENT_LISTENERS: Mutex<HashMap<usize, Vec<String>>> = Mutex::new(HashMap::new());
}

pub fn register(context: &mut Context, document: &fortrust_dom::Document<'static>) -> JsResult<()> {
    let doc_obj = build_document_object(context, document)?;
    context.register_global_property(js_string!("document"), doc_obj, Attribute::all())?;

    let win_obj = build_window_object(context)?;
    context.register_global_property(js_string!("window"), win_obj, Attribute::all())?;

    debug!("DOM API bindings registered");
    Ok(())
}

fn build_document_object(
    context: &mut Context,
    document: &fortrust_dom::Document<'static>,
) -> JsResult<JsValue> {
    let title_val = JsString::from(document.text_content().chars().take(80).collect::<String>());
    let _text_val = JsString::from(document.text_content());

    let doc_for_get: &'static Document<'static> =
        unsafe { &*(document as *const Document<'static>) };
    let descendants = doc_for_get.descendants();
    let get_element_by_id_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let id = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;

            if id.is_empty() {
                return Ok(JsValue::null());
            }

            let found = descendants.iter().find(|node| {
                node.as_element()
                    .and_then(|el| el.attr("id"))
                    .is_some_and(|attr| attr.eq_ignore_ascii_case(&id))
            });

            match found {
                Some(node) => wrap_element(ctx, node),
                None => Ok(JsValue::null()),
            }
        })
    };

    let doc_for_qs: &'static Document<'static> =
        unsafe { &*(document as *const Document<'static>) };
    let descendants_qs = doc_for_qs.descendants();
    let query_selector_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let selector = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;

            if selector.is_empty() {
                return Ok(JsValue::null());
            }

            let found = descendants_qs.iter().find(|node| {
                node.as_element().is_some_and(|el| {
                    let tag = el.local_name();
                    selector.eq_ignore_ascii_case(tag)
                        || selector.strip_prefix('.').is_some_and(|class| {
                            el.attr("class").is_some_and(|c| {
                                c.split_whitespace().any(|p| p.eq_ignore_ascii_case(class))
                            })
                        })
                        || selector.strip_prefix('#').is_some_and(|id| {
                            el.attr("id")
                                .is_some_and(|attr| attr.eq_ignore_ascii_case(id))
                        })
                })
            });

            match found {
                Some(node) => wrap_element(ctx, node),
                None => Ok(JsValue::null()),
            }
        })
    };

    let create_element_fn = unsafe {
        NativeFunction::from_closure(|_this, args, ctx| {
            let tag = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;

            if tag.is_empty() {
                return Err(BoaError::from(
                    JsNativeError::typ().with_message("createElement: tag name is required"),
                ));
            }

            let el = ObjectInitializer::new(ctx)
                .property(
                    js_string!("tagName"),
                    JsString::from(tag.to_uppercase()),
                    Attribute::all(),
                )
                .property(js_string!("innerHTML"), js_string!(""), Attribute::all())
                .property(js_string!("outerHTML"), js_string!(""), Attribute::all())
                .build();

            Ok(JsValue::from(el))
        })
    };

    let initial_title = document
        .first_element_by_tag("title")
        .map(|n| n.text_content())
        .unwrap_or_else(|| document.text_content())
        .chars()
        .take(80)
        .collect::<String>();
    let doc_ptr_usize = document as *const Document<'static> as usize;

    let obj = ObjectInitializer::new(context)
        .property(js_string!("title"), title_val, Attribute::all())
        .property(
            js_string!("documentElement"),
            JsValue::null(),
            Attribute::all(),
        )
        .property(js_string!("body"), JsValue::null(), Attribute::all())
        .property(js_string!("head"), JsValue::null(), Attribute::all())
        .property(
            js_string!("characterSet"),
            js_string!("UTF-8"),
            Attribute::all(),
        )
        .property(
            js_string!("contentType"),
            js_string!("text/html"),
            Attribute::all(),
        )
        .property(js_string!("cookie"), js_string!(""), Attribute::all())
        .property(js_string!("hidden"), false, Attribute::all())
        .property(
            js_string!("visibilityState"),
            js_string!("visible"),
            Attribute::all(),
        )
        .function(get_element_by_id_fn, js_string!("getElementById"), 1)
        .function(query_selector_fn, js_string!("querySelector"), 1)
        .function(create_element_fn, js_string!("createElement"), 1)
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, _args, _ctx| {
                    // getter for title
                    let guard = DOCUMENT_TITLES.lock().unwrap();
                    if let Some(title) = guard.get(&doc_ptr_usize) {
                        Ok(JsValue::from(JsString::from(title.as_str())))
                    } else {
                        Ok(JsValue::from(JsString::from(initial_title.as_str())))
                    }
                })
            },
            js_string!("getTitle"),
            0,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let new_title = args
                        .first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let mut guard = DOCUMENT_TITLES.lock().unwrap();
                    guard.insert(doc_ptr_usize, new_title.clone());
                        // notify host about title change
                        hooks::notify_title_changed(new_title.clone());
                    Ok(JsValue::undefined())
                })
            },
            js_string!("setTitle"),
            1,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    // dispatchEvent(name)
                    let name = args
                        .first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    if name.is_empty() { return Ok(JsValue::undefined()); }
                    // Notify host about dispatched event.
                    hooks::notify_event(name.clone(), String::new());
                    // Also record that an event was dispatched; if listeners are registered on the document, log for now.
                    let mut guard = DOCUMENT_EVENT_LISTENERS.lock().unwrap();
                    let _ = guard.entry(doc_ptr_usize).or_default();
                    Ok(JsValue::undefined())
                })
            },
            js_string!("dispatchEvent"),
            1,
        )
        .build();

    Ok(JsValue::from(obj))
}

fn build_window_object(context: &mut Context) -> JsResult<JsValue> {
    let obj = ObjectInitializer::new(context)
        .property(
            js_string!("innerWidth"),
            JsValue::from(1280),
            Attribute::all(),
        )
        .property(
            js_string!("innerHeight"),
            JsValue::from(720),
            Attribute::all(),
        )
        .property(js_string!("outerWidth"), 1280, Attribute::all())
        .property(js_string!("outerHeight"), 720, Attribute::all())
        .property(js_string!("screenX"), 0, Attribute::all())
        .property(js_string!("screenY"), 0, Attribute::all())
        .property(js_string!("devicePixelRatio"), 1.0, Attribute::all())
        .property(js_string!("scrollX"), 0.0, Attribute::all())
        .property(js_string!("scrollY"), 0.0, Attribute::all())
        .property(js_string!("pageXOffset"), 0.0, Attribute::all())
        .property(js_string!("pageYOffset"), 0.0, Attribute::all())
        .build();

    let alert_fn = unsafe {
        NativeFunction::from_closure(|_this, args, ctx| {
            let msg = args
                .first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            debug!("[JS alert] {}", msg);
            Ok(JsValue::undefined())
        })
    };

    obj.set(
        js_string!("alert"),
        FunctionObjectBuilder::new(context.realm(), alert_fn).build(),
        false,
        context,
    )
    .map_err(|e| BoaError::from(JsNativeError::typ().with_message(format!("{e}"))))?;

    Ok(JsValue::from(obj))
}

fn wrap_element(context: &mut Context, node: fortrust_dom::NodeRef<'_>) -> JsResult<JsValue> {
    let tag_str = node
        .as_element()
        .map(|el| el.local_name().to_uppercase())
        .unwrap_or_else(|| "#text".to_owned());

    let text_str = node.text_content();

    let node_ptr_usize = node as *const fortrust_dom::Node<'_> as usize;

    let obj = ObjectInitializer::new(context)
        .property(
            js_string!("tagName"),
            JsString::from(tag_str.clone()),
            Attribute::all(),
        )
        .property(js_string!("nodeType"), 1, Attribute::all())
        .property(
            js_string!("nodeName"),
            JsString::from(tag_str.clone()),
            Attribute::all(),
        )
        .property(
            js_string!("innerHTML"),
            JsString::from(text_str.clone()),
            Attribute::all(),
        )
        .property(
            js_string!("outerHTML"),
            JsString::from(format!("<{tag_str}>{text_str}</{tag_str}>")),
            Attribute::all(),
        )
        .property(
            js_string!("textContent"),
            JsString::from(text_str),
            Attribute::all(),
        )
        .property(js_string!("__node_ptr"), JsValue::from(node_ptr_usize as f64), Attribute::all())
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    // setAttribute(name, value)
                    let name = args
                        .first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let value = args
                        .get(1)
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    if name.is_empty() { return Ok(JsValue::undefined()); }
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    // Mutate the arena-backed element attributes directly.
                    if let Some(el) = node_ref.as_element() {
                        el.set_attr(&name, &value);
                    }
                    Ok(JsValue::undefined())
                })
            },
            js_string!("setAttribute"),
            2,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    // addEventListener(name, callback) - we only record the event name on the Rust side for now
                    let name = args
                        .first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    if name.is_empty() { return Ok(JsValue::undefined()); }
                    let mut guard = DOCUMENT_EVENT_LISTENERS.lock().unwrap();
                    let entry = guard.entry(node_ptr_usize).or_default();
                    if !entry.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                        entry.push(name);
                    }
                    Ok(JsValue::undefined())
                })
            },
            js_string!("addEventListener"),
            2,
        )
        .build();

    Ok(JsValue::from(obj))
}
