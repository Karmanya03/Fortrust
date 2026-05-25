use egui::Color32;

pub struct FortrustTheme {
    pub glass_bg: Color32,
    pub glass_border: Color32,
    pub glass_hover: Color32,
    pub accent_primary: Color32,
    pub accent_secondary: Color32,
    pub accent_shield: Color32,
    pub accent_shield_warn: Color32,
    pub accent_shield_off: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_placeholder: Color32,
    pub text_on_accent: Color32,
    pub tile_bg: Color32,
    pub tile_hover_overlay: Color32,
    pub tile_shadow: Color32,
}

impl FortrustTheme {
    pub fn dark() -> Self {
        Self {
            glass_bg: Color32::from_rgba_unmultiplied(20, 22, 30, 210),
            glass_border: Color32::from_rgba_unmultiplied(255, 255, 255, 18),
            glass_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
            accent_primary: Color32::from_rgb(130, 100, 255),
            accent_secondary: Color32::from_rgb(100, 80, 200),
            accent_shield: Color32::from_rgb(80, 200, 140),
            accent_shield_warn: Color32::from_rgb(255, 170, 60),
            accent_shield_off: Color32::from_rgb(120, 120, 130),
            text_primary: Color32::from_rgb(230, 230, 240),
            text_secondary: Color32::from_rgb(150, 150, 165),
            text_placeholder: Color32::from_rgb(100, 100, 115),
            text_on_accent: Color32::WHITE,
            tile_bg: Color32::from_rgba_unmultiplied(35, 38, 52, 220),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            tile_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 80),
        }
    }

    pub fn light() -> Self {
        Self {
            glass_bg: Color32::from_rgba_unmultiplied(240, 240, 248, 210),
            glass_border: Color32::from_rgba_unmultiplied(0, 0, 0, 18),
            glass_hover: Color32::from_rgba_unmultiplied(0, 0, 0, 8),
            accent_primary: Color32::from_rgb(100, 70, 220),
            accent_secondary: Color32::from_rgb(80, 55, 180),
            accent_shield: Color32::from_rgb(40, 170, 110),
            accent_shield_warn: Color32::from_rgb(220, 140, 30),
            accent_shield_off: Color32::from_rgb(160, 160, 170),
            text_primary: Color32::from_rgb(20, 20, 35),
            text_secondary: Color32::from_rgb(90, 90, 110),
            text_placeholder: Color32::from_rgb(160, 160, 175),
            text_on_accent: Color32::WHITE,
            tile_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 230),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(0, 0, 0, 12),
            tile_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 30),
        }
    }
}
