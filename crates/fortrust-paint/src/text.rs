//! Text rendering via cosmic-text + swash.
//!
//! Provides [`TextRenderer`] which rasterises text runs into RGBA pixel buffers
//! using cosmic-text's shaping and swash's rasteriser.

use cosmic_text::{
    Attrs, Buffer as CosmicBuffer, Color as CosmicColor, Family, FontSystem, Metrics, Shaping,
    Style as CosmicStyle, SwashCache, Weight as CosmicWeight,
};
use fortrust_layout::Rect;
use fortrust_style::{Color, FontStyle, FontWeight};

/// Parameters describing a single text render operation.
#[derive(Debug, Clone)]
pub struct TextRun {
    /// The text content to render.
    pub text: String,
    /// Font family name. Empty string means system default sans-serif.
    pub font_family: String,
    /// Font size in pixels.
    pub font_size_px: f32,
    /// Font weight.
    pub font_weight: FontWeight,
    /// Font style (normal, italic, oblique).
    pub font_style: FontStyle,
    /// Text color in RGBA.
    pub color: Color,
    /// Position and bounds for the text.
    pub rect: Rect,
}

/// Rasterised text output — an RGBA pixel buffer covering the text bounds.
#[derive(Debug, Clone)]
pub struct RasterizedText {
    /// RGBA pixel data (4 bytes per pixel, row-major).
    pub pixels: Vec<u8>,
    /// Width of the pixel buffer.
    pub width: u32,
    /// Height of the pixel buffer.
    pub height: u32,
}

/// A reusable text renderer backed by cosmic-text and swash.
///
/// Holds a [`FontSystem`] (with system fonts loaded) and a [`SwashCache`]
/// for glyph rasterisation. Create once and reuse across frames.
pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl TextRenderer {
    /// Create a new `TextRenderer` with system fonts loaded.
    ///
    /// The font fallback chain is:
    /// 1. Requested font family (if non-empty)
    /// 2. System default sans-serif
    /// 3. Bundled fallback (handled internally by cosmic-text)
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    /// Render a [`TextRun`] to a standalone RGBA pixel buffer.
    ///
    /// Returns [`RasterizedText`] with dimensions matching `run.rect` (clamped
    /// to at least 1×1). Transparent pixels are `[0,0,0,0]`.
    pub fn rasterize(&mut self, run: &TextRun) -> RasterizedText {
        let w = (run.rect.width.ceil().max(1.0)) as u32;
        let h = (run.rect.height.ceil().max(1.0)) as u32;
        let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];

        if run.text.is_empty() || run.rect.width <= 0.0 || run.rect.height <= 0.0 {
            return RasterizedText {
                pixels,
                width: w,
                height: h,
            };
        }

        let font_size = run.font_size_px.clamp(6.0, 200.0);
        let line_height = font_size * 1.2;
        let metrics = Metrics::new(font_size, line_height);

        let mut buffer = CosmicBuffer::new(&mut self.font_system, metrics);

        let attrs = build_attrs(&run.font_family, run.font_weight, run.font_style);
        buffer.set_text(&mut self.font_system, &run.text, attrs, Shaping::Advanced);
        buffer.set_size(
            &mut self.font_system,
            Some(run.rect.width),
            Some(run.rect.height),
        );

        let color_packed = CosmicColor::rgba(run.color.r, run.color.g, run.color.b, run.color.a);

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            color_packed,
            |x, y, _w, _h, pix_color| {
                let [r, g, b, a] = pix_color.as_rgba();
                if a == 0 {
                    return;
                }
                if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                    return;
                }
                let idx = (y as usize * w as usize + x as usize) * 4;
                blend_pixel(&mut pixels[idx..idx + 4], [r, g, b, a]);
            },
        );

        RasterizedText {
            pixels,
            width: w,
            height: h,
        }
    }

    /// Composite text directly into an existing framebuffer.
    ///
    /// This is the hot-path used by the renderer: it writes glyphs at the
    /// position specified by `rect` into the given `pixels` buffer of size
    /// `fb_width × fb_height`, respecting the `clip` rectangle.
    pub fn render_into(
        &mut self,
        pixels: &mut [u8],
        fb_width: usize,
        fb_height: usize,
        clip: Rect,
        rect: Rect,
        text: &str,
        font_size_px: f32,
        font_weight: FontWeight,
        font_style: FontStyle,
        rgba: [u8; 4],
    ) {
        if text.is_empty() || rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        let font_size = font_size_px.clamp(6.0, 200.0);
        let metrics = Metrics::new(font_size, font_size * 1.2);

        let mut buffer = CosmicBuffer::new(&mut self.font_system, metrics);

        let attrs = build_attrs("", font_weight, font_style);
        buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);

        let max_w = (rect.width * 1.5).ceil().max(100.0);
        let max_h = (rect.height * 2.0).ceil().max(font_size * 2.0);
        buffer.set_size(&mut self.font_system, Some(max_w), Some(max_h));

        let color_packed = CosmicColor::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);

        buffer.draw(
            &mut self.font_system,
            &mut self.swash_cache,
            color_packed,
            |x, y, _w, _h, pix_color| {
                let [r, g, b, a] = pix_color.as_rgba();
                if a == 0 {
                    return;
                }
                let px = (rect.x as i32 + x)
                    .max(clip.x as i32)
                    .min((clip.x + clip.width) as i32 - 1);
                let py = (rect.y as i32 + y)
                    .max(clip.y as i32)
                    .min((clip.y + clip.height) as i32 - 1);
                if px < 0 || px >= fb_width as i32 || py < 0 || py >= fb_height as i32 {
                    return;
                }
                let idx = (py as usize * fb_width + px as usize) * 4;
                blend_pixel(&mut pixels[idx..idx + 4], [r, g, b, a]);
            },
        );
    }

    /// Simplified render-into for callers that only have color as `[u8; 4]`
    /// and no font weight/style info (uses defaults).
    pub fn render_into_simple(
        &mut self,
        pixels: &mut [u8],
        fb_width: usize,
        fb_height: usize,
        clip: Rect,
        rect: Rect,
        text: &str,
        font_size_px: f32,
        rgba: [u8; 4],
    ) {
        self.render_into(
            pixels,
            fb_width,
            fb_height,
            clip,
            rect,
            text,
            font_size_px,
            FontWeight::Normal,
            FontStyle::Normal,
            rgba,
        );
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Build cosmic-text `Attrs` from our style types.
fn build_attrs(font_family: &str, weight: FontWeight, style: FontStyle) -> Attrs<'_> {
    let mut attrs = Attrs::new();

    // Font family: requested → sans-serif fallback
    if !font_family.is_empty() {
        attrs = attrs.family(Family::Name(font_family));
    } else {
        attrs = attrs.family(Family::SansSerif);
    }

    // Font weight mapping
    attrs = attrs.weight(match weight {
        FontWeight::Normal => CosmicWeight::NORMAL,
        FontWeight::Bold => CosmicWeight::BOLD,
        FontWeight::Bolder => CosmicWeight::EXTRA_BOLD,
        FontWeight::Lighter => CosmicWeight::LIGHT,
        FontWeight::Number(n) => CosmicWeight(n),
    });

    // Font style mapping
    attrs = attrs.style(match style {
        FontStyle::Normal => CosmicStyle::Normal,
        FontStyle::Italic => CosmicStyle::Italic,
        FontStyle::Oblique => CosmicStyle::Oblique,
    });

    attrs
}

/// Alpha-blend a source RGBA pixel over a destination.
#[inline(always)]
fn blend_pixel(dst: &mut [u8], src: [u8; 4]) {
    let alpha = src[3] as f32 / 255.0;
    if alpha <= 0.0 {
        return;
    }
    let inv = 1.0 - alpha;
    dst[0] = (src[0] as f32 * alpha + dst[0] as f32 * inv) as u8;
    dst[1] = (src[1] as f32 * alpha + dst[1] as f32 * inv) as u8;
    dst[2] = (src[2] as f32 * alpha + dst[2] as f32 * inv) as u8;
    dst[3] = ((src[3] as f32 + dst[3] as f32 * inv).min(255.0)) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_simple_ascii_produces_nonempty_pixels() {
        let mut renderer = TextRenderer::new();
        let run = TextRun {
            text: "Hello, Fortrust!".to_string(),
            font_family: String::new(),
            font_size_px: 16.0,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            color: Color { r: 255, g: 255, b: 255, a: 255 },
            rect: Rect { x: 0.0, y: 0.0, width: 200.0, height: 30.0 },
        };

        let result = renderer.rasterize(&run);
        assert_eq!(result.width, 200);
        assert_eq!(result.height, 30);
        // At least some pixels should be non-transparent (glyphs were rendered)
        let non_transparent = result.pixels.chunks(4).filter(|p| p[3] > 0).count();
        assert!(
            non_transparent > 0,
            "Expected rendered glyphs to produce non-transparent pixels"
        );
    }

    #[test]
    fn render_bold_text() {
        let mut renderer = TextRenderer::new();
        let run = TextRun {
            text: "Bold".to_string(),
            font_family: String::new(),
            font_size_px: 20.0,
            font_weight: FontWeight::Bold,
            font_style: FontStyle::Normal,
            color: Color { r: 0, g: 0, b: 0, a: 255 },
            rect: Rect { x: 0.0, y: 0.0, width: 100.0, height: 30.0 },
        };

        let result = renderer.rasterize(&run);
        let non_transparent = result.pixels.chunks(4).filter(|p| p[3] > 0).count();
        assert!(non_transparent > 0, "Bold text should produce visible glyphs");
    }

    #[test]
    fn render_italic_text() {
        let mut renderer = TextRenderer::new();
        let run = TextRun {
            text: "Italic".to_string(),
            font_family: String::new(),
            font_size_px: 16.0,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Italic,
            color: Color { r: 128, g: 128, b: 128, a: 255 },
            rect: Rect { x: 0.0, y: 0.0, width: 100.0, height: 30.0 },
        };

        let result = renderer.rasterize(&run);
        let non_transparent = result.pixels.chunks(4).filter(|p| p[3] > 0).count();
        assert!(non_transparent > 0, "Italic text should produce visible glyphs");
    }

    #[test]
    fn render_empty_text_produces_all_transparent() {
        let mut renderer = TextRenderer::new();
        let run = TextRun {
            text: String::new(),
            font_family: String::new(),
            font_size_px: 16.0,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            color: Color { r: 255, g: 255, b: 255, a: 255 },
            rect: Rect { x: 0.0, y: 0.0, width: 100.0, height: 20.0 },
        };

        let result = renderer.rasterize(&run);
        let non_transparent = result.pixels.chunks(4).filter(|p| p[3] > 0).count();
        assert_eq!(non_transparent, 0, "Empty text should produce no visible pixels");
    }

    #[test]
    fn render_into_framebuffer() {
        let mut renderer = TextRenderer::new();
        let fb_width = 320;
        let fb_height = 240;
        let mut pixels = vec![0u8; fb_width * fb_height * 4];
        let clip = Rect { x: 0.0, y: 0.0, width: 320.0, height: 240.0 };
        let rect = Rect { x: 10.0, y: 10.0, width: 200.0, height: 30.0 };

        renderer.render_into(
            &mut pixels,
            fb_width,
            fb_height,
            clip,
            rect,
            "Test text",
            16.0,
            FontWeight::Normal,
            FontStyle::Normal,
            [255, 255, 255, 255],
        );

        let non_transparent = pixels.chunks(4).filter(|p| p[3] > 0).count();
        assert!(non_transparent > 0, "render_into should produce visible glyphs");
    }
}
