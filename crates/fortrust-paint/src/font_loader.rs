//! Helpers for loading @font-face fonts into a [`TextRenderer`].
//!
//! Callers fetch font file bytes (via HTTP, file system, etc.) and then
//! pass them to [`load_font_data`] or the higher-level [`process_font_faces`].

use fortrust_style::FontFaceRule;

use crate::TextRenderer;

/// Process a list of @font-face rules by loading pre-fetched font data for
/// each matching rule.
///
/// `font_data_map` should map a font-face source URL to the raw bytes of the
/// downloaded font file.  Only rules whose `sources` have an entry in the map
/// will be loaded.
///
/// Loaded fonts are registered with the cosmic-text `FontSystem` and will be
/// available for subsequent text rendering using the font-family name declared
/// in the @font-face rule.
pub fn process_font_faces(
    renderer: &mut TextRenderer,
    rules: &[FontFaceRule],
    font_data_map: &std::collections::HashMap<String, Vec<u8>>,
) {
    for rule in rules {
        for source in &rule.sources {
            if let Some(data) = font_data_map.get(&source.url) {
                renderer.load_font_data(data.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TextRenderer, TextRun};
    use fortrust_layout::Rect;
    use fortrust_style::{Color, FontWeight, FontStyle};

    #[test]
    fn load_font_data_does_not_crash() {
        // Ensure that loading garbage data is handled gracefully
        // (fontdb should reject invalid font data without panicking).
        let mut renderer = TextRenderer::new();
        renderer.load_font_data(vec![0u8; 128]);
        // Even with failed font load, rendering should still work
        let run = TextRun {
            text: "Hello".to_string(),
            font_family: "sans-serif".to_string(),
            font_size_px: 16.0,
            font_weight: FontWeight::Normal,
            font_style: FontStyle::Normal,
            color: Color { r: 255, g: 255, b: 255, a: 255 },
            rect: Rect { x: 0.0, y: 0.0, width: 100.0, height: 30.0 },
        };
        let result = renderer.rasterize(&run);
        assert!(result.pixels.chunks(4).any(|p| p[3] > 0));
    }

    #[test]
    fn process_font_faces_handles_empty_rules() {
        let mut renderer = TextRenderer::new();
        let map = std::collections::HashMap::new();
        // Should not panic
        process_font_faces(&mut renderer, &[], &map);
    }
}
