//! Kleine Geometrie-Helfer (Rotation etc.).

use egui::{Pos2, Vec2};

/// Rotiert einen Vektor um den Winkel (Grad) im Uhrzeigersinn im
/// Bildschirm-Koordinatensystem (y zeigt nach unten).
pub fn rotate_vec(v: Vec2, deg: f32) -> Vec2 {
    let r = deg.to_radians();
    let (s, c) = (r.sin(), r.cos());
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Vier lokale Ecken (gegen den Uhrzeigersinn) eines Rechtecks der Größe (w,h),
/// Mittelpunkt im Ursprung.
pub fn local_corners(w: f32, h: f32) -> [Vec2; 4] {
    let hw = w / 2.0;
    let hh = h / 2.0;
    [
        Vec2::new(-hw, -hh),
        Vec2::new(hw, -hh),
        Vec2::new(hw, hh),
        Vec2::new(-hw, hh),
    ]
}

/// Wandelt eine lokale Ecke in Bildschirm-/Seitenkoordinaten um.
pub fn local_to_world(center: Pos2, rotation_deg: f32, local: Vec2) -> Pos2 {
    center + rotate_vec(local, rotation_deg)
}

/// Wandelt eine Weltkoordinate in eine lokale Koordinate um.
pub fn world_to_local(center: Pos2, rotation_deg: f32, world: Pos2) -> Vec2 {
    rotate_vec(world - center, -rotation_deg)
}

/// Snapt den Punkt `p` so, dass der Vektor von `fixed` zu `p` auf einem
/// 45°-Raster liegt (0°, 45°, 90°, 135°, …). Die Länge des Vektors bleibt
/// erhalten. Dies ergibt horizontale, vertikale und 45°-Diagonal-Richtungen.
pub fn snap_angle_45(fixed: Pos2, p: Pos2) -> Pos2 {
    let dx = p.x - fixed.x;
    let dy = p.y - fixed.y;
    let len = dx.hypot(dy);
    if len < 0.001 {
        return p;
    }
    let angle = dy.atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    Pos2::new(fixed.x + snapped.cos() * len, fixed.y + snapped.sin() * len)
}
