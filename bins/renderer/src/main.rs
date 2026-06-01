#![allow(dead_code, unused_imports, unused_variables)]

use fortrust_core::TabId;
use fortrust_dom::DomArena;
use fortrust_ipc::{BincodeCodec, BrowserToRenderer, RendererToBrowser};
use fortrust_js::{EventLoop, JsRuntime, WebApiRegistry, new_shared_runtime};
use fortrust_layout::{LayoutConstraints, LayoutEngine};
use fortrust_net::NetworkClient;
use fortrust_paint::{DisplayCommand, PaintOptions, Painter};
use fortrust_renderer::{RenderedPage, StaticRenderer};
use fortrust_style::{StyleEngine, Stylesheet};
use tracing::{debug, error, info, warn};

struct RendererInstance {
    tab_id: TabId,
    url: String,
    last_page: Option<RenderedPage>,
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
            last_page: None,
            dom_arena: DomArena::new(),
            js_runtime: None,
            event_loop: None,
            renderer: StaticRenderer::new(),
            network: None,
            width,
            height,
        }
    }

    fn navigate(&mut self, url: &str, html: &str) -> Vec<RendererToBrowser> {
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

        let mut events = vec![
            RendererToBrowser::NavigationStart { url: url.to_owned() },
            RendererToBrowser::LoadProgress { percent: 0.15, state: fortrust_ipc::LoadState::Parsing },
        ];

        let document = match fortrust_dom::parse_html(&self.dom_arena, &html_source) {
            Ok(document) => document,
            Err(error) => {
                events.push(RendererToBrowser::NavigationError {
                    url: url.to_owned(),
                    error: format!("Parse failed: {error:?}"),
                });
                return events;
            }
        };

        events.push(RendererToBrowser::LoadProgress { percent: 0.45, state: fortrust_ipc::LoadState::Layout });

        let rendered = match self.renderer.render(
            &html_source,
            &[],
            fortrust_renderer::Viewport {
                width: self.width as f32,
                height: self.height as f32,
            },
        ) {
            Ok(page) => page,
            Err(error) => {
                events.push(RendererToBrowser::NavigationError {
                    url: url.to_owned(),
                    error: format!("Render failed: {error:?}"),
                });
                return events;
            }
        };

        self.last_page = Some(rendered.clone());
        events.push(RendererToBrowser::LoadProgress { percent: 0.8, state: fortrust_ipc::LoadState::Painting });

        let title = document
            .first_element_by_tag("title")
            .map(|title_node| title_node.text_content())
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| {
                if rendered.text_content.len() > 80 {
                    rendered.text_content[..80].to_owned()
                } else {
                    rendered.text_content.clone()
                }
            });

        let frame = render_page_frame(&rendered, self.width, self.height, &title);
        events.push(RendererToBrowser::TitleChanged { title });
        events.push(RendererToBrowser::FrameReady {
            texture_data: frame,
            width: self.width,
            height: self.height,
            stride: self.width.saturating_mul(4),
        });
        events.push(RendererToBrowser::LoadProgress { percent: 1.0, state: fortrust_ipc::LoadState::Loaded });
        events.push(RendererToBrowser::LoadComplete);
        events
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }
}

fn render_page_frame(page: &RenderedPage, width: u32, height: u32, title: &str) -> Vec<u8> {
    let width = width.max(1) as usize;
    let height = height.max(1) as usize;
    let mut pixels = vec![0u8; width * height * 4];

    for y in 0..height {
        let t = y as f32 / height as f32;
        for x in 0..width {
            let idx = (y * width + x) * 4;
            pixels[idx] = (13.0 + 20.0 * t) as u8;
            pixels[idx + 1] = (16.0 + 24.0 * t) as u8;
            pixels[idx + 2] = (20.0 + 30.0 * t) as u8;
            pixels[idx + 3] = 255;
        }
    }

    let mut clip_stack = vec![fortrust_layout::Rect { x: 0.0, y: 0.0, width: width as f32, height: height as f32 }];

    for command in page.display_list.commands() {
        match command {
            DisplayCommand::ClipPush(rect) => {
                let next = intersect_rect(*clip_stack.last().unwrap(), *rect);
                clip_stack.push(next);
            }
            DisplayCommand::ClipPop => {
                if clip_stack.len() > 1 {
                    clip_stack.pop();
                }
            }
            DisplayCommand::FillRect { rect, color } => {
                paint_rect(&mut pixels, width, height, clip_stack.last().copied().unwrap(), *rect, color_to_rgba(*color));
            }
            DisplayCommand::DrawBorder { rect, top_width, right_width, bottom_width, left_width, top_color, right_color, bottom_color, left_color, .. } => {
                let clip = clip_stack.last().copied().unwrap();
                paint_rect(&mut pixels, width, height, clip, fortrust_layout::Rect { x: rect.x, y: rect.y, width: rect.width, height: *top_width }, color_to_rgba(*top_color));
                paint_rect(&mut pixels, width, height, clip, fortrust_layout::Rect { x: rect.x, y: rect.y + rect.height - *bottom_width, width: rect.width, height: *bottom_width }, color_to_rgba(*bottom_color));
                paint_rect(&mut pixels, width, height, clip, fortrust_layout::Rect { x: rect.x, y: rect.y, width: *left_width, height: rect.height }, color_to_rgba(*left_color));
                paint_rect(&mut pixels, width, height, clip, fortrust_layout::Rect { x: rect.x + rect.width - *right_width, y: rect.y, width: *right_width, height: rect.height }, color_to_rgba(*right_color));
            }
            DisplayCommand::DrawBoxShadow { rect, offset_x, offset_y, blur, color, .. } => {
                let shadow_rect = fortrust_layout::Rect { x: rect.x + *offset_x, y: rect.y + *offset_y, width: rect.width + blur * 0.25, height: rect.height + blur * 0.25 };
                paint_rect(&mut pixels, width, height, clip_stack.last().copied().unwrap(), shadow_rect, color_to_rgba(*color));
            }
            DisplayCommand::DrawOutline { rect, width: outline_width, color, .. } => {
                let clip = clip_stack.last().copied().unwrap();
                let outer = fortrust_layout::Rect { x: rect.x - *outline_width, y: rect.y - *outline_width, width: rect.width + *outline_width * 2.0, height: rect.height + *outline_width * 2.0 };
                paint_rect(&mut pixels, width, height, clip, outer, color_to_rgba(*color));
            }
            DisplayCommand::DrawText { rect, text, color, .. } => {
                let clip = clip_stack.last().copied().unwrap();
                paint_text_block(&mut pixels, width, height, clip, *rect, text, color_to_rgba(*color));
            }
            DisplayCommand::DrawImage { rect, image_id, natural_width, natural_height, alt } => {
                let clip = clip_stack.last().copied().unwrap();
                if let Some(img) = page.images.get(*image_id) {
                    paint_image(&mut pixels, width, height, clip, *rect, img, *natural_width, *natural_height);
                } else if !alt.is_empty() {
                    paint_text_block(&mut pixels, width, height, clip, *rect, &format!("[image: {alt}]"), [160, 160, 160, 255]);
                } else {
                    paint_rect(&mut pixels, width, height, clip, *rect, [40, 44, 52, 255]);
                }
            }
        }
    }

    let header = format!("{title}  •  {} commands", page.display_list.len());
    paint_title_banner(&mut pixels, width, height, &header);
    pixels
}

fn generate_status_frame(width: u32, height: u32, label: &str) -> Vec<u8> {
    let width = width.max(1) as usize;
    let height = height.max(1) as usize;
    let mut pixels = vec![0u8; width * height * 4];

    for y in 0..height {
        let t = y as f32 / height as f32;
        for x in 0..width {
            let idx = (y * width + x) * 4;
            pixels[idx] = (14.0 + 16.0 * t) as u8;
            pixels[idx + 1] = (17.0 + 20.0 * t) as u8;
            pixels[idx + 2] = (22.0 + 26.0 * t) as u8;
            pixels[idx + 3] = 255;
        }
    }

    paint_title_banner(&mut pixels, width, height, label);
    pixels
}

fn paint_title_banner(pixels: &mut [u8], width: usize, height: usize, label: &str) {
    let banner_height = (height as f32 * 0.14).max(24.0) as usize;
    for y in 0..banner_height.min(height) {
        for x in 0..width {
            let idx = (y * width + x) * 4;
            pixels[idx] = 28;
            pixels[idx + 1] = 33;
            pixels[idx + 2] = 40;
            pixels[idx + 3] = 255;
        }
    }

    let text_width = label.len().saturating_mul(7).min(width.saturating_sub(16));
    let start_x = 8usize.min(width.saturating_sub(1));
    let start_y = (banner_height / 2).saturating_sub(6);
    for y in 0..12.min(height) {
        for x in 0..text_width {
            let idx = ((start_y + y).min(height.saturating_sub(1)) * width + (start_x + x).min(width.saturating_sub(1))) * 4;
            pixels[idx] = 77;
            pixels[idx + 1] = 159;
            pixels[idx + 2] = 255;
            pixels[idx + 3] = 255;
        }
    }
}

fn paint_rect(pixels: &mut [u8], width: usize, height: usize, clip: fortrust_layout::Rect, rect: fortrust_layout::Rect, rgba: [u8; 4]) {
    let x0 = rect.x.max(clip.x).floor().max(0.0) as usize;
    let y0 = rect.y.max(clip.y).floor().max(0.0) as usize;
    let x1 = (rect.x + rect.width).min(clip.x + clip.width).ceil().max(0.0) as usize;
    let y1 = (rect.y + rect.height).min(clip.y + clip.height).ceil().max(0.0) as usize;

    for y in y0.min(height)..y1.min(height) {
        for x in x0.min(width)..x1.min(width) {
            let idx = (y * width + x) * 4;
            blend_rgba(&mut pixels[idx..idx + 4], rgba);
        }
    }
}

fn paint_text_block(pixels: &mut [u8], width: usize, height: usize, clip: fortrust_layout::Rect, rect: fortrust_layout::Rect, text: &str, rgba: [u8; 4]) {
    let char_width = (rect.width / text.chars().count().max(1) as f32).max(4.0);
    for (index, _) in text.chars().enumerate() {
        let char_rect = fortrust_layout::Rect {
            x: rect.x + index as f32 * char_width,
            y: rect.y,
            width: (char_width * 0.72).max(3.0),
            height: rect.height.max(8.0),
        };
        paint_rect(pixels, width, height, clip, char_rect, rgba);
    }
}

/// Render a decoded image into the given rect, scaling bilinearly. The image
/// data lives in `image.rgba` as 4 bytes per pixel in (R, G, B, A) order.
fn paint_image(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    clip: fortrust_layout::Rect,
    rect: fortrust_layout::Rect,
    image: &fortrust_core::DecodedImage,
    natural_width: u32,
    natural_height: u32,
) {
    if natural_width == 0 || natural_height == 0 || image.rgba.is_empty() {
        paint_rect(pixels, width, height, clip, rect, [40, 44, 52, 255]);
        return;
    }

    let x0 = rect.x.max(clip.x).floor().max(0.0) as usize;
    let y0 = rect.y.max(clip.y).floor().max(0.0) as usize;
    let x1 = (rect.x + rect.width).min(clip.x + clip.width).ceil().max(0.0) as usize;
    let y1 = (rect.y + rect.height).min(clip.y + clip.height).ceil().max(0.0) as usize;
    if x1 <= x0 || y1 <= y0 { return; }

    let dst_w = x1.saturating_sub(x0);
    let dst_h = y1.saturating_sub(y0);
    let src_w = natural_width as usize;
    let src_h = natural_height as usize;

    for dy in 0..dst_h {
        let py = y0 + dy;
        if py >= height { break; }
        // Map dst pixel y -> src pixel y (with sub-pixel sampling -> nearest)
        let sy = ((dy as f32) / dst_h as f32 * src_h as f32) as usize;
        let sy = sy.min(src_h - 1);
        for dx in 0..dst_w {
            let px = x0 + dx;
            if px >= width { break; }
            let sx = ((dx as f32) / dst_w as f32 * src_w as f32) as usize;
            let sx = sx.min(src_w - 1);
            let src_idx = (sy * src_w + sx) * 4;
            if src_idx + 4 > image.rgba.len() { continue; }
            let src = [
                image.rgba[src_idx],
                image.rgba[src_idx + 1],
                image.rgba[src_idx + 2],
                image.rgba[src_idx + 3],
            ];
            let dst_idx = (py * width + px) * 4;
            blend_rgba(&mut pixels[dst_idx..dst_idx + 4], src);
        }
    }
}

fn intersect_rect(a: fortrust_layout::Rect, b: fortrust_layout::Rect) -> fortrust_layout::Rect {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.width).min(b.x + b.width);
    let y2 = (a.y + a.height).min(b.y + b.height);
    if x2 <= x1 || y2 <= y1 {
        return fortrust_layout::Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
    }
    fortrust_layout::Rect { x: x1, y: y1, width: x2 - x1, height: y2 - y1 }
}

fn color_to_rgba(color: fortrust_style::Color) -> [u8; 4] {
    [color.r, color.g, color.b, color.a]
}

fn blend_rgba(dst: &mut [u8], src: [u8; 4]) {
    let alpha = src[3] as f32 / 255.0;
    if alpha <= 0.0 {
        return;
    }
    let inverse = 1.0 - alpha;
    dst[0] = (src[0] as f32 * alpha + dst[0] as f32 * inverse) as u8;
    dst[1] = (src[1] as f32 * alpha + dst[1] as f32 * inverse) as u8;
    dst[2] = (src[2] as f32 * alpha + dst[2] as f32 * inverse) as u8;
    dst[3] = 255;
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

    // If the renderer is started with `--ipc-connect <ADDR>` then connect to the parent and speak IPC.
    if let Some(pos) = args.iter().position(|a| a == "--ipc-connect")
        && let Some(addr) = args.get(pos + 1) {
            info!("Renderer connecting to {addr}");
            if let Ok(stream) = tokio::net::TcpStream::connect(addr).await {
                let (sender, receiver) = fortrust_ipc::create_tcp_endpoint(stream);

                // Forward JS document title changes from the in-process JS runtime
                // to the browser via the IPC sender.
                {
                    use std::sync::Arc;
                    let sender_clone = sender.clone();
                    fortrust_js::set_title_handler(Some(Arc::new(move |title: String| {
                        let s = sender_clone.clone();
                        // spawn a tokio task to send the IPC message asynchronously
                        tokio::spawn(async move {
                            let _ = s.send_renderer_message(&fortrust_ipc::RendererToBrowser::DocumentTitleChanged {
                                title: title.clone(),
                            }).await;
                        });
                    })));
                }
                // Forward JS-dispatched events to the browser as console messages (level="event").
                {
                    use std::sync::Arc;
                    let sender_clone = sender.clone();
                    fortrust_js::set_event_handler(Some(Arc::new(move |name: String, detail: String| {
                            let s = sender_clone.clone();
                            tokio::spawn(async move {
                                let _ = s.send_renderer_message(&fortrust_ipc::RendererToBrowser::DomEvent {
                                    origin: String::new(),
                                    name: name.clone(),
                                    detail: detail.clone(),
                                }).await;
                            });
                        })));
                }

                let _renderer_thread = std::thread::spawn(move || {
                    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                        Ok(runtime) => runtime,
                        Err(_) => return,
                    };

                    rt.block_on(async move {
                        let mut renderer = RendererInstance::new(tab_id, width, height);
                        while let Ok(msg) = receiver.recv_browser_message().await {
                            let responses = match msg {
                                fortrust_ipc::BrowserToRenderer::Navigate { url } => renderer.navigate(&url, ""),
                                fortrust_ipc::BrowserToRenderer::ExecuteScript { js } => vec![
                                    fortrust_ipc::RendererToBrowser::ConsoleMessage {
                                        level: "info".into(),
                                        message: format!("executed script: {js}"),
                                    },
                                ],
                                fortrust_ipc::BrowserToRenderer::Resize { width, height } => {
                                    renderer.resize(width, height);
                                    let frame = if let Some(page) = renderer.last_page.as_ref() {
                                        render_page_frame(page, width, height, &renderer.url)
                                    } else {
                                        generate_status_frame(width, height, &format!("{} x {}", width, height))
                                    };
                                    vec![
                                        fortrust_ipc::RendererToBrowser::LoadProgress {
                                            percent: 1.0,
                                            state: fortrust_ipc::LoadState::Loaded,
                                        },
                                        fortrust_ipc::RendererToBrowser::FrameReady {
                                            texture_data: frame,
                                            width,
                                            height,
                                            stride: width.saturating_mul(4),
                                        },
                                    ]
                                }
                                fortrust_ipc::BrowserToRenderer::Reload => {
                                    if renderer.url.is_empty() {
                                        vec![fortrust_ipc::RendererToBrowser::NavigationError {
                                            url: String::new(),
                                            error: "no page loaded to reload".into(),
                                        }]
                                    } else {
                                        let current_url = renderer.url.clone();
                                        renderer.navigate(&current_url, "")
                                    }
                                }
                                fortrust_ipc::BrowserToRenderer::Shutdown => {
                                    let _ = sender.send_renderer_message(&fortrust_ipc::RendererToBrowser::ShutdownAck).await;
                                    break;
                                }
                                fortrust_ipc::BrowserToRenderer::GoBack
                                | fortrust_ipc::BrowserToRenderer::GoForward
                                | fortrust_ipc::BrowserToRenderer::Stop
                                | fortrust_ipc::BrowserToRenderer::KeyEvent { .. }
                                | fortrust_ipc::BrowserToRenderer::MouseEvent { .. }
                                | fortrust_ipc::BrowserToRenderer::ZoomChange { .. }
                                | fortrust_ipc::BrowserToRenderer::ScrollTo { .. }
                                | fortrust_ipc::BrowserToRenderer::SetPrivacySettings { .. } => vec![
                                    fortrust_ipc::RendererToBrowser::ConsoleMessage {
                                        level: "debug".into(),
                                        message: "input event received".into(),
                                    },
                                ],
                            };

                            for response in responses {
                                let _ = sender.send_renderer_message(&response).await;
                            }
                        }
                    });
                });
            }
    }

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
