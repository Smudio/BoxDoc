//! Zeichenfang — wie der Objektfang in CAD-Programmen, aber leise.
//!
//! Beim Zeichnen von Linien und Pfaden rastet der Cursor auf markante Punkte
//! anderer Objekte ein (Endpunkt, Mitte, Zentrum, Quadrant), auf das **Lot**
//! vom Startpunkt auf eine Kante, oder — wenn kein Objekt in der Nähe ist —
//! auf die Richtungen 0°/45°/90° vom Startpunkt aus (Polarfang).
//!
//! **Leise heißt:** Die Fangweite wird in **Bildschirmpixeln** gemessen und
//! ist klein. Ein Punkt zieht erst an, wenn der Cursor schon fast darauf
//! steht, und das gilt bei 10 % Zoom genauso wie bei 5000 %. Mit Alt ist der
//! Fang ganz aus. Wer nur ungefähr in die Nähe zielt, zeichnet frei.
//!
//! Reine Geometrie ohne UI — gezeichnet wird der Marker in `canvas`.

use egui::{Pos2, Vec2};

use crate::geometry;
use crate::model::{Element, ElementKind};

/// Fangweite für Objektpunkte, in Bildschirmpixeln.
pub const POINT_SNAP_PX: f32 = 8.0;
/// Fangweite für den Polarfang, in Bildschirmpixeln — bewusst kleiner: Eine
/// fast waagrechte Linie soll nur einrasten, wenn sie es auch sein soll.
pub const POLAR_SNAP_PX: f32 = 5.0;

/// Woran eingerastet wurde.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SnapKind {
    /// Endpunkt einer Linie, Ecke eines Rechtecks/Bilds, Knoten eines Pfads.
    Endpoint,
    /// Mitte einer Linie oder Kante.
    Midpoint,
    /// Mittelpunkt einer Ellipse oder eines Rechtecks.
    Center,
    /// Einer der vier Scheitelpunkte einer Ellipse.
    Quadrant,
    /// Fußpunkt des Lots vom Startpunkt auf eine Kante.
    Perpendicular,
    /// Richtung vom Startpunkt, in Grad (0, 45, 90, … im Uhrzeigersinn).
    Polar(i32),
}

impl SnapKind {
    pub fn label(self) -> String {
        match self {
            SnapKind::Endpoint => String::from("Endpunkt"),
            SnapKind::Midpoint => String::from("Mitte"),
            SnapKind::Center => String::from("Zentrum"),
            SnapKind::Quadrant => String::from("Quadrant"),
            SnapKind::Perpendicular => String::from("Lot"),
            SnapKind::Polar(a) => {
                // Anzeige wie auf dem Geodreieck: gegen den Uhrzeigersinn,
                // 0°…180°, damit „senkrecht" immer 90° heißt.
                let a = (360 - a).rem_euclid(180);

                format!("{a}°")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    /// Der eingerastete Punkt in Seitenkoordinaten.
    pub point: Pos2,
    pub kind: SnapKind,
}

/// Sucht den Fangpunkt zum Cursor.
///
/// * `from` — der Startpunkt der gerade entstehenden Strecke, falls es einen
///   gibt. Nur dann gibt es Lot und Polarfang.
/// * `exclude` — Objekte, an denen nicht gefangen wird (das gerade bearbeitete
///   selbst; sonst rastete ein gezogener Knoten auf seiner eigenen Position).
/// * `zoom` — Bildschirmpixel je Punkt; rechnet die Fangweite um.
pub fn snap_point(
    cursor: Pos2,
    from: Option<Pos2>,
    elements: &[Element],
    exclude: &[u64],
    zoom: f32,
) -> Option<Snap> {
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    let tol = POINT_SNAP_PX / zoom;

    // 1. Objektpunkte und Lot: der nächste gewinnt. Das Lot bekommt einen
    //    kleinen Aufschlag, damit ein Endpunkt, der zufällig auch Lotfußpunkt
    //    ist, als Endpunkt angezeigt wird.
    let mut best: Option<(f32, Snap)> = None;
    let mut consider = |p: Pos2, kind: SnapKind, bias: f32| {
        let d = (p - cursor).length();
        if d <= tol && best.map_or(true, |(bd, _)| d + bias < bd) {
            best = Some((d + bias, Snap { point: p, kind }));
        }
    };
    for el in elements.iter().filter(|e| !exclude.contains(&e.id)) {
        for (p, kind) in object_points(el) {
            consider(p, kind, 0.0);
        }
        if let Some(f) = from {
            for (a, b) in object_segments(el) {
                if let Some(foot) = perpendicular_foot(f, a, b) {
                    // Ein Lot auf eine Kante, die durch den Startpunkt läuft,
                    // ist der Startpunkt selbst — nutzlos.
                    if (foot - f).length() > tol {
                        consider(foot, SnapKind::Perpendicular, tol * 0.25);
                    }
                }
            }
        }
    }
    if let Some((_, s)) = best {
        return Some(s);
    }

    // 2. Polarfang: 0°/45°/90°/… vom Startpunkt aus.
    let from = from?;
    let v = cursor - from;
    let len = v.length();
    // Ganz nah am Startpunkt ist jede Richtung „fast" eine Rasterrichtung —
    // da würde der Fang nur zappeln.
    if len < 3.0 * POINT_SNAP_PX / zoom {
        return None;
    }
    let step = 45.0_f32;
    let ang = v.y.atan2(v.x).to_degrees();
    let snapped = (ang / step).round() * step;
    let dir = Vec2::angled(snapped.to_radians());
    let along = v.dot(dir);
    if along <= 0.0 {
        return None;
    }
    let off = (v - dir * along).length();
    if off * zoom > POLAR_SNAP_PX {
        return None;
    }
    Some(Snap {
        point: from + dir * along,
        kind: SnapKind::Polar((snapped.round() as i32).rem_euclid(360)),
    })
}

/// Markante Punkte eines Objekts in Seitenkoordinaten.
fn object_points(el: &Element) -> Vec<(Pos2, SnapKind)> {
    let mut out = Vec::new();
    match el.kind {
        ElementKind::Line => {
            let (a, b) = geometry::line_endpoints(el);
            out.push((a, SnapKind::Endpoint));
            out.push((b, SnapKind::Endpoint));
            out.push((a + (b - a) / 2.0, SnapKind::Midpoint));
        }
        ElementKind::Rectangle | ElementKind::Image => {
            let c = geometry::quad_corners(el);
            for i in 0..4 {
                out.push((c[i], SnapKind::Endpoint));
                let n = c[(i + 1) % 4];
                out.push((c[i] + (n - c[i]) / 2.0, SnapKind::Midpoint));
            }
            out.push((geometry::element_center(el), SnapKind::Center));
        }
        ElementKind::Ellipse => {
            let center = geometry::element_center(el);
            out.push((center, SnapKind::Center));
            let (hw, hh) = (el.w / 2.0, el.h / 2.0);
            for v in [
                Vec2::new(hw, 0.0),
                Vec2::new(-hw, 0.0),
                Vec2::new(0.0, hh),
                Vec2::new(0.0, -hh),
            ] {
                out.push((geometry::local_to_world(center, el.rotation, v), SnapKind::Quadrant));
            }
        }
        ElementKind::Path => {
            let nodes = geometry::path_nodes(el);
            for n in &nodes {
                out.push((n.anchor, SnapKind::Endpoint));
            }
            for (a, b) in path_straight_segments(el) {
                out.push((a + (b - a) / 2.0, SnapKind::Midpoint));
            }
        }
        // Text fängt nicht: Seine Box ist keine sichtbare Kante, und Punkte
        // auf jedem Textblock machten den Fang auf einer Textseite unruhig.
        ElementKind::Text => {}
    }
    out
}

/// Gerade Kanten eines Objekts — für das Lot.
fn object_segments(el: &Element) -> Vec<(Pos2, Pos2)> {
    match el.kind {
        ElementKind::Line => {
            let (a, b) = geometry::line_endpoints(el);
            vec![(a, b)]
        }
        ElementKind::Rectangle | ElementKind::Image => {
            let c = geometry::quad_corners(el);
            (0..4).map(|i| (c[i], c[(i + 1) % 4])).collect()
        }
        ElementKind::Path => path_straight_segments(el),
        ElementKind::Ellipse | ElementKind::Text => Vec::new(),
    }
}

fn path_straight_segments(el: &Element) -> Vec<(Pos2, Pos2)> {
    let Some((start, segs)) = geometry::path_segments(el) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut prev = start;
    for s in segs {
        match s {
            geometry::PathSeg::Line(p) => {
                out.push((prev, p));
                prev = p;
            }
            geometry::PathSeg::Cubic(_, _, p) => prev = p,
        }
    }
    out
}

/// Fußpunkt des Lots von `p` auf die Strecke `a`–`b`, sofern er auf der
/// Strecke liegt.
fn perpendicular_foot(p: Pos2, a: Pos2, b: Pos2) -> Option<Pos2> {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 < 1e-9 {
        return None;
    }
    let t = (p - a).dot(ab) / len2;
    (0.0..=1.0).contains(&t).then(|| a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: u64, a: (f32, f32), b: (f32, f32)) -> Element {
        let mut el = Element::new_line(id, 0.0, 0.0);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy);
        el.w = len;
        el.rotation = dy.atan2(dx).to_degrees();
        el.x = (a.0 + b.0) / 2.0 - len / 2.0;
        el.y = (a.1 + b.1) / 2.0;
        el
    }

    #[test]
    fn endpunkt_rastet_nur_in_der_naehe_ein() {
        let els = vec![line(1, (100.0, 100.0), (200.0, 100.0))];
        let s = snap_point(Pos2::new(202.0, 101.0), None, &els, &[], 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Endpoint);
        assert!((s.point - Pos2::new(200.0, 100.0)).length() < 1e-3);
        assert!(snap_point(Pos2::new(215.0, 100.0), None, &els, &[], 1.0).is_none());
    }

    #[test]
    fn fangweite_schrumpft_beim_hineinzoomen() {
        // 4 pt daneben: bei 100 % (4 px) fängt es, bei 1000 % (40 px) nicht.
        let els = vec![line(1, (100.0, 100.0), (200.0, 100.0))];
        let c = Pos2::new(204.0, 100.0);
        assert!(snap_point(c, None, &els, &[], 1.0).is_some());
        assert!(snap_point(c, None, &els, &[], 10.0).is_none());
    }

    #[test]
    fn lot_auf_eine_kante() {
        let els = vec![line(1, (100.0, 100.0), (300.0, 100.0))];
        let from = Pos2::new(180.0, 20.0);
        let s = snap_point(Pos2::new(181.0, 98.0), Some(from), &els, &[], 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Perpendicular);
        assert!((s.point - Pos2::new(180.0, 100.0)).length() < 1e-3);
    }

    #[test]
    fn polarfang_nur_fast_auf_der_achse() {
        let from = Pos2::new(0.0, 0.0);
        let s = snap_point(Pos2::new(200.0, 3.0), Some(from), &[], &[], 1.0).unwrap();
        assert_eq!(s.kind, SnapKind::Polar(0));
        assert!((s.point - Pos2::new(200.0, 0.0)).length() < 1e-3);
        // 20 px daneben ist Absicht, kein Zittern.
        assert!(snap_point(Pos2::new(200.0, 20.0), Some(from), &[], &[], 1.0).is_none());
        let s = snap_point(Pos2::new(2.0, -150.0), Some(from), &[], &[], 1.0).unwrap();
        assert_eq!(s.kind.label(), "90°");
    }

    #[test]
    fn eigenes_objekt_wird_ausgelassen() {
        let els = vec![line(1, (100.0, 100.0), (200.0, 100.0))];
        assert!(snap_point(Pos2::new(200.0, 100.0), None, &els, &[1], 1.0).is_none());
    }
}
