use boa_engine::{
    Context, JsError as BoaError, JsNativeError, JsResult, JsString, JsValue, NativeFunction,
    js_string, object::FunctionObjectBuilder, object::ObjectInitializer, property::Attribute,
};
use fortrust_dom::Document;
use fortrust_dom::event::{
    CallbackId, CallbackResult, DomEvent, EventCallbackInvoker, EventCallbackRegistry, 
    EventListener, EventListenerSet, ListenerPhase, dispatch_event_chain,
};
use fortrust_dom::ancestor_chain;
use tracing::debug;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::hooks;

lazy_static::lazy_static! {
    static ref DOCUMENT_TITLES: Mutex<HashMap<usize, String>> = Mutex::new(HashMap::new());
    static ref EVENT_CALLBACK_REGISTRY: EventCallbackRegistry = EventCallbackRegistry::new();
    /// Tracks the "dirty" flag — set to true when DOM mutations occur that require re-render.
    static ref DOM_DIRTY: Mutex<bool> = Mutex::new(false);
    pub static ref CANVAS_CONTEXTS: Mutex<HashMap<usize, crate::canvas::Canvas2D>> = Mutex::new(HashMap::new());
}

// JS callbacks are stored per-thread since JsObject is !Send.
// The JS runtime is single-threaded, so this is safe.
thread_local! {
    static JS_CALLBACKS: std::cell::RefCell<HashMap<CallbackId, boa_engine::JsObject>> = std::cell::RefCell::new(HashMap::new());
}

/// Mark the DOM as dirty (needs re-render). Called from mutation closures.
pub fn mark_dom_dirty() {
    if let Ok(mut dirty) = DOM_DIRTY.lock() {
        *dirty = true;
    }
}

/// Check and clear the DOM dirty flag.
pub fn take_dom_dirty() -> bool {
    DOM_DIRTY.lock().map(|mut d| { let v = *d; *d = false; v }).unwrap_or(false)
}

/// Store a global arena reference for use in closures.
static mut ARENA_REF: Option<&'static fortrust_dom::DomArena> = None;
static mut DOC_REF: Option<&'static Document<'static>> = None;

pub fn register(
    context: &mut Context,
    document: &Document<'static>,
    arena: Option<&'static fortrust_dom::DomArena>,
) -> JsResult<()> {
    // Store arena globally for createElement closures.
    // Safety: the arena and document are Box::leak'd by the trust-engine,
    // so they live for the entire program lifetime.
    unsafe {
        ARENA_REF = arena;
        DOC_REF = Some(&*(document as *const Document<'static>));
    }

    let doc_obj = build_document_object(context, document)?;
    context.register_global_property(js_string!("document"), doc_obj, Attribute::all())?;

    let win_obj = build_window_object(context)?;
    context.register_global_property(js_string!("window"), win_obj, Attribute::all())?;

    // Register FormData constructor
    register_form_data(context)?;

    debug!("DOM API bindings registered (with full mutation support)");
    Ok(())
}

fn get_arena() -> Option<&'static fortrust_dom::DomArena> {
    unsafe { ARENA_REF }
}

fn build_document_object(
    context: &mut Context,
    document: &Document<'static>,
) -> JsResult<JsValue> {
    let title_val = JsString::from(document.text_content().chars().take(80).collect::<String>());

    let doc_for_get: &'static Document<'static> =
        unsafe { &*(document as *const Document<'static>) };
    let get_element_by_id_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let id = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            if id.is_empty() { return Ok(JsValue::null()); }
            match doc_for_get.get_element_by_id(&id) {
                Some(node) => wrap_element(ctx, node),
                None => Ok(JsValue::null()),
            }
        })
    };

    let doc_for_qs: &'static Document<'static> =
        unsafe { &*(document as *const Document<'static>) };
    let query_selector_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let selector = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            if selector.is_empty() { return Ok(JsValue::null()); }
            match doc_for_qs.query_selector(&selector) {
                Some(node) => wrap_element(ctx, node),
                None => Ok(JsValue::null()),
            }
        })
    };

    let doc_for_qsa: &'static Document<'static> =
        unsafe { &*(document as *const Document<'static>) };
    let query_selector_all_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let selector = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            if selector.is_empty() {
                return Ok(JsValue::from(boa_engine::object::builtins::JsArray::new(ctx)));
            }
            let nodes = doc_for_qsa.query_selector_all(&selector);
            let arr = boa_engine::object::builtins::JsArray::new(ctx);
            for node in &nodes {
                let el = wrap_element(ctx, node)?;
                let _ = arr.push(el, ctx);
            }
            Ok(JsValue::from(arr))
        })
    };

    let doc_for_gbtn: &'static Document<'static> =
        unsafe { &*(document as *const Document<'static>) };
    let get_elements_by_tag_name_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let tag = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            if tag.is_empty() {
                return Ok(JsValue::from(boa_engine::object::builtins::JsArray::new(ctx)));
            }
            let nodes = doc_for_gbtn.get_elements_by_tag_name(&tag);
            let arr = boa_engine::object::builtins::JsArray::new(ctx);
            for node in &nodes {
                let el = wrap_element(ctx, node)?;
                let _ = arr.push(el, ctx);
            }
            Ok(JsValue::from(arr))
        })
    };

    let doc_for_gbcn: &'static Document<'static> =
        unsafe { &*(document as *const Document<'static>) };
    let get_elements_by_class_name_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let class = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            if class.is_empty() {
                return Ok(JsValue::from(boa_engine::object::builtins::JsArray::new(ctx)));
            }
            let nodes = doc_for_gbcn.get_elements_by_class_name(&class);
            let arr = boa_engine::object::builtins::JsArray::new(ctx);
            for node in &nodes {
                let el = wrap_element(ctx, node)?;
                let _ = arr.push(el, ctx);
            }
            Ok(JsValue::from(arr))
        })
    };

    let create_element_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let tag = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            if tag.is_empty() {
                return Err(BoaError::from(
                    JsNativeError::typ().with_message("createElement: tag name is required"),
                ));
            }
            let arena = get_arena().ok_or_else(|| BoaError::from(
                JsNativeError::typ().with_message("createElement: no arena available"),
            ))?;
            let tag_lower = tag.to_ascii_lowercase();
            let node = arena.create_element(&tag_lower);
            wrap_element(ctx, node)
        })
    };

    let create_text_node_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let arena = get_arena().ok_or_else(|| BoaError::from(
                JsNativeError::typ().with_message("createTextNode: no arena available"),
            ))?;
            let node = arena.create_text_node(&text);
            wrap_element(ctx, node)
        })
    };

    let doc_ptr_usize = document as *const Document<'static> as usize;
    let initial_title = document.first_element_by_tag("title")
        .map(|n| n.text_content())
        .unwrap_or_default();

    let obj = ObjectInitializer::new(context)
        .property(js_string!("title"), title_val, Attribute::all())
        .property(js_string!("documentElement"), JsValue::null(), Attribute::all())
        .property(js_string!("body"), JsValue::null(), Attribute::all())
        .property(js_string!("head"), JsValue::null(), Attribute::all())
        .property(js_string!("characterSet"), js_string!("UTF-8"), Attribute::all())
        .property(js_string!("contentType"), js_string!("text/html"), Attribute::all())
        .property(js_string!("cookie"), js_string!(""), Attribute::all())
        .property(js_string!("hidden"), false, Attribute::all())
        .property(js_string!("visibilityState"), js_string!("visible"), Attribute::all())
        .function(get_element_by_id_fn, js_string!("getElementById"), 1)
        .function(query_selector_fn, js_string!("querySelector"), 1)
        .function(query_selector_all_fn, js_string!("querySelectorAll"), 1)
        .function(get_elements_by_tag_name_fn, js_string!("getElementsByTagName"), 1)
        .function(get_elements_by_class_name_fn, js_string!("getElementsByClassName"), 1)
        .function(create_element_fn, js_string!("createElement"), 1)
        .function(create_text_node_fn, js_string!("createTextNode"), 1)
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, _args, _ctx| {
                    let guard = DOCUMENT_TITLES.lock().unwrap();
                    if let Some(title) = guard.get(&doc_ptr_usize) {
                        Ok(JsValue::from(JsString::from(title.as_str())))
                    } else {
                        Ok(JsValue::from(JsString::from(initial_title.as_str())))
                    }
                })
            },
            js_string!("getTitle"), 0,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let new_title = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let mut guard = DOCUMENT_TITLES.lock().unwrap();
                    guard.insert(doc_ptr_usize, new_title.clone());
                    hooks::notify_title_changed(new_title.clone());
                    Ok(JsValue::undefined())
                })
            },
            js_string!("setTitle"), 1,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let event_type = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    if event_type.is_empty() { return Ok(JsValue::undefined()); }
                    hooks::notify_event(event_type, String::new());
                    Ok(JsValue::undefined())
                })
            },
            js_string!("dispatchEvent"), 1,
        )
        .build();

    Ok(JsValue::from(obj))
}

fn build_window_object(context: &mut Context) -> JsResult<JsValue> {
    let obj = ObjectInitializer::new(context)
        .property(js_string!("innerWidth"), JsValue::from(1280), Attribute::all())
        .property(js_string!("innerHeight"), JsValue::from(720), Attribute::all())
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
            let msg = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            debug!("[JS alert] {}", msg);
            Ok(JsValue::undefined())
        })
    };

    obj.set(
        js_string!("alert"),
        FunctionObjectBuilder::new(context.realm(), alert_fn).build(),
        false, context,
    ).map_err(|e| BoaError::from(JsNativeError::typ().with_message(format!("{e}"))))?;

    Ok(JsValue::from(obj))
}

/// Wrap a DOM node as a full-featured JS element with mutation and event APIs.
fn wrap_element(context: &mut Context, node: fortrust_dom::NodeRef<'static>) -> JsResult<JsValue> {
    let tag_str = node.as_element()
        .map(|el| el.local_name().to_uppercase())
        .unwrap_or_else(|| "#text".to_owned());
    let text_str = node.text_content();
    let node_ptr_usize = node as *const fortrust_dom::Node<'static> as usize;

    let is_canvas = tag_str == "CANVAS";

    let obj = ObjectInitializer::new(context)
        .property(js_string!("tagName"), JsString::from(tag_str.clone()), Attribute::all())
        .property(js_string!("nodeType"), 1, Attribute::all())
        .property(js_string!("nodeName"), JsString::from(tag_str.clone()), Attribute::all())
        .property(js_string!("innerHTML"), JsString::from(text_str.clone()), Attribute::all())
        .property(js_string!("outerHTML"), JsString::from(format!("<{tag_str}>{text_str}</{tag_str}>")), Attribute::all())
        .property(js_string!("textContent"), JsString::from(text_str), Attribute::all())
        .property(js_string!("__node_ptr"), JsValue::from(node_ptr_usize as f64), Attribute::all())
        // ─── setAttribute / getAttribute / removeAttribute ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    let value = args.get(1).map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if name.is_empty() { return Ok(JsValue::undefined()); }
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = { &*node_ptr };
                    if let Some(el) = node_ref.as_element() {
                        el.set_attr(&name, &value);
                        mark_dom_dirty();
                    }
                    Ok(JsValue::undefined())
                })
            },
            js_string!("setAttribute"), 2,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if name.is_empty() { return Ok(JsValue::null()); }
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = { &*node_ptr };
                    match node_ref.as_element().and_then(|el| el.attr(&name)) {
                        Some(val) => Ok(JsValue::from(JsString::from(val.as_str()))),
                        None => Ok(JsValue::null()),
                    }
                })
            },
            js_string!("getAttribute"), 1,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if name.is_empty() { return Ok(JsValue::undefined()); }
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = { &*node_ptr };
                    if let Some(el) = node_ref.as_element() {
                        el.remove_attr(&name);
                        mark_dom_dirty();
                    }
                    Ok(JsValue::undefined())
                })
            },
            js_string!("removeAttribute"), 1,
        )
        // ─── appendChild ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let child_val = args.first().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("appendChild requires a child argument")))?;
                    let child_obj = child_val.as_object().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("child must be an object")))?;
                    let child_ptr_val = child_obj.get(js_string!("__node_ptr"), ctx)?;
                    let child_ptr_num = child_ptr_val.as_number().unwrap_or(0.0) as usize;
                    if child_ptr_num == 0 { return Err(BoaError::from(JsNativeError::typ().with_message("child is not a valid DOM node"))); }

                    let parent_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let parent_ref: &fortrust_dom::Node<'static> = &*parent_ptr;
                    let child_ref: &fortrust_dom::Node<'static> = &*(child_ptr_num as *const fortrust_dom::Node<'static>);

                    parent_ref.append_child(child_ref);
                    parent_ref.mark_dirty();
                    mark_dom_dirty();
                    Ok(child_val.clone())
                })
            },
            js_string!("appendChild"), 1,
        )
        // ─── removeChild ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let child_val = args.first().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("removeChild requires a child argument")))?;
                    let child_obj = child_val.as_object().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("child must be an object")))?;
                    let child_ptr_val = child_obj.get(js_string!("__node_ptr"), ctx)?;
                    let child_ptr_num = child_ptr_val.as_number().unwrap_or(0.0) as usize;
                    if child_ptr_num == 0 { return Err(BoaError::from(JsNativeError::typ().with_message("child is not a valid DOM node"))); }

                    let parent_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let parent_ref: &fortrust_dom::Node<'static> = &*parent_ptr;
                    let child_ref: &fortrust_dom::Node<'static> = &*(child_ptr_num as *const fortrust_dom::Node<'static>);

                    match parent_ref.remove_child_checked(child_ref) {
                        Ok(()) => {
                            parent_ref.mark_dirty();
                            mark_dom_dirty();
                            Ok(child_val.clone())
                        }
                        Err(_) => Err(BoaError::from(JsNativeError::typ().with_message("node is not a child of this element"))),
                    }
                })
            },
            js_string!("removeChild"), 1,
        )
        // ─── insertBefore ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let new_val = args.first().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("insertBefore requires newChild")))?;
                    let ref_val = args.get(1);
                    let new_obj = new_val.as_object().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("newChild must be an object")))?;
                    let new_ptr_num = new_obj.get(js_string!("__node_ptr"), ctx)?.as_number().unwrap_or(0.0) as usize;
                    if new_ptr_num == 0 { return Err(BoaError::from(JsNativeError::typ().with_message("newChild is not a valid DOM node"))); }

                    let parent_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let parent_ref: &fortrust_dom::Node<'static> = &*parent_ptr;
                    let new_ref: &fortrust_dom::Node<'static> = &*(new_ptr_num as *const fortrust_dom::Node<'static>);

                    let ref_node = if let Some(rv) = ref_val {
                        if rv.is_null() || rv.is_undefined() {
                            None
                        } else if let Some(ro) = rv.as_object() {
                            let rp = ro.get(js_string!("__node_ptr"), ctx)?.as_number().unwrap_or(0.0) as usize;
                            if rp == 0 { None } else { Some(&*(rp as *const fortrust_dom::Node<'static>) as fortrust_dom::NodeRef<'static>) }
                        } else { None }
                    } else { None };

                    match parent_ref.insert_before(new_ref, ref_node) {
                        Ok(()) => { parent_ref.mark_dirty(); mark_dom_dirty(); Ok(new_val.clone()) }
                        Err(e) => Err(BoaError::from(JsNativeError::typ().with_message(format!("{e:?}")))),
                    }
                })
            },
            js_string!("insertBefore"), 2,
        )
        // ─── replaceChild ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let new_val = args.first().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("replaceChild requires newChild")))?;
                    let old_val = args.get(1).ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("replaceChild requires oldChild")))?;
                    let new_obj = new_val.as_object().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("newChild must be an object")))?;
                    let old_obj = old_val.as_object().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("oldChild must be an object")))?;
                    let new_ptr = new_obj.get(js_string!("__node_ptr"), ctx)?.as_number().unwrap_or(0.0) as usize;
                    let old_ptr = old_obj.get(js_string!("__node_ptr"), ctx)?.as_number().unwrap_or(0.0) as usize;
                    if new_ptr == 0 || old_ptr == 0 { return Err(BoaError::from(JsNativeError::typ().with_message("invalid DOM node"))); }

                    let parent_ref: &fortrust_dom::Node<'static> = &*(node_ptr_usize as *const _);
                    let new_ref: &fortrust_dom::Node<'static> = &*(new_ptr as *const _);
                    let old_ref: &fortrust_dom::Node<'static> = &*(old_ptr as *const _);

                    match parent_ref.replace_child(new_ref, old_ref) {
                        Ok(_) => { parent_ref.mark_dirty(); mark_dom_dirty(); Ok(old_val.clone()) }
                        Err(e) => Err(BoaError::from(JsNativeError::typ().with_message(format!("{e:?}")))),
                    }
                })
            },
            js_string!("replaceChild"), 2,
        )
        // ─── textContent setter (via setTextContent) ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let text = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    let arena = get_arena().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("no arena")))?;
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    node_ref.set_text_content(arena, &text);
                    node_ref.mark_dirty();
                    mark_dom_dirty();
                    Ok(JsValue::undefined())
                })
            },
            js_string!("setTextContent"), 1,
        )
        // ─── innerHTML setter (via setInnerHTML) ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let html = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    let arena = get_arena().ok_or_else(|| BoaError::from(JsNativeError::typ().with_message("no arena")))?;
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    node_ref.set_inner_html(arena, &html);
                    node_ref.mark_dirty();
                    mark_dom_dirty();
                    Ok(JsValue::undefined())
                })
            },
            js_string!("setInnerHTML"), 1,
        )
        // ─── addEventListener (with real callback storage) ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let event_type = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    let callback_val = args.get(1).cloned().unwrap_or(JsValue::undefined());
                    if event_type.is_empty() { return Ok(JsValue::undefined()); }

                    let callback_obj = callback_val.as_object().cloned();
                    let options = args.get(2);
                    let (capture, once) = parse_listener_options(options, ctx);

                    let phase = if capture { ListenerPhase::Capture } else { ListenerPhase::Bubble };

                    // Register callback in the global registry
                    let cb_id = EVENT_CALLBACK_REGISTRY.register(event_type.clone(), node_ptr_usize);

                    // Store the JS function for later invocation
                    if let Some(obj) = callback_obj {
                        JS_CALLBACKS.with(|cbs| {
                            cbs.borrow_mut().insert(cb_id, obj);
                        });
                    }

                    // Add to the node's event listener set
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    node_ref.event_listeners.add(EventListener {
                        event_type,
                        callback_id: cb_id,
                        phase,
                        once,
                    });

                    Ok(JsValue::undefined())
                })
            },
            js_string!("addEventListener"), 2,
        )
        // ─── removeEventListener ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let event_type = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if event_type.is_empty() { return Ok(JsValue::undefined()); }
                    // For now, remove all listeners of this type on this node
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    node_ref.event_listeners.remove_all_for_type(&event_type);
                    Ok(JsValue::undefined())
                })
            },
            js_string!("removeEventListener"), 1,
        )
        // ─── dispatchEvent (with full event propagation) ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let event_type = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if event_type.is_empty() { return Ok(JsValue::from(false)); }

                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;

                    // Build ancestor chain
                    let chain_up = ancestor_chain(node_ref);
                    let target_depth = chain_up.len() - 1;
                    let chain_reversed: Vec<fortrust_dom::NodeRef<'static>> = chain_up.into_iter().rev().collect();

                    // Build dispatch chain: (ptr, &EventListenerSet)
                    let dispatch_chain: Vec<(usize, &EventListenerSet)> = chain_reversed.iter()
                        .map(|n| (*n as *const fortrust_dom::Node<'static> as usize, &n.event_listeners))
                        .collect();

                    let mut event = DomEvent::bubbling(&event_type);

                    struct BoaInvoker<'a> { ctx: &'a mut Context }
                    impl EventCallbackInvoker for BoaInvoker<'_> {
                        fn invoke(&mut self, callback_id: CallbackId, event: &DomEvent) -> CallbackResult {
                            let func_obj: Option<boa_engine::JsObject> = JS_CALLBACKS.with(|cbs| {
                                cbs.borrow().get(&callback_id).cloned()
                            });
                            let Some(func_obj) = func_obj else { return CallbackResult::default(); };
                            // Build a JS event object
                            let evt_obj = ObjectInitializer::new(self.ctx)
                                .property(js_string!("type"), JsValue::from(js_string!(event.event_type.as_str())), Attribute::all())
                                .property(js_string!("bubbles"), event.bubbles, Attribute::all())
                                .property(js_string!("cancelable"), event.cancelable, Attribute::all())
                                .property(js_string!("target"), JsValue::null(), Attribute::all())
                                .property(js_string!("currentTarget"), JsValue::null(), Attribute::all())
                                .build();
                            let evt_val = JsValue::from(evt_obj);
                            let func: JsValue = JsValue::from(func_obj);
                            let func_callable = func.as_callable();
                            if let Some(callable) = func_callable {
                                match callable.call(&JsValue::undefined(), &[evt_val], self.ctx) {
                                    Ok(_) => CallbackResult::default(),
                                    Err(_) => CallbackResult::default(),
                                }
                            } else {
                                CallbackResult::default()
                            }
                        }
                    }

                    let result = dispatch_event_chain(&dispatch_chain, target_depth, &mut event, &mut BoaInvoker { ctx });
                    Ok(JsValue::from(result.default_prevented))
                })
            },
            js_string!("dispatchEvent"), 1,
        )
        // ─── querySelector / querySelectorAll on element ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let selector = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if selector.is_empty() { return Ok(JsValue::null()); }
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    match node_ref.query_selector(&selector) {
                        Some(found) => wrap_element(ctx, found),
                        None => Ok(JsValue::null()),
                    }
                })
            },
            js_string!("querySelector"), 1,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let selector = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    let nodes = node_ref.query_selector_all(&selector);
                    let arr = boa_engine::object::builtins::JsArray::new(ctx);
                    for n in &nodes {
                        let el = wrap_element(ctx, n)?;
                        let _ = arr.push(el, ctx);
                    }
                    Ok(JsValue::from(arr))
                })
            },
            js_string!("querySelectorAll"), 1,
        )
        // ─── classList.toggle / add / remove (simplified) ───
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let class_name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if class_name.is_empty() { return Ok(JsValue::undefined()); }
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    if let Some(el) = node_ref.as_element() {
                        let current = el.attr("class").unwrap_or_default();
                        let classes: Vec<&str> = current.split_whitespace().collect();
                        if !classes.contains(&class_name.as_str()) {
                            let new_class = format!("{} {}", current, class_name).trim().to_owned();
                            el.set_attr("class", &new_class);
                            mark_dom_dirty();
                            node_ref.mark_dirty();
                        }
                    }
                    Ok(JsValue::undefined())
                })
            },
            js_string!("addClass"), 1,
        )
        .function(
            unsafe {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let class_name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
                    if class_name.is_empty() { return Ok(JsValue::undefined()); }
                    let node_ptr = node_ptr_usize as *const fortrust_dom::Node<'static>;
                    let node_ref: &fortrust_dom::Node<'static> = &*node_ptr;
                    if let Some(el) = node_ref.as_element() {
                        let current = el.attr("class").unwrap_or_default();
                        let new_class: String = current.split_whitespace()
                            .filter(|c| *c != class_name.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        el.set_attr("class", &new_class);
                        mark_dom_dirty();
                        node_ref.mark_dirty();
                    }
                    Ok(JsValue::undefined())
                })
            },
            js_string!("removeClass"), 1,
        )
        .build();

    // ─── Standard classList (DOMTokenList) ───
    let class_list = build_class_list(context, node_ptr_usize)?;
    let _ = obj.set(js_string!("classList"), class_list, false, context);

    if is_canvas {
        let _ = obj.set(js_string!("width"), JsValue::from(300), false, context);
        let _ = obj.set(js_string!("height"), JsValue::from(150), false, context);

        let ctx_node_ptr = node_ptr_usize;
        let get_context_fn = unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let ctx_type = args.first()
                    .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                    .unwrap_or(Ok(String::new()))?;
                if ctx_type != "2d" { return Ok(JsValue::null()); }

                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                map.entry(ctx_node_ptr).or_insert_with(|| {
                    crate::canvas::Canvas2D::new(300, 150)
                });

                let ctx_obj = build_canvas_2d_context(ctx, ctx_node_ptr)?;
                Ok(ctx_obj)
            })
        };
        let _ = obj.set(
            js_string!("getContext"),
            FunctionObjectBuilder::new(context.realm(), get_context_fn).build(),
            false, context,
        );

        let to_data_url_fn = unsafe {
            NativeFunction::from_closure(move |_this, _args, _ctx| {
                let map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(canvas) = map.get(&ctx_node_ptr) {
                    Ok(JsValue::from(js_string!(canvas.to_data_url().as_str())))
                } else {
                    Ok(JsValue::from(js_string!("data:,")))
                }
            })
        };
        let _ = obj.set(
            js_string!("toDataURL"),
            FunctionObjectBuilder::new(context.realm(), to_data_url_fn).build(),
            false, context,
        );
    }

    Ok(JsValue::from(obj))
}

/// Build a DOMTokenList-like object with standard classList API.
fn build_class_list(context: &mut Context, node_ptr: usize) -> JsResult<JsValue> {
    let add_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let node_ref: &fortrust_dom::Node<'static> = &*(node_ptr as *const _);
            for arg in args.iter() {
                let name = arg.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                if name.is_empty() { continue; }
                if let Some(el) = node_ref.as_element() {
                    let current = el.attr("class").unwrap_or_default();
                    if !current.split_whitespace().any(|c| c == name) {
                        let new = if current.is_empty() { name.clone() } else { format!("{} {}", current, name) };
                        el.set_attr("class", &new);
                    }
                }
            }
            node_ref.mark_dirty();
            mark_dom_dirty();
            Ok(JsValue::undefined())
        })
    };
    let remove_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let node_ref: &fortrust_dom::Node<'static> = &*(node_ptr as *const _);
            for arg in args.iter() {
                let name = arg.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                if name.is_empty() { continue; }
                if let Some(el) = node_ref.as_element() {
                    let current = el.attr("class").unwrap_or_default();
                    let new: String = current.split_whitespace()
                        .filter(|c| *c != name.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    el.set_attr("class", &new);
                }
            }
            node_ref.mark_dirty();
            mark_dom_dirty();
            Ok(JsValue::undefined())
        })
    };
    let toggle_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
            let force = args.get(1).and_then(|v| v.as_boolean());
            let node_ref: &fortrust_dom::Node<'static> = &*(node_ptr as *const _);
            if let Some(el) = node_ref.as_element() {
                let current = el.attr("class").unwrap_or_default();
                let classes: Vec<&str> = current.split_whitespace().collect();
                let has = classes.contains(&name.as_str());
                let add = force.unwrap_or(!has);
                if add && !has {
                    let new = if current.is_empty() { name.clone() } else { format!("{} {}", current, name) };
                    el.set_attr("class", &new);
                } else if !add && has {
                    let new: String = classes.into_iter().filter(|c| *c != name.as_str()).collect::<Vec<_>>().join(" ");
                    el.set_attr("class", &new);
                }
                node_ref.mark_dirty();
                mark_dom_dirty();
                return Ok(JsValue::from(add));
            }
            Ok(JsValue::from(false))
        })
    };
    let contains_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
            let node_ref: &fortrust_dom::Node<'static> = &*(node_ptr as *const _);
            if let Some(el) = node_ref.as_element() {
                let current = el.attr("class").unwrap_or_default();
                return Ok(JsValue::from(current.split_whitespace().any(|c| c == name)));
            }
            Ok(JsValue::from(false))
        })
    };
    let replace_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let old_name = args.first().map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
            let new_name = args.get(1).map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped())).unwrap_or(Ok(String::new()))?;
            let node_ref: &fortrust_dom::Node<'static> = &*(node_ptr as *const _);
            if let Some(el) = node_ref.as_element() {
                let current = el.attr("class").unwrap_or_default();
                let classes: Vec<&str> = current.split_whitespace().collect();
                if classes.contains(&old_name.as_str()) {
                    let new: String = classes.iter()
                        .map(|c| if *c == old_name.as_str() { new_name.as_str() } else { *c })
                        .collect::<Vec<_>>()
                        .join(" ");
                    el.set_attr("class", &new);
                    node_ref.mark_dirty();
                    mark_dom_dirty();
                    return Ok(JsValue::from(true));
                }
            }
            Ok(JsValue::from(false))
        })
    };

    let class_list = ObjectInitializer::new(context)
        .function(add_fn, js_string!("add"), 1)
        .function(remove_fn, js_string!("remove"), 1)
        .function(toggle_fn, js_string!("toggle"), 1)
        .function(contains_fn, js_string!("contains"), 1)
        .function(replace_fn, js_string!("replace"), 2)
        .build();

    let _ = class_list.set(js_string!("length"), JsValue::from(0), false, context);
    Ok(JsValue::Object(class_list))
}

fn build_canvas_2d_context(ctx: &mut Context, canvas_ptr: usize) -> JsResult<JsValue> {
    let fill_rect_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let w = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let h = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.fill_rect(x, y, w, h); }
            Ok(JsValue::undefined())
        })
    };
    let clear_rect_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let w = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let h = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.clear_rect(x, y, w, h); }
            Ok(JsValue::undefined())
        })
    };
    let fill_text_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let x = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let max_w = args.get(3).and_then(|v| v.as_number()).map(|n| n as f32);
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.fill_text(&text, x, y, max_w); }
            Ok(JsValue::undefined())
        })
    };

    let save_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.save(); }
            Ok(JsValue::undefined())
        })
    };
    let restore_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.restore(); }
            Ok(JsValue::undefined())
        })
    };

    let translate_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.translate(x, y); }
            Ok(JsValue::undefined())
        })
    };
    let rotate_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let angle = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.rotate(angle); }
            Ok(JsValue::undefined())
        })
    };
    let scale_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(1.0) as f32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(1.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.scale(x, y); }
            Ok(JsValue::undefined())
        })
    };
    let set_transform_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let a = args.first().and_then(|v| v.as_number()).unwrap_or(1.0) as f32;
            let b = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let c = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let d = args.get(3).and_then(|v| v.as_number()).unwrap_or(1.0) as f32;
            let e = args.get(4).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let f = args.get(5).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(cvs) = map.get_mut(&canvas_ptr) { cvs.set_transform(a, b, c, d, e, f); }
            Ok(JsValue::undefined())
        })
    };

    let begin_path_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.begin_path(); }
            Ok(JsValue::undefined())
        })
    };
    let close_path_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.close_path(); }
            Ok(JsValue::undefined())
        })
    };
    let move_to_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.move_to(x, y); }
            Ok(JsValue::undefined())
        })
    };
    let line_to_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.line_to(x, y); }
            Ok(JsValue::undefined())
        })
    };
    let arc_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let cx = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let cy = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let r = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let sa = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let ea = args.get(4).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let acw = args.get(5).and_then(|v| v.as_boolean()).unwrap_or(false);
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.arc(cx, cy, r, sa, ea, acw); }
            Ok(JsValue::undefined())
        })
    };
    let rect_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let w = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let h = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.rect(x, y, w, h); }
            Ok(JsValue::undefined())
        })
    };
    let fill_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.fill(); }
            Ok(JsValue::undefined())
        })
    };
    let stroke_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.stroke(); }
            Ok(JsValue::undefined())
        })
    };

    let get_image_data_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as i32;
            let y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as i32;
            let w = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
            let h = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
            let map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get(&canvas_ptr) {
                let (data, iw, ih) = c.get_image_data(x, y, w, h);
                let arr = boa_engine::object::builtins::JsArray::new(ctx);
                for (i, &byte) in data.iter().enumerate() {
                    let _ = arr.set(i, JsValue::from(byte as i32), false, ctx);
                }
                let img_data = ObjectInitializer::new(ctx)
                    .property(js_string!("data"), JsValue::from(arr), Attribute::all())
                    .property(js_string!("width"), JsValue::from(iw as i32), Attribute::all())
                    .property(js_string!("height"), JsValue::from(ih as i32), Attribute::all())
                    .build();
                Ok(JsValue::from(img_data))
            } else {
                Ok(JsValue::null())
            }
        })
    };
    let put_image_data_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let img_data = args.first();
            let dx = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as i32;
            let dy = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as i32;
            if let Some(data_obj) = img_data.and_then(|v| v.as_object()) {
                let w_val = data_obj.get(js_string!("width"), ctx).ok();
                let h_val = data_obj.get(js_string!("height"), ctx).ok();
                let w = w_val.and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
                let h = h_val.and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
                let data_val = data_obj.get(js_string!("data"), ctx).ok();
                if let Some(arr) = data_val.and_then(|v| v.as_object().cloned()) {
                    let len = arr.get(js_string!("length"), ctx).ok()
                        .and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
                    let n = (w * h * 4).min(len);
                    let mut pixels = Vec::with_capacity(n as usize);
                    for i in 0..n {
                        let val = arr.get(i, ctx).ok().and_then(|v| v.as_number()).unwrap_or(0.0) as u8;
                        pixels.push(val);
                    }
                    let mut map = CANVAS_CONTEXTS.lock().unwrap();
                    if let Some(c) = map.get_mut(&canvas_ptr) {
                        c.put_image_data(&pixels, w, h, dx, dy);
                    }
                }
            }
            Ok(JsValue::undefined())
        })
    };
    let create_image_data_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let w = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
            let h = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
            let map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get(&canvas_ptr) {
                let (_data, iw, ih) = c.create_image_data(w, h);
                let arr = boa_engine::object::builtins::JsArray::new(ctx);
                let img_data = ObjectInitializer::new(ctx)
                    .property(js_string!("data"), JsValue::from(arr), Attribute::all())
                    .property(js_string!("width"), JsValue::from(iw as i32), Attribute::all())
                    .property(js_string!("height"), JsValue::from(ih as i32), Attribute::all())
                    .build();
                Ok(JsValue::from(img_data))
            } else {
                Ok(JsValue::null())
            }
        })
    };
    let stroke_text_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let x = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let max_w = args.get(3).and_then(|v| v.as_number()).map(|n| n as f32);
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.stroke_text(&text, x, y, max_w); }
            Ok(JsValue::undefined())
        })
    };
    let measure_text_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let text = args.first()
                .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                .unwrap_or(Ok(String::new()))?;
            let map = CANVAS_CONTEXTS.lock().unwrap();
            let width = map.get(&canvas_ptr).map(|c| c.measure_text(&text)).unwrap_or(0.0);
            Ok(JsValue::from(ObjectInitializer::new(ctx)
                .property(js_string!("width"), JsValue::from(width), Attribute::all())
                .build()))
        })
    };
    let bezier_curve_to_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let cp1x = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let cp1y = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let cp2x = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let cp2y = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let x = args.get(4).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(5).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y); }
            Ok(JsValue::undefined())
        })
    };
    let quadratic_curve_to_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let cpx = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let cpy = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let x = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.quadratic_curve_to(cpx, cpy, x, y); }
            Ok(JsValue::undefined())
        })
    };
    let draw_image_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let img_obj = args.first().and_then(|v| v.as_object());
            let dx = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let dy = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            if let Some(img) = img_obj {
                let w_val = img.get(js_string!("width"), ctx).ok();
                let h_val = img.get(js_string!("height"), ctx).ok();
                let data_val = img.get(js_string!("data"), ctx).ok();
                let w = w_val.and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
                let h = h_val.and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
                if let Some(data_arr) = data_val.and_then(|v| v.as_object().cloned()) {
                    let len = data_arr.get(js_string!("length"), ctx).ok()
                        .and_then(|v| v.as_number()).unwrap_or(0.0) as u32;
                    let n = (w * h * 4).min(len);
                    let mut pixels = Vec::with_capacity(n as usize);
                    for i in 0..n {
                        let val = data_arr.get(i, ctx).ok().and_then(|v| v.as_number()).unwrap_or(0.0) as u8;
                        pixels.push(val);
                    }
                    let dw = args.get(3).and_then(|v| v.as_number()).map(|n| n as f32);
                    let dh = args.get(4).and_then(|v| v.as_number()).map(|n| n as f32);
                    let mut map = CANVAS_CONTEXTS.lock().unwrap();
                    if let Some(c) = map.get_mut(&canvas_ptr) {
                        c.draw_image(&pixels, w, h, dx, dy, dw, dh);
                    }
                }
            }
            Ok(JsValue::undefined())
        })
    };
    let clip_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.clip(); }
            Ok(JsValue::undefined())
        })
    };

    let arc_to_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, _ctx| {
            let x1 = args.first().and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y1 = args.get(1).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let x2 = args.get(2).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let y2 = args.get(3).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let r = args.get(4).and_then(|v| v.as_number()).unwrap_or(0.0) as f32;
            let mut map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get_mut(&canvas_ptr) { c.arc_to(x1, y1, x2, y2, r); }
            Ok(JsValue::undefined())
        })
    };

    // ── Accessor properties (wire JS setter to Rust state) ──
    let cp = canvas_ptr;
    let fill_style_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp).map(|c| JsValue::from(js_string!(c.get_fill_style().as_str()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let fill_style_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp) { c.set_fill_style(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let stroke_style_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp).map(|c| JsValue::from(js_string!(c.get_stroke_style().as_str()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let stroke_style_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp) { c.set_stroke_style(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let line_width_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp).map(|c| JsValue::from(c.get_line_width() as f64)).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let line_width_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_number()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp) { c.set_line_width(val as f32); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let global_alpha_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp).map(|c| JsValue::from(c.get_global_alpha() as f64)).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let global_alpha_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_number()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp) { c.set_global_alpha(val as f32); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let font_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp).map(|c| JsValue::from(js_string!(c.get_font().as_str()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let font_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp) { c.set_font(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let text_align_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp).map(|c| JsValue::from(js_string!(c.get_text_align()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let text_align_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp) { c.set_text_align(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let text_baseline_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp).map(|c| JsValue::from(js_string!(c.get_text_baseline()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let text_baseline_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp) { c.set_text_baseline(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };

    // ── New accessors for lineCap, lineJoin, miterLimit ──
    let cp_lc = canvas_ptr;
    let line_cap_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_lc).map(|c| JsValue::from(js_string!(c.get_line_cap()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let line_cap_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_lc) { c.set_line_cap(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let cp_lj = canvas_ptr;
    let line_join_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_lj).map(|c| JsValue::from(js_string!(c.get_line_join()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let line_join_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_lj) { c.set_line_join(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let cp_ml = canvas_ptr;
    let miter_limit_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_ml).map(|c| JsValue::from(c.get_miter_limit() as f64)).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let miter_limit_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_number()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_ml) { c.set_miter_limit(val as f32); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };

    // ── Shadow accessors ──
    let cp_sc = canvas_ptr;
    let shadow_color_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_sc).map(|c| JsValue::from(js_string!(c.get_shadow_color().as_str()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let shadow_color_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_sc) { c.set_shadow_color(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let cp_sb = canvas_ptr;
    let shadow_blur_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_sb).map(|c| JsValue::from(c.get_shadow_blur() as f64)).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let shadow_blur_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_number()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_sb) { c.set_shadow_blur(val as f32); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let cp_sox = canvas_ptr;
    let shadow_offset_x_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_sox).map(|c| JsValue::from(c.get_shadow_offset_x() as f64)).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let shadow_offset_x_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_number()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_sox) { c.set_shadow_offset_x(val as f32); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let cp_soy = canvas_ptr;
    let shadow_offset_y_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_soy).map(|c| JsValue::from(c.get_shadow_offset_y() as f64)).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let shadow_offset_y_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_number()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_soy) { c.set_shadow_offset_y(val as f32); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };

    // ── globalCompositeOperation ──
    let cp_gco = canvas_ptr;
    let gco_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_gco).map(|c| JsValue::from(js_string!(c.get_global_composite_operation()))).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let gco_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(val) = args.first() {
                let s = val.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_gco) { c.set_global_composite_operation(&s); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };

    // ── imageSmoothingEnabled ──
    let cp_ise = canvas_ptr;
    let ise_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_ise).map(|c| JsValue::from(c.get_image_smoothing_enabled())).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let ise_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_boolean()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_ise) { c.set_image_smoothing_enabled(val); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };

    // ── lineDashOffset ──
    let cp_ldo = canvas_ptr;
    let line_dash_offset_getter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, _args, _ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            Ok(map.get(&cp_ldo).map(|c| JsValue::from(c.get_line_dash_offset() as f64)).unwrap_or(JsValue::undefined()))
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };
    let line_dash_offset_setter = {
        let f = unsafe { NativeFunction::from_closure(move |_this, args, _ctx| {
            if let Some(val) = args.first().and_then(|v| v.as_number()) {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_ldo) { c.set_line_dash_offset(val as f32); }
            }
            Ok(JsValue::undefined())
        })};
        FunctionObjectBuilder::new(ctx.realm(), f).build()
    };

    // ── setLineDash / getLineDash ──
    let cp_sld = canvas_ptr;
    let set_line_dash_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            if let Some(arr_val) = args.first().and_then(|v| v.as_object()) {
                let len_val = arr_val.get(js_string!("length"), ctx).ok()
                    .and_then(|v| v.as_number()).unwrap_or(0.0) as usize;
                let mut dash = Vec::with_capacity(len_val);
                for i in 0..len_val {
                    if let Ok(v) = arr_val.get(i, ctx) {
                        if let Some(n) = v.as_number() {
                            dash.push(n as f32);
                        }
                    }
                }
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_sld) { c.set_line_dash(dash); }
            } else {
                let mut map = CANVAS_CONTEXTS.lock().unwrap();
                if let Some(c) = map.get_mut(&cp_sld) { c.set_line_dash(Vec::new()); }
            }
            Ok(JsValue::undefined())
        })
    };
    let cp_gld = canvas_ptr;
    let get_line_dash_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let map = CANVAS_CONTEXTS.lock().unwrap();
            if let Some(c) = map.get(&cp_gld) {
                let dash = c.get_line_dash();
                let arr = boa_engine::object::builtins::JsArray::new(ctx);
                for (i, v) in dash.iter().enumerate() {
                    let _ = arr.set(i, JsValue::from(*v as f64), false, ctx);
                }
                Ok(JsValue::from(arr))
            } else {
                let arr = boa_engine::object::builtins::JsArray::new(ctx);
                Ok(JsValue::from(arr))
            }
        })
    };

    let ctx_obj = ObjectInitializer::new(ctx)
        .accessor(js_string!("fillStyle"), Some(fill_style_getter), Some(fill_style_setter), Attribute::all())
        .accessor(js_string!("strokeStyle"), Some(stroke_style_getter), Some(stroke_style_setter), Attribute::all())
        .accessor(js_string!("lineWidth"), Some(line_width_getter), Some(line_width_setter), Attribute::all())
        .accessor(js_string!("lineCap"), Some(line_cap_getter), Some(line_cap_setter), Attribute::all())
        .accessor(js_string!("lineJoin"), Some(line_join_getter), Some(line_join_setter), Attribute::all())
        .accessor(js_string!("miterLimit"), Some(miter_limit_getter), Some(miter_limit_setter), Attribute::all())
        .accessor(js_string!("globalAlpha"), Some(global_alpha_getter), Some(global_alpha_setter), Attribute::all())
        .accessor(js_string!("globalCompositeOperation"), Some(gco_getter), Some(gco_setter), Attribute::all())
        .accessor(js_string!("font"), Some(font_getter), Some(font_setter), Attribute::all())
        .accessor(js_string!("textAlign"), Some(text_align_getter), Some(text_align_setter), Attribute::all())
        .accessor(js_string!("textBaseline"), Some(text_baseline_getter), Some(text_baseline_setter), Attribute::all())
        .accessor(js_string!("shadowColor"), Some(shadow_color_getter), Some(shadow_color_setter), Attribute::all())
        .accessor(js_string!("shadowBlur"), Some(shadow_blur_getter), Some(shadow_blur_setter), Attribute::all())
        .accessor(js_string!("shadowOffsetX"), Some(shadow_offset_x_getter), Some(shadow_offset_x_setter), Attribute::all())
        .accessor(js_string!("shadowOffsetY"), Some(shadow_offset_y_getter), Some(shadow_offset_y_setter), Attribute::all())
        .accessor(js_string!("lineDashOffset"), Some(line_dash_offset_getter), Some(line_dash_offset_setter), Attribute::all())
        .accessor(js_string!("imageSmoothingEnabled"), Some(ise_getter), Some(ise_setter), Attribute::all())
        .function(fill_rect_fn, js_string!("fillRect"), 4)
        .function(clear_rect_fn, js_string!("clearRect"), 4)
        .function(fill_text_fn, js_string!("fillText"), 4)
        .function(save_fn, js_string!("save"), 0)
        .function(restore_fn, js_string!("restore"), 0)
        .function(translate_fn, js_string!("translate"), 2)
        .function(rotate_fn, js_string!("rotate"), 1)
        .function(scale_fn, js_string!("scale"), 2)
        .function(set_transform_fn, js_string!("setTransform"), 6)
        .function(begin_path_fn, js_string!("beginPath"), 0)
        .function(close_path_fn, js_string!("closePath"), 0)
        .function(move_to_fn, js_string!("moveTo"), 2)
        .function(line_to_fn, js_string!("lineTo"), 2)
        .function(arc_fn, js_string!("arc"), 6)
        .function(rect_fn, js_string!("rect"), 4)
        .function(fill_fn, js_string!("fill"), 0)
        .function(stroke_fn, js_string!("stroke"), 0)
        .function(get_image_data_fn, js_string!("getImageData"), 4)
        .function(put_image_data_fn, js_string!("putImageData"), 3)
        .function(create_image_data_fn, js_string!("createImageData"), 2)
        .function(stroke_text_fn, js_string!("strokeText"), 4)
        .function(measure_text_fn, js_string!("measureText"), 1)
        .function(bezier_curve_to_fn, js_string!("bezierCurveTo"), 6)
        .function(quadratic_curve_to_fn, js_string!("quadraticCurveTo"), 4)
        .function(draw_image_fn, js_string!("drawImage"), 5)
        .function(clip_fn, js_string!("clip"), 0)
        .function(arc_to_fn, js_string!("arcTo"), 5)
        .function(set_line_dash_fn, js_string!("setLineDash"), 1)
        .function(get_line_dash_fn, js_string!("getLineDash"), 0)
        .build();

    Ok(JsValue::from(ctx_obj))
}

fn parse_listener_options(options: Option<&JsValue>, ctx: &mut Context) -> (bool, bool) {
    let Some(val) = options else { return (false, false); };
    if val.is_null() || val.is_undefined() { return (false, false); }

    // If it's a boolean, it's the `capture` flag
    if let Some(b) = val.as_boolean() {
        return (b, false);
    }

    // If it's an object, read .capture and .once
    if let Some(obj) = val.as_object() {
        let capture = obj.get(js_string!("capture"), ctx)
            .ok()
            .and_then(|v| v.as_boolean())
            .unwrap_or(false);
        let once = obj.get(js_string!("once"), ctx)
            .ok()
            .and_then(|v| v.as_boolean())
            .unwrap_or(false);
        return (capture, once);
    }

    (false, false)
}

/// Register the FormData constructor in the JS global scope.
/// FormData provides an in-memory key-value store for form submission data.
fn register_form_data(context: &mut Context) -> JsResult<()> {
    // Thread-local storage for FormData entries (keyed by instance id)
    thread_local! {
        static FORM_DATA_STORE: std::cell::RefCell<HashMap<u64, Vec<(String, String)>>> =
            std::cell::RefCell::new(HashMap::new());
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_FD_ID: AtomicU64 = AtomicU64::new(1);

    let fd_ctor = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let id = NEXT_FD_ID.fetch_add(1, Ordering::Relaxed);
            FORM_DATA_STORE.with(|store| {
                store.borrow_mut().insert(id, Vec::new());
            });

            let id_append = id;
            let append_fn = {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let value = args.get(1)
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    FORM_DATA_STORE.with(|store| {
                        if let Some(entries) = store.borrow_mut().get_mut(&id_append) {
                            entries.push((name, value));
                        }
                    });
                    Ok(JsValue::undefined())
                })
            };

            let id_get = id;
            let get_fn = {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let val = FORM_DATA_STORE.with(|store| {
                        let store = store.borrow();
                        store.get(&id_get)
                            .and_then(|entries| entries.iter().find(|(k, _)| k == &name))
                            .map(|(_, v)| v.clone())
                    });
                    match val {
                        Some(v) => Ok(JsValue::from(JsString::from(v.as_str()))),
                        None => Ok(JsValue::null()),
                    }
                })
            };

            let id_getall = id;
            let get_all_fn = {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let values: Vec<String> = FORM_DATA_STORE.with(|store| {
                        let store = store.borrow();
                        store.get(&id_getall)
                            .map(|entries| entries.iter()
                                .filter(|(k, _)| k == &name)
                                .map(|(_, v)| v.clone())
                                .collect())
                            .unwrap_or_default()
                    });
                    let arr = boa_engine::object::builtins::JsArray::new(ctx);
                    for v in &values {
                        let _ = arr.push(JsValue::from(JsString::from(v.as_str())), ctx);
                    }
                    Ok(JsValue::from(arr))
                })
            };

            let id_has = id;
            let has_fn = {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let found = FORM_DATA_STORE.with(|store| {
                        let store = store.borrow();
                        store.get(&id_has)
                            .map(|entries| entries.iter().any(|(k, _)| k == &name))
                            .unwrap_or(false)
                    });
                    Ok(JsValue::from(found))
                })
            };

            let id_delete = id;
            let delete_fn = {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    FORM_DATA_STORE.with(|store| {
                        if let Some(entries) = store.borrow_mut().get_mut(&id_delete) {
                            entries.retain(|(k, _)| k != &name);
                        }
                    });
                    Ok(JsValue::undefined())
                })
            };

            let id_set = id;
            let set_fn = {
                NativeFunction::from_closure(move |_this, args, ctx| {
                    let name = args.first()
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    let value = args.get(1)
                        .map(|v| v.to_string(ctx).map(|s| s.to_std_string_escaped()))
                        .unwrap_or(Ok(String::new()))?;
                    FORM_DATA_STORE.with(|store| {
                        if let Some(entries) = store.borrow_mut().get_mut(&id_set) {
                            entries.retain(|(k, _)| k != &name);
                            entries.push((name, value));
                        }
                    });
                    Ok(JsValue::undefined())
                })
            };

            let obj = ObjectInitializer::new(ctx)
                .function(append_fn, js_string!("append"), 2)
                .function(get_fn, js_string!("get"), 1)
                .function(get_all_fn, js_string!("getAll"), 1)
                .function(has_fn, js_string!("has"), 1)
                .function(delete_fn, js_string!("delete"), 1)
                .function(set_fn, js_string!("set"), 2)
                .build();

            Ok(JsValue::from(obj))
        })
    };

    let global = context.global_object();
    let fd_val: JsValue = FunctionObjectBuilder::new(context.realm(), fd_ctor)
        .build()
        .into();
    global.set(js_string!("FormData"), fd_val, false, context)?;
    Ok(())
}
