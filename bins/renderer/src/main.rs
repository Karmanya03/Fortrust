#![allow(dead_code, unused_imports, unused_variables)]

use fortrust_core::TabId;
use fortrust_dom::DomArena;
use fortrust_ipc::{BincodeCodec, BrowserToRenderer, RendererToBrowser};
use fortrust_js::{EventLoop, JsRuntime, WebApiRegistry, new_shared_runtime};
use fortrust_layout::{LayoutConstraints, LayoutEngine};
use fortrust_net::NetworkClient;
use fortrust_paint::{PaintOptions, Painter};
use fortrust_renderer::StaticRenderer;
use fortrust_style::{StyleEngine, Stylesheet};
use tracing::{debug, error, info, warn};

struct RendererInstance {
    tab_id: TabId,
    url: String,
    dom_arena: DomArena,
    js_runtime: Option<fortrust_js::SharedJsRuntime>,
    event_loop: Option<EventLoop>,
    renderer: StaticRenderer,
    network: Option<NetworkClient>,
    width: u32,
    height: u32,
}

impl RendererInstance {
    fn new(tab_id: TabId, width: u32, height: u32) -> Self {
        Self {
            tab_id,
            url: String::new(),
            dom_arena: DomArena::new(),
            js_runtime: None,
            event_loop: None,
            renderer: StaticRenderer::new(),
            network: None,
            width,
            height,
        }
    }

    fn navigate(&mut self, url: &str, html: &str) -> RendererToBrowser {
        self.url = url.to_owned();
        self.dom_arena = DomArena::new();

        let html_source = if html.is_empty() {
            format!(
                "<!doctype html><html><head><title>{}</title></head><body><p>Loading {url}...</p></body></html>",
                url
            )
        } else {
            html.to_owned()
        };

        match fortrust_dom::parse_html(&self.dom_arena, &html_source) {
            Ok(document) => {
                let _viewport = LayoutConstraints {
                    viewport_width: self.width as f32,
                    viewport_height: self.height as f32,
                    containing_block: None,
                };

                let _style_engine = StyleEngine::new();
                if let Some(title_node) = document.first_element_by_tag("title") {
                    let title = title_node.text_content();
                    return RendererToBrowser::TitleChanged { title };
                }

                match self.renderer.render(
                    &html_source,
                    &[],
                    fortrust_renderer::Viewport {
                        width: self.width as f32,
                        height: self.height as f32,
                    },
                ) {
                    Ok(page) => {
                        let title = if page.text_content.len() > 80 {
                            page.text_content[..80].to_owned()
                        } else {
                            page.text_content
                        };
                        RendererToBrowser::TitleChanged { title }
                    }
                    Err(error) => RendererToBrowser::NavigationError {
                        url: url.to_owned(),
                        error: format!("Render failed: {error:?}"),
                    },
                }
            }
            Err(error) => RendererToBrowser::NavigationError {
                url: url.to_owned(),
                error: format!("Parse failed: {error:?}"),
            },
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }
}

async fn handle_renderer_commands() {
    info!("Renderer process starting");
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let args: Vec<String> = std::env::args().collect();
    let tab_id = args
        .iter()
        .position(|a| a == "--tab-id")
        .and_then(|i| args.get(i + 1))
        .and_then(|id| id.parse::<u64>().ok())
        .map(TabId)
        .unwrap_or(TabId(1));

    info!("Renderer[{tab_id:?}]: Starting");

    let width = args
        .iter()
        .position(|a| a == "--width")
        .and_then(|i| args.get(i + 1))
        .and_then(|w| w.parse::<u32>().ok())
        .unwrap_or(1280);

    let height = args
        .iter()
        .position(|a| a == "--height")
        .and_then(|i| args.get(i + 1))
        .and_then(|h| h.parse::<u32>().ok())
        .unwrap_or(720);

    let _renderer = RendererInstance::new(tab_id, width, height);

    if let Some(url) = args
        .iter()
        .position(|a| a == "--url")
        .and_then(|i| args.get(i + 1))
    {
        info!("Renderer[{tab_id:?}]: Initial URL: {url}");
    }

    tokio::signal::ctrl_c().await.ok();
    info!("Renderer[{tab_id:?}]: Shutting down");
}
