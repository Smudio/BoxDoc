//! Geometrie: Rotation und **Element-Umrisse in Seitenkoordinaten**.
//!
//! # Warum die Umrisse hier liegen
//!
//! Canvas und PDF-Export haben ihre Formen früher jeder für sich berechnet —
//! mit drei verschiedenen Vorzeichen-Konventionen in derselben Datei:
//!
//! * **Linie:** Der Canvas verankerte sie am *Mittelpunkt* und zeichnete
//!   ±w/2 um ihn herum, der PDF-Export am *Startpunkt* und lief von dort w
//!   weit. Bei `rotation = 0` fiel das nicht auf, bei jedem anderen Winkel
//!   stand die Linie im PDF völlig woanders.
//! * **Rechteck:** rotierte im PDF gegen den Uhrzeigersinn, auf dem Bildschirm
//!   im Uhrzeigersinn (lokales y-oben statt y-unten).
//! * **Ellipse:** wieder anders (y-unten), zufällig richtig.
//!
//! Deshalb gibt es diese Umrisse jetzt genau einmal, in
//! **BoxDoc-Seitenkoordinaten**: Punkte (pt), Ursprung oben links, **y wächst
//! nach unten**, Rotation im Uhrzeigersinn. Beide Renderer bilden davon nur
//! noch ab:
//!
//! * Canvas: `to_screen(p)` — Zoom und Verschiebung.
//! * PDF:    `(pt_to_mm(p.x), page_h_mm - pt_to_mm(p.y))` — y-Achse spiegeln.
//!
//! Da beide Abbildungen affin und uniform skalierend sind, ist das Ergebnis
//! per Konstruktion deckungsgleich.

use egui::{Pos2, Vec2};

use crate::model::Element;

/// Stützpunkte pro Viertelkreis an einer abgerundeten Rechteck-Ecke.
pub const CORNER_SEGMENTS: usize = 8;
/// Stützpunkte für den vollen Ellipsen-Umriss.
pub const ELLIPSE_SEGMENTS: usize = 64;

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

// ===========================================================================
// Element-Umrisse in Seitenkoordinaten (pt, y nach unten)
// ===========================================================================

/// Drehpunkt eines Elements: der Mittelpunkt seiner Box.
///
/// Gilt für **alle** Element-Arten, auch für Linien (deren `h` in aller Regel
/// 0 ist, sodass der Mittelpunkt auf der Linie selbst liegt).
pub fn element_center(el: &Element) -> Pos2 {
    Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0)
}

/// Die beiden Endpunkte einer Linie.
///
/// Eine Linie ist eine Box der Länge `w`, die um ihren Mittelpunkt gedreht
/// wird. Bei `rotation = 0` läuft sie waagerecht von `(x, y)` nach
/// `(x + w, y)`.
pub fn line_endpoints(el: &Element) -> (Pos2, Pos2) {
    let center = element_center(el);
    let half = Vec2::new(el.w / 2.0, 0.0);
    (
        local_to_world(center, el.rotation, -half),
        local_to_world(center, el.rotation, half),
    )
}

/// Die vier Eckpunkte der Element-Box (ab oben links).
///
/// Für Bilder und Auswahlrahmen.
pub fn quad_corners(el: &Element) -> [Pos2; 4] {
    let center = element_center(el);
    let l = local_corners(el.w, el.h);
    [
        local_to_world(center, el.rotation, l[0]),
        local_to_world(center, el.rotation, l[1]),
        local_to_world(center, el.rotation, l[2]),
        local_to_world(center, el.rotation, l[3]),
    ]
}

/// Umriss eines Rechtecks als geschlossener Linienzug.
///
/// Bei `corner_radius > 0` werden die Ecken durch Viertelkreise ersetzt.
/// Bewusst als Polygonzug und nicht als Bézier-Kurve: Nur so ist garantiert,
/// dass Bildschirm und PDF exakt dieselbe Form zeigen.
pub fn rect_outline(el: &Element) -> Vec<Pos2> {
    let center = element_center(el);
    let (hw, hh) = (el.w / 2.0, el.h / 2.0);
    let r = el.corner_radius.max(0.0).min(hw.abs()).min(hh.abs());

    if r <= 0.05 {
        return local_corners(el.w, el.h)
            .iter()
            .map(|l| local_to_world(center, el.rotation, *l))
            .collect();
    }

    // Mittelpunkte der vier Eckkreise und der jeweilige Startwinkel, im
    // Uhrzeigersinn ab oben links. In diesem y-unten-System läuft ein
    // wachsender Winkel visuell im Uhrzeigersinn.
    let corners = [
        (Vec2::new(-hw + r, -hh + r), std::f32::consts::PI),
        (Vec2::new(hw - r, -hh + r), 1.5 * std::f32::consts::PI),
        (Vec2::new(hw - r, hh - r), 0.0),
        (Vec2::new(-hw + r, hh - r), 0.5 * std::f32::consts::PI),
    ];

    let mut pts = Vec::with_capacity(corners.len() * (CORNER_SEGMENTS + 1));
    for (c, start) in corners {
        for i in 0..=CORNER_SEGMENTS {
            let t = start + (i as f32 / CORNER_SEGMENTS as f32) * 0.5 * std::f32::consts::PI;
            let local = c + Vec2::new(r * t.cos(), r * t.sin());
            pts.push(local_to_world(center, el.rotation, local));
        }
    }
    pts
}

/// Umriss einer Ellipse als geschlossener Linienzug (Kreis = Sonderfall w == h).
pub fn ellipse_outline(el: &Element) -> Vec<Pos2> {
    let center = element_center(el);
    let rx = el.w / 2.0;
    let ry = el.h / 2.0;
    (0..ELLIPSE_SEGMENTS)
        .map(|i| {
            let t = i as f32 * std::f32::consts::TAU / ELLIPSE_SEGMENTS as f32;
            let local = Vec2::new(rx * t.cos(), ry * t.sin());
            local_to_world(center, el.rotation, local)
        })
        .collect()
}

/// Umriss eines freien Pfads als Linienzug in Seitenkoordinaten.
///
/// Die Stützpunkte liegen im Element normalisiert auf `[0,1]²` vor (siehe
/// [`Element::points`]); hier werden sie auf die Box gelegt und mitgedreht.
/// Ob der Zug geschlossen ist, steht in `el.path_closed` — der Umriss selbst
/// enthält den Startpunkt **nicht** doppelt.
pub fn path_outline(el: &Element) -> Vec<Pos2> {
    let center = element_center(el);
    let (hw, hh) = (el.w / 2.0, el.h / 2.0);
    el.points
        .iter()
        .map(|[nx, ny]| {
            let local = Vec2::new(nx * el.w - hw, ny * el.h - hh);
            local_to_world(center, el.rotation, local)
        })
        .collect()
}

/// Ist das Polygon konvex? Kollineare Ecken (Vorzeichen 0) stören nicht.
fn is_convex(pts: &[Pos2]) -> bool {
    let n = pts.len();
    let mut sign = 0i8;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let c = pts[(i + 2) % n];
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        let s = if cross > 1e-6 {
            1
        } else if cross < -1e-6 {
            -1
        } else {
            0
        };
        if s != 0 {
            if sign != 0 && s != sign {
                return false;
            }
            sign = s;
        }
    }
    true
}

/// Zerlegt ein einfaches Polygon in Dreiecke (Ear Clipping).
///
/// Gebraucht wird das für die **Füllung**: Ein Dreiecksfächer vom ersten Punkt
/// aus — der naheliegende Weg — ist nur bei konvexen Formen richtig. Bei einem
/// konkaven Umriss (jedes L, jeder Pfeil, jedes Sternchen aus einem PDF) malt
/// er über die Einbuchtung hinweg. Ear Clipping ist für überschneidungsfreie
/// Polygone korrekt, und genau solche liefert ein PDF-Teilpfad.
///
/// Rückgabe sind Indizes in `pts`, je drei ein Dreieck. Bei entarteten
/// Eingaben (< 3 Punkte) ist das Ergebnis leer.
pub fn triangulate(pts: &[Pos2]) -> Vec<[u32; 3]> {
    let n = pts.len();
    if n < 3 {
        return Vec::new();
    }

    // Konvexe Umrisse — Rechteck, Ellipse, die meisten importierten Pfade —
    // brauchen kein Ear Clipping. Der Fächer ist hier korrekt und linear
    // statt quadratisch; das zählt, weil gefüllt wird, solange das Fenster
    // offen ist.
    if is_convex(pts) {
        return (1..n as u32 - 1).map(|i| [0, i, i + 1]).collect();
    }

    // Umlaufsinn bestimmen (Gaußsche Trapezformel). Wir arbeiten intern immer
    // gegen den Uhrzeigersinn, damit „innen" ein festes Vorzeichen hat.
    let mut area2 = 0.0f32;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        area2 += a.x * b.y - b.x * a.y;
    }
    let mut idx: Vec<u32> = (0..n as u32).collect();
    if area2 < 0.0 {
        idx.reverse();
    }

    let cross = |o: Pos2, a: Pos2, b: Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let in_triangle = |p: Pos2, a: Pos2, b: Pos2, c: Pos2| {
        // Alle drei Kanten-Kreuzprodukte gleich signiert → innerhalb.
        let d1 = cross(a, b, p);
        let d2 = cross(b, c, p);
        let d3 = cross(c, a, p);
        d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
    };

    let mut out = Vec::with_capacity(n.saturating_sub(2));
    // Notbremse: Bei numerisch entarteten Polygonen (Punkte doppelt, Kanten
    // kollinear) findet die Schleife irgendwann kein Ohr mehr. Statt endlos zu
    // drehen, brechen wir ab und liefern, was bis dahin sicher ist.
    let mut guard = n * n + 16;
    let mut i = 0usize;
    while idx.len() > 2 && guard > 0 {
        guard -= 1;
        let len = idx.len();
        let (ia, ib, ic) = (idx[i % len], idx[(i + 1) % len], idx[(i + 2) % len]);
        let (a, b, c) = (pts[ia as usize], pts[ib as usize], pts[ic as usize]);

        // Konvexe Ecke?
        if cross(a, b, c) > 0.0 {
            // Kein anderer Punkt darf im Ohr liegen.
            let clean = idx
                .iter()
                .filter(|k| **k != ia && **k != ib && **k != ic)
                .all(|k| !in_triangle(pts[*k as usize], a, b, c));
            if clean {
                out.push([ia, ib, ic]);
                idx.remove((i + 1) % len);
                if i >= idx.len() {
                    i = 0;
                }
                continue;
            }
        }
        i = (i + 1) % idx.len();
    }
    out
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use crate::model::Element;

    /// Fläche eines Polygons (Betrag) — Maßstab für die Triangulierung.
    fn polygon_area(pts: &[Pos2]) -> f32 {
        let n = pts.len();
        let mut a = 0.0;
        for i in 0..n {
            let p = pts[i];
            let q = pts[(i + 1) % n];
            a += p.x * q.y - q.x * p.y;
        }
        (a / 2.0).abs()
    }

    fn triangle_area(a: Pos2, b: Pos2, c: Pos2) -> f32 {
        ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() / 2.0
    }

    /// Die Dreiecke müssen die Polygonfläche exakt ausfüllen — nicht mehr
    /// (Überlappung/Ausbuchtung) und nicht weniger (Loch).
    fn assert_covers(pts: &[Pos2], label: &str) {
        let tris = triangulate(pts);
        assert_eq!(
            tris.len(),
            pts.len() - 2,
            "{label}: {} Dreiecke für {} Punkte",
            tris.len(),
            pts.len()
        );
        let sum: f32 = tris
            .iter()
            .map(|[a, b, c]| triangle_area(pts[*a as usize], pts[*b as usize], pts[*c as usize]))
            .sum();
        let area = polygon_area(pts);
        assert!(
            (sum - area).abs() < area * 0.001,
            "{label}: Dreiecksfläche {sum} != Polygonfläche {area}"
        );
    }

    #[test]
    fn triangulierung_deckt_konvexe_polygone() {
        assert_covers(
            &[
                Pos2::new(0.0, 0.0),
                Pos2::new(100.0, 0.0),
                Pos2::new(100.0, 50.0),
                Pos2::new(0.0, 50.0),
            ],
            "Rechteck",
        );
    }

    #[test]
    fn triangulierung_deckt_konkave_polygone() {
        // Ein L. Der frühere Dreiecksfächer hätte die Einbuchtung mit gefüllt
        // und damit mehr Fläche gemalt, als die Form hat.
        assert_covers(
            &[
                Pos2::new(0.0, 0.0),
                Pos2::new(60.0, 0.0),
                Pos2::new(60.0, 20.0),
                Pos2::new(20.0, 20.0),
                Pos2::new(20.0, 80.0),
                Pos2::new(0.0, 80.0),
            ],
            "L-Form",
        );
    }

    #[test]
    fn triangulierung_egal_ob_im_oder_gegen_uhrzeigersinn() {
        let mut pts = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(60.0, 0.0),
            Pos2::new(60.0, 20.0),
            Pos2::new(20.0, 20.0),
            Pos2::new(20.0, 80.0),
            Pos2::new(0.0, 80.0),
        ];
        assert_covers(&pts, "L vorwärts");
        pts.reverse();
        assert_covers(&pts, "L rückwärts");
    }

    #[test]
    fn triangulierung_bricht_bei_entarteten_eingaben_ab() {
        // Darf weder panicken noch hängen bleiben.
        assert!(triangulate(&[]).is_empty());
        assert!(triangulate(&[Pos2::ZERO, Pos2::new(1.0, 1.0)]).is_empty());
        let doppelt = vec![Pos2::ZERO, Pos2::ZERO, Pos2::ZERO, Pos2::ZERO];
        let _ = triangulate(&doppelt);
    }

    #[test]
    fn pfad_umriss_folgt_box_und_drehung() {
        // Normalisierte Punkte müssen sich exakt auf die Box abbilden.
        let el = Element::new_path(
            1,
            &[(100.0, 200.0), (300.0, 200.0), (300.0, 260.0)],
            true,
        );
        assert!((el.x - 100.0).abs() < 0.01 && (el.y - 200.0).abs() < 0.01);
        assert!((el.w - 200.0).abs() < 0.01 && (el.h - 60.0).abs() < 0.01);

        let out = path_outline(&el);
        for (p, (x, y)) in out
            .iter()
            .zip([(100.0, 200.0), (300.0, 200.0), (300.0, 260.0)])
        {
            assert!(
                (p.x - x).abs() < 0.01 && (p.y - y).abs() < 0.01,
                "Punkt {p:?} statt ({x},{y})"
            );
        }

        // Verschieben und Drehen laufen über die Box — die Stützpunkte bleiben.
        let mut moved = el.clone();
        moved.x += 50.0;
        moved.rotation = 90.0;
        assert_eq!(moved.points, el.points);
        let center = element_center(&moved);
        for (p, q) in path_outline(&moved).iter().zip(out.iter()) {
            // Jeder Punkt muss um 90 Grad um die (verschobene) Mitte gedreht sein.
            let expected = local_to_world(
                center,
                90.0,
                Vec2::new(q.x + 50.0 - center.x, q.y - center.y),
            );
            assert!((*p - expected).length() < 0.01, "{p:?} != {expected:?}");
        }
    }

    #[test]
    fn pfad_ohne_ausdehnung_bleibt_nutzbar() {
        // Senkrechter Zug: Breite 0 — die Normalisierung darf nicht durch
        // null teilen und das Element nicht unsichtbar machen.
        let el = Element::new_path(1, &[(50.0, 10.0), (50.0, 90.0)], false);
        assert!(el.w > 0.0 && el.h > 0.0);
        assert!(el.points.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
        let out = path_outline(&el);
        assert_eq!(out.len(), 2);
        assert!((out[1].y - out[0].y - 80.0).abs() < 0.01);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Element;

    fn line(x: f32, y: f32, w: f32, rot: f32) -> Element {
        let mut el = Element::new_line(1, x, y);
        el.w = w;
        el.h = 0.0;
        el.rotation = rot;
        el
    }

    fn close(a: Pos2, b: Pos2) -> bool {
        (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
    }

    #[test]
    fn unrotierte_linie_laeuft_von_x_nach_x_plus_w() {
        let el = line(100.0, 200.0, 300.0, 0.0);
        let (a, b) = line_endpoints(&el);
        assert!(close(a, Pos2::new(100.0, 200.0)), "Start war {a:?}");
        assert!(close(b, Pos2::new(400.0, 200.0)), "Ende war {b:?}");
    }

    #[test]
    fn linie_dreht_um_ihren_mittelpunkt() {
        // Genau hier lag der PDF-Fehler: Der Export drehte um den Startpunkt,
        // wodurch die Linie an einer ganz anderen Stelle landete.
        let el = line(100.0, 200.0, 200.0, 90.0);
        let (a, b) = line_endpoints(&el);
        assert!(close(element_center(&el), Pos2::new(200.0, 200.0)));
        assert!(close(a, Pos2::new(200.0, 100.0)), "Start war {a:?}");
        assert!(close(b, Pos2::new(200.0, 300.0)), "Ende war {b:?}");
    }

    #[test]
    fn linienlaenge_bleibt_bei_jeder_drehung_erhalten() {
        for rot in [0.0_f32, 17.0, 45.0, 90.0, 180.0, 275.0, -33.0] {
            let el = line(50.0, 50.0, 240.0, rot);
            let (a, b) = line_endpoints(&el);
            let len = (b - a).length();
            assert!(
                (len - 240.0).abs() < 0.01,
                "bei {rot} Grad war die Länge {len}"
            );
            let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            assert!(close(mid, element_center(&el)), "Mitte wandert bei {rot}");
        }
    }

    #[test]
    fn rotation_laeuft_im_uhrzeigersinn() {
        // Positive Rotation muss visuell im Uhrzeigersinn drehen — auf dem
        // Bildschirm wie im PDF. Beim Rechteck war das im PDF invertiert.
        let mut el = Element::new_rectangle(1, 0.0, 0.0);
        el.x = 0.0;
        el.y = 0.0;
        el.w = 100.0;
        el.h = 100.0;
        el.rotation = 90.0;
        let corners = quad_corners(&el);
        // Ecke oben links (-50,-50) landet nach 90 Grad im Uhrzeigersinn bei
        // (+50,-50) relativ zum Mittelpunkt (50,50) -> (100, 0).
        assert!(close(corners[0], Pos2::new(100.0, 0.0)), "war {:?}", corners[0]);
    }

    #[test]
    fn unrotiertes_rechteck_deckt_die_box_ab() {
        let mut el = Element::new_rectangle(1, 10.0, 20.0);
        el.w = 200.0;
        el.h = 100.0;
        el.corner_radius = 0.0;
        let pts = rect_outline(&el);
        assert_eq!(pts.len(), 4);
        let min_x = pts.iter().fold(f32::MAX, |m, p| m.min(p.x));
        let max_x = pts.iter().fold(f32::MIN, |m, p| m.max(p.x));
        let min_y = pts.iter().fold(f32::MAX, |m, p| m.min(p.y));
        let max_y = pts.iter().fold(f32::MIN, |m, p| m.max(p.y));
        assert!((min_x - 10.0).abs() < 0.01);
        assert!((max_x - 210.0).abs() < 0.01);
        assert!((min_y - 20.0).abs() < 0.01);
        assert!((max_y - 120.0).abs() < 0.01);
    }

    #[test]
    fn eckradius_bleibt_innerhalb_der_box() {
        let mut el = Element::new_rectangle(1, 0.0, 0.0);
        el.w = 100.0;
        el.h = 60.0;
        el.corner_radius = 500.0; // absurd gross -> muss begrenzt werden
        for p in rect_outline(&el) {
            assert!(
                p.x >= -0.01 && p.x <= 100.01 && p.y >= -0.01 && p.y <= 60.01,
                "Punkt {p:?} liegt ausserhalb der Box"
            );
        }
    }

    #[test]
    fn ellipse_liegt_in_ihrer_box() {
        let mut el = Element::new_ellipse(1, 30.0, 40.0);
        el.w = 200.0;
        el.h = 80.0;
        let pts = ellipse_outline(&el);
        assert_eq!(pts.len(), ELLIPSE_SEGMENTS);
        for p in &pts {
            assert!(
                p.x >= 29.99 && p.x <= 230.01 && p.y >= 39.99 && p.y <= 120.01,
                "Punkt {p:?} liegt ausserhalb"
            );
        }
    }
}
