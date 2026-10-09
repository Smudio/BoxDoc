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
use serde::{Deserialize, Serialize};

use crate::geometry;
use crate::model::{Element, ElementKind};

/// Welche Fangarten aktiv sind — einstellbar im Menü „Fang", gespeichert mit
/// den übrigen Einstellungen. Fehlende Felder (ältere Einstellungsdatei)
/// gelten als eingeschaltet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapSettings {
    /// Hauptschalter (F3). Aus = kein Fang irgendeiner Art.
    pub enabled: bool,
    /// Beim Verschieben und Skalieren an Seitenrändern, Seitenmitte und
    /// Kanten anderer Objekte ausrichten.
    pub align: bool,
    pub endpoint: bool,
    pub midpoint: bool,
    pub center: bool,
    pub quadrant: bool,
    pub intersection: bool,
    pub perpendicular: bool,
    pub polar: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        SnapSettings {
            enabled: true,
            align: true,
            endpoint: true,
            midpoint: true,
            center: true,
            quadrant: true,
            intersection: true,
            perpendicular: true,
            polar: true,
        }
    }
}

impl SnapSettings {
    pub fn allows(&self, kind: SnapKind) -> bool {
        self.enabled
            && match kind {
                SnapKind::Endpoint => self.endpoint,
                SnapKind::Midpoint => self.midpoint,
                SnapKind::Center => self.center,
                SnapKind::Quadrant => self.quadrant,
                SnapKind::Intersection => self.intersection,
                SnapKind::Perpendicular => self.perpendicular,
                SnapKind::Polar(_) => self.polar,
            }
    }

    /// Ausrichten beim Verschieben/Skalieren aktiv?
    pub fn align_active(&self) -> bool {
        self.enabled && self.align
    }

    /// Alle Fangarten ein- oder ausschalten (der Hauptschalter bleibt).
    pub fn set_all(&mut self, on: bool) {
        let enabled = self.enabled;
        *self = SnapSettings {
            enabled,
            align: on,
            endpoint: on,
            midpoint: on,
            center: on,
            quadrant: on,
            intersection: on,
            perpendicular: on,
            polar: on,
        };
    }
}

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
    /// Schnittpunkt zweier Kanten (Linien, Rechteck- und Pfadkanten,
    /// Ellipsen).
    Intersection,
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
            SnapKind::Intersection => String::from("Schnittpunkt"),
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
/// * `settings` — welche Fangarten aktiv sind.
pub fn snap_point(
    cursor: Pos2,
    from: Option<Pos2>,
    elements: &[Element],
    exclude: &[u64],
    zoom: f32,
    settings: &SnapSettings,
) -> Option<Snap> {
    if !settings.enabled {
        return None;
    }
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    let tol = POINT_SNAP_PX / zoom;

    // 1. Objektpunkte, Schnittpunkte und Lot: der nächste gewinnt. Schnitt-
    //    punkt und Lot bekommen einen kleinen Aufschlag, damit eine
    //    Rechteckecke — auch Schnittpunkt zweier Kanten — als Endpunkt
    //    angezeigt wird.
    let mut best: Option<(f32, Snap)> = None;
    let mut consider = |p: Pos2, kind: SnapKind, bias: f32| {
        if !settings.allows(kind) {
            return;
        }
        let d = (p - cursor).length();
        if d <= tol && best.map_or(true, |(bd, _)| d + bias < bd) {
            best = Some((d + bias, Snap { point: p, kind }));
        }
    };
    let candidates = || elements.iter().filter(|e| !exclude.contains(&e.id));
    for el in candidates() {
        for (p, kind) in object_points(el) {
            consider(p, kind, 0.0);
        }
    }
    if settings.allows(SnapKind::Intersection) {
        for p in intersections_near(cursor, tol, candidates()) {
            consider(p, SnapKind::Intersection, tol * 0.1);
        }
    }
    for el in candidates() {
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
    if !settings.polar {
        return None;
    }
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

/// Eine Kante, an der ein Schnittpunkt liegen kann.
#[derive(Clone, Copy)]
enum Edge {
    Seg(Pos2, Pos2),
    Ellipse { c: Pos2, rx: f32, ry: f32, rot: f32 },
}

/// Schnittpunkte aller Kanten in Cursornähe.
///
/// Gerechnet wird nur mit Kanten, die selbst höchstens `tol` vom Cursor
/// entfernt sind — ein Schnittpunkt im Fangbereich muss auf zwei solchen
/// liegen. Das hält die paarweise Suche klein, auch auf vollen Seiten.
///
/// Kurven gehen als feiner Streckenzug ein (dieselbe Zerlegung wie auf dem
/// Bildschirm); deren innere Stützstellen sind keine Schnittpunkte und werden
/// übersprungen.
fn intersections_near<'a>(
    cursor: Pos2,
    tol: f32,
    elements: impl Iterator<Item = &'a Element>,
) -> Vec<Pos2> {
    let mut edges: Vec<(u64, Edge)> = Vec::new();
    for el in elements {
        match el.kind {
            ElementKind::Ellipse => {
                let (rx, ry) = (el.w.abs() / 2.0, el.h.abs() / 2.0);
                if rx < 1e-6 || ry < 1e-6 {
                    continue;
                }
                let c = geometry::element_center(el);
                let q = geometry::rotate_vec(cursor - c, -el.rotation);
                let r = ((q.x / rx).powi(2) + (q.y / ry).powi(2)).sqrt();
                if (r - 1.0).abs() * rx.min(ry) <= tol {
                    edges.push((el.id, Edge::Ellipse { c, rx, ry, rot: el.rotation }));
                }
            }
            ElementKind::Path => {
                if let Some((start, segs)) = geometry::path_segments(el) {
                    let mut prev = start;
                    for s in segs {
                        match s {
                            geometry::PathSeg::Line(p) => {
                                edges.push((el.id, Edge::Seg(prev, p)));
                                prev = p;
                            }
                            geometry::PathSeg::Cubic(c1, c2, p) => {
                                let mut pts = vec![prev];
                                geometry::flatten_cubic(prev, c1, c2, p, &mut pts);
                                for w in pts.windows(2) {
                                    edges.push((el.id, Edge::Seg(w[0], w[1])));
                                }
                                prev = p;
                            }
                        }
                    }
                }
            }
            _ => {
                for (a, b) in object_segments(el) {
                    edges.push((el.id, Edge::Seg(a, b)));
                }
            }
        }
    }
    edges.retain(|(_, e)| match e {
        Edge::Seg(a, b) => dist_to_segment(cursor, *a, *b) <= tol,
        Edge::Ellipse { .. } => true,
    });

    let mut out = Vec::new();
    for i in 0..edges.len() {
        for j in i + 1..edges.len() {
            let (ida, ea) = edges[i];
            let (idb, eb) = edges[j];
            let pts = match (ea, eb) {
                (Edge::Seg(a, b), Edge::Seg(c, d)) => {
                    let Some(p) = seg_seg(a, b, c, d) else { continue };
                    // Innerhalb eines Objekts treffen sich benachbarte Kanten
                    // an ihrem gemeinsamen Punkt — das ist eine Ecke oder eine
                    // Stützstelle einer Kurve, kein Schnittpunkt.
                    if ida == idb {
                        let eps = 1e-3;
                        let shared = [a, b].iter().any(|q| (p - *q).length() < eps)
                            && [c, d].iter().any(|q| (p - *q).length() < eps);
                        if shared {
                            continue;
                        }
                    }
                    vec![p]
                }
                (Edge::Seg(a, b), Edge::Ellipse { c, rx, ry, rot })
                | (Edge::Ellipse { c, rx, ry, rot }, Edge::Seg(a, b)) => {
                    seg_ellipse(a, b, c, rx, ry, rot)
                }
                // Zwei Ellipsen: eine Gleichung vierten Grades — selten
                // gebraucht, bewusst weggelassen.
                (Edge::Ellipse { .. }, Edge::Ellipse { .. }) => continue,
            };
            out.extend(pts);
        }
    }
    out
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 < 1e-12 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Schnittpunkt zweier Strecken (parallele zählen nicht).
fn seg_seg(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> Option<Pos2> {
    let r = b - a;
    let s = d - c;
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-9 {
        return None;
    }
    let q = c - a;
    let t = (q.x * s.y - q.y * s.x) / den;
    let u = (q.x * r.y - q.y * r.x) / den;
    let eps = 1e-4;
    ((-eps..=1.0 + eps).contains(&t) && (-eps..=1.0 + eps).contains(&u)).then(|| a + r * t)
}

/// Schnittpunkte einer Strecke mit einer (gedrehten) Ellipse: im Raum der
/// Ellipse ist sie ein Einheitskreis, dort ist es eine quadratische
/// Gleichung.
fn seg_ellipse(a: Pos2, b: Pos2, c: Pos2, rx: f32, ry: f32, rot: f32) -> Vec<Pos2> {
    let to_unit = |p: Pos2| {
        let q = geometry::rotate_vec(p - c, -rot);
        Vec2::new(q.x / rx, q.y / ry)
    };
    let (p0, p1) = (to_unit(a), to_unit(b));
    let d = p1 - p0;
    let qa = d.length_sq();
    if qa < 1e-12 {
        return Vec::new();
    }
    let qb = 2.0 * p0.dot(d);
    let qc = p0.length_sq() - 1.0;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return Vec::new();
    }
    let sq = disc.sqrt();
    [(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)]
        .into_iter()
        .filter(|t| (0.0..=1.0).contains(t))
        .map(|t| a + (b - a) * t)
        .collect()
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
        let s = snap_point(Pos2::new(202.0, 101.0), None, &els, &[], 1.0, &SnapSettings::default()).unwrap();
        assert_eq!(s.kind, SnapKind::Endpoint);
        assert!((s.point - Pos2::new(200.0, 100.0)).length() < 1e-3);
        assert!(snap_point(Pos2::new(215.0, 100.0), None, &els, &[], 1.0, &SnapSettings::default()).is_none());
    }

    #[test]
    fn fangweite_schrumpft_beim_hineinzoomen() {
        // 4 pt daneben: bei 100 % (4 px) fängt es, bei 1000 % (40 px) nicht.
        let els = vec![line(1, (100.0, 100.0), (200.0, 100.0))];
        let c = Pos2::new(204.0, 100.0);
        assert!(snap_point(c, None, &els, &[], 1.0, &SnapSettings::default()).is_some());
        assert!(snap_point(c, None, &els, &[], 10.0, &SnapSettings::default()).is_none());
    }

    #[test]
    fn lot_auf_eine_kante() {
        let els = vec![line(1, (100.0, 100.0), (300.0, 100.0))];
        let from = Pos2::new(180.0, 20.0);
        let s = snap_point(Pos2::new(181.0, 98.0), Some(from), &els, &[], 1.0, &SnapSettings::default()).unwrap();
        assert_eq!(s.kind, SnapKind::Perpendicular);
        assert!((s.point - Pos2::new(180.0, 100.0)).length() < 1e-3);
    }

    #[test]
    fn polarfang_nur_fast_auf_der_achse() {
        let from = Pos2::new(0.0, 0.0);
        let s = snap_point(Pos2::new(200.0, 3.0), Some(from), &[], &[], 1.0, &SnapSettings::default()).unwrap();
        assert_eq!(s.kind, SnapKind::Polar(0));
        assert!((s.point - Pos2::new(200.0, 0.0)).length() < 1e-3);
        // 20 px daneben ist Absicht, kein Zittern.
        assert!(snap_point(Pos2::new(200.0, 20.0), Some(from), &[], &[], 1.0, &SnapSettings::default()).is_none());
        let s = snap_point(Pos2::new(2.0, -150.0), Some(from), &[], &[], 1.0, &SnapSettings::default()).unwrap();
        assert_eq!(s.kind.label(), "90°");
    }

    #[test]
    fn schnittpunkt_zweier_linien() {
        let els = vec![
            line(1, (100.0, 100.0), (300.0, 100.0)),
            line(2, (180.0, 20.0), (180.0, 200.0)),
        ];
        let all = SnapSettings::default();
        let s = snap_point(Pos2::new(183.0, 102.0), None, &els, &[], 1.0, &all).unwrap();
        assert_eq!(s.kind, SnapKind::Intersection);
        assert!((s.point - Pos2::new(180.0, 100.0)).length() < 1e-3);
    }

    #[test]
    fn schnittpunkt_linie_mit_ellipse() {
        let mut e = Element::new_ellipse(2, 100.0, 100.0);
        e.w = 100.0;
        e.h = 100.0; // Kreis um (150,150), r = 50
        let els = vec![line(1, (0.0, 150.0), (300.0, 150.0)), e];
        let s = snap_point(Pos2::new(203.0, 151.0), None, &els, &[], 1.0, &SnapSettings::default())
            .unwrap();
        // Der Quadrant liegt genau dort auch — beide sind richtig, der Punkt zählt.
        assert!((s.point - Pos2::new(200.0, 150.0)).length() < 1e-2, "{s:?}");
        let mut only_x = SnapSettings::default();
        only_x.set_all(false);
        only_x.intersection = true;
        let s = snap_point(Pos2::new(203.0, 151.0), None, &els, &[], 1.0, &only_x).unwrap();
        assert_eq!(s.kind, SnapKind::Intersection);
    }

    #[test]
    fn rechteckecke_bleibt_endpunkt_und_kurven_haben_keine_scheinschnitte() {
        let r = Element::new_rectangle(1, 100.0, 100.0);
        let s = snap_point(Pos2::new(101.0, 101.0), None, &[r], &[], 1.0, &SnapSettings::default())
            .unwrap();
        assert_eq!(s.kind, SnapKind::Endpoint);

        // Ein gekrümmter Pfad allein: Seine Zerlegungsstellen sind keine
        // Schnittpunkte.
        let nodes = vec![
            geometry::PathNode::corner(Pos2::new(0.0, 0.0)),
            geometry::PathNode {
                anchor: Pos2::new(100.0, 0.0),
                in_h: Pos2::new(50.0, -80.0),
                out_h: Pos2::new(100.0, 0.0),
            },
        ];
        let p = geometry::path_from_nodes(1, &nodes, false);
        let mid = geometry::cubic_at(
            Pos2::new(0.0, 0.0),
            Pos2::new(0.0, 0.0),
            Pos2::new(50.0, -80.0),
            Pos2::new(100.0, 0.0),
            0.5,
        );
        let mut only_x = SnapSettings::default();
        only_x.set_all(false);
        only_x.intersection = true;
        assert!(snap_point(mid, None, &[p], &[], 1.0, &only_x).is_none());
    }

    #[test]
    fn hauptschalter_und_einzelne_arten() {
        let els = vec![line(1, (100.0, 100.0), (200.0, 100.0))];
        let mut s = SnapSettings::default();
        s.enabled = false;
        assert!(snap_point(Pos2::new(200.0, 100.0), None, &els, &[], 1.0, &s).is_none());
        let mut s = SnapSettings::default();
        s.endpoint = false;
        assert!(snap_point(Pos2::new(200.0, 100.0), None, &els, &[], 1.0, &s).is_none());
        let mut s = SnapSettings::default();
        s.polar = false;
        assert!(snap_point(Pos2::new(500.0, 2.0), Some(Pos2::ZERO), &[], &[], 1.0, &s).is_none());
    }

    #[test]
    fn eigenes_objekt_wird_ausgelassen() {
        let els = vec![line(1, (100.0, 100.0), (200.0, 100.0))];
        assert!(snap_point(Pos2::new(200.0, 100.0), None, &els, &[1], 1.0, &SnapSettings::default()).is_none());
    }
}
