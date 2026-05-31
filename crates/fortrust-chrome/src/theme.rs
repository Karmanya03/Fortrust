use egui::Color32;

#[derive(Clone, Copy)]
pub struct FortrustTheme {
    pub glass_bg: Color32,
    pub glass_border: Color32,
    pub glass_hover: Color32,

    // Surface colors from v2 design
    pub surface_deepest: Color32,
    pub surface_rail: Color32,
    pub surface_sidebar: Color32,
    pub surface_tab_bar: Color32,
    pub surface_card: Color32,
    pub surface_hover: Color32,

    pub accent_primary: Color32,
    pub accent_secondary: Color32,
    pub accent_shield: Color32,
    pub accent_shield_warn: Color32,
    pub accent_shield_off: Color32,

    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub text_placeholder: Color32,
    pub text_on_accent: Color32,

    pub tile_bg: Color32,
    pub tile_hover_overlay: Color32,
    pub tile_shadow: Color32,

    pub border_subtle: Color32,
    pub border_strong: Color32,
    pub accent_danger: Color32,
}

impl FortrustTheme {
    pub fn dark() -> Self {
        Self::dark_with_glass_strength(82)
    }

    pub fn dark_with_glass_strength(_glass_strength: u8) -> Self {
        Self {
            glass_bg: Color32::from_rgba_unmultiplied(20, 22, 30, 200),
            glass_border: Color32::from_rgba_unmultiplied(255, 255, 255, 18),
            glass_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 12),

            surface_deepest: Color32::from_rgb(13, 15, 18), // c0
            surface_rail: Color32::from_rgb(19, 22, 27), // c1
            surface_sidebar: Color32::from_rgb(24, 28, 34), // c2
            surface_tab_bar: Color32::from_rgb(29, 34, 42), // c3
            surface_card: Color32::from_rgb(35, 40, 48), // c4
            surface_hover: Color32::from_rgb(44, 50, 64), // c5

            accent_primary: Color32::from_rgb(79, 158, 255), // blue
            accent_secondary: Color32::from_rgb(60, 130, 220),
            accent_shield: Color32::from_rgb(63, 176, 110), // green
            accent_shield_warn: Color32::from_rgb(255, 170, 60),
            accent_shield_off: Color32::from_rgb(120, 120, 130),

            text_primary: Color32::from_rgb(221, 225, 234), // t1
            text_secondary: Color32::from_rgb(144, 152, 168), // t2
            text_muted: Color32::from_rgb(79, 86, 104), // t3
            text_placeholder: Color32::from_rgb(100, 100, 115),
            text_on_accent: Color32::WHITE,

            tile_bg: Color32::from_rgba_unmultiplied(35, 38, 52, 200),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            tile_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 80),

            border_subtle: Color32::from_rgb(39, 45, 56), // border
            border_strong: Color32::from_rgb(50, 57, 73), // border2
            accent_danger: Color32::from_rgb(255, 92, 92), // red
        }
    }

    pub fn light() -> Self {
        Self::light_with_glass_strength(82)
    }

    pub fn light_with_glass_strength(_glass_strength: u8) -> Self {
        Self {
            glass_bg: Color32::from_rgba_unmultiplied(240, 240, 248, 210),
            glass_border: Color32::from_rgba_unmultiplied(0, 0, 0, 18),
            glass_hover: Color32::from_rgba_unmultiplied(0, 0, 0, 8),

            surface_deepest: Color32::from_rgb(245, 245, 250),
            surface_rail: Color32::from_rgb(235, 235, 242),
            surface_sidebar: Color32::from_rgb(240, 240, 248),
            surface_tab_bar: Color32::from_rgb(238, 238, 245),
            surface_card: Color32::from_rgb(248, 248, 252),
            surface_hover: Color32::from_rgb(220, 220, 230),

            accent_primary: Color32::from_rgb(79, 158, 255),
            accent_secondary: Color32::from_rgb(60, 130, 220),
            accent_shield: Color32::from_rgb(63, 176, 110),
            accent_shield_warn: Color32::from_rgb(255, 170, 60),
            accent_shield_off: Color32::from_rgb(160, 160, 170),

            text_primary: Color32::from_rgb(30, 32, 38),
            text_secondary: Color32::from_rgb(110, 115, 130),
            text_muted: Color32::from_rgb(160, 165, 178),
            text_placeholder: Color32::from_rgb(180, 185, 198),
            text_on_accent: Color32::WHITE,

            tile_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 230),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(0, 0, 0, 12),
            tile_shadow: Color32::from_rgba_unmultiplied(0, 0, 0, 30),

            border_subtle: Color32::from_rgb(210, 210, 220),
            border_strong: Color32::from_rgb(180, 180, 190),
            accent_danger: Color32::from_rgb(255, 70, 70),
        }
    }
}
