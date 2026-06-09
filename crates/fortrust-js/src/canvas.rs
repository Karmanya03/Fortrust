use std::sync::Mutex;

use fortrust_paint::TextRenderer;

thread_local! {
    static TEXT_RENDERER: Mutex<TextRenderer> = Mutex::new(TextRenderer::new());
}

fn parse_css_color(s: &str) -> [u8; 4] {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        parse_hex_color(hex)
    } else if s.starts_with("rgba") || s.starts_with("rgba(") {
        parse_rgba(s)
    } else if s.starts_with("rgb") || s.starts_with("rgb(") {
        let mut c = parse_rgba(s);
        c[3] = 255;
        c
    } else {
        named_color(s).unwrap_or([0, 0, 0, 255])
    }
}

fn parse_hex_color(hex: &str) -> [u8; 4] {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).unwrap_or(0) * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(0) * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(0) * 17;
            [r, g, b, 255]
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            [r, g, b, 255]
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(0);
            [r, g, b, a]
        }
        _ => [0, 0, 0, 255],
    }
}

fn parse_rgba(s: &str) -> [u8; 4] {
    let s = s.trim_start_matches("rgba").trim_start_matches("rgb");
    let s = s.trim_start_matches('(').trim_end_matches(')');
    let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
    let r = parts.first().and_then(|v| v.parse().ok()).unwrap_or(0);
    let g = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
    let b = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let a = parts.get(3).and_then(|v| {
        if v.contains('.') { Some((v.parse::<f32>().unwrap_or(1.0) * 255.0) as u8) }
        else { v.parse::<u8>().ok() }
    }).unwrap_or(255);
    [r, g, b, a]
}

fn named_color(name: &str) -> Option<[u8; 4]> {
    Some(match name.to_ascii_lowercase().as_str() {
        "transparent" => [0, 0, 0, 0],
        "black" | "#000000" => [0, 0, 0, 255],
        "white" => [255, 255, 255, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "yellow" => [255, 255, 0, 255],
        "orange" => [255, 165, 0, 255],
        "purple" => [128, 0, 128, 255],
        "gray" | "grey" => [128, 128, 128, 255],
        "silver" => [192, 192, 192, 255],
        "maroon" => [128, 0, 0, 255],
        "fuchsia" | "magenta" => [255, 0, 255, 255],
        "lime" => [0, 255, 0, 255],
        "olive" => [128, 128, 0, 255],
        "navy" => [0, 0, 128, 255],
        "teal" => [0, 128, 128, 255],
        "aqua" | "cyan" => [0, 255, 255, 255],
        _ => return None,
    })
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

#[derive(Clone)]
struct CanvasState {
    fill_style: [u8; 4],
    stroke_style: [u8; 4],
    line_width: f32,
    global_alpha: f32,
    font_size: f32,
    font_family: String,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    transform: [f32; 6],
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            fill_style: [0, 0, 0, 255],
            stroke_style: [0, 0, 0, 255],
            line_width: 1.0,
            global_alpha: 1.0,
            font_size: 10.0,
            font_family: "sans-serif".into(),
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum TextAlign { Start, End, Left, Right, Center }

#[derive(Clone, Copy, PartialEq)]
enum TextBaseline { Top, Hanging, Middle, Alphabetic, Ideographic, Bottom }

#[derive(Clone)]
enum PathOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    ClosePath,
    Arc(f32, f32, f32, f32, f32, bool),
    BezierCurve(f32, f32, f32, f32, f32, f32), // cp1x, cp1y, cp2x, cp2y, x, y
    QuadraticCurve(f32, f32, f32, f32), // cpx, cpy, x, y
    Rect(f32, f32, f32, f32),
}

pub struct Canvas2D {
    width: u32,
    height: u32,
    buffer: Vec<u8>,
    state: Vec<CanvasState>,
    path: Vec<PathOp>,
    subpath_empty: bool,
    clip_mask: Option<Vec<bool>>,
}

impl Canvas2D {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width.max(1) * height.max(1) * 4) as usize;
        Self {
            width: width.max(1),
            height: height.max(1),
            buffer: vec![0u8; size],
            state: vec![CanvasState::default()],
            path: Vec::new(),
            subpath_empty: true,
            clip_mask: None,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width.max(1);
        self.height = height.max(1);
        self.buffer = vec![0u8; (self.width * self.height * 4) as usize];
    }

    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }

    /// Returns (width, height, RGBA buffer) — a snapshot of the current canvas pixels.
    pub fn pixel_buffer(&self) -> (u32, u32, Vec<u8>) {
        (self.width, self.height, self.buffer.clone())
    }

    fn state(&self) -> &CanvasState { self.state.last().unwrap() }
    fn state_mut(&mut self) -> &mut CanvasState { self.state.last_mut().unwrap() }

    fn apply_transform(&self, x: f32, y: f32) -> (f32, f32) {
        let t = self.state().transform;
        (t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5])
    }

    // ─── State ───

    pub fn save(&mut self) {
        self.state.push(self.state().clone());
    }

    pub fn restore(&mut self) {
        if self.state.len() > 1 { self.state.pop(); }
    }

    pub fn set_fill_style(&mut self, color: &str) {
        self.state_mut().fill_style = parse_css_color(color);
    }

    pub fn set_stroke_style(&mut self, color: &str) {
        self.state_mut().stroke_style = parse_css_color(color);
    }

    pub fn set_line_width(&mut self, w: f32) {
        self.state_mut().line_width = w.max(0.0);
    }

    pub fn set_global_alpha(&mut self, a: f32) {
        self.state_mut().global_alpha = a.clamp(0.0, 1.0);
    }

    pub fn set_font(&mut self, font: &str) {
        let parts: Vec<&str> = font.split_whitespace().collect();
        let mut size = 10.0;
        let mut family = "sans-serif".to_owned();
        for part in parts {
            if part.ends_with("px") {
                size = part.trim_end_matches("px").parse().unwrap_or(10.0);
            } else if part.ends_with("pt") {
                size = part.trim_end_matches("pt").parse::<f32>().unwrap_or(10.0) * 1.333;
            } else if !part.contains(|c: char| c.is_ascii_digit() || c == '.')
                && !["bold", "italic", "normal", "oblique", "small-caps", "bolder", "lighter"].contains(&part.to_ascii_lowercase().as_str())
                && part.len() > 2
            {
                family = part.trim_matches('"').trim_matches('\'').to_owned();
            }
        }
        self.state_mut().font_size = size;
        self.state_mut().font_family = family;
    }

    pub fn get_fill_style(&self) -> String {
        let c = self.state().fill_style;
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }

    pub fn get_stroke_style(&self) -> String {
        let c = self.state().stroke_style;
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }

    pub fn get_line_width(&self) -> f32 {
        self.state().line_width
    }

    pub fn get_global_alpha(&self) -> f32 {
        self.state().global_alpha
    }

    pub fn get_font(&self) -> String {
        format!("{}px {}", self.state().font_size, self.state().font_family)
    }

    pub fn get_text_align(&self) -> &'static str {
        match self.state().text_align {
            TextAlign::Start => "start",
            TextAlign::End => "end",
            TextAlign::Left => "left",
            TextAlign::Right => "right",
            TextAlign::Center => "center",
        }
    }

    pub fn get_text_baseline(&self) -> &'static str {
        match self.state().text_baseline {
            TextBaseline::Top => "top",
            TextBaseline::Hanging => "hanging",
            TextBaseline::Middle => "middle",
            TextBaseline::Alphabetic => "alphabetic",
            TextBaseline::Ideographic => "ideographic",
            TextBaseline::Bottom => "bottom",
        }
    }

    pub fn set_text_align(&mut self, align: &str) {
        self.state_mut().text_align = match align {
            "left" => TextAlign::Left,
            "right" => TextAlign::Right,
            "center" => TextAlign::Center,
            "end" => TextAlign::End,
            _ => TextAlign::Start,
        };
    }

    pub fn set_text_baseline(&mut self, baseline: &str) {
        self.state_mut().text_baseline = match baseline {
            "top" => TextBaseline::Top,
            "hanging" => TextBaseline::Hanging,
            "middle" => TextBaseline::Middle,
            "ideographic" => TextBaseline::Ideographic,
            "bottom" => TextBaseline::Bottom,
            _ => TextBaseline::Alphabetic,
        };
    }

    // ─── Transforms ───

    pub fn translate(&mut self, x: f32, y: f32) {
        let t = &mut self.state_mut().transform;
        t[4] += t[0] * x + t[2] * y;
        t[5] += t[1] * x + t[3] * y;
    }

    pub fn rotate(&mut self, angle: f32) {
        let cos = angle.cos();
        let sin = angle.sin();
        let t = &mut self.state_mut().transform;
        let (a, b, c, d) = (t[0], t[1], t[2], t[3]);
        t[0] = a * cos + c * sin;
        t[1] = b * cos + d * sin;
        t[2] = c * cos - a * sin;
        t[3] = d * cos - b * sin;
    }

    pub fn scale(&mut self, x: f32, y: f32) {
        let t = &mut self.state_mut().transform;
        t[0] *= x;
        t[1] *= x;
        t[2] *= y;
        t[3] *= y;
    }

    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.state_mut().transform = [a, b, c, d, e, f];
    }

    // ─── Immediate drawing ───

    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let (x1, y1) = self.apply_transform(x, y);
        let (x2, y2) = self.apply_transform(x + w, y + h);
        let x0 = x1.min(x2).floor().max(0.0) as usize;
        let y0 = y1.min(y2).floor().max(0.0) as usize;
        let x1u = x1.max(x2).ceil().min(self.width as f32) as usize;
        let y1u = y1.max(y2).ceil().min(self.height as f32) as usize;
        for py in y0..y1u {
            for px in x0..x1u {
                if !self.is_in_clip(px, py) { continue; }
                let idx = (py * self.width as usize + px) * 4;
                self.buffer[idx..idx + 4].fill(0);
            }
        }
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let (x1, y1) = self.apply_transform(x, y);
        let (x2, y2) = self.apply_transform(x + w, y + h);
        let x0 = x1.min(x2).floor().max(0.0) as usize;
        let y0 = y1.min(y2).floor().max(0.0) as usize;
        let x1u = x1.max(x2).ceil().min(self.width as f32) as usize;
        let y1u = y1.max(y2).ceil().min(self.height as f32) as usize;
        let mut color = self.state().fill_style;
        color[3] = (color[3] as f32 * self.state().global_alpha) as u8;
        for py in y0..y1u {
            for px in x0..x1u {
                if !self.is_in_clip(px, py) { continue; }
                let idx = (py * self.width as usize + px) * 4;
                blend_rgba(&mut self.buffer[idx..idx + 4], color);
            }
        }
    }

    pub fn fill_text(&mut self, text: &str, x: f32, y: f32, max_width: Option<f32>) {
        let s = self.state();
        let (tx, ty) = self.apply_transform(x, y);
        if text.is_empty() { return; }

        let mut color = s.fill_style;
        color[3] = (color[3] as f32 * s.global_alpha) as u8;
        let font_size = s.font_size;

        // Estimate text width roughly (chars * font_size * 0.6)
        let text_width = text.len() as f32 * font_size * 0.6;
        let scale = max_width.map(|mw| (mw / text_width).min(1.0)).unwrap_or(1.0);

        let (align_offset, baseline_offset) = self.text_offsets(text, font_size);
        let draw_x = tx + align_offset;
        let draw_y = ty + baseline_offset + font_size * 0.8;

        let clip = fortrust_layout::Rect { x: 0.0, y: 0.0, width: self.width as f32, height: self.height as f32 };
        let rect = fortrust_layout::Rect { x: draw_x, y: draw_y - font_size, width: text_width * scale, height: font_size * 1.2 };

        let font_family = s.font_family.clone();
        TEXT_RENDERER.with(|tr| {
            let mut tr = tr.lock().unwrap();
            tr.render_into(
                &mut self.buffer,
                self.width as usize,
                self.height as usize,
                clip,
                rect,
                text,
                font_size * scale,
                &font_family,
                fortrust_style::FontWeight::Normal,
                fortrust_style::FontStyle::Normal,
                color,
            );
        });
    }

    fn text_offsets(&self, text: &str, font_size: f32) -> (f32, f32) {
        let text_width = text.len() as f32 * font_size * 0.6;
        let align_offset = match self.state().text_align {
            TextAlign::Start | TextAlign::Left => 0.0,
            TextAlign::Center => -text_width / 2.0,
            TextAlign::End | TextAlign::Right => -text_width,
        };
        let baseline_offset = match self.state().text_baseline {
            TextBaseline::Top => 0.0,
            TextBaseline::Hanging => font_size * 0.2,
            TextBaseline::Middle => -font_size * 0.4,
            TextBaseline::Alphabetic => -font_size * 0.8,
            TextBaseline::Ideographic => -font_size * 0.9,
            TextBaseline::Bottom => -font_size,
        };
        (align_offset, baseline_offset)
    }

    // ─── Pixel data ───

    pub fn create_image_data(&self, w: u32, h: u32) -> (Vec<u8>, u32, u32) {
        let size = (w.max(1) * h.max(1) * 4) as usize;
        (vec![0u8; size], w.max(1), h.max(1))
    }

    pub fn get_image_data(&self, x: i32, y: i32, w: u32, h: u32) -> (Vec<u8>, u32, u32) {
        let w = w.max(1);
        let h = h.max(1);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for dy in 0..h.min(self.height) {
            for dx in 0..w.min(self.width) {
                let sx = (x.max(0) as u32 + dx).min(self.width - 1);
                let sy = (y.max(0) as u32 + dy).min(self.height - 1);
                let src_idx = (sy * self.width + sx) * 4;
                let dst_idx = (dy * w + dx) as usize * 4;
                let src_idx = src_idx as usize;
                data[dst_idx..dst_idx + 4].copy_from_slice(&self.buffer[src_idx..src_idx + 4]);
            }
        }
        (data, w, h)
    }

    pub fn put_image_data(&mut self, data: &[u8], w: u32, h: u32, x: i32, y: i32) {
        for dy in 0..h.min(self.height) {
            for dx in 0..w.min(self.width) {
                let sx = (x.max(0) as u32 + dx).min(self.width - 1);
                let sy = (y.max(0) as u32 + dy).min(self.height - 1);
                let src_idx = (dy * w + dx) as usize * 4;
                let dst_idx = (sy * self.width + sx) as usize * 4;
                if src_idx + 4 <= data.len() {
                    let src = [data[src_idx], data[src_idx + 1], data[src_idx + 2], data[src_idx + 3]];
                    blend_rgba(&mut self.buffer[dst_idx..dst_idx + 4], src);
                }
            }
        }
    }

    pub fn to_data_url(&self) -> String {
        use std::io::Cursor;
        let mut cursor = Cursor::new(Vec::new());
        let _ = image::write_buffer_with_format(
            &mut cursor,
            &self.buffer,
            self.width,
            self.height,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        );
        let png_bytes = cursor.into_inner();
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &png_bytes);
        format!("data:image/png;base64,{b64}")
    }

    // ─── Path operations ───

    pub fn begin_path(&mut self) {
        self.path.clear();
        self.subpath_empty = true;
    }

    pub fn close_path(&mut self) {
        self.path.push(PathOp::ClosePath);
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.path.push(PathOp::MoveTo(x, y));
        self.subpath_empty = false;
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        if self.subpath_empty {
            self.path.push(PathOp::MoveTo(x, y));
            self.subpath_empty = false;
        } else {
            self.path.push(PathOp::LineTo(x, y));
        }
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.path.push(PathOp::Rect(x, y, w, h));
    }

    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, start_angle: f32, end_angle: f32, anticlockwise: bool) {
        if r <= 0.0 { return; }
        if self.subpath_empty {
            let (sx, sy) = angle_to_point(cx, cy, r, start_angle);
            self.path.push(PathOp::MoveTo(sx, sy));
            self.subpath_empty = false;
        }
        self.path.push(PathOp::Arc(cx, cy, r, start_angle, end_angle, anticlockwise));
    }

    pub fn fill(&mut self) {
        let segments = self.flatten_path();
        if segments.is_empty() { return; }
        self.rasterize_fill(&segments);
        self.path.clear();
        self.subpath_empty = true;
    }

    pub fn stroke(&mut self) {
        let segments = self.flatten_path();
        if segments.is_empty() { return; }
        self.rasterize_stroke(&segments);
        self.path.clear();
        self.subpath_empty = true;
    }

    // ─── Text drawing ───

    pub fn stroke_text(&mut self, text: &str, x: f32, y: f32, max_width: Option<f32>) {
        let s = self.state();
        let (tx, ty) = self.apply_transform(x, y);
        if text.is_empty() { return; }

        let mut color = s.stroke_style;
        color[3] = (color[3] as f32 * s.global_alpha) as u8;
        let font_size = s.font_size;

        let text_width = text.len() as f32 * font_size * 0.6;
        let scale = max_width.map(|mw| (mw / text_width).min(1.0)).unwrap_or(1.0);

        let (align_offset, baseline_offset) = self.text_offsets(text, font_size);
        let draw_x = tx + align_offset;
        let draw_y = ty + baseline_offset + font_size * 0.8;

        let clip = fortrust_layout::Rect { x: 0.0, y: 0.0, width: self.width as f32, height: self.height as f32 };
        let rect = fortrust_layout::Rect { x: draw_x, y: draw_y - font_size, width: text_width * scale, height: font_size * 1.2 };

        let font_family = s.font_family.clone();
        TEXT_RENDERER.with(|tr| {
            let mut tr = tr.lock().unwrap();
            tr.render_into(
                &mut self.buffer,
                self.width as usize,
                self.height as usize,
                clip,
                rect,
                text,
                font_size * scale,
                &font_family,
                fortrust_style::FontWeight::Normal,
                fortrust_style::FontStyle::Normal,
                color,
            );
        });
    }

    pub fn measure_text(&self, text: &str) -> f64 {
        let font_size = self.state().font_size;
        (text.len() as f32 * font_size * 0.6) as f64
    }

    // ─── Curves ───

    pub fn bezier_curve_to(&mut self, cp1x: f32, cp1y: f32, cp2x: f32, cp2y: f32, x: f32, y: f32) {
        if self.subpath_empty {
            self.path.push(PathOp::MoveTo(cp1x, cp1y));
            self.subpath_empty = false;
        }
        self.path.push(PathOp::BezierCurve(cp1x, cp1y, cp2x, cp2y, x, y));
    }

    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        if self.subpath_empty {
            self.path.push(PathOp::MoveTo(cpx, cpy));
            self.subpath_empty = false;
        }
        self.path.push(PathOp::QuadraticCurve(cpx, cpy, x, y));
    }

    // ─── Clip ───

    pub fn clip(&mut self) {
        let segments = self.flatten_path();
        self.clip_mask = Some(self.rasterize_clip_mask(&segments));
        self.path.clear();
        self.subpath_empty = true;
    }

    fn is_in_clip(&self, px: usize, py: usize) -> bool {
        match &self.clip_mask {
            Some(mask) => {
                let idx = py * self.width as usize + px;
                idx < mask.len() && mask[idx]
            }
            None => true,
        }
    }

    fn rasterize_clip_mask(&self, segments: &[[(f32, f32); 2]]) -> Vec<bool> {
        let w = self.width as usize;
        let h = self.height as usize;
        let mut mask = vec![false; w * h];

        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for seg in segments {
            let y1 = seg[0].1.min(seg[1].1);
            let y2 = seg[0].1.max(seg[1].1);
            if y1 < min_y { min_y = y1; }
            if y2 > max_y { max_y = y2; }
        }

        let y_start = (min_y.floor().max(0.0) as usize).min(h);
        let y_end = (max_y.ceil().min(self.height as f32) as usize).min(h);

        for py in y_start..y_end {
            let y = py as f32 + 0.5;
            let mut intersections = Vec::new();
            for seg in segments {
                let (x1, y1) = seg[0];
                let (x2, y2) = seg[1];
                if y1 == y2 { continue; }
                if (y < y1.min(y2)) || (y >= y1.max(y2)) { continue; }
                let t = (y - y1) / (y2 - y1);
                let ix = x1 + t * (x2 - x1);
                intersections.push(ix);
            }
            intersections.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for chunk in intersections.chunks(2) {
                if chunk.len() < 2 { break; }
                let x_start = chunk[0].floor().max(0.0) as usize;
                let x_end = chunk[1].ceil().min(self.width as f32) as usize;
                for px in x_start..x_end.min(w) {
                    mask[py * w + px] = true;
                }
            }
        }
        mask
    }

    // ─── Image drawing ───

    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(&mut self, data: &[u8], img_w: u32, img_h: u32, dx: f32, dy: f32, dw: Option<f32>, dh: Option<f32>) {
        let (tx, ty) = self.apply_transform(dx, dy);
        let draw_w = dw.unwrap_or(img_w as f32).max(1.0) as u32;
        let draw_h = dh.unwrap_or(img_h as f32).max(1.0) as u32;

        for sy in 0..img_h.min(draw_h) {
            for sx in 0..img_w.min(draw_w) {
                let src_idx = (sy * img_w + sx) as usize * 4;
                if src_idx + 4 > data.len() { continue; }
                let px = (tx + sx as f32) as i32;
                let py = (ty + sy as f32) as i32;
                if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 { continue; }
                let dst_idx = (py as u32 * self.width + px as u32) as usize * 4;
                let src = [data[src_idx], data[src_idx + 1], data[src_idx + 2], data[src_idx + 3]];
                let pxn = px as usize;
                let pyn = py as usize;
                if !self.is_in_clip(pxn, pyn) { continue; }
                blend_rgba(&mut self.buffer[dst_idx..dst_idx + 4], src);
            }
        }
    }

    // ─── Path flattening ───

    fn flatten_path(&self) -> Vec<[(f32, f32); 2]> {
        let mut segments = Vec::new();
        let mut current = (0.0, 0.0);
        let mut subpath_start = (0.0, 0.0);
        let mut i = 0;
        while i < self.path.len() {
            match &self.path[i] {
                PathOp::MoveTo(x, y) => {
                    let (tx, ty) = self.apply_transform(*x, *y);
                    current = (tx, ty);
                    subpath_start = (tx, ty);
                }
                PathOp::LineTo(x, y) => {
                    let (tx, ty) = self.apply_transform(*x, *y);
                    segments.push([current, (tx, ty)]);
                    current = (tx, ty);
                }
                PathOp::ClosePath => {
                    if current != subpath_start {
                        segments.push([current, subpath_start]);
                        current = subpath_start;
                    }
                }
                PathOp::Arc(cx, cy, r, sa, ea, acw) => {
                    let (tcx, tcy) = self.apply_transform(*cx, *cy);
                    let (sdx, sdy) = self.apply_transform(*cx + *r, *cy);
                    let tr = ((sdx - tcx).powi(2) + (sdy - tcy).powi(2)).sqrt();
                    if tr <= 0.0 { continue; }
                    let start = *sa;
                    let mut end = *ea;
                    if *acw {
                        while end < start { end += std::f32::consts::TAU; }
                    } else {
                        while end > start { end -= std::f32::consts::TAU; }
                    }
                    let steps = ((end - start).abs() / (std::f32::consts::PI / 18.0)).ceil() as u32;
                    let steps = steps.max(4);
                    for s in 0..steps {
                        let t = s as f32 / steps as f32;
                        let angle = start + (end - start) * t;
                        let (px, py) = angle_to_point(tcx, tcy, tr, angle);
                        if s == 0 && self.subpath_empty && i > 0 {
                            // first point from arc when already in subpath
                        } else if s == 0 {
                            // first point
                        } else {
                            segments.push([current, (px, py)]);
                        }
                        current = (px, py);
                        if s == 0 { subpath_start = (px, py); }
                    }
                }
                PathOp::Rect(x, y, w, h) => {
                    let corners = [
                        (*x, *y),
                        (*x + *w, *y),
                        (*x + *w, *y + *h),
                        (*x, *y + *h),
                    ];
                    if let Some(&first) = corners.first() {
                        let (tx, ty) = self.apply_transform(first.0, first.1);
                        current = (tx, ty);
                        subpath_start = (tx, ty);
                        for &corner in corners.iter().skip(1) {
                            let (tx, ty) = self.apply_transform(corner.0, corner.1);
                            segments.push([current, (tx, ty)]);
                            current = (tx, ty);
                        }
                        segments.push([current, subpath_start]);
                        current = subpath_start;
                    }
                }
                PathOp::BezierCurve(cp1x, cp1y, cp2x, cp2y, x, y) => {
                    let (tcp1x, tcp1y) = self.apply_transform(*cp1x, *cp1y);
                    let (tcp2x, tcp2y) = self.apply_transform(*cp2x, *cp2y);
                    let (tx, ty) = self.apply_transform(*x, *y);
                    let steps = 20;
                    let mut prev = current;
                    for s in 1..=steps {
                        let t = s as f32 / steps as f32;
                        let mt = 1.0 - t;
                        let mt2 = mt * mt;
                        let mt3 = mt2 * mt;
                        let t2 = t * t;
                        let t3 = t2 * t;
                        let px = mt3 * current.0 + 3.0 * mt2 * t * tcp1x + 3.0 * mt * t2 * tcp2x + t3 * tx;
                        let py = mt3 * current.1 + 3.0 * mt2 * t * tcp1y + 3.0 * mt * t2 * tcp2y + t3 * ty;
                        segments.push([prev, (px, py)]);
                        prev = (px, py);
                    }
                    current = (tx, ty);
                }
                PathOp::QuadraticCurve(cpx, cpy, x, y) => {
                    let (tcpx, tcpy) = self.apply_transform(*cpx, *cpy);
                    let (tx, ty) = self.apply_transform(*x, *y);
                    let steps = 15;
                    let mut prev = current;
                    for s in 1..=steps {
                        let t = s as f32 / steps as f32;
                        let mt = 1.0 - t;
                        let mt2 = mt * mt;
                        let t2 = t * t;
                        let px = mt2 * current.0 + 2.0 * mt * t * tcpx + t2 * tx;
                        let py = mt2 * current.1 + 2.0 * mt * t * tcpy + t2 * ty;
                        segments.push([prev, (px, py)]);
                        prev = (px, py);
                    }
                    current = (tx, ty);
                }
            }
            i += 1;
        }
        segments
    }

    // ─── Rasterization ───

    fn rasterize_fill(&mut self, segments: &[[(f32, f32); 2]]) {
        let color = self.state().fill_style;
        let alpha = (color[3] as f32 * self.state().global_alpha) as u8;
        let rgba = [color[0], color[1], color[2], alpha];

        let w = self.width as usize;
        let h = self.height as usize;

        // Find bounding box
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for seg in segments {
            let y1 = seg[0].1.min(seg[1].1);
            let y2 = seg[0].1.max(seg[1].1);
            if y1 < min_y { min_y = y1; }
            if y2 > max_y { max_y = y2; }
        }

        let y_start = (min_y.floor().max(0.0) as usize).min(h);
        let y_end = (max_y.ceil().min(self.height as f32) as usize).min(h);

        for py in y_start..y_end {
            let y = py as f32 + 0.5;
            let mut intersections = Vec::new();
            for seg in segments {
                let (x1, y1) = seg[0];
                let (x2, y2) = seg[1];
                if y1 == y2 { continue; }
                if (y < y1.min(y2)) || (y >= y1.max(y2)) { continue; }
                let t = (y - y1) / (y2 - y1);
                let ix = x1 + t * (x2 - x1);
                intersections.push(ix);
            }
            intersections.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for chunk in intersections.chunks(2) {
                if chunk.len() < 2 { break; }
                let x_start = chunk[0].floor().max(0.0) as usize;
                let x_end = chunk[1].ceil().min(self.width as f32) as usize;
                for px in x_start..x_end.min(w) {
                    if !self.is_in_clip(px, py) { continue; }
                    let idx = (py * w + px) * 4;
                    blend_rgba(&mut self.buffer[idx..idx + 4], rgba);
                }
            }
        }
    }

    fn rasterize_stroke(&mut self, segments: &[[(f32, f32); 2]]) {
        let color = self.state().stroke_style;
        let alpha = (color[3] as f32 * self.state().global_alpha) as u8;
        let rgba = [color[0], color[1], color[2], alpha];
        let lw = self.state().line_width.max(0.5);
        let half = lw / 2.0;

        let w = self.width;
        let h = self.height;

        for seg in segments {
            let (x1, y1) = seg[0];
            let (x2, y2) = seg[1];
            let dx = x2 - x1;
            let dy = y2 - y1;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 0.001 { continue; }
            let _ux = dx / len;
            let _uy = dy / len;

            let min_x = (x1.min(x2) - half).floor().max(0.0) as usize;
            let max_x = (x1.max(x2) + half).ceil().min(w as f32) as usize;
            let min_y = (y1.min(y2) - half).floor().max(0.0) as usize;
            let max_y = (y1.max(y2) + half).ceil().min(h as f32) as usize;

            for py in min_y..max_y {
                for px in min_x..max_x {
                    let px_f = px as f32 + 0.5;
                    let py_f = py as f32 + 0.5;

                    // Project point onto line
                    let t = ((px_f - x1) * dx + (py_f - y1) * dy) / (len * len);
                    let t = t.clamp(0.0, 1.0);
                    let near_x = x1 + t * dx;
                    let near_y = y1 + t * dy;
                    let dist = ((px_f - near_x).powi(2) + (py_f - near_y).powi(2)).sqrt();

                    if dist <= half {
                        if !self.is_in_clip(px, py) { continue; }
                        let idx = (py * w as usize + px) * 4;
                        blend_rgba(&mut self.buffer[idx..idx + 4], rgba);
                    }
                }
            }
        }
    }
}

fn angle_to_point(cx: f32, cy: f32, r: f32, angle: f32) -> (f32, f32) {
    (cx + r * angle.cos(), cy + r * angle.sin())
}
