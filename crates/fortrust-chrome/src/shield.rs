use crate::theme::FortrustTheme;
use egui::{self, Color32, CornerRadius, Ui};

pub struct ShieldState {
    pub enabled: bool,
    pub ads_blocked: u32,
    pub trackers_blocked: u32,
    pub fingerprint_attempts: u32,
    pub https_upgraded: bool,
    pub popup_open: bool,
    pub popup_opacity: f32,
}

impl Default for ShieldState {
    fn default() -> Self {
        Self {
            enabled: true,
            ads_blocked: 0,
            trackers_blocked: 0,
            fingerprint_attempts: 0,
            https_upgraded: true,
            popup_open: false,
            popup_opacity: 0.0,
        }
    }
}

impl ShieldState {
    pub fn render_button(&mut self, ui: &mut Ui, theme: &FortrustTheme) {
        let total_blocked = self.ads_blocked + self.trackers_blocked;
        let color = if !self.enabled {
            theme.accent_shield_off
        } else if total_blocked == 0 {
            theme.accent_shield_warn
        } else {
            theme.accent_shield
        };

        let label = format!("🛡 {}", total_blocked);

        egui::Frame {
            fill: Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30),
            corner_radius: CornerRadius::same(8),
            stroke: egui::Stroke::new(1.0, color),
            inner_margin: egui::Margin::symmetric(8, 4),
            ..Default::default()
        }
        .show(ui, |ui| {
            if ui
                .label(egui::RichText::new(&label).color(color).size(13.0))
                .interact(egui::Sense::click())
                .clicked()
            {
                self.popup_open = !self.popup_open;
            }
        });
    }

    pub fn render_popup(&mut self, ctx: &egui::Context, theme: &FortrustTheme) {
        if !self.popup_open && self.popup_opacity < 0.01 {
            return;
        }

        let target = if self.popup_open { 1.0f32 } else { 0.0 };
        self.popup_opacity += (target - self.popup_opacity) * 0.25;
        ctx.request_repaint();

        egui::Window::new("Shield")
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, egui::Vec2::new(-12.0, 48.0))
            .frame(egui::Frame {
                fill: theme.glass_bg,
                corner_radius: CornerRadius::same(14),
                stroke: egui::Stroke::new(1.0, theme.glass_border),
                inner_margin: egui::Margin::same(20),
                shadow: egui::epaint::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(80),
                },
                ..Default::default()
            })
            .show(ctx, |ui| {
                ui.set_width(280.0);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("🛡 Fortrust Shields")
                            .size(15.0)
                            .strong()
                            .color(theme.text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕").clicked() {
                            self.popup_open = false;
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Shields for this site")
                            .color(theme.text_primary)
                            .size(13.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let enabled = self.enabled;
                        ui.toggle_value(&mut self.enabled, if enabled { "ON" } else { "OFF" });
                    });
                });

                ui.add_space(8.0);

                stat_row(ui, "Ads blocked", self.ads_blocked, theme);
                stat_row(ui, "Trackers blocked", self.trackers_blocked, theme);
                stat_row(ui, "Fingerprint attempts", self.fingerprint_attempts, theme);

                ui.add_space(8.0);
                let https_text = if self.https_upgraded {
                    "✓ Upgraded to HTTPS"
                } else {
                    "✓ Already HTTPS"
                };
                ui.label(
                    egui::RichText::new(https_text)
                        .color(theme.accent_shield)
                        .size(12.0),
                );
            });
    }
}

fn stat_row(ui: &mut Ui, label: &str, count: u32, theme: &FortrustTheme) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .color(theme.text_secondary)
                .size(12.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(count.to_string())
                    .color(theme.text_primary)
                    .size(12.0)
                    .strong(),
            );
        });
    });
}
