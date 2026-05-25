use crate::animation::SidebarAnimation;
use crate::theme::FortrustTheme;
use egui::{self, Color32, CornerRadius, Ui};
use fortrust_core::Tab;

#[derive(PartialEq, Clone, Copy)]
pub enum SidebarPage {
    Tabs,
    Bookmarks,
    History,
    Downloads,
    Notes,
    Settings,
}

pub fn render_sidebar(
    ui: &mut Ui,
    anim: &mut SidebarAnimation,
    current_page: &mut SidebarPage,
    theme: &FortrustTheme,
    _tabs: &[Tab],
    _active_tab: &mut usize,
) {
    let width = anim.current_width();
    let label_opacity = anim.label_opacity();

    egui::SidePanel::left("sidebar")
        .exact_width(width)
        .resizable(false)
        .frame(egui::Frame {
            fill: theme.glass_bg,
            inner_margin: egui::Margin::symmetric(8, 12),
            corner_radius: CornerRadius::ZERO,
            stroke: egui::Stroke::new(0.5, theme.glass_border),
            ..Default::default()
        })
        .show_inside(ui, |ui| {
            ui.set_width(width);
            ui.vertical(|ui| {
                if icon_button(ui, "🔒", theme).clicked() {
                    if anim.current_width() > 100.0 {
                        anim.collapse();
                    } else {
                        anim.expand();
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                sidebar_item(
                    ui,
                    "📑",
                    "Tabs",
                    SidebarPage::Tabs,
                    current_page,
                    label_opacity,
                    theme,
                );
                sidebar_item(
                    ui,
                    "⭐",
                    "Bookmarks",
                    SidebarPage::Bookmarks,
                    current_page,
                    label_opacity,
                    theme,
                );
                sidebar_item(
                    ui,
                    "🕘",
                    "History",
                    SidebarPage::History,
                    current_page,
                    label_opacity,
                    theme,
                );
                sidebar_item(
                    ui,
                    "⬇",
                    "Downloads",
                    SidebarPage::Downloads,
                    current_page,
                    label_opacity,
                    theme,
                );
                sidebar_item(
                    ui,
                    "📝",
                    "Notes",
                    SidebarPage::Notes,
                    current_page,
                    label_opacity,
                    theme,
                );

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    sidebar_item(
                        ui,
                        "⚙",
                        "Settings",
                        SidebarPage::Settings,
                        current_page,
                        label_opacity,
                        theme,
                    );
                });
            });
        });
}

fn icon_button(ui: &mut Ui, icon: &str, theme: &FortrustTheme) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(icon)
                .size(18.0)
                .color(theme.text_primary),
        )
        .fill(Color32::TRANSPARENT)
        .min_size(egui::Vec2::new(36.0, 36.0))
        .corner_radius(8)
        .stroke(egui::Stroke::NONE),
    )
}

fn sidebar_item(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    page: SidebarPage,
    current: &mut SidebarPage,
    label_opacity: f32,
    theme: &FortrustTheme,
) {
    let is_active = *current == page;
    let bg = if is_active {
        theme.accent_primary
    } else {
        Color32::TRANSPARENT
    };

    let response = ui
        .horizontal(|ui| {
            ui.painter().rect_filled(
                ui.available_rect_before_wrap().shrink(2.0),
                CornerRadius::same(8),
                bg,
            );
            ui.label(egui::RichText::new(icon).size(18.0).color(if is_active {
                theme.text_on_accent
            } else {
                theme.text_secondary
            }));
            if label_opacity > 0.01 {
                ui.label(
                    egui::RichText::new(label)
                        .size(13.0)
                        .color(theme.text_primary.linear_multiply(label_opacity)),
                );
            }
        })
        .response;

    if response.interact(egui::Sense::click()).clicked() {
        *current = page;
    }

    if response.hovered() {
        ui.painter().rect_filled(
            response.rect.shrink(2.0),
            CornerRadius::same(8),
            theme.glass_hover,
        );
    }
}
