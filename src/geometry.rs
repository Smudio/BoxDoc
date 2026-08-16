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

/// Achsenparalleles Rechteck, das das gedrehte Element vollständig umschließt.
///
/// Bewusst **nicht** einfach `x/y/w/h`: Ein gedrehtes Element ragt darüber
/// hinaus. Wer damit eine Leinwand aufspannt — der SVG-Export tut das —, würde
/// sonst Ecken abschneiden.
///
/// Die Kontur zählt, nicht die Box: Bei einem Pfad ist `w`×`h` zwar per
/// Konstruktion die Hüllbox der Kurve, bei einer Linie mit `h == 0` aber nicht
/// mehr, sobald sie gedreht ist.
pub fn element_bounds(el: &Element) -> egui::Rect {
    let pts: Vec<Pos2> = match el.kind {
        crate::model::ElementKind::Line => {
            let (a, b) = line_endpoints(el);
            vec![a, b]
        }
        crate::model::ElementKind::Path => {
            let outline = path_outline(el);
            if outline.is_empty() {
                quad_corners(el).to_vec()
            } else {
                outline
            }
        }
        _ => quad_corners(el).to_vec(),
    };
    let mut r = egui::Rect::NOTHING;
    for p in pts {
        r.extend_with(p);
    }
    r
}

/// Gemeinsame Hüllbox mehrerer Elemente. `None`, wenn die Liste leer ist.
pub fn elements_bounds<'a>(els: impl IntoIterator<Item = &'a Element>) -> Option<egui::Rect> {
    let mut out: Option<egui::Rect> = None;
    for el in els {
        let b = element_bounds(el);
        out = Some(match out {
            Some(acc) => acc.union(b),
            None => b,
        });
    }
    out
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

// ===========================================================================
// Freie Pfade: Knoten, Kurven, Bearbeitung
// ===========================================================================

/// Ein Pfadknoten in **absoluten Seitenkoordinaten** (pt).
///
/// `in_h` und `out_h` sind die kubischen Bézier-Kontrollpunkte *vor* und
/// *nach* dem Stützpunkt — als Positionen, nicht als Abstände, genau wie im
/// Modell ([`Element::handles`]). Ein **Eckknoten** hat beide Griffe auf dem
/// Stützpunkt liegen; das ist kein Sonderfall in den Formeln, sondern fällt
/// automatisch auf eine Gerade zusammen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathNode {
    pub anchor: Pos2,
    pub in_h: Pos2,
    pub out_h: Pos2,
}

impl PathNode {
    /// Eckknoten: beide Griffe liegen auf dem Stützpunkt.
    pub fn corner(anchor: Pos2) -> Self {
        PathNode {
            anchor,
            in_h: anchor,
            out_h: anchor,
        }
    }

    /// Ist das ein Eckknoten (beide Griffe auf dem Stützpunkt)?
    pub fn is_corner(&self) -> bool {
        (self.in_h - self.anchor).length() < CORNER_EPS
            && (self.out_h - self.anchor).length() < CORNER_EPS
    }

    /// Liegen beide Griffe auf einer Geraden durch den Stützpunkt?
    /// Nur dann läuft die Kurve durch den Knoten ohne Knick.
    pub fn is_smooth(&self) -> bool {
        let a = self.anchor - self.in_h;
        let b = self.out_h - self.anchor;
        if a.length() < CORNER_EPS || b.length() < CORNER_EPS {
            return false;
        }
        let cross = a.x * b.y - a.y * b.x;
        cross.abs() < a.length() * b.length() * 0.02
    }

    fn translated(&self, d: Vec2) -> Self {
        PathNode {
            anchor: self.anchor + d,
            in_h: self.in_h + d,
            out_h: self.out_h + d,
        }
    }
}

/// Abstand, unterhalb dessen ein Griff als „auf dem Stützpunkt" gilt (pt).
const CORNER_EPS: f32 = 0.001;

/// Kleinste Boxkante eines Pfads in pt — darunter wäre er nicht mehr
/// anklickbar und die Normalisierung numerisch instabil.
pub const PATH_MIN_EXTENT: f32 = 0.5;

/// Ein Segment eines Pfads, wie es ein Zeichensystem entgegennimmt.
///
/// Gebraucht für den **PDF-Export**: printpdf kann echte kubische Kurven
/// schreiben, und nur so kommt eine importierte Kurve als Kurve zurück statt
/// als Vieleck mit ein paar hundert Ecken.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSeg {
    /// Gerade zum Punkt.
    Line(Pos2),
    /// Kubische Kurve: zwei Kontrollpunkte, dann der Endpunkt.
    Cubic(Pos2, Pos2, Pos2),
}

/// Die Stützpunkte eines Pfads in Seitenkoordinaten, mit ihren Griffen.
///
/// Die Werte liegen im Element normalisiert auf `[0,1]²` vor (siehe
/// [`Element::points`]); hier werden sie auf die Box gelegt und mitgedreht.
/// Griffe durchlaufen **dieselbe** Abbildung wie Stützpunkte — daher gibt es
/// hier keinen Sonderfall für Drehung oder ungleichmäßige Skalierung.
pub fn path_nodes(el: &Element) -> Vec<PathNode> {
    let center = element_center(el);
    let (hw, hh) = (el.w / 2.0, el.h / 2.0);
    let map = |nx: f32, ny: f32| {
        local_to_world(center, el.rotation, Vec2::new(nx * el.w - hw, ny * el.h - hh))
    };
    let has_handles = el.handles.len() == el.points.len();
    el.points
        .iter()
        .enumerate()
        .map(|(i, [nx, ny])| {
            let anchor = map(*nx, *ny);
            if has_handles {
                let h = el.handles[i];
                PathNode {
                    anchor,
                    in_h: map(h[0], h[1]),
                    out_h: map(h[2], h[3]),
                }
            } else {
                PathNode::corner(anchor)
            }
        })
        .collect()
}

/// Schreibt Knoten in **absoluten Seitenkoordinaten** zurück ins Element und
/// legt Box und Normalisierung neu fest.
///
/// Das ist der einzige erlaubte Weg, Stützpunkte zu ändern. Der Grund ist die
/// Invariante, an der alles andere hängt: **die Box umschließt den Pfad**.
/// Hit-Test, Auswahl-Rechteck, Snapping und Ausrichtung lesen ausschließlich
/// `x`/`y`/`w`/`h` — zöge ein Knoten aus der Box heraus, wäre der Pfad an
/// dieser Stelle weder anklickbar noch ausrichtbar.
///
/// Die Box wird über die **tatsächlich gezeichnete Kurve** gelegt, nicht über
/// die Stützpunkte: Eine Kurve beult zwischen zwei Stützpunkten aus, und ihre
/// Griffe liegen umgekehrt oft weit außerhalb der Form. Beides ergäbe eine
/// Box, die entweder zu klein oder sichtbar zu groß ist.
///
/// Die Drehung bleibt erhalten; gerechnet wird deshalb im unrotierten Rahmen
/// des Elements, und der Mittelpunkt wandert mitgedreht an seinen neuen Platz.
pub fn set_path_nodes(el: &mut Element, nodes: &[PathNode]) {
    if nodes.is_empty() {
        el.points.clear();
        el.handles.clear();
        el.w = el.w.max(PATH_MIN_EXTENT);
        el.h = el.h.max(PATH_MIN_EXTENT);
        return;
    }

    let center = element_center(el);
    let rot = el.rotation;
    let to_local = |p: Pos2| rotate_vec(p - center, -rot).to_pos2();
    let local: Vec<PathNode> = nodes
        .iter()
        .map(|n| PathNode {
            anchor: to_local(n.anchor),
            in_h: to_local(n.in_h),
            out_h: to_local(n.out_h),
        })
        .collect();

    // Hüllbox über die gezeichnete Kurve.
    let flat = flatten_nodes(&local, el.path_closed);
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in &flat {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let (span_x, span_y) = (max_x - min_x, max_y - min_y);
    let w = span_x.max(PATH_MIN_EXTENT);
    let h = span_y.max(PATH_MIN_EXTENT);
    // Bei einem Pfad ohne Ausdehnung in einer Richtung (senkrechter Zug) wird
    // die Mindestbreite um ihn herum zentriert, damit der Ursprung der
    // Normalisierung und die Boxkante zusammenfallen.
    let origin_x = min_x - (w - span_x) / 2.0;
    let origin_y = min_y - (h - span_y) / 2.0;

    // Der Mittelpunkt der neuen Box, zurück in Seitenkoordinaten.
    let center_local = Vec2::new(origin_x + w / 2.0, origin_y + h / 2.0);
    let new_center = center + rotate_vec(center_local, rot);
    el.x = new_center.x - w / 2.0;
    el.y = new_center.y - h / 2.0;
    el.w = w;
    el.h = h;

    let nx = |x: f32| (x - origin_x) / w;
    let ny = |y: f32| (y - origin_y) / h;
    el.points = local.iter().map(|n| [nx(n.anchor.x), ny(n.anchor.y)]).collect();
    if local.iter().all(|n| n.is_corner()) {
        // Reiner Streckenzug: Das Feld bleibt leer und taucht in der Datei
        // gar nicht erst auf.
        el.handles.clear();
    } else {
        el.handles = local
            .iter()
            .map(|n| [nx(n.in_h.x), ny(n.in_h.y), nx(n.out_h.x), ny(n.out_h.y)])
            .collect();
    }
}

/// Baut ein Pfad-Element aus Knoten in absoluten Seitenkoordinaten.
pub fn path_from_nodes(id: u64, nodes: &[PathNode], closed: bool) -> Element {
    let mut el = Element::new_path(id, &[], closed);
    // `new_path` legt eine Mini-Box im Ursprung an; `set_path_nodes` rechnet
    // von dort aus und setzt Box wie Normalisierung selbst.
    set_path_nodes(&mut el, nodes);
    el
}

/// Die Segmente eines Pfads: Startpunkt und was danach kommt.
///
/// Gerade Abschnitte kommen als [`PathSeg::Line`] zurück, auch wenn der Pfad
/// insgesamt Kurven hat — ein gerades Stück als Kurve zu schreiben wäre im PDF
/// nur unnötiger Ballast.
pub fn path_segments(el: &Element) -> Option<(Pos2, Vec<PathSeg>)> {
    let nodes = path_nodes(el);
    if nodes.len() < 2 {
        return None;
    }
    let mut segs = Vec::with_capacity(nodes.len());
    for (a, b) in node_pairs(&nodes, el.path_closed) {
        segs.push(segment_between(a, b));
    }
    Some((nodes[0].anchor, segs))
}

/// Ein einzelnes Segment zwischen zwei Knoten.
fn segment_between(a: PathNode, b: PathNode) -> PathSeg {
    if (a.out_h - a.anchor).length() < CORNER_EPS && (b.in_h - b.anchor).length() < CORNER_EPS {
        PathSeg::Line(b.anchor)
    } else {
        PathSeg::Cubic(a.out_h, b.in_h, b.anchor)
    }
}

/// Alle aufeinanderfolgenden Knotenpaare; bei geschlossenem Pfad zusätzlich
/// das Paar (letzter, erster).
fn node_pairs(nodes: &[PathNode], closed: bool) -> Vec<(PathNode, PathNode)> {
    let mut out: Vec<(PathNode, PathNode)> =
        nodes.windows(2).map(|w| (w[0], w[1])).collect();
    if closed && nodes.len() > 2 {
        out.push((nodes[nodes.len() - 1], nodes[0]));
    }
    out
}

/// Anzahl der Segmente eines Pfads (siehe [`node_pairs`]).
pub fn path_segment_count(el: &Element) -> usize {
    let n = el.points.len();
    if n < 2 {
        0
    } else if el.path_closed && n > 2 {
        n
    } else {
        n - 1
    }
}

/// Stützstellen, in die eine kubische Kurve zerlegt wird.
///
/// Die Zahl richtet sich nach der groben Länge: Eine 2 pt lange Rundung
/// braucht keine 24 Punkte, ein großer Bogen schon.
///
/// Die Obergrenze ist bewusst niedrig. `path_outline` läuft in **jedem Frame**
/// für jeden sichtbaren Pfad, und eine gefüllte Fläche wird anschließend
/// trianguliert — das kostet quadratisch in der Punktzahl. Genauigkeit kostet
/// das kaum: Ein Viertelkreis mit Radius 100 pt weicht bei 24 Stützstellen um
/// rund 0,05 pt von der echten Kurve ab, also weit unter einem Bildpunkt.
/// Ins PDF geht ohnehin die echte Kurve (siehe [`path_segments`]).
fn cubic_steps(p0: Pos2, c1: Pos2, c2: Pos2, p3: Pos2) -> usize {
    let rough = (c1 - p0).length() + (c2 - c1).length() + (p3 - c2).length();
    ((rough / 6.0).ceil() as usize).clamp(6, 24)
}

/// Punkt auf einer kubischen Bézier-Kurve.
pub fn cubic_at(p0: Pos2, c1: Pos2, c2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Pos2::new(
        a * p0.x + b * c1.x + c * c2.x + d * p3.x,
        a * p0.y + b * c1.y + c * c2.y + d * p3.y,
    )
}

/// Löst eine kubische Kurve in Strecken auf und hängt sie an `out` — **ohne**
/// den Startpunkt, der dort schon steht.
pub fn flatten_cubic(p0: Pos2, c1: Pos2, c2: Pos2, p3: Pos2, out: &mut Vec<Pos2>) {
    let steps = cubic_steps(p0, c1, c2, p3);
    for i in 1..=steps {
        out.push(cubic_at(p0, c1, c2, p3, i as f32 / steps as f32));
    }
}

/// Zerlegt eine Knotenfolge in einen Streckenzug.
///
/// Gerade Abschnitte bleiben eine einzige Strecke — ein Pfad ohne Kurven
/// ergibt damit exakt seine Stützpunkte und nichts weiter.
fn flatten_nodes(nodes: &[PathNode], closed: bool) -> Vec<Pos2> {
    if nodes.is_empty() {
        return Vec::new();
    }
    let mut out = vec![nodes[0].anchor];
    for (a, b) in node_pairs(nodes, closed) {
        match segment_between(a, b) {
            PathSeg::Line(p) => out.push(p),
            PathSeg::Cubic(c1, c2, p) => flatten_cubic(a.anchor, c1, c2, p, &mut out),
        }
    }
    // Der geschlossene Zug läuft zum Startpunkt zurück; der Umriss enthält ihn
    // aber nur einmal (siehe [`path_outline`]).
    if closed && out.len() > 1 {
        out.pop();
    }
    out
}

/// Umriss eines freien Pfads als Linienzug in Seitenkoordinaten.
///
/// Kurven sind hier bereits in Strecken aufgelöst — der Bildschirm zeichnet
/// den Umriss, der PDF-Export dagegen die echten Kurven aus
/// [`path_segments`]. Der sichtbare Unterschied liegt unter einem Zehntel
/// Punkt; dafür bleibt eine importierte Kurve im PDF eine Kurve.
///
/// Ob der Zug geschlossen ist, steht in `el.path_closed` — der Umriss selbst
/// enthält den Startpunkt **nicht** doppelt.
pub fn path_outline(el: &Element) -> Vec<Pos2> {
    flatten_nodes(&path_nodes(el), el.path_closed)
}

/// Der Punkt auf einem Pfad, der einem gegebenen Punkt am nächsten liegt.
#[derive(Debug, Clone, Copy)]
pub struct PathHit {
    /// Index des Segments (0 = zwischen Knoten 0 und 1).
    pub seg: usize,
    /// Lage im Segment, 0..1.
    pub t: f32,
    /// Der Punkt selbst, in Seitenkoordinaten.
    pub pos: Pos2,
    /// Abstand zum gesuchten Punkt, in Seitenkoordinaten.
    pub dist: f32,
}

/// Sucht den nächstliegenden Punkt **auf** dem Pfad.
///
/// Gebraucht an zwei Stellen: für den Hit-Test (ein offener Pfad ist nur auf
/// seiner Linie anklickbar, nicht in seiner ganzen Box) und zum Einfügen eines
/// Knotens per Doppelklick auf ein Segment.
pub fn path_nearest(el: &Element, target: Pos2) -> Option<PathHit> {
    let nodes = path_nodes(el);
    if nodes.len() < 2 {
        return None;
    }
    /// Abtastung je Segment. Feiner als nötig für den Hit-Test, aber der
    /// Einfügepunkt soll spürbar unter dem Cursor liegen.
    const SAMPLES: usize = 24;

    let mut best: Option<PathHit> = None;
    for (seg, (a, b)) in node_pairs(&nodes, el.path_closed).into_iter().enumerate() {
        let (c1, c2) = match segment_between(a, b) {
            PathSeg::Line(p) => {
                // Gerade: exakt statt abgetastet.
                let ab = p - a.anchor;
                let len2 = ab.dot(ab);
                let t = if len2 > 1e-9 {
                    ((target - a.anchor).dot(ab) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let pos = a.anchor + ab * t;
                let dist = (target - pos).length();
                if best.is_none_or(|h| dist < h.dist) {
                    best = Some(PathHit { seg, t, pos, dist });
                }
                continue;
            }
            PathSeg::Cubic(c1, c2, _) => (c1, c2),
        };
        for i in 0..=SAMPLES {
            let t = i as f32 / SAMPLES as f32;
            let pos = cubic_at(a.anchor, c1, c2, b.anchor, t);
            let dist = (target - pos).length();
            if best.is_none_or(|h| dist < h.dist) {
                best = Some(PathHit { seg, t, pos, dist });
            }
        }
    }
    best
}

/// Liegt ein Punkt innerhalb eines geschlossenen Umrisses?
/// (Strahlverfahren, gerade/ungerade Schnitte.)
pub fn point_in_polygon(pts: &[Pos2], p: Pos2) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Fügt an der Stelle `hit` einen Knoten ein, **ohne die Form zu verändern**.
///
/// Bei einer Kurve geschieht das über die Unterteilung nach De Casteljau: Die
/// beiden Teilkurven zusammen sind punktgleich mit der ursprünglichen, es
/// ändern sich nur die Griffe der Nachbarknoten. Genau das erwartet man beim
/// Einfügen — die Linie darf nicht springen.
pub fn insert_node(el: &mut Element, hit: &PathHit) -> Option<usize> {
    let nodes = path_nodes(el);
    let count = path_segment_count(el);
    if hit.seg >= count {
        return None;
    }
    let i = hit.seg;
    let j = (i + 1) % nodes.len();
    let (a, b) = (nodes[i], nodes[j]);
    let t = hit.t.clamp(0.0, 1.0);

    let mut out = nodes.clone();
    match segment_between(a, b) {
        PathSeg::Line(_) => {
            let p = a.anchor + (b.anchor - a.anchor) * t;
            out.insert(i + 1, PathNode::corner(p));
        }
        PathSeg::Cubic(c1, c2, _) => {
            // De Casteljau: einmal auf jeder Ebene interpolieren.
            let lerp = |p: Pos2, q: Pos2| p + (q - p) * t;
            let p01 = lerp(a.anchor, c1);
            let p12 = lerp(c1, c2);
            let p23 = lerp(c2, b.anchor);
            let p012 = lerp(p01, p12);
            let p123 = lerp(p12, p23);
            let mid = lerp(p012, p123);

            out[i].out_h = p01;
            out[j].in_h = p23;
            out.insert(
                i + 1,
                PathNode {
                    anchor: mid,
                    in_h: p012,
                    out_h: p123,
                },
            );
        }
    }
    set_path_nodes(el, &out);
    Some(i + 1)
}

/// Entfernt einen Knoten. Unter drei Knoten (geschlossen) bzw. zwei (offen)
/// wäre nichts mehr übrig, was ein Pfad wäre — dann passiert nichts.
pub fn remove_node(el: &mut Element, index: usize) -> bool {
    let min = if el.path_closed { 3 } else { 2 };
    if el.points.len() <= min || index >= el.points.len() {
        return false;
    }
    let mut nodes = path_nodes(el);
    nodes.remove(index);
    set_path_nodes(el, &nodes);
    true
}

/// Legt die Griffe eines Knotens so, dass die Kurve glatt hindurchläuft
/// (Catmull-Rom: Richtung aus den beiden Nachbarn, Länge ein Sechstel).
///
/// Ohne Nachbarn auf beiden Seiten — also an den Enden eines offenen Pfads —
/// gibt es keine sinnvolle Richtung; dort bleibt der Knoten eine Ecke.
fn smooth_node(nodes: &[PathNode], i: usize, closed: bool) -> PathNode {
    let n = nodes.len();
    let prev = if i > 0 {
        Some(nodes[i - 1].anchor)
    } else if closed {
        Some(nodes[n - 1].anchor)
    } else {
        None
    };
    let next = if i + 1 < n {
        Some(nodes[i + 1].anchor)
    } else if closed {
        Some(nodes[0].anchor)
    } else {
        None
    };
    let p = nodes[i].anchor;
    match (prev, next) {
        (Some(a), Some(b)) => {
            let tangent = (b - a) / 6.0;
            PathNode {
                anchor: p,
                in_h: p - tangent,
                out_h: p + tangent,
            }
        }
        _ => PathNode::corner(p),
    }
}

/// Glättet alle Knoten eines Pfads.
pub fn smooth_path(el: &mut Element) {
    let nodes = path_nodes(el);
    let closed = el.path_closed;
    let out: Vec<PathNode> = (0..nodes.len())
        .map(|i| smooth_node(&nodes, i, closed))
        .collect();
    set_path_nodes(el, &out);
}

/// Macht aus allen Knoten wieder Ecken — der Pfad wird ein Streckenzug.
pub fn sharpen_path(el: &mut Element) {
    let out: Vec<PathNode> = path_nodes(el)
        .iter()
        .map(|n| PathNode::corner(n.anchor))
        .collect();
    set_path_nodes(el, &out);
}

/// Schaltet einen einzelnen Knoten zwischen Ecke und glattem Übergang um.
pub fn toggle_node_smooth(el: &mut Element, index: usize) {
    let mut nodes = path_nodes(el);
    if index >= nodes.len() {
        return;
    }
    nodes[index] = if nodes[index].is_corner() {
        smooth_node(&nodes, index, el.path_closed)
    } else {
        PathNode::corner(nodes[index].anchor)
    };
    set_path_nodes(el, &nodes);
}

/// Verschiebt einen Knoten samt seiner Griffe an eine neue Position.
pub fn move_node(el: &mut Element, index: usize, to: Pos2) {
    let mut nodes = path_nodes(el);
    if index >= nodes.len() {
        return;
    }
    let d = to - nodes[index].anchor;
    nodes[index] = nodes[index].translated(d);
    set_path_nodes(el, &nodes);
}

/// Zieht einen Griff an eine neue Position.
///
/// `mirror` hält den gegenüberliegenden Griff in Gegenrichtung — dann bleibt
/// der Knoten glatt. Ohne `mirror` (Alt-Taste) lassen sich die beiden Seiten
/// unabhängig legen, was für Spitzen und Knicke gebraucht wird.
pub fn move_handle(el: &mut Element, index: usize, outgoing: bool, to: Pos2, mirror: bool) {
    let mut nodes = path_nodes(el);
    if index >= nodes.len() {
        return;
    }
    let node = &mut nodes[index];
    let anchor = node.anchor;
    if outgoing {
        node.out_h = to;
    } else {
        node.in_h = to;
    }
    if mirror {
        // Gegenseite spiegeln, aber ihre bisherige Länge behalten: Sonst
        // zöge ein kurzer Griff den langen auf der anderen Seite mit ein.
        let dir = anchor - to;
        let len = dir.length();
        if len > CORNER_EPS {
            let other_len = if outgoing {
                (node.in_h - anchor).length()
            } else {
                (node.out_h - anchor).length()
            };
            let keep = if other_len > CORNER_EPS { other_len } else { len };
            let unit = dir / len;
            if outgoing {
                node.in_h = anchor + unit * keep;
            } else {
                node.out_h = anchor + unit * keep;
            }
        }
    }
    set_path_nodes(el, &nodes);
}

/// Dünnt einen aufgezeichneten Zug aus (Ramer-Douglas-Peucker).
///
/// Freihand-Zeichnen liefert einen Punkt pro Frame — bei 60 Hz also hunderte
/// für einen kurzen Strich, samt Zittern der Hand. Erst das Ausdünnen macht
/// daraus einen Pfad, der sich mit ein paar Knoten bearbeiten lässt.
pub fn simplify_polyline(pts: &[Pos2], eps: f32) -> Vec<Pos2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    rdp(pts, 0, pts.len() - 1, eps, &mut keep);
    pts.iter()
        .zip(keep.iter())
        .filter(|(_, k)| **k)
        .map(|(p, _)| *p)
        .collect()
}

fn rdp(pts: &[Pos2], first: usize, last: usize, eps: f32, keep: &mut [bool]) {
    if last <= first + 1 {
        return;
    }
    let (a, b) = (pts[first], pts[last]);
    let ab = b - a;
    let len2 = ab.dot(ab);
    let mut worst = (0usize, -1.0f32);
    for i in (first + 1)..last {
        let ap = pts[i] - a;
        let d = if len2 > 1e-9 {
            let t = (ap.dot(ab) / len2).clamp(0.0, 1.0);
            (pts[i] - (a + ab * t)).length()
        } else {
            ap.length()
        };
        if d > worst.1 {
            worst = (i, d);
        }
    }
    if worst.1 > eps {
        keep[worst.0] = true;
        rdp(pts, first, worst.0, eps, keep);
        rdp(pts, worst.0, last, eps, keep);
    }
}

/// Baut aus einem ausgedünnten Zug glatte Knoten (Catmull-Rom-Tangenten).
pub fn nodes_from_polyline(pts: &[Pos2], closed: bool) -> Vec<PathNode> {
    let corners: Vec<PathNode> = pts.iter().map(|p| PathNode::corner(*p)).collect();
    (0..corners.len())
        .map(|i| smooth_node(&corners, i, closed))
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
mod bounds_tests {
    use super::*;
    use crate::model::{Element, ElementKind};

    #[test]
    fn ungedrehte_box_ist_ihre_eigene_huellbox() {
        let mut el = Element::new_rectangle(1, 100.0, 200.0);
        el.w = 60.0;
        el.h = 40.0;
        let b = element_bounds(&el);
        assert!((b.min.x - 100.0).abs() < 0.01);
        assert!((b.min.y - 200.0).abs() < 0.01);
        assert!((b.max.x - 160.0).abs() < 0.01);
        assert!((b.max.y - 240.0).abs() < 0.01);
    }

    #[test]
    fn gedrehtes_rechteck_waechst_ueber_seine_box_hinaus() {
        let mut el = Element::new_rectangle(1, 100.0, 100.0);
        el.w = 100.0;
        el.h = 100.0;
        el.rotation = 45.0;
        let b = element_bounds(&el);
        // Die Diagonale eines 100er-Quadrats ist ~141,4.
        assert!(
            (b.width() - 141.42).abs() < 0.1,
            "erwartet ~141,4, war {}",
            b.width()
        );
        assert!(b.min.x < 100.0, "ragt links über x hinaus");
    }

    #[test]
    fn gedrehte_linie_hat_hoehe_obwohl_h_null_ist() {
        // Der Fall, der eine aus `x/y/w/h` gebaute Leinwand zerstört: Eine
        // Linie hat `h == 0`. Senkrecht gedreht ist ihre Hüllbox trotzdem
        // so hoch wie die Linie lang.
        let mut el = Element::new_line(1, 100.0, 300.0);
        el.w = 200.0;
        el.h = 0.0;
        el.rotation = 90.0;
        let b = element_bounds(&el);
        assert!(
            (b.height() - 200.0).abs() < 0.1,
            "erwartet 200, war {}",
            b.height()
        );
        assert!(b.width() < 0.1, "senkrecht → keine Breite");
    }

    #[test]
    fn huellbox_eines_pfads_folgt_der_kurve() {
        let nodes = vec![
            PathNode::corner(Pos2::new(0.0, 0.0)),
            PathNode {
                anchor: Pos2::new(200.0, 0.0),
                in_h: Pos2::new(150.0, -100.0),
                out_h: Pos2::new(200.0, 0.0),
            },
        ];
        let el = path_from_nodes(1, &nodes, false);
        let b = element_bounds(&el);
        assert!(b.min.y < -10.0, "die Ausbuchtung muss drin sein: {b:?}");
    }

    #[test]
    fn mehrere_elemente_ergeben_ihre_gemeinsame_huellbox() {
        let a = Element::new_rectangle(1, 0.0, 0.0);
        let mut b = Element::new_rectangle(2, 300.0, 400.0);
        b.w = 50.0;
        b.h = 50.0;
        let u = elements_bounds([&a, &b]).expect("zwei Elemente");
        assert!((u.min.x - 0.0).abs() < 0.01);
        assert!((u.max.x - 350.0).abs() < 0.01);
        assert!((u.max.y - 450.0).abs() < 0.01);
    }

    #[test]
    fn leere_liste_hat_keine_huellbox() {
        let leer: [&Element; 0] = [];
        assert!(elements_bounds(leer).is_none());
    }

    #[test]
    fn kind_line_wird_wirklich_ueber_die_endpunkte_gerechnet() {
        let mut el = Element::new_line(1, 0.0, 0.0);
        el.w = 100.0;
        assert_eq!(el.kind, ElementKind::Line);
        let b = element_bounds(&el);
        let (p, q) = line_endpoints(&el);
        assert!((b.min.x - p.x.min(q.x)).abs() < 0.01);
        assert!((b.max.x - p.x.max(q.x)).abs() < 0.01);
    }
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

    // -----------------------------------------------------------------
    // Kurven
    // -----------------------------------------------------------------

    fn node(ax: f32, ay: f32, ix: f32, iy: f32, ox: f32, oy: f32) -> PathNode {
        PathNode {
            anchor: Pos2::new(ax, ay),
            in_h: Pos2::new(ix, iy),
            out_h: Pos2::new(ox, oy),
        }
    }

    /// Eine Welle: Ecke, glatter Knoten, Ecke.
    fn welle() -> Vec<PathNode> {
        vec![
            PathNode::corner(Pos2::new(100.0, 200.0)),
            node(200.0, 150.0, 160.0, 150.0, 240.0, 150.0),
            PathNode::corner(Pos2::new(300.0, 200.0)),
        ]
    }

    fn close_p(a: Pos2, b: Pos2, eps: f32) -> bool {
        (a - b).length() < eps
    }

    #[test]
    fn knoten_ueberleben_das_normalisieren() {
        let el = path_from_nodes(1, &welle(), false);
        for (got, want) in path_nodes(&el).iter().zip(welle().iter()) {
            assert!(close_p(got.anchor, want.anchor, 0.01), "{got:?} != {want:?}");
            assert!(close_p(got.in_h, want.in_h, 0.01), "{got:?} != {want:?}");
            assert!(close_p(got.out_h, want.out_h, 0.01), "{got:?} != {want:?}");
        }
    }

    #[test]
    fn knoten_ueberleben_das_normalisieren_auch_gedreht() {
        // Der gefährliche Fall: `set_path_nodes` rechnet im unrotierten Rahmen
        // und muss den Mittelpunkt mitgedreht an seinen neuen Platz setzen.
        let mut el = path_from_nodes(1, &welle(), false);
        el.rotation = 37.0;
        let before = path_nodes(&el);
        set_path_nodes(&mut el, &before);
        assert!((el.rotation - 37.0).abs() < 0.001, "Drehung ging verloren");
        for (got, want) in path_nodes(&el).iter().zip(before.iter()) {
            assert!(close_p(got.anchor, want.anchor, 0.02), "{got:?} != {want:?}");
            assert!(close_p(got.in_h, want.in_h, 0.02), "{got:?} != {want:?}");
            assert!(close_p(got.out_h, want.out_h, 0.02), "{got:?} != {want:?}");
        }
    }

    #[test]
    fn box_umschliesst_die_kurve_und_liegt_eng_an() {
        // Genau daran hängt alles andere: Hit-Test, Auswahl-Rechteck und
        // Ausrichtung lesen nur x/y/w/h. Eine Kurve beult zwischen ihren
        // Stützpunkten aus — die Box muss diese Ausbuchtung enthalten.
        let el = path_from_nodes(1, &welle(), false);
        let out = path_outline(&el);
        for p in &out {
            assert!(
                p.x >= el.x - 0.05
                    && p.x <= el.x + el.w + 0.05
                    && p.y >= el.y - 0.05
                    && p.y <= el.y + el.h + 0.05,
                "Kurvenpunkt {p:?} liegt ausserhalb der Box ({}, {}, {}, {})",
                el.x,
                el.y,
                el.w,
                el.h
            );
        }
        // Eng: Jede Kante wird von der Kurve auch berührt.
        let min_y = out.iter().fold(f32::MAX, |m, p| m.min(p.y));
        let max_y = out.iter().fold(f32::MIN, |m, p| m.max(p.y));
        assert!((min_y - el.y).abs() < 0.1, "oben klafft eine Lücke");
        assert!((max_y - (el.y + el.h)).abs() < 0.1, "unten klafft eine Lücke");
    }

    #[test]
    fn box_enthaelt_auch_die_ausbuchtung_zwischen_zwei_stuetzpunkten() {
        // Der Fall, an dem eine Hüllbox über die bloßen Stützpunkte scheitert:
        // Beide Stützpunkte liegen auf y = 0, die Kurve wölbt sich aber weit
        // nach oben. Eine Box über die Stützpunkte wäre hier null hoch.
        let nodes = vec![
            node(0.0, 0.0, 0.0, 0.0, 0.0, -100.0),
            node(100.0, 0.0, 100.0, -100.0, 100.0, 0.0),
        ];
        let el = path_from_nodes(1, &nodes, false);
        assert!(el.h > 60.0, "Box ist nur {} hoch", el.h);
        for p in path_outline(&el) {
            assert!(
                p.y >= el.y - 0.05 && p.y <= el.y + el.h + 0.05,
                "Kurvenpunkt {p:?} liegt ausserhalb der Box"
            );
        }
        // Und die Griffe dürfen die Box nicht mit aufblähen: Sie reichen bis
        // y = -100, die Kurve nur bis -75.
        assert!(el.h < 80.0, "Box wurde von den Griffen aufgeblaeht: {}", el.h);
    }

    #[test]
    fn eingefuegter_pfad_trifft_groesse_und_mitte_genau() {
        // Dieselbe Welle, die „Einfügen → Pfad" auf die Seite legt
        // (`EditorApp::add_path`): zwei Ecken unten, ein glatter Knoten oben
        // mit waagrechten Griffen. Weil die Kurve dabei weder über die
        // Stützpunkte hinausschießt noch hinter ihnen zurückbleibt, muss die
        // Box exakt die gewünschte Größe haben — ein eingefügtes Objekt, das
        // 240 × 80 sein soll, darf nicht 247 × 84 werden.
        let (cx, cy, w, h) = (300.0_f32, 400.0_f32, 240.0_f32, 80.0_f32);
        let (l, r) = (cx - w / 2.0, cx + w / 2.0);
        let (top, bot) = (cy - h / 2.0, cy + h / 2.0);
        let nodes = vec![
            PathNode::corner(Pos2::new(l, bot)),
            node(cx, top, cx - w / 4.0, top, cx + w / 4.0, top),
            PathNode::corner(Pos2::new(r, bot)),
        ];
        let el = path_from_nodes(1, &nodes, false);

        assert!((el.w - w).abs() < 0.05, "Breite {} statt {w}", el.w);
        assert!((el.h - h).abs() < 0.05, "Höhe {} statt {h}", el.h);
        let center = element_center(&el);
        assert!(
            (center - Pos2::new(cx, cy)).length() < 0.05,
            "Mitte {center:?} statt ({cx},{cy})"
        );
        assert!(el.path_is_curved(), "der Knoten oben soll eine Kurve sein");
    }

    #[test]
    fn streckenzug_bleibt_ohne_griffe_gespeichert() {
        // Ein Pfad ohne Kurven darf das `handles`-Feld nicht anfassen —
        // sonst stünde in jeder Datei ein Vektor voller Wiederholungen.
        let el = path_from_nodes(
            1,
            &[
                PathNode::corner(Pos2::new(0.0, 0.0)),
                PathNode::corner(Pos2::new(50.0, 80.0)),
            ],
            false,
        );
        assert!(el.handles.is_empty());
        assert!(!el.path_is_curved());
        // ... und der Umriss ist exakt die Punktfolge, ohne Zwischenpunkte.
        assert_eq!(path_outline(&el).len(), 2);
    }

    #[test]
    fn gerade_abschnitte_bleiben_gerade_auch_im_kurvenpfad() {
        // Segment 0 (Ecke -> glatt) ist eine Kurve, denn der Zielknoten hat
        // einen Eingangsgriff. Ein Abschnitt zwischen zwei Ecken dagegen muss
        // eine Gerade bleiben — sonst wächst jedes PDF unnötig.
        let nodes = vec![
            PathNode::corner(Pos2::new(0.0, 0.0)),
            PathNode::corner(Pos2::new(100.0, 0.0)),
            node(200.0, 50.0, 150.0, 50.0, 250.0, 50.0),
        ];
        let el = path_from_nodes(1, &nodes, false);
        let (start, segs) = path_segments(&el).expect("Segmente");
        assert!(close_p(start, Pos2::new(0.0, 0.0), 0.01));
        assert_eq!(segs.len(), 2);
        assert!(matches!(segs[0], PathSeg::Line(_)), "war {:?}", segs[0]);
        assert!(matches!(segs[1], PathSeg::Cubic(..)), "war {:?}", segs[1]);
    }

    #[test]
    fn geschlossener_pfad_hat_ein_segment_mehr() {
        let mut el = path_from_nodes(1, &welle(), false);
        assert_eq!(path_segment_count(&el), 2);
        el.path_closed = true;
        assert_eq!(path_segment_count(&el), 3);
        let (_, segs) = path_segments(&el).unwrap();
        assert_eq!(segs.len(), 3);
    }

    #[test]
    fn knoten_einfuegen_veraendert_die_form_nicht() {
        // De Casteljau muss punktgleich unterteilen. Wenn die Linie beim
        // Einfügen springt, ist genau das kaputt.
        let mut el = path_from_nodes(1, &welle(), false);
        let before = path_outline(&el);
        let hit = path_nearest(&el, Pos2::new(150.0, 165.0)).expect("Treffer");
        let idx = insert_node(&mut el, &hit).expect("eingefuegt");
        assert_eq!(idx, 1);
        assert_eq!(el.points.len(), 4);

        // Beide Umrisse an denselben Bogenlängen vergleichen: Die Punktzahl
        // ändert sich durch die Unterteilung, die Form nicht.
        let after = path_outline(&el);
        for p in &after {
            let d = before
                .windows(2)
                .map(|w| {
                    let ab = w[1] - w[0];
                    let t = (( *p - w[0]).dot(ab) / ab.dot(ab).max(1e-9)).clamp(0.0, 1.0);
                    (*p - (w[0] + ab * t)).length()
                })
                .fold(f32::MAX, f32::min);
            assert!(d < 0.5, "Punkt {p:?} weicht um {d} ab");
        }
    }

    #[test]
    fn knoten_entfernen_hat_eine_untergrenze() {
        let mut el = path_from_nodes(1, &welle(), false);
        assert!(remove_node(&mut el, 1));
        assert_eq!(el.points.len(), 2);
        // Zwei Punkte sind das Minimum eines offenen Pfads.
        assert!(!remove_node(&mut el, 0));
        assert_eq!(el.points.len(), 2);
    }

    #[test]
    fn glaetten_und_schaerfen_sind_gegenlaeufig() {
        let mut el = path_from_nodes(
            1,
            &[
                PathNode::corner(Pos2::new(0.0, 0.0)),
                PathNode::corner(Pos2::new(50.0, 100.0)),
                PathNode::corner(Pos2::new(100.0, 0.0)),
            ],
            false,
        );
        smooth_path(&mut el);
        assert!(el.path_is_curved(), "Glaetten hat keine Griffe gesetzt");
        sharpen_path(&mut el);
        assert!(!el.path_is_curved());
        assert!(el.handles.is_empty(), "leere Griffe muessen verschwinden");
    }

    #[test]
    fn knoten_verschieben_zieht_seine_griffe_mit() {
        let mut el = path_from_nodes(1, &welle(), false);
        let before = path_nodes(&el);
        move_node(&mut el, 1, Pos2::new(200.0, 100.0));
        let after = path_nodes(&el);
        let d = after[1].anchor - before[1].anchor;
        assert!(close_p(after[1].anchor, Pos2::new(200.0, 100.0), 0.02));
        assert!(close_p(after[1].in_h, before[1].in_h + d, 0.02));
        assert!(close_p(after[1].out_h, before[1].out_h + d, 0.02));
        // Die Nachbarn bleiben stehen.
        assert!(close_p(after[0].anchor, before[0].anchor, 0.02));
    }

    #[test]
    fn griff_spiegeln_haelt_den_knoten_glatt() {
        let mut el = path_from_nodes(1, &welle(), false);
        move_handle(&mut el, 1, true, Pos2::new(240.0, 120.0), true);
        let n = path_nodes(&el)[1];
        assert!(n.is_smooth(), "Knoten hat einen Knick bekommen: {n:?}");
        // Ohne Spiegeln darf die Gegenseite stehen bleiben.
        let mut el2 = path_from_nodes(1, &welle(), false);
        let before = path_nodes(&el2)[1].in_h;
        move_handle(&mut el2, 1, true, Pos2::new(240.0, 120.0), false);
        assert!(close_p(path_nodes(&el2)[1].in_h, before, 0.02));
    }

    #[test]
    fn ausduennen_behaelt_die_form_und_die_enden() {
        // Ein Zug mit vielen Punkten auf einer Geraden plus einem Knick.
        let mut pts: Vec<Pos2> = (0..=50).map(|i| Pos2::new(i as f32 * 2.0, 0.0)).collect();
        pts.extend((1..=50).map(|i| Pos2::new(100.0, i as f32 * 2.0)));
        let simple = simplify_polyline(&pts, 0.5);
        assert_eq!(simple.first(), pts.first());
        assert_eq!(simple.last(), pts.last());
        assert!(simple.len() <= 4, "{} Punkte uebrig", simple.len());
    }

    #[test]
    fn naechster_punkt_liegt_auf_dem_pfad() {
        let el = path_from_nodes(1, &welle(), false);
        let hit = path_nearest(&el, Pos2::new(200.0, 400.0)).expect("Treffer");
        // Der gesuchte Punkt liegt weit unterhalb der nach oben gewölbten
        // Welle — am nächsten ist einer der beiden Endpunkte auf y = 200,
        // also 100 zur Seite und 200 nach unten.
        assert!(
            (hit.dist - 100.0f32.hypot(200.0)).abs() < 1.0,
            "Abstand {}",
            hit.dist
        );
        let outline = path_outline(&el);
        let on_path = outline
            .windows(2)
            .map(|w| {
                let ab = w[1] - w[0];
                let t = ((hit.pos - w[0]).dot(ab) / ab.dot(ab).max(1e-9)).clamp(0.0, 1.0);
                (hit.pos - (w[0] + ab * t)).length()
            })
            .fold(f32::MAX, f32::min);
        assert!(on_path < 0.5, "Punkt liegt {on_path} neben dem Pfad");
    }

    #[test]
    fn punkt_im_polygon() {
        let quad = [
            Pos2::new(0.0, 0.0),
            Pos2::new(100.0, 0.0),
            Pos2::new(100.0, 100.0),
            Pos2::new(0.0, 100.0),
        ];
        assert!(point_in_polygon(&quad, Pos2::new(50.0, 50.0)));
        assert!(!point_in_polygon(&quad, Pos2::new(150.0, 50.0)));
        // Konkav: Der Punkt in der Einbuchtung liegt draußen.
        let l = [
            Pos2::new(0.0, 0.0),
            Pos2::new(60.0, 0.0),
            Pos2::new(60.0, 20.0),
            Pos2::new(20.0, 20.0),
            Pos2::new(20.0, 80.0),
            Pos2::new(0.0, 80.0),
        ];
        assert!(point_in_polygon(&l, Pos2::new(10.0, 50.0)));
        assert!(!point_in_polygon(&l, Pos2::new(50.0, 50.0)));
    }

    #[test]
    fn kaputte_griffe_werden_repariert_statt_zu_verstummen() {
        let mut el = path_from_nodes(1, &welle(), false);
        assert!(el.path_handles_valid());
        el.handles.truncate(1);
        assert!(!el.path_handles_valid());
        el.repair_path_handles();
        assert!(el.path_handles_valid());
        assert_eq!(el.handles.len(), el.points.len());
        // Der aufgefüllte Rest ist eine Ecke, kein Zufallswert.
        let n = path_nodes(&el);
        assert!(n[2].is_corner());
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
