use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};

pub(super) fn blend_color(base: Color32, overlay: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let inverse = 1.0 - amount;

    Color32::from_rgba_unmultiplied(
        ((base.r() as f32 * inverse) + (overlay.r() as f32 * amount)) as u8,
        ((base.g() as f32 * inverse) + (overlay.g() as f32 * amount)) as u8,
        ((base.b() as f32 * inverse) + (overlay.b() as f32 * amount)) as u8,
        ((base.a() as f32 * inverse) + (overlay.a() as f32 * amount)) as u8,
    )
}

pub(super) fn dim_color(color: Color32, factor: f32) -> Color32 {
    let factor = factor.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (color.r() as f32 * factor) as u8,
        (color.g() as f32 * factor) as u8,
        (color.b() as f32 * factor) as u8,
        (color.a() as f32 * (0.45 + (factor * 0.55))) as u8,
    )
}

pub(super) fn draw_background(painter: &Painter, rect: Rect, pan: Vec2, zoom: f32) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(19, 23, 29));

    let step = (56.0 * zoom.clamp(0.6, 1.8)).max(20.0);
    let origin = rect.center() + pan;

    let mut x = origin.x.rem_euclid(step);
    while x < rect.right() {
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 70, 80, 70)),
        );
        x += step;
    }

    let mut y = origin.y.rem_euclid(step);
    while y < rect.bottom() {
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 70, 80, 70)),
        );
        y += step;
    }
}

pub(super) fn circle_visible(rect: Rect, position: Pos2, radius: f32) -> bool {
    !(position.x + radius < rect.left()
        || position.x - radius > rect.right()
        || position.y + radius < rect.top()
        || position.y - radius > rect.bottom())
}

pub(super) fn edge_visible(rect: Rect, start: Pos2, end: Pos2, padding: f32) -> bool {
    let min_x = start.x.min(end.x) - padding;
    let max_x = start.x.max(end.x) + padding;
    let min_y = start.y.min(end.y) - padding;
    let max_y = start.y.max(end.y) + padding;

    if max_x < rect.left() || min_x > rect.right() || max_y < rect.top() || min_y > rect.bottom() {
        return false;
    }

    if rect.contains(start) || rect.contains(end) {
        return true;
    }

    let top_left = rect.left_top();
    let top_right = rect.right_top();
    let bottom_left = rect.left_bottom();
    let bottom_right = rect.right_bottom();

    segments_intersect(start, end, top_left, top_right)
        || segments_intersect(start, end, top_right, bottom_right)
        || segments_intersect(start, end, bottom_right, bottom_left)
        || segments_intersect(start, end, bottom_left, top_left)
}

fn segments_intersect(a1: Pos2, a2: Pos2, b1: Pos2, b2: Pos2) -> bool {
    fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
        let oa = a - o;
        let ob = b - o;
        (oa.x * ob.y) - (oa.y * ob.x)
    }

    let a_min_x = a1.x.min(a2.x);
    let a_max_x = a1.x.max(a2.x);
    let a_min_y = a1.y.min(a2.y);
    let a_max_y = a1.y.max(a2.y);
    let b_min_x = b1.x.min(b2.x);
    let b_max_x = b1.x.max(b2.x);
    let b_min_y = b1.y.min(b2.y);
    let b_max_y = b1.y.max(b2.y);

    if a_max_x < b_min_x || b_max_x < a_min_x || a_max_y < b_min_y || b_max_y < a_min_y {
        return false;
    }

    let c1 = cross(a1, a2, b1);
    let c2 = cross(a1, a2, b2);
    let c3 = cross(b1, b2, a1);
    let c4 = cross(b1, b2, a2);

    (c1 <= 0.0 && c2 >= 0.0 || c1 >= 0.0 && c2 <= 0.0)
        && (c3 <= 0.0 && c4 >= 0.0 || c3 >= 0.0 && c4 <= 0.0)
}

pub(super) fn world_to_screen(rect: Rect, pan: Vec2, zoom: f32, world: Vec2) -> Pos2 {
    rect.center() + pan + world * zoom
}

pub(super) fn screen_to_world(rect: Rect, pan: Vec2, zoom: f32, screen: Pos2) -> Vec2 {
    (screen - rect.center() - pan) / zoom
}

fn normalize_log(value: u64, min: u64, max: u64) -> f32 {
    let min = min.max(1) as f64;
    let max = max.max(min as u64) as f64;
    let value = value.max(1) as f64;

    if (max - min).abs() < f64::EPSILON {
        return 0.5;
    }

    let denominator = max.ln() - min.ln();
    if denominator.abs() < f64::EPSILON {
        return 0.5;
    }

    ((value.ln() - min.ln()) / denominator).clamp(0.0, 1.0) as f32
}

pub(super) fn node_radius(metric: u64, min: u64, max: u64) -> f32 {
    6.0 + (normalize_log(metric, min, max) * 26.0)
}

pub(super) fn metric_color(metric: u64, min: u64, max: u64) -> Color32 {
    let t = normalize_log(metric, min, max);
    let r = (55.0 + (190.0 * t)) as u8;
    let g = (150.0 - (70.0 * t)) as u8;
    let b = (215.0 - (155.0 * t)) as u8;
    Color32::from_rgb(r, g, b)
}
