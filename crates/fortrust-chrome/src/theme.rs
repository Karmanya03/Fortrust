use egui::Color32;


#[derive(Clone, Copy)]
pub struct FortrustTheme {
    // Glass surfaces
    pub glass_bg: Color32,          // Main panel background (translucent)
    pub glass_border: Color32,      // Panel border / separator
    pub glass_hover: Color32,       // Panel hover state overlay

    // Accent (changes per theme)
    pub accent_primary: Color32,    // CTA buttons, active tab indicator, links
    pub accent_secondary: Color32,  // Hover states, secondary actions
    pub accent_shield: Color32,     // Shields UP color (green)
    pub accent_shield_warn: Color32,// Shields partial (amber)
    pub accent_shield_off: Color32, // Shields DOWN (muted)

    // Text
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_placeholder: Color32,
    pub text_on_accent: Color32,

    // Speed dial
    pub tile_bg: Color32,
    pub tile_hover_overlay: Color32,
    pub tile_shadow: Color32,
}

impl FortrustTheme {
    /// Opera Air-inspired dark theme (default)
    pub fn dark() -> Self {
        Self::dark_with_glass_strength(82)
    }

    pub fn dark_with_glass_strength(glass_strength: u8) -> Self {
        let glass_strength = glass_strength.min(100);
        let glass_bg_alpha = scale_alpha(210, glass_strength);
        let glass_border_alpha = scale_alpha(18, glass_strength);
        let glass_hover_alpha = scale_alpha(12, glass_strength);
        let tile_bg_alpha = scale_alpha(220, glass_strength);

        Self {
            glass_bg: Color32::from_rgba_unmultiplied(20, 22, 30, glass_bg_alpha),
            glass_border: Color32::from_rgba_unmultiplied(255, 255, 255, glass_border_alpha),
            glass_hover: Color32::from_rgba_unmultiplied(255, 255, 255, glass_hover_alpha),
            accent_primary: Color32::from_rgb(130, 100, 255),
            accent_secondary: Color32::from_rgb(100, 80, 200),
            accent_shield: Color32::from_rgb(80, 200, 140),
            accent_shield_warn: Color32::from_rgb(255, 170, 60),
            accent_shield_off: Color32::from_rgb(120, 120, 130),
            text_primary: Color32::from_rgb(230, 230, 240),
            text_secondary: Color32::from_rgb(150, 150, 165),
            text_placeholder: Color32::from_rgb(100, 100, 115),
            text_on_accent: Color32::WHITE,
            tile_bg: Color32::from_rgba_unmultiplied(35, 38, 52, tile_bg_alpha),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            tile_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 80),
        }
    }

    /// Opera Air-inspired light theme
    pub fn light() -> Self {
        Self::light_with_glass_strength(82)
    }

    pub fn light_with_glass_strength(glass_strength: u8) -> Self {
        let glass_strength = glass_strength.min(100);
        let glass_bg_alpha = scale_alpha(210, glass_strength);
        let glass_border_alpha = scale_alpha(18, glass_strength);
        let glass_hover_alpha = scale_alpha(8, glass_strength);
        let tile_bg_alpha = scale_alpha(230, glass_strength);

        Self {
            glass_bg: Color32::from_rgba_unmultiplied(240, 240, 248, glass_bg_alpha),
            glass_border: Color32::from_rgba_unmultiplied(0, 0, 0, glass_border_alpha),
            glass_hover: Color32::from_rgba_unmultiplied(0, 0, 0, glass_hover_alpha),
            accent_primary: Color32::from_rgb(100, 70, 220),
            accent_secondary: Color32::from_rgb(80, 55, 180),
            accent_shield: Color32::from_rgb(40, 170, 110),
            accent_shield_warn: Color32::from_rgb(220, 140, 30),
            accent_shield_off: Color32::from_rgb(160, 160, 170),
            text_primary: Color32::from_rgb(20, 20, 35),
            text_secondary: Color32::from_rgb(90, 90, 110),
            text_placeholder: Color32::from_rgb(160, 160, 175),
            text_on_accent: Color32::WHITE,
            tile_bg: Color32::from_rgba_unmultiplied(255, 255, 255, tile_bg_alpha),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(0, 0, 0, 12),
            tile_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 30),
        }
    }
}

fn scale_alpha(base: u8, glass_strength: u8) -> u8 {
    let strength = glass_strength.min(100) as f32 / 100.0;
    let multiplier = 0.55 + (strength * 0.45);
    ((base as f32) * multiplier).round().clamp(0.0, 255.0) as u8
}
 
