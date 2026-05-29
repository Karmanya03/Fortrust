use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, Vec2};

fn center_rect(r: Rect, size: Vec2) -> Rect {
    Rect::from_center_size(r.center(), size)
}

fn s(r: Rect, x: f32, y: f32) -> Pos2 {
    Pos2::new(r.min.x + x / 14.0 * r.width(), r.min.y + y / 14.0 * r.height())
}

fn s12(r: Rect, x: f32, y: f32) -> Pos2 {
    Pos2::new(r.min.x + x / 12.0 * r.width(), r.min.y + y / 12.0 * r.height())
}

fn draw_path(painter: &Painter, points: &[Pos2], color: Color32, width: f32) {
    if points.len() < 2 { return; }
    painter.add(Shape::line(points.to_vec(), Stroke::new(width, color)));
}

pub fn paint_back_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 9.0, 3.0), s(r, 5.0, 7.0), s(r, 9.0, 11.0)], color, 1.7);
}

pub fn paint_forward_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 5.0, 3.0), s(r, 9.0, 7.0), s(r, 5.0, 11.0)], color, 1.7);
}

pub fn paint_reload_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.5, Stroke::new(1.7, color));
    draw_path(painter, &[s(r, 12.5, 3.5), s(r, 12.5, 7.0), s(r, 9.0, 7.0)], color, 1.7);
}

pub fn paint_star_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[
        s(r, 7.0, 1.5), s(r, 8.7, 5.0), s(r, 12.5, 5.55), s(r, 9.75, 8.23),
        s(r, 10.4, 12.05), s(r, 7.0, 10.1), s(r, 3.6, 12.05), s(r, 4.25, 8.23),
        s(r, 1.5, 5.55), s(r, 5.3, 5.0), s(r, 7.0, 1.5),
    ], color, 1.6);
}

pub fn paint_sidebar_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.add(Shape::rect_stroke(
        Rect::from_min_size(s(r, 1.5, 1.5), Vec2::new(11.0, 11.0)),
        CornerRadius::same(2), Stroke::new(1.6, color), egui::StrokeKind::Inside,
    ));
    painter.add(Shape::line_segment([s(r, 5.0, 1.5), s(r, 5.0, 12.5)], Stroke::new(1.6, color)));
}

pub fn paint_menu_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 2.0, 4.0), s(r, 12.0, 4.0)], color, 1.6);
    draw_path(painter, &[s(r, 2.0, 7.0), s(r, 12.0, 7.0)], color, 1.6);
    draw_path(painter, &[s(r, 2.0, 10.0), s(r, 12.0, 10.0)], color, 1.6);
}

pub fn paint_setup_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    let centers: [(f32, f32); 9] = [
        (7.0,7.0),(7.0,2.0),(7.0,12.0),(2.0,7.0),(12.0,7.0),
        (3.8,3.8),(10.2,3.8),(3.8,10.2),(10.2,10.2),
    ];
    for (cx, cy) in &centers {
        let radius = if *cx == 7.0 && *cy == 7.0 { 1.2 } else if *cx == 7.0 || *cy == 7.0 { 1.0 } else { 0.9 };
        painter.circle_filled(s(r, *cx, *cy), radius, color);
    }
}

pub fn paint_feeds_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 2.0, 3.0), s(r, 12.0, 3.0)], color, 1.5);
    draw_path(painter, &[s(r, 2.0, 6.5), s(r, 12.0, 6.5)], color, 1.5);
    draw_path(painter, &[s(r, 2.0, 10.0), s(r, 8.0, 10.0)], color, 1.5);
}

pub fn paint_ai_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.5, Stroke::new(1.5, color));
    painter.circle_filled(s(r, 5.0, 6.0), 0.8, color);
    painter.circle_filled(s(r, 9.0, 6.0), 0.8, color);
    draw_path(painter, &[s(r, 5.0, 9.5), s(r, 9.0, 9.5)], color, 1.5);
}

pub fn paint_downloads_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 11.5, 9.0), s(r, 11.5, 11.5), s(r, 2.5, 11.5), s(r, 2.5, 9.0)], color, 1.5);
    draw_path(painter, &[s(r, 7.0, 2.0), s(r, 7.0, 8.0)], color, 1.5);
    draw_path(painter, &[s(r, 4.5, 6.0), s(r, 7.0, 8.5), s(r, 9.5, 6.0)], color, 1.5);
}

pub fn paint_more_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_filled(s(r, 7.0, 3.0), 0.8, color);
    painter.circle_filled(s(r, 7.0, 7.0), 0.8, color);
    painter.circle_filled(s(r, 7.0, 11.0), 0.8, color);
}

pub fn paint_lock_icon(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let r = Rect::from_center_size(center, Vec2::new(size, size));
    painter.add(Shape::rect_stroke(
        Rect::from_min_size(s12(r, 3.0, 5.0), Vec2::new(6.0, 5.5)),
        CornerRadius::same(1), Stroke::new(1.4, color), egui::StrokeKind::Inside,
    ));
    draw_path(painter, &[s12(r, 3.5, 5.0), s12(r, 3.5, 3.5), s12(r, 8.5, 3.5), s12(r, 8.5, 5.0)], color, 1.4);
    painter.circle_filled(s12(r, 6.0, 8.0), 0.7, color);
}

pub fn paint_search_icon(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let r = Rect::from_center_size(center, Vec2::new(size, size));
    painter.circle_stroke(s(r, 6.0, 6.0), 4.0, Stroke::new(1.6, color));
    draw_path(painter, &[s(r, 9.5, 9.5), s(r, 12.0, 12.0)], color, 1.6);
}

#[allow(dead_code)]
pub fn paint_shield_icon(painter: &Painter, center: Pos2, size: f32, color: Color32) {
    let r = Rect::from_center_size(center, Vec2::new(size, size));
    draw_path(painter, &[
        s12(r, 6.0, 1.0), s12(r, 1.5, 3.5), s12(r, 1.5, 6.5),
        s12(r, 6.0, 11.0), s12(r, 10.5, 6.5), s12(r, 10.5, 3.5), s12(r, 6.0, 1.0),
    ], color, 1.4);
}

pub fn paint_close_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 2.0, 2.0), s(r, 12.0, 12.0)], color, 1.6);
    draw_path(painter, &[s(r, 12.0, 2.0), s(r, 2.0, 12.0)], color, 1.6);
}

pub fn paint_plus_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 7.0, 2.0), s(r, 7.0, 12.0)], color, 1.6);
    draw_path(painter, &[s(r, 2.0, 7.0), s(r, 12.0, 7.0)], color, 1.6);
}

pub fn paint_check_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 2.5, 7.5), s(r, 6.0, 11.0), s(r, 11.5, 3.5)], color, 1.6);
}

pub fn paint_google_g_icon(painter: &Painter, rect: Rect) {
    let r = center_rect(rect, Vec2::new(24.0, 24.0));
    let cx = r.center().x;
    let cy = r.center().y;
    let radius = 10.5;
    let steps = 16usize;

    let (blue, red, yellow, green) = (
        Color32::from_rgb(66, 133, 244),
        Color32::from_rgb(234, 67, 53),
        Color32::from_rgb(251, 188, 4),
        Color32::from_rgb(52, 168, 83),
    );

    let arc = |start_deg: f32, end_deg: f32, color: Color32| {
        let mut pts = vec![Pos2::new(cx, cy)];
        let n = steps.max(2);
        for i in 0..=n {
            let t = start_deg + (end_deg - start_deg) * i as f32 / n as f32;
            let rad = t * std::f32::consts::TAU / 360.0;
            pts.push(Pos2::new(cx + rad.cos() * radius, cy + rad.sin() * radius));
        }
        painter.add(Shape::Path(egui::epaint::PathShape {
            points: pts, closed: true, fill: color, stroke: Default::default(),
        }));
    };

    // Blue: 315-45 degrees (top-right arc)
    arc(315.0, 45.0, blue);
    // Red: 45-135 degrees (bottom-right arc)
    arc(45.0, 135.0, red);
    // Yellow: 135-225 degrees (bottom-left arc)
    arc(135.0, 225.0, yellow);
    // Green: 225-315 degrees (top-left arc)
    arc(225.0, 315.0, green);

    // White "G" letter
    painter.text(
        Pos2::new(cx, cy),
        egui::Align2::CENTER_CENTER,
        "G",
        egui::FontId::proportional(15.0),
        Color32::WHITE,
    );
}

pub fn paint_shield_icon_rect(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[
        s12(r, 6.0, 1.0), s12(r, 1.5, 3.5), s12(r, 1.5, 6.5),
        s12(r, 6.0, 11.0), s12(r, 10.5, 6.5), s12(r, 10.5, 3.5), s12(r, 6.0, 1.0),
    ], color, 1.4);
}

// ── Sidebar Row Icons ─────────────────────────────

pub fn paint_sun_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_filled(s(r, 7.0, 7.0), 2.2, color);
    for &(dx, dy) in &[(0.0,-4.5),(3.2,-3.2),(4.5,0.0),(3.2,3.2),(0.0,4.5),(-3.2,3.2),(-4.5,0.0),(-3.2,-3.2)] {
        draw_path(painter, &[s(r, 7.0, 7.0), s(r, 7.0+dx, 7.0+dy)], color, 1.3);
    }
}

pub fn paint_timer_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.0, Stroke::new(1.4, color));
    draw_path(painter, &[s(r, 7.0, 3.0), s(r, 7.0, 7.0)], color, 1.4);
    draw_path(painter, &[s(r, 7.0, 7.0), s(r, 9.5, 9.5)], color, 1.4);
    painter.circle_filled(s(r, 7.0, 2.0), 1.0, color);
}

pub fn paint_chat_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    let b = Rect::from_min_size(s(r, 1.5, 2.0), Vec2::new(11.0, 8.0));
    painter.add(Shape::rect_stroke(b, CornerRadius::same(3), Stroke::new(1.4, color), egui::StrokeKind::Inside));
    draw_path(painter, &[s(r, 4.0, 10.0), s(r, 2.0, 12.5), s(r, 5.5, 10.0)], color, 1.4);
}

pub fn paint_globe_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.0, Stroke::new(1.4, color));
    draw_path(painter, &[s(r, 4.5, 3.5), s(r, 9.5, 3.5)], color, 1.4);
    draw_path(painter, &[s(r, 2.5, 7.0), s(r, 11.5, 7.0)], color, 1.4);
    draw_path(painter, &[s(r, 4.5, 10.5), s(r, 9.5, 10.5)], color, 1.4);
}

pub fn paint_at_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 6.5), 3.2, Stroke::new(1.4, color));
    draw_path(painter, &[s(r, 10.2, 6.5), s(r, 10.2, 9.0), s(r, 7.0, 9.0)], color, 1.4);
}

pub fn paint_camera_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    let b = Rect::from_min_size(s(r, 1.5, 3.5), Vec2::new(11.0, 9.0));
    painter.add(Shape::rect_stroke(b, CornerRadius::same(3), Stroke::new(1.4, color), egui::StrokeKind::Inside));
    painter.circle_stroke(s(r, 7.0, 7.5), 2.5, Stroke::new(1.4, color));
    painter.circle_filled(s(r, 10.5, 4.0), 0.8, color);
}

pub fn paint_bookmark_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 11.0, 2.0), s(r, 11.0, 13.0), s(r, 7.0, 10.0), s(r, 3.0, 13.0), s(r, 3.0, 2.0)], color, 1.4);
}

pub fn paint_history_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.5, Stroke::new(1.4, color));
    draw_path(painter, &[s(r, 7.0, 4.0), s(r, 7.0, 7.0)], color, 1.4);
    draw_path(painter, &[s(r, 7.0, 7.0), s(r, 9.0, 9.0)], color, 1.4);
}

pub fn paint_puzzle_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.add(Shape::rect_stroke(Rect::from_min_size(s(r, 2.0, 2.5), Vec2::new(10.0, 9.0)), CornerRadius::same(3), Stroke::new(1.4, color), egui::StrokeKind::Inside));
    painter.circle_filled(s(r, 11.0, 7.0), 1.8, color);
}

pub fn paint_grid_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.add(Shape::rect_stroke(Rect::from_min_size(s(r, 1.5, 1.5), Vec2::new(4.5, 4.5)), CornerRadius::same(1), Stroke::new(1.2, color), egui::StrokeKind::Inside));
    painter.add(Shape::rect_stroke(Rect::from_min_size(s(r, 8.0, 1.5), Vec2::new(4.5, 4.5)), CornerRadius::same(1), Stroke::new(1.2, color), egui::StrokeKind::Inside));
    painter.add(Shape::rect_stroke(Rect::from_min_size(s(r, 1.5, 8.0), Vec2::new(4.5, 4.5)), CornerRadius::same(1), Stroke::new(1.2, color), egui::StrokeKind::Inside));
    painter.add(Shape::rect_stroke(Rect::from_min_size(s(r, 8.0, 8.0), Vec2::new(4.5, 4.5)), CornerRadius::same(1), Stroke::new(1.2, color), egui::StrokeKind::Inside));
}

pub fn paint_gear_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 2.5, Stroke::new(1.4, color));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.5, Stroke::new(1.4, color));
    for &(dx, dy) in &[(0.0,-4.8),(4.5,-1.8),(4.5,3.0),(0.0,4.8),(-4.5,3.0),(-4.5,-1.8)] {
        painter.circle_filled(s(r, 7.0+dx, 7.0+dy), 1.2, color);
    }
}

pub fn paint_face_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.5, Stroke::new(1.4, color));
    painter.circle_filled(s(r, 5.0, 6.0), 0.9, color);
    painter.circle_filled(s(r, 9.0, 6.0), 0.9, color);
    draw_path(painter, &[s(r, 4.5, 9.5), s(r, 9.5, 9.5)], color, 1.4);
}

pub fn paint_compass_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.circle_stroke(s(r, 7.0, 7.0), 5.0, Stroke::new(1.4, color));
    draw_path(painter, &[s(r, 7.0, 2.5), s(r, 4.5, 10.0), s(r, 7.0, 8.0), s(r, 9.5, 4.5)], color, 1.4);
}

pub fn paint_heart_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 7.0, 11.0), s(r, 2.0, 6.0), s(r, 2.0, 4.5), s(r, 7.0, 2.5), s(r, 12.0, 4.5), s(r, 12.0, 6.0), s(r, 7.0, 11.0)], color, 1.4);
}

pub fn paint_x_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    draw_path(painter, &[s(r, 2.5, 2.5), s(r, 11.5, 11.5)], color, 1.4);
    draw_path(painter, &[s(r, 11.5, 2.5), s(r, 2.5, 11.5)], color, 1.4);
}

pub fn paint_signal_icon(painter: &Painter, rect: Rect, color: Color32) {
    let r = center_rect(rect, Vec2::new(14.0, 14.0));
    painter.add(Shape::rect_stroke(Rect::from_min_size(s(r, 2.0, 2.5), Vec2::new(10.0, 9.0)), CornerRadius::same(3), Stroke::new(1.4, color), egui::StrokeKind::Inside));
    draw_path(painter, &[s(r, 4.5, 5.5), s(r, 9.5, 9.5)], color, 1.4);
    draw_path(painter, &[s(r, 5.0, 7.5), s(r, 8.0, 7.5)], color, 1.4);
}
