use crate::animation::SidebarAnimation;
use crate::theme::FortrustTheme;
use egui::{self, Color32, CornerRadius, Ui, Vec2};
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

    ui.set_width(width);
    println!("Fortrust:sidebar width={} max_rect={:?}", width, ui.max_rect());
    // Make the sidebar background subtle for the light, minimal design
    ui.painter().rect_filled(ui.max_rect(), CornerRadius::ZERO, Color32::TRANSPARENT);
    ui.painter().rect_stroke(
        ui.max_rect(),
        CornerRadius::ZERO,
        egui::Stroke::new(0.3, theme.glass_border),
        egui::StrokeKind::Outside,
    );

    ui.vertical(|ui: &mut Ui| {
        if icon_button(ui, "◀", theme).clicked() {
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
            false,
            0,
        );
        sidebar_item(
            ui,
            "⭐",
            "Bookmarks",
            SidebarPage::Bookmarks,
            current_page,
            label_opacity,
            theme,
            false,
            0,
        );
        sidebar_item(
            ui,
            "🕘",
            "History",
            SidebarPage::History,
            current_page,
            label_opacity,
            theme,
            false,
            0,
        );
        sidebar_item(
            ui,
            "⬇",
            "Downloads",
            SidebarPage::Downloads,
            current_page,
            label_opacity,
            theme,
            false,
            0,
        );
        sidebar_item(
            ui,
            "📝",
            "Notes",
            SidebarPage::Notes,
            current_page,
            label_opacity,
            theme,
            false,
            0,
        );

        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui: &mut Ui| {
            sidebar_item(
                ui,
                "⚙",
                "Settings",
                SidebarPage::Settings,
                current_page,
                label_opacity,
                theme,
                false,
                0,
            );
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

#[allow(clippy::too_many_arguments)]
fn sidebar_item(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    page: SidebarPage,
    current: &mut SidebarPage,
    label_opacity: f32,
    theme: &FortrustTheme,
    show_badge: bool,
    _badge_count: u32,
) {
    let is_active = *current == page;
    let bg = if is_active {
        theme.accent_primary
    } else {
        Color32::TRANSPARENT
    };

    let response = ui
        .horizontal(|ui: &mut Ui| {
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
            // Notification badge dot
            if show_badge {
                let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(
                    badge_rect.center(),
                    4.0,
                    Color32::from_rgb(255, 69, 58),
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

/// iOS-style toggle switch. Returns true if the value changed.
pub fn toggle_switch(ui: &mut Ui, label: &str, value: &mut bool, theme: &FortrustTheme) -> bool {
    let green = Color32::from_rgb(92, 170, 111);
    let gray = Color32::from_rgba_unmultiplied(120, 120, 130, 80);
    let pill_w = 44.0;
    let pill_h = 24.0;
    let knob_r = 10.0;
    let padding = 2.0;

    ui.horizontal(|ui| {
        ui.add_space(4.0);
        let resp = ui.allocate_response(Vec2::new(pill_w, pill_h), egui::Sense::click());
        let rect = resp.rect;
        let is_on = *value;

        // Pill background
        let bg_color = if is_on { green } else { gray };
        ui.painter().rect_filled(rect, CornerRadius::same(12), bg_color);

        // Knob
        let knob_x = if is_on {
            rect.right() - padding - knob_r * 2.0
        } else {
            rect.left() + padding
        };
        let knob_center = egui::Pos2::new(knob_x + knob_r, rect.center().y);
        ui.painter().circle_filled(knob_center, knob_r, Color32::WHITE);

        if resp.clicked() {
            *value = !*value;
        }

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(label)
                .size(13.0)
                .color(theme.text_primary),
        );
    })
    .response
    .clicked()
}
