#![cfg(feature = "javascript")]

use trust_engine::{TrustEngine, Viewport};

#[test]
fn inline_script_runs_and_renders() {
    let engine = TrustEngine::offline();

    let html = r#"
    <!doctype html>
    <html>
      <head><title>Test</title></head>
      <body>
        <script>
          // simple inline script that does nothing harmful
          console.log('hello from inline script');
        </script>
        <p>content</p>
      </body>
    </html>
    "#;

    let viewport = Viewport { width: 800.0, height: 600.0 };

    let page = engine.render_html("about:test", html, &[], viewport).expect("render");
    // verify we produced a non-empty rendered page
    assert!(!page.rendered.display_list.is_empty(), "expected display commands");
}
