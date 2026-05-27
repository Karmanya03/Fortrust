use eframe::egui::{self, Color32, Pos2, Rect, Vec2, TextureId};
use crate::theme::FortrustTheme;
use fortrust_core::UiConfig;
use std::path::PathBuf;
use std::sync::Mutex;
use once_cell::sync::Lazy;
use image::io::Reader as ImageReader;

use egui::epaint::ahash::AHashMap;

// Cache loaded textures keyed by absolute path
static LOADED: Lazy<Mutex<AHashMap<String, TextureId>>> = Lazy::new(|| Mutex::new(AHashMap::default()));

// Simple procedural textured wallpapers drawn into the background layer.
pub fn paint_background(ctx: &egui::Context, screen_rect: Rect, _theme: &FortrustTheme, ui: &UiConfig) {
    let painter = ctx.layer_painter(egui::LayerId::background());

    // If user selected a photo wallpaper, try to draw it first
    if try_draw_photo_wallpaper(ctx, &painter, ui, screen_rect) {
        return;
    }

    // base gradient based on choice
    match ui.wallpaper.as_str() {
        "forest" => {
            let top = Color32::from_rgb(240, 246, 238);
            let bottom = Color32::from_rgb(214, 233, 221);
            paint_vertical_gradient(&painter, screen_rect, top, bottom);
            paint_watercolor_blots(&painter, screen_rect, 6, 0.18 * (ui.wallpaper_strength as f32 / 100.0));
            paint_grain(&painter, screen_rect, ui.wallpaper_strength, Color32::from_rgba_unmultiplied(10, 20, 12, 6));
        }
        "none" => {
            painter.rect_filled(screen_rect, 0.0, Color32::from_rgb(247, 243, 238));
        }
        _ => {
            // default: watercolor/paper
            let top = Color32::from_rgb(250, 247, 243);
            let bottom = Color32::from_rgb(232, 236, 238);
            paint_vertical_gradient(&painter, screen_rect, top, bottom);
            paint_watercolor_blots(&painter, screen_rect, 5, 0.12 * (ui.wallpaper_strength as f32 / 100.0));
            paint_grain(&painter, screen_rect, ui.wallpaper_strength, Color32::from_rgba_unmultiplied(24, 20, 16, 6));
        }
    }
}

fn try_draw_photo_wallpaper(ctx: &egui::Context, painter: &egui::Painter, ui: &UiConfig, screen_rect: Rect) -> bool {
    // if wallpaper string contains a dot assume filename
    let name = ui.wallpaper.clone();
    let assets_dir = std::path::Path::new("assets/backgrounds");
    if !assets_dir.exists() {
        return false;
    }

    // search for candidate file matching stem or exact name
    let mut candidate: Option<PathBuf> = None;
    for entry in std::fs::read_dir(assets_dir).ok().into_iter().flatten() {
        if let Ok(entry) = entry {
            let path = entry.path();
            if !path.is_file() { continue; }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.eq_ignore_ascii_case(&name) || path.file_name().and_then(|n| n.to_str()).map(|s| s.eq_ignore_ascii_case(&name)).unwrap_or(false) {
                    candidate = Some(path);
                    break;
                }
            }
        }
    }

    let path = match candidate {
        Some(p) => p,
        None => return false,
    };

    let key = path.to_string_lossy().to_string();
    let mut map = LOADED.lock().unwrap();
    if let Some(tex) = map.get(&key) {
        // draw cached texture stretched to screen_rect
        painter.add(egui::Shape::image(*tex, screen_rect, egui::Rect::from_min_max(Pos2::new(0.0,0.0), Pos2::new(1.0,1.0)), Color32::WHITE));
        return true;
    }

    // Load image from disk and create texture
    match std::fs::read(&path) {
        Ok(bytes) => {
            let reader = ImageReader::new(std::io::Cursor::new(bytes));
            match reader.with_guessed_format() {
                Ok(reader) => match reader.decode() {
                    Ok(img) => {
                        let img = img.to_rgba8();
                        let (w, h) = img.dimensions();
                        let pixels: Vec<u8> = img.into_raw();
                        let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
                        let handle = ctx.load_texture(&key, color_image, egui::TextureOptions::LINEAR);
                        let tex_id = handle.id();
                        map.insert(key.clone(), tex_id);
                        painter.add(egui::Shape::image(tex_id, screen_rect, egui::Rect::from_min_max(Pos2::new(0.0,0.0), Pos2::new(1.0,1.0)), Color32::WHITE));
                        return true;
                    }
                    Err(_) => return false,
                },
                Err(_) => return false,
            }
        }
        Err(_) => return false,
    }

}

fn paint_vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    // approximate gradient by drawing horizontal bands
    let steps = 24usize;
    let h = rect.height() / steps as f32;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let r = lerp_color(top, bottom, t);
        let band = Rect::from_min_size(Pos2::new(rect.left(), rect.top() + i as f32 * h), Vec2::new(rect.width(), h + 1.0));
        painter.rect_filled(band, 0.0, r);
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        255,
    )
}

fn paint_watercolor_blots(painter: &egui::Painter, rect: Rect, count: usize, strength: f32) {
    // Draw some large soft blotches to emulate watercolor shapes
    let w = rect.width();
    let h = rect.height();
    let mut seed = xor_shift((w as u64) ^ (h as u64));
    for _i in 0..count {
        seed = xor_shift(seed);
        let fx = ((seed as f32) / (u32::MAX as f32)).fract();
        seed = xor_shift(seed);
        let fy = ((seed as f32) / (u32::MAX as f32)).fract();
        let cx = rect.left() + fx * w;
        let cy = rect.top() + fy * h * 0.7 + h * 0.15;
        let radius = (w.max(h) * (0.18 + ((seed % 100) as f32 / 500.0))).clamp(80.0, w.max(h) * 0.6);
        let alpha = (24.0 * strength).clamp(6.0, 180.0) as u8;
        // draw overlapping circles with decreasing alpha
        let layers = 6;
        for l in 0..layers {
            let t = l as f32 / (layers as f32);
            let r = radius * (1.0 + t * 0.28);
            let a = ((alpha as f32) * (1.0 - t * 0.84)) as u8;
            painter.circle_filled(Pos2::new(cx + t * 6.0, cy - t * 8.0), r, Color32::from_rgba_unmultiplied(200, 220, 200, a));
        }
    }
}

fn paint_grain(painter: &egui::Painter, rect: Rect, strength: u8, color: Color32) {
    // Draw many tiny translucent circles as grain/noise
    let count = 600usize * ((strength as usize).max(10) / 40);
    let w = rect.width();
    let h = rect.height();
    let mut seed = xor_shift((w as u64) ^ (h as u64) ^ (strength as u64));
    for _i in 0..count {
        seed = xor_shift(seed);
        let fx = ((seed as f32) / (u32::MAX as f32)).fract();
        seed = xor_shift(seed);
        let fy = ((seed as f32) / (u32::MAX as f32)).fract();
        let x = rect.left() + fx * w;
        let y = rect.top() + fy * h;
        seed = xor_shift(seed);
        let r = 0.5 + ((seed % 100) as f32 / 100.0) * 2.0;
        // color alpha scaled down
        let a = (color.a() as f32 * (0.6 + (strength as f32 / 200.0))).clamp(2.0, 28.0) as u8;
        painter.circle_filled(Pos2::new(x, y), r, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a));
    }
}

fn xor_shift(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}
