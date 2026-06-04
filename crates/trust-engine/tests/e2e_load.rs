use trust_engine::TrustEngine;
use trust_engine::Viewport;

#[test]
fn e2e_render_html_returns_display_list_and_title() {
    let engine = TrustEngine::offline();
    let viewport = Viewport { width: 800.0, height: 600.0 };

    let html = "<html><head><title>End-to-End Test</title></head><body><h1>Hello E2E</h1></body></html>";
    let page = engine.render_html("https://example.test/", html, &[], viewport).expect("render_html succeeded");

    // Assert we produced a non-empty display list
    assert!(!page.rendered.display_list.is_empty(), "display list should not be empty");

    // Assert title was extracted (or provided by JS) correctly
    assert_eq!(page.title, "End-to-End Test");
}
