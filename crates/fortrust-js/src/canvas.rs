use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use fortrust_paint::TextRenderer;

thread_local! {
    static TEXT_RENDERER: Mutex<TextRenderer> = Mutex::new(TextRenderer::new());
}

static NEXT_GRADIENT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PATTERN_ID: AtomicU64 = AtomicU64::new(1);

lazy_static::lazy_static! {
    static ref GRADIENT_REGISTRY: Mutex<HashMap<u64, CanvasGradientInner>> = Mutex::new(HashMap::new());
    static ref PATTERN_REGISTRY: Mutex<HashMap<u64, CanvasPatternInner>> = Mutex::new(HashMap::new());
}

#[derive(Clone, Copy, PartialEq)]
enum LineCap { Butt, Round, Square }

#[derive(Clone, Copy, PartialEq)]
enum LineJoin { Miter, Round, Bevel }

#[derive(Clone)]
enum PaintStyle {
    Solid([u8; 4]),
    Gradient(u64),
    Pattern(u64),
}

impl PaintStyle {
    fn as_hex_string(&self) -> String {
        match self {
            PaintStyle::Solid(c) => format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]),
            PaintStyle::Gradient(_) => "[object CanvasGradient]".to_owned(),
            PaintStyle::Pattern(_) => "[object CanvasPattern]".to_owned(),
        }
    }
}

#[derive(Clone)]
enum GradientType {
    Linear { x0: f32, y0: f32, x1: f32, y1: f32 },
    Radial { x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32 },
}

#[derive(Clone)]
struct GradientStop {
    offset: f32,
    color: [u8; 4],
}

#[derive(Clone)]
struct CanvasGradientInner {
    gradient_type: GradientType,
    stops: Vec<GradientStop>,
}

fn eval_linear_gradient(grad: &CanvasGradientInner, x: f32, y: f32) -> [u8; 4] {
    let (x0, y0, x1, y1) = match grad.gradient_type {
        GradientType::Linear { x0, y0, x1, y1 } => (x0, y0, x1, y1),
        _ => return [0, 0, 0, 255],
    };
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.0001 { return eval_stops(&grad.stops, 0.0); }
    let t = ((x - x0) * dx + (y - y0) * dy) / len_sq;
    eval_stops(&grad.stops, t.clamp(0.0, 1.0))
}

fn eval_radial_gradient(grad: &CanvasGradientInner, x: f32, y: f32) -> [u8; 4] {
    let (cx0, cy0, r0, _cx1, _cy1, r1) = match grad.gradient_type {
        GradientType::Radial { x0, y0, r0, x1, y1, r1 } => (x0, y0, r0, x1, y1, r1),
        _ => return [0, 0, 0, 255],
    };
    let dx = x - cx0;
    let dy = y - cy0;
    let dist = (dx * dx + dy * dy).sqrt().max(0.0);
    let range = (r1 - r0).max(0.001);
    let t = ((dist - r0) / range).clamp(0.0, 1.0);
    eval_stops(&grad.stops, t)
}

fn eval_stops(stops: &[GradientStop], t: f32) -> [u8; 4] {
    if stops.is_empty() { return [0, 0, 0, 255]; }
    if stops.len() == 1 { return stops[0].color; }
    if t <= stops[0].offset { return stops[0].color; }
    if t >= stops.last().unwrap().offset { return stops.last().unwrap().color; }
    for i in 0..stops.len() - 1 {
        let a = &stops[i];
        let b = &stops[i + 1];
        if t >= a.offset && t <= b.offset {
            let range = b.offset - a.offset;
            if range < 0.0001 { return b.color; }
            let local_t = (t - a.offset) / range;
            return lerp_color(a.color, b.color, local_t);
        }
    }
    stops.last().unwrap().color
}

fn lerp_color(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t).round() as u8,
    ]
}

#[derive(Clone)]
struct CanvasPatternInner {
    image_data: Vec<u8>,
    width: u32,
    height: u32,
    repetition: PatternRepetition,
}

#[derive(Clone, Copy, PartialEq)]
enum PatternRepetition {
    Repeat,
    RepeatX,
    RepeatY,
    NoRepeat,
}

#[derive(Clone, Copy, PartialEq)]
enum CompositeOp {
    SourceOver, SourceIn, SourceOut, SourceAtop,
    DestinationOver, DestinationIn, DestinationOut, DestinationAtop,
    Lighter, Copy, Xor,
}

impl CompositeOp {
    fn from_str(s: &str) -> Self {
        match s {
            "source-in" => Self::SourceIn,
            "source-out" => Self::SourceOut,
            "source-atop" => Self::SourceAtop,
            "destination-over" => Self::DestinationOver,
            "destination-in" => Self::DestinationIn,
            "destination-out" => Self::DestinationOut,
            "destination-atop" => Self::DestinationAtop,
            "lighter" => Self::Lighter,
            "copy" => Self::Copy,
            "xor" => Self::Xor,
            _ => Self::SourceOver,
        }
    }
}

fn composite_blend(dst: &mut [u8], src: [u8; 4], op: CompositeOp) {
    let sa = src[3] as f32 / 255.0;
    if sa <= 0.0 && op == CompositeOp::SourceOver { return; }
    let da = dst[3] as f32 / 255.0;
    match op {
        CompositeOp::SourceOver => {
            let inverse = 1.0 - sa;
            dst[0] = (src[0] as f32 * sa + dst[0] as f32 * inverse) as u8;
            dst[1] = (src[1] as f32 * sa + dst[1] as f32 * inverse) as u8;
            dst[2] = (src[2] as f32 * sa + dst[2] as f32 * inverse) as u8;
            dst[3] = (sa * 255.0 + da * 255.0 * (1.0 - sa)) as u8;
        }
        CompositeOp::SourceIn => {
            dst[0] = (src[0] as f32 * da) as u8;
            dst[1] = (src[1] as f32 * da) as u8;
            dst[2] = (src[2] as f32 * da) as u8;
            dst[3] = (sa * da * 255.0) as u8;
        }
        CompositeOp::SourceOut => {
            dst[0] = (src[0] as f32 * (1.0 - da)) as u8;
            dst[1] = (src[1] as f32 * (1.0 - da)) as u8;
            dst[2] = (src[2] as f32 * (1.0 - da)) as u8;
            dst[3] = (sa * (1.0 - da) * 255.0) as u8;
        }
        CompositeOp::SourceAtop => {
            dst[0] = (src[0] as f32 * da + dst[0] as f32 * (1.0 - sa)) as u8;
            dst[1] = (src[1] as f32 * da + dst[1] as f32 * (1.0 - sa)) as u8;
            dst[2] = (src[2] as f32 * da + dst[2] as f32 * (1.0 - sa)) as u8;
            dst[3] = da as u8 * 255;
        }
        CompositeOp::DestinationOver => {
            let inverse = 1.0 - da;
            dst[0] = (src[0] as f32 * inverse + dst[0] as f32) as u8;
            dst[1] = (src[1] as f32 * inverse + dst[1] as f32) as u8;
            dst[2] = (src[2] as f32 * inverse + dst[2] as f32) as u8;
        }
        CompositeOp::DestinationIn => {
            dst[0] = (dst[0] as f32 * sa) as u8;
            dst[1] = (dst[1] as f32 * sa) as u8;
            dst[2] = (dst[2] as f32 * sa) as u8;
            dst[3] = (da * sa * 255.0) as u8;
        }
        CompositeOp::DestinationOut => {
            dst[0] = (dst[0] as f32 * (1.0 - sa)) as u8;
            dst[1] = (dst[1] as f32 * (1.0 - sa)) as u8;
            dst[2] = (dst[2] as f32 * (1.0 - sa)) as u8;
            dst[3] = (da * (1.0 - sa) * 255.0) as u8;
        }
        CompositeOp::DestinationAtop => {
            dst[0] = (src[0] as f32 * (1.0 - da) + dst[0] as f32 * sa) as u8;
            dst[1] = (src[1] as f32 * (1.0 - da) + dst[1] as f32 * sa) as u8;
            dst[2] = (src[2] as f32 * (1.0 - da) + dst[2] as f32 * sa) as u8;
        }
        CompositeOp::Lighter => {
            dst[0] = (src[0] as f32 + dst[0] as f32).min(255.0) as u8;
            dst[1] = (src[1] as f32 + dst[1] as f32).min(255.0) as u8;
            dst[2] = (src[2] as f32 + dst[2] as f32).min(255.0) as u8;
            dst[3] = (sa * 255.0 + da * 255.0).min(255.0) as u8;
        }
        CompositeOp::Copy => {
            dst.copy_from_slice(&src);
        }
        CompositeOp::Xor => {
            dst[0] = (src[0] as f32 * (1.0 - da) + dst[0] as f32 * (1.0 - sa)) as u8;
            dst[1] = (src[1] as f32 * (1.0 - da) + dst[1] as f32 * (1.0 - sa)) as u8;
            dst[2] = (src[2] as f32 * (1.0 - da) + dst[2] as f32 * (1.0 - sa)) as u8;
            dst[3] = (sa * (1.0 - da) * 255.0 + da * (1.0 - sa) * 255.0) as u8;
        }
    }
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

fn eval_pattern(p: &CanvasPatternInner, x: f32, y: f32) -> [u8; 4] {
    let (px, py) = match p.repetition {
        PatternRepetition::Repeat => {
            let px = ((x as i32).rem_euclid(p.width as i32)) as u32;
            let py = ((y as i32).rem_euclid(p.height as i32)) as u32;
            (px, py)
        }
        PatternRepetition::RepeatX => {
            let px = ((x as i32).rem_euclid(p.width as i32)) as u32;
            let py = (y as i32).clamp(0, p.height as i32 - 1) as u32;
            (px, py)
        }
        PatternRepetition::RepeatY => {
            let px = (x as i32).clamp(0, p.width as i32 - 1) as u32;
            let py = ((y as i32).rem_euclid(p.height as i32)) as u32;
            (px, py)
        }
        PatternRepetition::NoRepeat => {
            let px = (x as i32).clamp(0, p.width as i32 - 1) as u32;
            let py = (y as i32).clamp(0, p.height as i32 - 1) as u32;
            (px, py)
        }
    };
    let idx = (py * p.width + px) as usize * 4;
    if (idx + 4) <= p.image_data.len() {
        [p.image_data[idx], p.image_data[idx + 1], p.image_data[idx + 2], p.image_data[idx + 3]]
    } else {
        [0, 0, 0, 0]
    }
}

fn blend_rgba(dst: &mut [u8], src: [u8; 4]) {
    // Default source-over blend (used by most internal operations)
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
    fill_style: PaintStyle,
    stroke_style: PaintStyle,
    line_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    miter_limit: f32,
    global_alpha: f32,
    global_composite_op: CompositeOp,
    font_size: f32,
    font_family: String,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    transform: [f32; 6],
    shadow_color: [u8; 4],
    shadow_blur: f32,
    shadow_offset_x: f32,
    shadow_offset_y: f32,
    line_dash: Vec<f32>,
    line_dash_offset: f32,
    image_smoothing_enabled: bool,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            fill_style: PaintStyle::Solid([0, 0, 0, 255]),
            stroke_style: PaintStyle::Solid([0, 0, 0, 255]),
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            global_alpha: 1.0,
            global_composite_op: CompositeOp::SourceOver,
            font_size: 10.0,
            font_family: "sans-serif".into(),
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            shadow_color: [0, 0, 0, 0],
            shadow_blur: 0.0,
            shadow_offset_x: 0.0,
            shadow_offset_y: 0.0,
            line_dash: Vec::new(),
            line_dash_offset: 0.0,
            image_smoothing_enabled: true,
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
    Ellipse(f32, f32, f32, f32, f32, f32, f32, bool), // cx, cy, rx, ry, rot, start, end, anticlockwise
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
        self.state_mut().fill_style = PaintStyle::Solid(parse_css_color(color));
    }

    pub fn set_fill_style_gradient(&mut self, id: u64) {
        self.state_mut().fill_style = PaintStyle::Gradient(id);
    }

    pub fn set_fill_style_pattern(&mut self, id: u64) {
        self.state_mut().fill_style = PaintStyle::Pattern(id);
    }

    pub fn set_stroke_style(&mut self, color: &str) {
        self.state_mut().stroke_style = PaintStyle::Solid(parse_css_color(color));
    }

    pub fn set_stroke_style_gradient(&mut self, id: u64) {
        self.state_mut().stroke_style = PaintStyle::Gradient(id);
    }

    pub fn set_stroke_style_pattern(&mut self, id: u64) {
        self.state_mut().stroke_style = PaintStyle::Pattern(id);
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
        self.state().fill_style.as_hex_string()
    }

    pub fn get_stroke_style(&self) -> String {
        self.state().stroke_style.as_hex_string()
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

    // ─── Line styles ───

    pub fn set_line_cap(&mut self, cap: &str) {
        self.state_mut().line_cap = match cap {
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => LineCap::Butt,
        };
    }

    pub fn get_line_cap(&self) -> &'static str {
        match self.state().line_cap {
            LineCap::Butt => "butt",
            LineCap::Round => "round",
            LineCap::Square => "square",
        }
    }

    pub fn set_line_join(&mut self, join: &str) {
        self.state_mut().line_join = match join {
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => LineJoin::Miter,
        };
    }

    pub fn get_line_join(&self) -> &'static str {
        match self.state().line_join {
            LineJoin::Miter => "miter",
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
        }
    }

    pub fn set_miter_limit(&mut self, limit: f32) {
        self.state_mut().miter_limit = limit.max(0.0);
    }

    pub fn get_miter_limit(&self) -> f32 {
        self.state().miter_limit
    }

    // ─── Line dash ───

    pub fn set_line_dash(&mut self, dash: Vec<f32>) {
        let sanitized: Vec<f32> = dash.into_iter().map(|v| v.max(0.0)).collect();
        self.state_mut().line_dash = if sanitized.is_empty() { Vec::new() } else { sanitized };
    }

    pub fn get_line_dash(&self) -> Vec<f32> {
        self.state().line_dash.clone()
    }

    pub fn set_line_dash_offset(&mut self, offset: f32) {
        self.state_mut().line_dash_offset = offset;
    }

    pub fn get_line_dash_offset(&self) -> f32 {
        self.state().line_dash_offset
    }

    // ─── Shadows ───

    pub fn set_shadow_color(&mut self, color: &str) {
        self.state_mut().shadow_color = parse_css_color(color);
    }

    pub fn get_shadow_color(&self) -> String {
        let c = self.state().shadow_color;
        format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3])
    }

    pub fn set_shadow_blur(&mut self, blur: f32) {
        self.state_mut().shadow_blur = blur.max(0.0);
    }

    pub fn get_shadow_blur(&self) -> f32 {
        self.state().shadow_blur
    }

    pub fn set_shadow_offset_x(&mut self, ox: f32) {
        self.state_mut().shadow_offset_x = ox;
    }

    pub fn get_shadow_offset_x(&self) -> f32 {
        self.state().shadow_offset_x
    }

    pub fn set_shadow_offset_y(&mut self, oy: f32) {
        self.state_mut().shadow_offset_y = oy;
    }

    pub fn get_shadow_offset_y(&self) -> f32 {
        self.state().shadow_offset_y
    }

    // ─── Compositing ───

    pub fn set_global_composite_operation(&mut self, op: &str) {
        self.state_mut().global_composite_op = CompositeOp::from_str(op);
    }

    pub fn get_global_composite_operation(&self) -> &'static str {
        match self.state().global_composite_op {
            CompositeOp::SourceOver => "source-over",
            CompositeOp::SourceIn => "source-in",
            CompositeOp::SourceOut => "source-out",
            CompositeOp::SourceAtop => "source-atop",
            CompositeOp::DestinationOver => "destination-over",
            CompositeOp::DestinationIn => "destination-in",
            CompositeOp::DestinationOut => "destination-out",
            CompositeOp::DestinationAtop => "destination-atop",
            CompositeOp::Lighter => "lighter",
            CompositeOp::Copy => "copy",
            CompositeOp::Xor => "xor",
        }
    }

    // ─── Image smoothing ───

    pub fn set_image_smoothing_enabled(&mut self, enabled: bool) {
        self.state_mut().image_smoothing_enabled = enabled;
    }

    pub fn get_image_smoothing_enabled(&self) -> bool {
        self.state().image_smoothing_enabled
    }

    // ─── Paint style evaluation ───

    fn eval_fill_style_at(&self, x: f32, y: f32) -> [u8; 4] {
        match &self.state().fill_style {
            PaintStyle::Solid(c) => *c,
            PaintStyle::Gradient(id) => {
                let reg = GRADIENT_REGISTRY.lock().unwrap();
                reg.get(id).map(|g| match &g.gradient_type {
                    GradientType::Linear { .. } => eval_linear_gradient(g, x, y),
                    GradientType::Radial { .. } => eval_radial_gradient(g, x, y),
                }).unwrap_or([0, 0, 0, 255])
            }
            PaintStyle::Pattern(id) => {
                let reg = PATTERN_REGISTRY.lock().unwrap();
                reg.get(id).map(|p| eval_pattern(p, x, y)).unwrap_or([0, 0, 0, 255])
            }
        }
    }

    fn eval_stroke_style_at(&self, x: f32, y: f32) -> [u8; 4] {
        match &self.state().stroke_style {
            PaintStyle::Solid(c) => *c,
            PaintStyle::Gradient(id) => {
                let reg = GRADIENT_REGISTRY.lock().unwrap();
                reg.get(id).map(|g| match &g.gradient_type {
                    GradientType::Linear { .. } => eval_linear_gradient(g, x, y),
                    GradientType::Radial { .. } => eval_radial_gradient(g, x, y),
                }).unwrap_or([0, 0, 0, 255])
            }
            PaintStyle::Pattern(id) => {
                let reg = PATTERN_REGISTRY.lock().unwrap();
                reg.get(id).map(|p| eval_pattern(p, x, y)).unwrap_or([0, 0, 0, 255])
            }
        }
    }

    fn fill_color_at(&self, x: f32, y: f32) -> [u8; 4] {
        let mut c = self.eval_fill_style_at(x, y);
        c[3] = (c[3] as f32 * self.state().global_alpha) as u8;
        c
    }

    fn stroke_color_at(&self, x: f32, y: f32) -> [u8; 4] {
        let mut c = self.eval_stroke_style_at(x, y);
        c[3] = (c[3] as f32 * self.state().global_alpha) as u8;
        c
    }

    // ─── Gradient / Pattern creation ───

    pub fn create_linear_gradient(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> u64 {
        let id = NEXT_GRADIENT_ID.fetch_add(1, Ordering::Relaxed);
        let grad = CanvasGradientInner {
            gradient_type: GradientType::Linear { x0, y0, x1, y1 },
            stops: Vec::new(),
        };
        GRADIENT_REGISTRY.lock().unwrap().insert(id, grad);
        id
    }

    pub fn create_radial_gradient(&self, x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32) -> u64 {
        let id = NEXT_GRADIENT_ID.fetch_add(1, Ordering::Relaxed);
        let grad = CanvasGradientInner {
            gradient_type: GradientType::Radial { x0, y0, r0, x1, y1, r1 },
            stops: Vec::new(),
        };
        GRADIENT_REGISTRY.lock().unwrap().insert(id, grad);
        id
    }

    pub fn add_gradient_color_stop(&self, gradient_id: u64, offset: f32, color: &str) {
        if let Some(grad) = GRADIENT_REGISTRY.lock().unwrap().get_mut(&gradient_id) {
            let offset = offset.clamp(0.0, 1.0);
            let parsed = parse_css_color(color);
            grad.stops.push(GradientStop { offset, color: parsed });
            grad.stops.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap());
        }
    }

    pub fn create_pattern(&self, image_data: Vec<u8>, width: u32, height: u32, repetition: &str) -> u64 {
        let id = NEXT_PATTERN_ID.fetch_add(1, Ordering::Relaxed);
        let rep = match repetition {
            "repeat-x" => PatternRepetition::RepeatX,
            "repeat-y" => PatternRepetition::RepeatY,
            "no-repeat" => PatternRepetition::NoRepeat,
            _ => PatternRepetition::Repeat,
        };
        let pattern = CanvasPatternInner { image_data, width, height, repetition: rep };
        PATTERN_REGISTRY.lock().unwrap().insert(id, pattern);
        id
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

    pub fn reset_transform(&mut self) {
        self.state_mut().transform = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    }

    pub fn get_transform(&self) -> [f32; 6] {
        self.state().transform
    }

    // ─── Immediate drawing ───

    pub fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let (x1, y1) = self.apply_transform(x, y);
        let (x2, y2) = self.apply_transform(x + w, y + h);
        let x0 = x1.min(x2).floor().max(0.0) as usize;
        let y0 = y1.min(y2).floor().max(0.0) as usize;
        let x1u = x1.max(x2).ceil().min(self.width as f32) as usize;
        let y1u = y1.max(y2).ceil().min(self.height as f32) as usize;
        let op = self.state().global_composite_op;
        for py in y0..y1u {
            for px in x0..x1u {
                if !self.is_in_clip(px, py) { continue; }
                let idx = (py * self.width as usize + px) * 4;
                if op != CompositeOp::DestinationOut {
                    self.buffer[idx..idx + 4].fill(0);
                } else {
                    // destination-out with clear effectively keeps destination
                }
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
        let (op, sx, sy, blur, shad, ga) = {
            let s = self.state();
            (s.global_composite_op, s.shadow_offset_x, s.shadow_offset_y, s.shadow_blur, s.shadow_color, s.global_alpha)
        };

        if blur > 0.0 && shad[3] > 0 {
            let spread = blur.max(1.0) as i32;
            for py in y0..y1u {
                for px in x0..x1u {
                    if !self.is_in_clip(px, py) { continue; }
                    let shadow_px = (px as f32 + sx) as i32;
                    let shadow_py = (py as f32 + sy) as i32;
                    for dy in -spread..=spread {
                        for dx in -spread..=spread {
                            let sx2 = shadow_px + dx;
                            let sy2 = shadow_py + dy;
                            if sx2 < 0 || sy2 < 0 || sx2 >= self.width as i32 || sy2 >= self.height as i32 { continue; }
                            let dist = ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt();
                            let falloff = (1.0 - (dist / (spread as f32 + 1.0))).max(0.0);
                            let mut sa = shad;
                            sa[3] = (shad[3] as f32 * falloff * ga) as u8;
                            let idx = (sy2 as usize * self.width as usize + sx2 as usize) * 4;
                            composite_blend(&mut self.buffer[idx..idx + 4], sa, op);
                        }
                    }
                }
            }
        }

        for py in y0..y1u {
            for px in x0..x1u {
                if !self.is_in_clip(px, py) { continue; }
                let idx = (py * self.width as usize + px) * 4;
                let color = self.fill_color_at(px as f32 + 0.5, py as f32 + 0.5);
                composite_blend(&mut self.buffer[idx..idx + 4], color, op);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_text_with_shadow(&mut self, text: &str, x: f32, y: f32, _color: [u8; 4], font_size: f32, scale: f32, font_family: &str) {
        let (op, has_shadow, shad, spread, sox, soy, ga) = {
            let s = self.state();
            (
                s.global_composite_op,
                s.shadow_blur > 0.0 && s.shadow_color[3] > 0,
                s.shadow_color,
                s.shadow_blur.max(1.0) as i32,
                s.shadow_offset_x,
                s.shadow_offset_y,
                s.global_alpha,
            )
        };

        if has_shadow {
            let shadow_rect = fortrust_layout::Rect {
                x: x + sox,
                y: y + soy - font_size,
                width: text.len() as f32 * font_size * 0.6 * scale,
                height: font_size * 1.2,
            };
            let clip = fortrust_layout::Rect { x: 0.0, y: 0.0, width: self.width as f32, height: self.height as f32 };
            for dy in -spread..=spread {
                for dx in -spread..=spread {
                    let dist = ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt();
                    let falloff = (1.0 - (dist / (spread as f32 + 1.0))).max(0.0);
                    let mut sa = shad;
                    sa[3] = (shad[3] as f32 * falloff * ga) as u8;
                    let off_rect = fortrust_layout::Rect {
                        x: shadow_rect.x + dx as f32,
                        y: shadow_rect.y + dy as f32,
                        ..shadow_rect
                    };
                    let w = self.width as usize;
                    let h = self.height as usize;
                    TEXT_RENDERER.with(|tr| {
                        let mut tr = tr.lock().unwrap();
                        let mut dummy = vec![0u8; w * h * 4];
                        tr.render_into(
                            &mut dummy, w, h, clip, off_rect, text,
                            font_size * scale, font_family,
                            fortrust_style::FontWeight::Normal,
                            fortrust_style::FontStyle::Normal, sa,
                        );
                        for (dst_px, src_px) in self.buffer.chunks_mut(4).zip(dummy.chunks(4)) {
                            if src_px[3] > 0 {
                                composite_blend(dst_px, [src_px[0], src_px[1], src_px[2], src_px[3]], op);
                            }
                        }
                    });
                }
            }
        }
    }

    pub fn fill_text(&mut self, text: &str, x: f32, y: f32, max_width: Option<f32>) {
        let s = self.state();
        let (tx, ty) = self.apply_transform(x, y);
        if text.is_empty() { return; }

        let color = self.fill_color_at(tx, ty);
        let font_size = s.font_size;

        let text_width = text.len() as f32 * font_size * 0.6;
        let scale = max_width.map(|mw| (mw / text_width).min(1.0)).unwrap_or(1.0);

        let (align_offset, baseline_offset) = self.text_offsets(text, font_size);
        let draw_x = tx + align_offset;
        let draw_y = ty + baseline_offset + font_size * 0.8;

        let clip = fortrust_layout::Rect { x: 0.0, y: 0.0, width: self.width as f32, height: self.height as f32 };
        let rect = fortrust_layout::Rect { x: draw_x, y: draw_y - font_size, width: text_width * scale, height: font_size * 1.2 };

        let font_family = s.font_family.clone();
        self.paint_text_with_shadow(text, draw_x, draw_y, color, font_size, scale, &font_family);

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

    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, rotation: f32, start_angle: f32, end_angle: f32, anticlockwise: bool) {
        if rx <= 0.0 || ry <= 0.0 { return; }
        if self.subpath_empty {
            let (sx, sy) = angle_to_point_ellipse(cx, cy, rx, ry, rotation, start_angle);
            self.path.push(PathOp::MoveTo(sx, sy));
            self.subpath_empty = false;
        }
        self.path.push(PathOp::Ellipse(cx, cy, rx, ry, rotation, start_angle, end_angle, anticlockwise));
    }

    pub fn arc_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, radius: f32) {
        if radius <= 0.0 { return; }
        // Get current point (last path point or 0,0)
        let (x0, y0) = self.get_current_point();

        let dx1 = x0 - x1;
        let dy1 = y0 - y1;
        let dx2 = x2 - x1;
        let dy2 = y2 - y1;
        let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
        let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
        if len1 < 0.001 || len2 < 0.001 { return; }

        // Compute the angle between the two line segments
        let cos_theta = (dx1 * dx2 + dy1 * dy2) / (len1 * len2);
        let theta = cos_theta.clamp(-1.0, 1.0).acos();
        // If lines are collinear, just line to (x1, y1) then (x2, y2)
        if theta.abs() < 0.001 || (std::f32::consts::PI - theta).abs() < 0.001 {
            self.line_to(x1, y1);
            self.line_to(x2, y2);
            return;
        }

        // Compute tangent distance
        let tan_half_theta = (theta * 0.5).tan();
        let t = radius / tan_half_theta;
        let t_clamped = t.min(len1 * 0.5).min(len2 * 0.5);

        // Arc start point along first line
        let arc_start_x = x1 + (dx1 / len1) * t_clamped;
        let arc_start_y = y1 + (dy1 / len1) * t_clamped;
        // Arc end point along second line
        let arc_end_x = x1 + (dx2 / len2) * t_clamped;
        let arc_end_y = y1 + (dy2 / len2) * t_clamped;

        // Compute center of arc
        let mid_x = (arc_start_x + arc_end_x) * 0.5;
        let mid_y = (arc_start_y + arc_end_y) * 0.5;
        let _bisect_len = (t_clamped * t_clamped + radius * radius).sqrt();
        let perp_x = -(arc_end_y - arc_start_y);
        let perp_y = arc_end_x - arc_start_x;
        let perp_len = (perp_x * perp_x + perp_y * perp_y).sqrt();
        let perp_len = if perp_len < 0.001 { 1.0 } else { perp_len };

        let cx = mid_x + (perp_x / perp_len) * (radius / tan_half_theta);
        let cy = mid_y + (perp_y / perp_len) * (radius / tan_half_theta);

        let start_angle = (arc_start_y - cy).atan2(arc_start_x - cx);
        let end_angle = (arc_end_y - cy).atan2(arc_end_x - cx);
        let anticlockwise = (end_angle - start_angle) < 0.0;

        self.line_to(arc_start_x, arc_start_y);
        self.arc(cx, cy, radius, start_angle, end_angle, anticlockwise);
    }

    fn get_current_point(&self) -> (f32, f32) {
        for op in self.path.iter().rev() {
            match op {
                PathOp::MoveTo(x, y) | PathOp::LineTo(x, y) => return (*x, *y),
                PathOp::BezierCurve(_, _, _, _, x, y) => return (*x, *y),
                PathOp::QuadraticCurve(_, _, x, y) => return (*x, *y),
                PathOp::Ellipse(cx, cy, rx, ry, rot, _, ea, _) => {
                    let (ex, ey) = angle_to_point_ellipse(*cx, *cy, *rx, *ry, *rot, *ea);
                    return (ex, ey);
                }
                _ => {}
            }
        }
        (0.0, 0.0)
    }

    pub fn fill(&mut self) {
        let segments = self.flatten_path();
        if segments.is_empty() { return; }
        self.draw_fill_shadow(&segments);
        self.rasterize_fill(&segments);
        self.path.clear();
        self.subpath_empty = true;
    }

    pub fn stroke(&mut self) {
        let segments = self.flatten_path();
        if segments.is_empty() { return; }
        self.stroke_with_shadow(&segments);
        self.rasterize_stroke(&segments);
        self.path.clear();
        self.subpath_empty = true;
    }

    // ─── Text drawing ───

    pub fn stroke_text(&mut self, text: &str, x: f32, y: f32, max_width: Option<f32>) {
        let s = self.state();
        let (tx, ty) = self.apply_transform(x, y);
        if text.is_empty() { return; }

        let color = self.stroke_color_at(tx, ty);
        let font_size = s.font_size;

        let text_width = text.len() as f32 * font_size * 0.6;
        let scale = max_width.map(|mw| (mw / text_width).min(1.0)).unwrap_or(1.0);

        let (align_offset, baseline_offset) = self.text_offsets(text, font_size);
        let draw_x = tx + align_offset;
        let draw_y = ty + baseline_offset + font_size * 0.8;

        let clip = fortrust_layout::Rect { x: 0.0, y: 0.0, width: self.width as f32, height: self.height as f32 };
        let rect = fortrust_layout::Rect { x: draw_x, y: draw_y - font_size, width: text_width * scale, height: font_size * 1.2 };

        let font_family = s.font_family.clone();
        self.paint_text_with_shadow(text, draw_x, draw_y, color, font_size, scale, &font_family);

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
        if text.is_empty() { return 0.0; }
        let font_size = self.state().font_size;
        let font_family = self.state().font_family.clone();
        // Use cosmic-text for accurate measurement
        let measured = fortrust_paint::TextRenderer::measure_text(text, font_size, &font_family);
        if measured > 0.0 {
            measured as f64
        } else {
            // Fallback estimate
            (text.len() as f32 * font_size * 0.6) as f64
        }
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
        let smoothing = self.state().image_smoothing_enabled;

        let scale_x = img_w as f32 / draw_w as f32;
        let scale_y = img_h as f32 / draw_h as f32;

        for py in 0..draw_h {
            for px in 0..draw_w {
                let dst_px = (tx + px as f32) as i32;
                let dst_py = (ty + py as f32) as i32;
                if dst_px < 0 || dst_py < 0 || dst_px >= self.width as i32 || dst_py >= self.height as i32 { continue; }
                if !self.is_in_clip(dst_px as usize, dst_py as usize) { continue; }

                let src_x = px as f32 * scale_x;
                let src_y = py as f32 * scale_y;

                let src = if smoothing && (scale_x != 1.0 || scale_y != 1.0) {
                    sample_bilinear(data, img_w, img_h, src_x, src_y)
                } else {
                    sample_nearest(data, img_w, img_h, src_x, src_y)
                };

                let dst_idx = (dst_py as u32 * self.width + dst_px as u32) as usize * 4;
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
                PathOp::Ellipse(cx, cy, rx, ry, rot, sa, ea, acw) => {
                    let (tcx, tcy) = self.apply_transform(*cx, *cy);
                    let (rdx, rdy) = self.apply_transform(*cx + *rx, *cy);
                    let trx = ((rdx - tcx).powi(2) + (rdy - tcy).powi(2)).sqrt().max(0.001);
                    let (rudx, rudy) = self.apply_transform(*cx, *cy + *ry);
                    let try_ = ((rudx - tcx).powi(2) + (rudy - tcy).powi(2)).sqrt().max(0.001);
                    let cos_r = rot.cos();
                    let sin_r = rot.sin();
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
                        let px = tcx + cos_r * trx * angle.cos() - sin_r * try_ * angle.sin();
                        let py = tcy + sin_r * trx * angle.cos() + cos_r * try_ * angle.sin();
                        if s > 0 {
                            segments.push([current, (px, py)]);
                        }
                        current = (px, py);
                        if s == 0 { subpath_start = (px, py); }
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

    fn draw_fill_shadow(&mut self, segments: &[[(f32, f32); 2]]) {
        let s = self.state();
        if s.shadow_blur <= 0.0 || s.shadow_color[3] == 0 { return; }
        let shad = s.shadow_color;
        let blur = s.shadow_blur;
        let spread = blur.max(1.0) as i32;
        let op = s.global_composite_op;
        // Create a temporary buffer for shadow rendering
        let w = self.width as usize;
        let h = self.height as usize;
        let mut shadow_buf = vec![0u8; w * h * 4];

        for seg in segments {
            let (x1, y1) = seg[0];
            let (x2, y2) = seg[1];
            for dy in -spread..=spread {
                for dx in -spread..=spread {
                    let dist = ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt();
                    let falloff = (1.0 - (dist / (spread as f32 + 1.0))).max(0.0);
                    let mut sa = shad;
                    sa[3] = (shad[3] as f32 * falloff * s.global_alpha) as u8;
                    let offset_x = s.shadow_offset_x + dx as f32;
                    let offset_y = s.shadow_offset_y + dy as f32;

                    let min_y = ((y1 + offset_y).min(y2 + offset_y)).floor().max(0.0) as usize;
                    let max_y = ((y1 + offset_y).max(y2 + offset_y)).ceil().min(h as f32) as usize;
                    for py in min_y..max_y {
                        let yy = py as f32 + 0.5;
                        if (yy < (y1 + offset_y).min(y2 + offset_y)) || (yy >= (y1 + offset_y).max(y2 + offset_y)) { continue; }
                        let t = (yy - (y1 + offset_y)) / ((y2 + offset_y) - (y1 + offset_y));
                        let ix = (x1 + offset_x) + t * ((x2 + offset_x) - (x1 + offset_x));
                        let px = ix.floor().max(0.0) as usize;
                        if px < w && py < h {
                            let idx = (py * w + px) * 4;
                            composite_blend(&mut shadow_buf[idx..idx + 4], sa, CompositeOp::SourceOver);
                        }
                    }
                }
            }
        }
        // Blend shadow buffer into main buffer
        for (dst, src) in self.buffer.chunks_mut(4).zip(shadow_buf.chunks(4)) {
            if src[3] > 0 {
                composite_blend(dst, [src[0], src[1], src[2], src[3]], op);
            }
        }
    }

    fn rasterize_fill(&mut self, segments: &[[(f32, f32); 2]]) {
        let op = self.state().global_composite_op;

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
                    let color = self.fill_color_at(px as f32 + 0.5, py as f32 + 0.5);
                    composite_blend(&mut self.buffer[idx..idx + 4], color, op);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cap(pixels: &mut [u8], w: usize, h: usize, cx: f32, cy: f32, _angle: f32, half: f32, rgba: [u8; 4], cap: LineCap, op: CompositeOp, clip: &[bool], width: usize) {
        match cap {
            LineCap::Round => {
                for dy in -(half.ceil() as i32)..=(half.ceil() as i32) {
                    for dx in -(half.ceil() as i32)..=(half.ceil() as i32) {
                        let px = cx as i32 + dx;
                        let py = cy as i32 + dy;
                        if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
                        let dist = ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt();
                        if dist <= half {
                            let idx = (py as usize * w + px as usize) * 4;
                            if clip.is_empty() || clip[py as usize * width + px as usize] {
                                composite_blend(&mut pixels[idx..idx + 4], rgba, op);
                            }
                        }
                    }
                }
            }
            LineCap::Square => {
                let sx = (cx - half).floor().max(0.0) as usize;
                let sy = (cy - half).floor().max(0.0) as usize;
                let ex = (cx + half).ceil().min(w as f32) as usize;
                let ey = (cy + half).ceil().min(h as f32) as usize;
                for py in sy..ey {
                    for px in sx..ex {
                        let idx = (py * w + px) * 4;
                        if clip.is_empty() || clip[py * width + px] {
                            composite_blend(&mut pixels[idx..idx + 4], rgba, op);
                        }
                    }
                }
            }
            LineCap::Butt => {}
        }
    }

    fn rasterize_stroke(&mut self, segments: &[[(f32, f32); 2]]) {
        let (lw, half, cap, join, op, dash, dash_offset, has_dash) = {
            let s = self.state();
            (
                s.line_width.max(0.5),
                s.line_width.max(0.5) / 2.0,
                s.line_cap,
                s.line_join,
                s.global_composite_op,
                s.line_dash.clone(),
                s.line_dash_offset,
                !s.line_dash.is_empty(),
            )
        };

        let w = self.width as usize;
        let h = self.height as usize;

        let clip_ref: Vec<bool> = match &self.clip_mask {
            Some(m) => m.clone(),
            None => Vec::new(),
        };

        let total_dash_len: f32 = if has_dash {
            dash.iter().sum::<f32>().max(0.001)
        } else {
            1.0
        };
        for seg_idx in 0..segments.len() {
            let seg = &segments[seg_idx];
            let (x1, y1) = seg[0];
            let (x2, y2) = seg[1];
            let dx = x2 - x1;
            let dy = y2 - y1;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 0.001 { continue; }
            let ux = dx / len;
            let uy = dy / len;

            // Evaluate stroke color at midpoint of segment for gradient/pattern support
            let mx = (x1 + x2) * 0.5;
            let my = (y1 + y2) * 0.5;
            let rgba = self.stroke_color_at(mx, my);

            if has_dash {
                let mut dist = 0.0;
                let mut dash_idx = ((dash_offset.abs() / total_dash_len) * dash.len() as f32) as usize % dash.len();
                let mut dash_pos = dash_offset.abs() % total_dash_len;
                let mut drawing = true;

                while dist < len {
                    let remaining = len - dist;
                    let dash_len = if dash.is_empty() { remaining } else { dash[dash_idx] };
                    let draw_len = dash_len.min(remaining);

                    if drawing && draw_len > 0.0 {
                        let seg_x1 = x1 + ux * dist;
                        let seg_y1 = y1 + uy * dist;
                        let seg_x2 = x1 + ux * (dist + draw_len);
                        let seg_y2 = y1 + uy * (dist + draw_len);

                        Self::draw_thick_line(
                            self.buffer.as_mut_slice(), w, h,
                            seg_x1, seg_y1, seg_x2, seg_y2,
                            lw, half, rgba, cap, op, &clip_ref, w,
                        );
                        Self::draw_cap(self.buffer.as_mut_slice(), w, h, seg_x2, seg_y2, ux.atan2(uy), half, rgba, cap, op, &clip_ref, w);
                    }

                    if !drawing && draw_len > 0.0 {
                        let seg_x1 = x1 + ux * (dist + draw_len);
                        let seg_y1 = y1 + uy * (dist + draw_len);
                        Self::draw_cap(self.buffer.as_mut_slice(), w, h, seg_x1, seg_y1, ux.atan2(uy), half, rgba, cap, op, &clip_ref, w);
                    }

                    dist += draw_len;
                    dash_pos += draw_len;
                    if dash_pos >= dash[dash_idx] {
                        dash_pos = 0.0;
                        dash_idx = (dash_idx + 1) % dash.len();
                        drawing = !drawing;
                    }
                }
            } else {
                Self::draw_thick_line(
                    self.buffer.as_mut_slice(), w, h,
                    x1, y1, x2, y2,
                    lw, half, rgba, cap, op, &clip_ref, w,
                );
            }

            if seg_idx == 0 {
                Self::draw_cap(self.buffer.as_mut_slice(), w, h, x1, y1, ux.atan2(uy), half, rgba, cap, op, &clip_ref, w);
            }
            if seg_idx == segments.len() - 1 {
                Self::draw_cap(self.buffer.as_mut_slice(), w, h, x2, y2, ux.atan2(uy), half, rgba, cap, op, &clip_ref, w);
            }

            if seg_idx + 1 < segments.len() {
                let (nx1, ny1) = segments[seg_idx + 1][0];
                let (nx2, ny2) = segments[seg_idx + 1][1];
                let ndx = nx2 - nx1;
                let ndy = ny2 - ny1;
                let nlen = (ndx * ndx + ndy * ndy).sqrt();
                if nlen > 0.001 {
                    let nux = ndx / nlen;
                    let nuy = ndy / nlen;
                    Self::draw_join(
                        self.buffer.as_mut_slice(), w, h,
                        x2, y2, ux, uy, nux, nuy,
                        lw, half, rgba, join, op, &clip_ref, w,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_thick_line(
        pixels: &mut [u8], w: usize, h: usize,
        x1: f32, y1: f32, x2: f32, y2: f32,
        _lw: f32, half: f32, rgba: [u8; 4], _cap: LineCap,
        op: CompositeOp, clip: &[bool], _width: usize,
    ) {
        let dx = x2 - x1;
        let dy = y2 - y1;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.001 {
            // Single pixel
            let px = x1 as i32;
            let py = y1 as i32;
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                let idx = (py as usize * w + px as usize) * 4;
                if clip.is_empty() || clip[py as usize * w + px as usize] {
                    composite_blend(&mut pixels[idx..idx + 4], rgba, op);
                }
            }
            return;
        }

        // Perpendicular direction
        let perp_x = -dy / len * half;
        let perp_y = dx / len * half;

        // Four corners of the thick line rect
        let corners = [
            (x1 - perp_x, y1 - perp_y),
            (x1 + perp_x, y1 + perp_y),
            (x2 + perp_x, y2 + perp_y),
            (x2 - perp_x, y2 - perp_y),
        ];

        let min_x = corners.iter().fold(f32::MAX, |a, &(x, _)| a.min(x)).floor().max(0.0) as usize;
        let max_x = corners.iter().fold(f32::MIN, |a, &(x, _)| a.max(x)).ceil().min(w as f32) as usize;
        let min_y = corners.iter().fold(f32::MAX, |a, &(_, y)| a.min(y)).floor().max(0.0) as usize;
        let max_y = corners.iter().fold(f32::MIN, |a, &(_, y)| a.max(y)).ceil().min(h as f32) as usize;

        for py in min_y..max_y {
            for px in min_x..max_x {
                let px_f = px as f32 + 0.5;
                let py_f = py as f32 + 0.5;

                // Project onto line segment
                let t = ((px_f - x1) * dx + (py_f - y1) * dy) / (len * len);
                let t = t.clamp(0.0, 1.0);
                let near_x = x1 + t * dx;
                let near_y = y1 + t * dy;
                let dist = ((px_f - near_x).powi(2) + (py_f - near_y).powi(2)).sqrt();

                if dist <= half {
                    let idx = (py * w + px) * 4;
                    if clip.is_empty() || clip[py * w + px] {
                        composite_blend(&mut pixels[idx..idx + 4], rgba, op);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_join(
        pixels: &mut [u8], w: usize, h: usize,
        jx: f32, jy: f32, ux1: f32, uy1: f32, ux2: f32, uy2: f32,
        _lw: f32, half: f32, rgba: [u8; 4], join: LineJoin,
        op: CompositeOp, clip: &[bool], _width: usize,
    ) {
        match join {
            LineJoin::Round => {
                let steps = 12;
                for s in 0..steps {
                    let t = s as f32 / steps as f32;
                    let angle = ux1.atan2(uy1) * (1.0 - t) + ux2.atan2(uy2) * t;
                    let ex = jx + angle.cos() * half;
                    let ey = jy + angle.sin() * half;
                    let px = ex as i32;
                    let py = ey as i32;
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                        let idx = (py as usize * w + px as usize) * 4;
                        if clip.is_empty() || clip[py as usize * w + px as usize] {
                            composite_blend(&mut pixels[idx..idx + 4], rgba, op);
                        }
                    }
                }
            }
            LineJoin::Bevel => {
                // Fill the triangle between the two outward edges
                let perp1_x = -uy1 * half;
                let perp1_y = ux1 * half;
                let perp2_x = -uy2 * half;
                let perp2_y = ux2 * half;
                for t in 0..10 {
                    let frac = t as f32 / 10.0;
                    let ex = jx + perp1_x * (1.0 - frac) + perp2_x * frac;
                    let ey = jy + perp1_y * (1.0 - frac) + perp2_y * frac;
                    let px = ex as i32;
                    let py = ey as i32;
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                        let idx = (py as usize * w + px as usize) * 4;
                        if clip.is_empty() || clip[py as usize * w + px as usize] {
                            composite_blend(&mut pixels[idx..idx + 4], rgba, op);
                        }
                    }
                }
            }
            LineJoin::Miter => {
                // Simple miter: just fill the corner area
                let px = jx as i32;
                let py = jy as i32;
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    let idx = (py as usize * w + px as usize) * 4;
                    if clip.is_empty() || clip[py as usize * w + px as usize] {
                        composite_blend(&mut pixels[idx..idx + 4], rgba, op);
                    }
                }
            }
        }
    }

    fn stroke_with_shadow(&mut self, segments: &[[(f32, f32); 2]]) {
        let (shad, _blur, spread, op, half, lw, sox, soy, lc, ga) = {
            let s = self.state();
            if s.shadow_blur <= 0.0 || s.shadow_color[3] == 0 { return; }
            (
                s.shadow_color,
                s.shadow_blur,
                s.shadow_blur.max(1.0) as i32,
                s.global_composite_op,
                s.line_width.max(0.5) / 2.0,
                s.line_width.max(0.5),
                s.shadow_offset_x,
                s.shadow_offset_y,
                s.line_cap,
                s.global_alpha,
            )
        };

        for seg in segments {
            let (x1, y1) = seg[0];
            let (x2, y2) = seg[1];
            for dy in -spread..=spread {
                for dx in -spread..=spread {
                    let dist = ((dx as f32).powi(2) + (dy as f32).powi(2)).sqrt();
                    let falloff = (1.0 - (dist / (spread as f32 + 1.0))).max(0.0);
                    let mut sa = shad;
                    sa[3] = (shad[3] as f32 * falloff * ga) as u8;
                    Self::draw_thick_line(
                        self.buffer.as_mut_slice(),
                        self.width as usize, self.height as usize,
                        x1 + sox + dx as f32,
                        y1 + soy + dy as f32,
                        x2 + sox + dx as f32,
                        y2 + soy + dy as f32,
                        lw, half, sa,
                        lc, op, &[],
                        self.width as usize,
                    );
                }
            }
        }
    }
}

fn angle_to_point(cx: f32, cy: f32, r: f32, angle: f32) -> (f32, f32) {
    (cx + r * angle.cos(), cy + r * angle.sin())
}

fn sample_nearest(data: &[u8], w: u32, h: u32, sx: f32, sy: f32) -> [u8; 4] {
    let ix = (sx.floor() as i32).clamp(0, w as i32 - 1) as u32;
    let iy = (sy.floor() as i32).clamp(0, h as i32 - 1) as u32;
    let idx = (iy * w + ix) as usize * 4;
    if idx + 4 <= data.len() {
        [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]
    } else {
        [0, 0, 0, 0]
    }
}

fn sample_bilinear(data: &[u8], w: u32, h: u32, sx: f32, sy: f32) -> [u8; 4] {
    let ix = sx.floor();
    let iy = sy.floor();
    let fx = sx - ix;
    let fy = sy - iy;
    let x0 = (ix as i32).clamp(0, w as i32 - 1) as u32;
    let y0 = (iy as i32).clamp(0, h as i32 - 1) as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let p00 = (y0 * w + x0) as usize * 4;
    let p01 = (y0 * w + x1) as usize * 4;
    let p10 = (y1 * w + x0) as usize * 4;
    let p11 = (y1 * w + x1) as usize * 4;
    let mut out = [0u8; 4];
    for c in 0..4 {
        if p00 + c < data.len() && p01 + c < data.len() && p10 + c < data.len() && p11 + c < data.len() {
            let v00 = data[p00 + c] as f32;
            let v01 = data[p01 + c] as f32;
            let v10 = data[p10 + c] as f32;
            let v11 = data[p11 + c] as f32;
            let top = v00 + (v01 - v00) * fx;
            let bot = v10 + (v11 - v10) * fx;
            out[c] = (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

fn angle_to_point_ellipse(cx: f32, cy: f32, rx: f32, ry: f32, rotation: f32, angle: f32) -> (f32, f32) {
    let cos_r = rotation.cos();
    let sin_r = rotation.sin();
    let x = cx + cos_r * rx * angle.cos() - sin_r * ry * angle.sin();
    let y = cy + sin_r * rx * angle.cos() + cos_r * ry * angle.sin();
    (x, y)
}
