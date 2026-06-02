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
            glass_bg: Color32::from_rgba_unmultiplied(24, 24, 26, 200),
            glass_border: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            glass_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 12),

            // Using frontend-ui-dark-ts neutral palette
            surface_deepest: Color32::from_rgb(24, 24, 26),   // bg1: hsl(240, 6%, 10%)
            surface_rail: Color32::from_rgb(29, 29, 31),      // bg2: hsl(240, 5%, 12%)
            surface_sidebar: Color32::from_rgb(29, 29, 31),   // bg2
            surface_tab_bar: Color32::from_rgb(29, 29, 31),   // bg2
            surface_card: Color32::from_rgb(33, 33, 36),      // bg3: hsl(240, 5%, 14%)
            surface_hover: Color32::from_rgb(46, 46, 51),     // bg4: hsl(240, 4%, 18%)

            // Using frontend-ui-dark-ts brand colors
            accent_primary: Color32::from_rgb(130, 81, 238),  // brand: #8251EE
            accent_secondary: Color32::from_rgb(163, 126, 245), // brand.light
            accent_shield: Color32::from_rgb(16, 185, 129),   // status.success: #10B981
            accent_shield_warn: Color32::from_rgb(245, 158, 11), // status.warning: #F59E0B
            accent_shield_off: Color32::from_rgb(113, 113, 122), // text.muted: #71717A

            // Using frontend-ui-dark-ts text colors
            text_primary: Color32::WHITE,
            text_secondary: Color32::from_rgb(161, 161, 170), // text.secondary: #A1A1AA
            text_muted: Color32::from_rgb(113, 113, 122),     // text.muted: #71717A
            text_placeholder: Color32::from_rgb(113, 113, 122),
            text_on_accent: Color32::WHITE,

            tile_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            tile_shadow: Color32::from_rgba_unmultiplied(130, 81, 238, 40), // glow shadow

            // Using frontend-ui-dark-ts border colors
            border_subtle: Color32::from_rgba_unmultiplied(255, 255, 255, 20), // border.subtle
            border_strong: Color32::from_rgba_unmultiplied(255, 255, 255, 30), // border.default
            accent_danger: Color32::from_rgb(239, 68, 68),    // status.error: #EF4444
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

            surface_deepest: Color32::from_rgb(244, 244, 245), 
            surface_rail: Color32::from_rgb(250, 250, 250), 
            surface_sidebar: Color32::from_rgb(250, 250, 250),
            surface_tab_bar: Color32::from_rgb(250, 250, 250),
            surface_card: Color32::WHITE,
            surface_hover: Color32::from_rgb(228, 228, 231),

            accent_primary: Color32::from_rgb(130, 81, 238),
            accent_secondary: Color32::from_rgb(147, 102, 245),
            accent_shield: Color32::from_rgb(16, 185, 129),
            accent_shield_warn: Color32::from_rgb(245, 158, 11),
            accent_shield_off: Color32::from_rgb(161, 161, 170),

            text_primary: Color32::from_rgb(24, 24, 27),
            text_secondary: Color32::from_rgb(82, 82, 91),
            text_muted: Color32::from_rgb(113, 113, 122),
            text_placeholder: Color32::from_rgb(161, 161, 170),
            text_on_accent: Color32::WHITE,

            tile_bg: Color32::from_rgba_unmultiplied(0, 0, 0, 8),
            tile_hover_overlay: Color32::from_rgba_unmultiplied(0, 0, 0, 12),
            tile_shadow: Color32::from_rgba_unmultiplied(130, 81, 238, 20),

            border_subtle: Color32::from_rgba_unmultiplied(0, 0, 0, 20),
            border_strong: Color32::from_rgba_unmultiplied(0, 0, 0, 30),
            accent_danger: Color32::from_rgb(239, 68, 68),
        }
    }
}
