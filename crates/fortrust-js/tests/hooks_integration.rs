use std::sync::{Arc, Mutex};

use fortrust_dom::DomArena;
use fortrust_js::JsRuntime;

#[test]
fn title_hook_invoked_on_document_settitle() {
    // Create a minimal DOM document
    let arena = DomArena::new();
    let html = "<!doctype html><html><head><title>orig</title></head><body></body></html>";
    let document = fortrust_dom::parse_html(&arena, html).expect("parse html");
    let static_doc: &'static fortrust_dom::Document<'static> = unsafe { &*(&document as *const _ as *const _) };

    // Prepare JS runtime
    let mut js = JsRuntime::new();
    js.initialize(&mut fortrust_js::EventLoop::new()).expect("init");

    // Install a title handler
    let got = Arc::new(Mutex::new(None::<String>));
    let got_clone = got.clone();
    fortrust_js::set_title_handler(Some(std::sync::Arc::new(move |title: String| {
        let mut g = got_clone.lock().unwrap();
        *g = Some(title);
    })));

    // Attach document and run JS to set title
    js.attach_document(static_doc).expect("attach doc");
    let _ = js.eval("document.setTitle('from-js')").expect("eval");

    // Check hook was invoked
    let result = got.lock().unwrap().clone();
    assert_eq!(result, Some("from-js".to_string()));
}
