use crate::theme::FortrustTheme;
use egui::{self, Color32, CornerRadius, Rect, Vec2};

pub struct SpeedDialState {
    pub tiles: Vec<SpeedDialTile>,
    pub gradient_t: f32,
    pub search_query: String,
    pub search_engine: SearchEngine,
}

impl Default for SpeedDialState {
    fn default() -> Self {
        Self {
            tiles: vec![
                SpeedDialTile::new("ChatGPT", "https://chat.openai.com"),
                SpeedDialTile::new("GitHub", "https://github.com"),
                SpeedDialTile::new("DuckDuckGo", "https://duckduckgo.com"),
                SpeedDialTile::new("Wikipedia", "https://en.wikipedia.org"),
                SpeedDialTile::new("Reddit", "https://reddit.com"),
                SpeedDialTile::new("YouTube", "https://youtube.com"),
                SpeedDialTile::new("X", "https://x.com"),
                SpeedDialTile::new("Stack Overflow", "https://stackoverflow.com"),
            ],
            gradient_t: 0.0,
            search_query: String::new(),
            search_engine: SearchEngine::default(),
        }
    }
}

pub struct SpeedDialTile {
    pub title: String,
    pub url: String,
    pub hover_scale: f32,
}

impl SpeedDialTile {
    pub fn new(title: &str, url: &str) -> Self {
        Self {
            title: title.to_owned(),
            url: url.to_owned(),
            hover_scale: 1.0,
        }
    }
}

#[derive(Clone, PartialEq, Default)]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Google,
    Brave,
}

impl SearchEngine {
    pub fn all() -> Vec<Self> {
        vec![Self::DuckDuckGo, Self::Google, Self::Brave]
    }

    pub fn name(&self) -> &str {
        match self {
            Self::DuckDuckGo => "DDG",
            Self::Google => "Google",
            Self::Brave => "Brave",
        }
    }

    pub fn icon(&self) -> &str {
        match self {
            Self::DuckDuckGo => "🦆",
            Self::Google => "🌐",
            Self::Brave => "🦁",
        }
    }
}

impl SpeedDialState {
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        theme: &FortrustTheme,
        dt: f32,
        needs_new_tab: &mut bool,
    ) -> Option<String> {
        let mut search_navigation: Option<String> = None;
        let mut tile_navigation: Option<String> = None;

        self.gradient_t = (self.gradient_t + dt * 0.02).rem_euclid(1.0);
        // Light, watercolor-like background (off-white -> pale green)
        let bg_color = lerp_color(
            Color32::from_rgb(247, 243, 238),
            Color32::from_rgb(219, 233, 224),
            (self.gradient_t * std::f32::consts::TAU).sin() * 0.5 + 0.5,
        );

        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg_color);

        ui.vertical_centered(|ui| {
            ui.add_space(28.0);

            self.render_hero(ui, theme);

            ui.add_space(22.0);

            search_navigation = self.render_search_bar(ui, theme);

            ui.add_space(28.0);

            ui.separator();
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Quick Access")
                    .size(12.0)
                    .strong()
                    .color(theme.text_secondary),
            );
            ui.add_space(12.0);
            tile_navigation = self.render_tile_grid(ui, theme, dt);
        });

        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));

        if *needs_new_tab {
            self.search_query.clear();
            *needs_new_tab = false;
        }

        search_navigation.or(tile_navigation)
    }

    fn render_hero(&self, ui: &mut egui::Ui, theme: &FortrustTheme) {
        // Minimal hero: centered product name and subtitle for the light design
        ui.set_width(760.0);
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Fortrust")
                    .size(34.0)
                    .strong()
                    .color(theme.text_primary),
            );
            ui.add_space(6.0);
                ui.label(
                egui::RichText::new("Private browsing with secured Trust Engine.")
                    .size(14.0)
                    .color(theme.text_secondary),
            );
            ui.add_space(14.0);
        });
    }

    fn render_search_bar(&mut self, ui: &mut egui::Ui, theme: &FortrustTheme) -> Option<String> {
        let mut navigate: Option<String> = None;

        egui::Frame {
            fill: Color32::WHITE,
            corner_radius: CornerRadius::same(30),
            stroke: egui::Stroke::new(0.0, Color32::TRANSPARENT),
            inner_margin: egui::Margin::symmetric(18, 12),
            shadow: egui::epaint::Shadow {
                offset: [0, 4],
                blur: 18,
                spread: 0,
                color: Color32::from_black_alpha(18),
            },
            ..Default::default()
        }
        .show(ui, |ui| {
            ui.set_width(640.0);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("search_engine")
                    .selected_text(self.search_engine.icon())
                    .width(36.0)
                    .show_ui(ui, |ui| {
                        for engine in SearchEngine::all() {
                            ui.selectable_value(
                                &mut self.search_engine,
                                engine.clone(),
                                engine.name(),
                            );
                        }
                    });

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Search the web")
                        .frame(false)
                        .desired_width(ui.available_width() - 84.0)
                        .font(egui::FontId::proportional(16.0))
                        .text_color(theme.text_primary),
                );

                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let query = self.search_query.trim().to_string();
                    if !query.is_empty() {
                        navigate = Some(normalize_input(&query));
                    }
                }

                // plus / add button
                if ui
                    .add_sized(
                        [36.0, 36.0],
                        egui::Button::new(egui::RichText::new("+").size(20.0)).wrap(),
                    )
                    .clicked()
                {
                    // placeholder for add action
                }
            });
        });

        navigate
    }

    fn render_tile_grid(&mut self, ui: &mut egui::Ui, theme: &FortrustTheme, dt: f32) -> Option<String> {
        let tile_size = Vec2::new(140.0, 100.0);
        let cols = 4usize;
        let mut navigate: Option<String> = None;

        egui::Grid::new("speed_dial_grid")
            .spacing(Vec2::new(16.0, 16.0))
            .show(ui, |ui| {
                for (i, tile) in self.tiles.iter_mut().enumerate() {
                    if i > 0 && i % cols == 0 {
                        ui.end_row();
                    }

                    let tile_pos = ui.next_widget_position();
                    let tile_rect = Rect::from_min_size(tile_pos, tile_size);
                    let hovered = ui.rect_contains_pointer(tile_rect);
                    tile.hover_scale += ((if hovered { 1.05 } else { 1.0 }) - tile.hover_scale)
                        * (1.0 - (-12.0 * dt).exp());
                    if (tile.hover_scale - 1.0).abs() > 0.001 {
                        ui.ctx().request_repaint();
                    }

                    let scaled = Vec2::new(tile_size.x * tile.hover_scale, tile_size.y * tile.hover_scale);
                    let response = ui.add_sized(
                        scaled,
                        egui::Button::new(
                            egui::RichText::new(format!("{}\n{}", tile.title, compact_url(&tile.url)))
                                .size(12.0)
                                .color(theme.text_primary),
                        )
                        .fill(theme.tile_bg)
                        .stroke(egui::Stroke::new(1.0, theme.glass_border))
                        .corner_radius(14),
                    );

                    if response.clicked() {
                        navigate = Some(tile.url.clone());
                    }
                }
            });

        navigate
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}

fn normalize_input(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("fortrust://")
        || trimmed.starts_with("about:")
    {
        trimmed.to_owned()
    } else if trimmed.contains('.') && !trimmed.contains(' ') {
        format!("https://{}", trimmed)
    } else {
        format!("https://duckduckgo.com/?q={}", urlencoding::encode(trimmed))
    }
}

fn compact_url(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_owned()
}

fn hero_chip(ui: &mut egui::Ui, theme: &FortrustTheme, label: &str) {
    egui::Frame {
        fill: theme.glass_hover,
        corner_radius: CornerRadius::same(255),
        stroke: egui::Stroke::new(1.0, theme.glass_border),
        inner_margin: egui::Margin::symmetric(10, 5),
        ..Default::default()
    }
    .show(ui, |ui| {
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .strong()
                .color(theme.text_primary),
        );
    });
}
 
