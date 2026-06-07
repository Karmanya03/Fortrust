use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

use fortrust_layout::Rect;
use fortrust_paint::{process_font_faces, DisplayCommand, TextRenderer};
use fortrust_renderer::Viewport;
use fortrust_style::{Color, FontWeight, FontStyle, Stylesheet};
use trust_engine::TrustEngine;

thread_local! {
    static TEXT_RENDERER: RefCell<TextRenderer> = RefCell::new(TextRenderer::new());
}

fn color_to_rgba(color: Color) -> [u8; 4] {
    [color.r, color.g, color.b, color.a]
}

fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x1 = a.x.max(b.x);
    let y1 = a.y.max(b.y);
    let x2 = (a.x + a.width).min(b.x + b.width);
    let y2 = (a.y + a.height).min(b.y + b.height);
    if x2 <= x1 || y2 <= y1 {
        return Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 };
    }
    Rect { x: x1, y: y1, width: x2 - x1, height: y2 - y1 }
}

fn blend_rgba(dst: &mut [u8], src: [u8; 4]) {
    let alpha = src[3] as f32 / 255.0;
    if alpha <= 0.0 { return; }
    let inverse = 1.0 - alpha;
    dst[0] = (src[0] as f32 * alpha + dst[0] as f32 * inverse) as u8;
    dst[1] = (src[1] as f32 * alpha + dst[1] as f32 * inverse) as u8;
    dst[2] = (src[2] as f32 * alpha + dst[2] as f32 * inverse) as u8;
    dst[3] = 255;
}

fn paint_rect(pixels: &mut [u8], width: usize, height: usize, clip: Rect, rect: Rect, rgba: [u8; 4]) {
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

#[allow(clippy::too_many_arguments)]
fn paint_image(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    clip: Rect,
    rect: Rect,
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

#[allow(clippy::too_many_arguments)]
fn render_text_cosmic(
    pixels: &mut [u8],
    fb_width: usize,
    fb_height: usize,
    clip: Rect,
    rect: Rect,
    text: &str,
    font_size: f32,
    font_family: &str,
    font_weight: FontWeight,
    font_style: FontStyle,
    rgba: [u8; 4],
) {
    TEXT_RENDERER.with(|tr| {
        tr.borrow_mut().render_into(
            pixels, fb_width, fb_height, clip, rect, text, font_size, font_family, font_weight, font_style, rgba,
        );
    });
}

fn rasterize_page(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    page: &trust_engine::EnginePage,
    _title: &str,
) {
    let mut clip_stack = vec![Rect { x: 0.0, y: 0.0, width: width as f32, height: height as f32 }];
    let mut transform_stack: Vec<([f32; 6], f32, f32)> = vec![([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0.0, 0.0)];

    for command in page.rendered.display_list.commands() {
        match command {
            DisplayCommand::PushTransform { matrix, origin_x, origin_y } => {
                transform_stack.push((*matrix, *origin_x, *origin_y));
            }
            DisplayCommand::PopTransform => {
                if transform_stack.len() > 1 {
                    transform_stack.pop();
                }
            }
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
                paint_rect(pixels, width, height, *clip_stack.last().unwrap(), *rect, color_to_rgba(*color));
            }
            DisplayCommand::DrawBorder { rect, top_width, right_width, bottom_width, left_width, top_color, right_color, bottom_color, left_color, .. } => {
                let clip = *clip_stack.last().unwrap();
                paint_rect(pixels, width, height, clip, Rect { x: rect.x, y: rect.y, width: rect.width, height: *top_width }, color_to_rgba(*top_color));
                paint_rect(pixels, width, height, clip, Rect { x: rect.x, y: rect.y + rect.height - *bottom_width, width: rect.width, height: *bottom_width }, color_to_rgba(*bottom_color));
                paint_rect(pixels, width, height, clip, Rect { x: rect.x, y: rect.y, width: *left_width, height: rect.height }, color_to_rgba(*left_color));
                paint_rect(pixels, width, height, clip, Rect { x: rect.x + rect.width - *right_width, y: rect.y, width: *right_width, height: rect.height }, color_to_rgba(*right_color));
            }
            DisplayCommand::DrawBoxShadow { rect, offset_x, offset_y, blur, color, .. } => {
                let shadow_rect = Rect { x: rect.x + *offset_x, y: rect.y + *offset_y, width: rect.width + blur * 0.25, height: rect.height + blur * 0.25 };
                paint_rect(pixels, width, height, *clip_stack.last().unwrap(), shadow_rect, color_to_rgba(*color));
            }
            DisplayCommand::DrawOutline { rect, width: outline_width, color, .. } => {
                let clip = *clip_stack.last().unwrap();
                let outer = Rect { x: rect.x - *outline_width, y: rect.y - *outline_width, width: rect.width + *outline_width * 2.0, height: rect.height + *outline_width * 2.0 };
                paint_rect(pixels, width, height, clip, outer, color_to_rgba(*color));
            }
            DisplayCommand::DrawText { rect, text, color, font_size_px, font_weight, font_style, font_family } => {
                let clip = *clip_stack.last().unwrap();
                if !text.is_empty() {
                    render_text_cosmic(pixels, width, height, clip, *rect, text, *font_size_px, font_family, *font_weight, *font_style, color_to_rgba(*color));
                }
            }
            DisplayCommand::DrawImage { rect, image_id, natural_width, natural_height, alt } => {
                let clip = *clip_stack.last().unwrap();
                if let Some(img) = page.rendered.images.get(*image_id) {
                    paint_image(pixels, width, height, clip, *rect, img, *natural_width, *natural_height);
                } else if !alt.is_empty() {
                    render_text_cosmic(pixels, width, height, clip, *rect, &format!("[image: {alt}]"), 12.0, "sans-serif", FontWeight::Normal, FontStyle::Normal, [160, 160, 160, 255]);
                } else {
                    paint_rect(pixels, width, height, clip, *rect, [40, 44, 52, 255]);
                }
            }
        }
    }
}

pub enum HeadlessError {
    Fetch(String),
    Render(trust_engine::EngineError),
    Image(image::ImageError),
}

impl std::fmt::Display for HeadlessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeadlessError::Fetch(msg) => write!(f, "fetch: {msg}"),
            HeadlessError::Render(e) => write!(f, "render: {e:?}"),
            HeadlessError::Image(e) => write!(f, "image: {e}"),
        }
    }
}

impl From<trust_engine::EngineError> for HeadlessError {
    fn from(e: trust_engine::EngineError) -> Self { HeadlessError::Render(e) }
}

impl From<image::ImageError> for HeadlessError {
    fn from(e: image::ImageError) -> Self { HeadlessError::Image(e) }
}

/// Discover @font-face rules embedded in the given HTML, download the font
/// files, and register them with the global TEXT_RENDERER so that subsequent
/// page rendering uses the correct custom fonts.
///
/// External stylesheets (`<link rel="stylesheet">`) are not followed by this
/// helper; only `<style>...</style>` blocks are scanned.
fn load_font_faces_from_html(base_url: &str, html: &str) {
    let mut font_faces = Vec::new();

    // Simple extraction of <style> block contents
    let mut search_start = 0usize;
    while let Some(start) = html[search_start..].find("<style") {
        // Find the end of the opening <style ...> tag
        let tag_end = html[search_start + start..].find('>').map(|i| search_start + start + i + 1).unwrap_or(html.len());
        // Find </style>
        let content_start = tag_end;
        let close_tag = html[content_start..].find("</style>").map(|i| content_start + i).unwrap_or(html.len());
        let css_text = &html[content_start..close_tag];
        if let Ok(sheet) = Stylesheet::parse(css_text) {
            font_faces.extend(sheet.font_face_rules);
        }
        search_start = close_tag + 8; // "</style>".len()
        if search_start >= html.len() {
            break;
        }
    }

    if font_faces.is_empty() {
        return;
    }

    // Resolve font URLs and download font data
    let mut font_data: HashMap<String, Vec<u8>> = HashMap::new();
    for rule in &font_faces {
        for source in &rule.sources {
            if font_data.contains_key(&source.url) {
                continue;
            }
            // Resolve relative URLs against the base page URL
            let absolute_url = if source.url.starts_with("http://") || source.url.starts_with("https://") {
                source.url.clone()
            } else if source.url.starts_with('/') {
                // Absolute path on same origin
                let origin = base_url.trim_end_matches('/').trim_end_matches('/');
                format!("{}{}", origin, source.url)
            } else if let Some(base) = base_url.strip_suffix('/') {
                format!("{}/{}", base, source.url.trim_start_matches("./"))
            } else {
                format!("{}/{}", base_url, source.url.trim_start_matches("./"))
            };

            tracing::info!("Downloading font: {absolute_url}");
            match reqwest::blocking::get(&absolute_url) {
                Ok(resp) => {
                    match resp.bytes() {
                        Ok(bytes) => {
                            let data = bytes.to_vec();
                            tracing::info!("  → {} bytes loaded", data.len());
                            font_data.insert(source.url.clone(), data);
                        }
                        Err(e) => tracing::warn!("  → failed to read font body: {e}"),
                    }
                }
                Err(e) => tracing::warn!("  → failed to fetch font: {e}"),
            }
        }
    }

    if !font_data.is_empty() {
        TEXT_RENDERER.with(|tr| {
            let mut renderer = tr.borrow_mut();
            process_font_faces(&mut renderer, &font_faces, &font_data);
            tracing::info!("Loaded {} @font-face fonts", font_data.len());
        });
    }
}

pub fn run(url: &str, output: &Path, viewport_width: u32, viewport_height: u32) -> Result<(), HeadlessError> {
    let viewport = Viewport { width: viewport_width as f32, height: viewport_height as f32 };

    tracing::info!("Fetching {url}...");
    let html = reqwest::blocking::get(url)
        .map_err(|e| HeadlessError::Fetch(e.to_string()))?
        .text()
        .map_err(|e| HeadlessError::Fetch(e.to_string()))?;

    tracing::info!("Rendering {} bytes of HTML...", html.len());

    // Load @font-face fonts before rendering so custom fonts are available
    load_font_faces_from_html(url, &html);

    let engine = TrustEngine::offline();
    let page = engine.render_html(url, &html, &[], viewport)?;

    let w = viewport_width.max(1) as usize;
    let h = viewport_height.max(1) as usize;
    let mut pixels = vec![0u8; w * h * 4];

    for y in 0..h {
        let t = y as f32 / h as f32;
        for x in 0..w {
            let idx = (y * w + x) * 4;
            pixels[idx] = (13.0 + 20.0 * t) as u8;
            pixels[idx + 1] = (16.0 + 24.0 * t) as u8;
            pixels[idx + 2] = (20.0 + 30.0 * t) as u8;
            pixels[idx + 3] = 255;
        }
    }

    rasterize_page(&mut pixels, w, h, &page, &page.title);

    tracing::info!("Saving PNG to {}...", output.display());
    image::save_buffer(output, &pixels, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(HeadlessError::Image)?;

    tracing::info!("Done — {}x{} screenshot saved", w, h);
    Ok(())
}
