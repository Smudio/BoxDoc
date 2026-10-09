//! SVG-Import — macht aus einer SVG-Datei **bearbeitbare** BoxDoc-Objekte.
//!
//! Das Gegenstück zu [`crate::svg`]: Ein `<rect>` wird wieder ein Rechteck,
//! eine `<ellipse>` eine Ellipse, ein `<path>` ein Pfad mit echten
//! Bézier-Griffen, ein `<text>` ein Textblock. Nichts wird gerastert — sonst
//! wäre das Ergebnis ein Bild, an dem man nichts mehr ändern kann.
//!
//! **Was ankommt:** `rect`, `circle`, `ellipse`, `line`, `polyline`,
//! `polygon`, `path` (alle Befehle inklusive Bögen), `text`/`tspan`, `image`
//! (eingebettet als data-URI oder als Datei neben dem SVG), `use`, `g`,
//! verschachtelte `svg`. Transformationen (`matrix`, `translate`, `scale`,
//! `rotate`, `skewX/Y`), Stile als Attribut, als `style="…"` und aus einfachen
//! `<style>`-Regeln (`tag`, `.klasse`, `#id`), Deckkraft auf jeder Ebene.
//!
//! **Was angenähert wird**, weil BoxDoc es nicht kennt: Verläufe werden zur
//! Mischfarbe ihrer Stopps, Muster zu Grau. Ein Pfad mit mehreren Teilpfaden
//! wird in einzelne Pfade zerlegt — Löcher (der Innenraum eines „O") werden
//! dabei mitgefüllt. Schräg verzerrte Rechtecke und Ellipsen werden Pfade.
//! Masken, Filter und Schnittpfade (außer dem rechteckigen Bildausschnitt, den
//! BoxDocs eigener Export schreibt) fallen weg.
//!
//! **Keine UI-Abhängigkeiten** außer beim Feinsetzen der Texte
//! ([`fit_texts`]): Dafür braucht es die Schriftmetrik aus egui, und genau die
//! soll es sein — mit einer eigenen Schätzung stünde der Text um ein paar
//! Punkt neben der Stelle, an der er im SVG stand.

use std::collections::HashMap;

use egui::{Pos2, Vec2};

use crate::geometry::{self, PathNode};
use crate::model::{
    pt_to_mm, CustomFormat, Document, Element, ElementKind, Orientation, Page, PaperFormat,
    TextAlign, VAlign,
};

/// Ein Pixel (SVG-Nutzereinheit) in Punkten: CSS rechnet mit 96 px je Zoll.
const PX_TO_PT: f32 = 0.75;

/// Wie tief `<use>` sich selbst verweisen darf, bevor wir abbrechen. Schützt
/// vor Dateien, die sich im Kreis referenzieren.
const MAX_USE_DEPTH: usize = 16;

// ===========================================================================
// Ergebnis
// ===========================================================================

/// Ein Bild aus dem SVG. `id` ist die Element-ID — BoxDoc schlüsselt Bilder
/// über die ID des Elements, das sie zeigt.
#[derive(Debug, Clone)]
pub struct ImportedImage {
    pub id: u64,
    pub png: Vec<u8>,
    pub dim: (u32, u32),
}

/// Woran ein SVG-Text hängt: `text-anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// Wo ein Text im SVG **genau** stand: Ankerpunkt auf der Grundlinie der
/// ersten Zeile, in Seitenkoordinaten.
///
/// BoxDoc setzt Text über die linke obere Ecke der Box; wo dann die Grundlinie
/// landet, hängt von der Schrift ab. [`fit_texts`] misst das nach und
/// verschiebt die Box, bis die Grundlinie wieder dort liegt, wo das SVG sie
/// hatte.
#[derive(Debug, Clone)]
pub struct TextFix {
    pub id: u64,
    pub anchor_pt: Pos2,
    pub anchor: Anchor,
    /// Anfang jeder Zeile relativ zur ersten, in pt entlang der Textrichtung.
    /// Leer, wenn die Zeilen keine eigenen Positionen haben.
    ///
    /// Viele Programme — BoxDoc selbst eingeschlossen — schreiben
    /// zentrierten Text nicht als `text-anchor="middle"`, sondern geben jeder
    /// Zeile ihr eigenes, schon zentriertes x. Erst mit der Zeilenbreite aus
    /// der Schriftmetrik sieht man, ob die Zeilen links, mittig oder rechts
    /// bündig standen.
    pub line_dx: Vec<f32>,
    /// Breite und Ausrichtung stammen aus BoxDocs eigenen `data-boxdoc-*`
    /// Angaben und dürfen nicht neu vermessen werden — sonst gingen die
    /// weichen Umbrüche wieder verloren.
    pub keep_layout: bool,
}

#[derive(Debug, Clone)]
pub struct SvgImport {
    /// Leinwandgröße in pt.
    pub width: f32,
    pub height: f32,
    pub elements: Vec<Element>,
    pub images: Vec<ImportedImage>,
    pub text_fixes: Vec<TextFix>,
    /// Nächste freie ID nach dem Import.
    pub next_id: u64,
    /// Elemente, die es gab, die aber nicht übernommen werden konnten
    /// (Filter, Masken, unbekannte Tags) — für die Statuszeile.
    pub skipped: usize,
}

impl SvgImport {
    /// Macht aus dem Import ein eigenes Dokument mit einer Seite in genau der
    /// Größe der SVG-Leinwand.
    ///
    /// Füllt ein randloses Rechteck die ganze Leinwand — so schreibt BoxDoc
    /// selbst den Seitenhintergrund —, wird es wieder zum Seitenhintergrund
    /// statt zu einem Rechteck, das man beim Auswählen ständig erwischt.
    /// Ohne ein solches Rechteck bleibt die Seite **ohne** Hintergrund: Ein
    /// transparentes SVG soll transparent zurück exportiert werden.
    pub fn into_document(mut self, name: &str) -> (Document, Vec<ImportedImage>) {
        let mut background = None;
        if let Some(first) = self.elements.first() {
            if is_background_rect(first, self.width, self.height) {
                background = Some(first.fill_color);
                self.elements.remove(0);
            }
        }

        let (format, orientation, custom_formats) = paper_for(self.width, self.height, name);
        let doc = Document {
            format,
            orientation,
            custom_formats,
            background,
            pages: vec![Page {
                elements: self.elements,
            }],
        };
        (doc, self.images)
    }
}

fn is_background_rect(el: &Element, w: f32, h: f32) -> bool {
    const TOL: f32 = 0.5;
    el.kind == ElementKind::Rectangle
        && el.rotation.abs() < 0.01
        && el.corner_radius < 0.01
        && (el.stroke_width <= 0.0 || el.stroke_color[3] == 0)
        && el.fill_color[3] > 0
        && el.x.abs() < TOL
        && el.y.abs() < TOL
        && (el.w - w).abs() < TOL
        && (el.h - h).abs() < TOL
}

/// Papierformat zur Leinwand: ein Standardformat, wenn die Größe eines
/// trifft, sonst ein eigenes Format mit dem Dateinamen als Namen.
fn paper_for(w: f32, h: f32, name: &str) -> (PaperFormat, Orientation, Vec<CustomFormat>) {
    const TOL: f32 = 1.5;
    for fmt in PaperFormat::all() {
        let (pw, ph) = crate::model::page_size_pt(&fmt, Orientation::Portrait);
        if (w - pw).abs() < TOL && (h - ph).abs() < TOL {
            return (fmt, Orientation::Portrait, Vec::new());
        }
        if (w - ph).abs() < TOL && (h - pw).abs() < TOL {
            return (fmt, Orientation::Landscape, Vec::new());
        }
    }
    let name = if name.trim().is_empty() {
        String::from("SVG")
    } else {
        name.trim().to_string()
    };
    // Abmessungen auf 0,01 mm runden: Ein Format „123.456789 mm" sieht im
    // Format-Dialog nach einem Fehler aus.
    let round = |v: f32| (pt_to_mm(v) * 100.0).round() / 100.0;
    let custom = CustomFormat {
        name,
        w_mm: round(w.max(1.0)),
        h_mm: round(h.max(1.0)),
    };
    (
        PaperFormat::Custom(custom.clone()),
        Orientation::Portrait,
        vec![custom],
    )
}

/// Liest ein SVG.
///
/// `first_id` ist die erste zu vergebende Element-ID. `load_file` liefert die
/// Bytes eines Bildes, das per Dateiname (statt data-URI) eingebunden ist —
/// auf dem Desktop relativ zum SVG, im Browser gibt es das nicht.
pub fn parse(
    svg: &str,
    first_id: u64,
    load_file: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<SvgImport, String> {
    let dom = Dom::parse(svg)?;
    let root = dom
        .find_root()
        .ok_or_else(|| String::from("Die Datei enthält kein <svg>-Element."))?;

    let css = CssRules::collect(&dom);
    let root_node = &dom.nodes[root];

    // Leinwand: width/height in pt, viewBox legt die Nutzereinheiten darüber.
    let vb = root_node.attr("viewBox").and_then(parse_view_box);
    let base_font = 16.0;
    let w_attr = root_node
        .attr("width")
        .and_then(|s| parse_length(s, base_font, vb.map(|v| v[2]).unwrap_or(300.0)));
    let h_attr = root_node
        .attr("height")
        .and_then(|s| parse_length(s, base_font, vb.map(|v| v[3]).unwrap_or(150.0)));
    let (w_px, h_px) = match (w_attr, h_attr, vb) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(v)) if v[2] > 0.0 => (w, w * v[3] / v[2]),
        (None, Some(h), Some(v)) if v[3] > 0.0 => (h * v[2] / v[3], h),
        (_, _, Some(v)) => (v[2], v[3]),
        // Weder Größe noch viewBox: erst einmal provisorisch, unten wird die
        // Leinwand an den Inhalt angepasst.
        _ => (0.0, 0.0),
    };

    let mut root_m = Affine::scale(PX_TO_PT, PX_TO_PT);
    if let Some(v) = vb {
        if v[2] > 0.0 && v[3] > 0.0 && w_px > 0.0 && h_px > 0.0 {
            let par = root_node.attr("preserveAspectRatio").unwrap_or("");
            root_m = root_m.then_inner(&view_box_transform(v, w_px, h_px, par));
        }
    }

    let mut ctx = Walker {
        dom: &dom,
        css: &css,
        next_id: first_id,
        elements: Vec::new(),
        images: Vec::new(),
        text_fixes: Vec::new(),
        skipped: 0,
        load_file,
        viewport: (vb.map(|v| v[2]).unwrap_or(w_px), vb.map(|v| v[3]).unwrap_or(h_px)),
    };
    let style = ctx.style_for(root, &Style::default());
    if style.display {
        for child in dom.nodes[root].element_children() {
            ctx.walk(child, &root_m, &style, 0);
        }
    }

    let (mut width, mut height) = (w_px * PX_TO_PT, h_px * PX_TO_PT);
    if width <= 0.0 || height <= 0.0 {
        // Ohne Angaben: Leinwand bis zur rechten unteren Ecke des Inhalts.
        let b = geometry::elements_bounds(ctx.elements.iter());
        width = b.map(|b| b.max.x.max(1.0)).unwrap_or(595.0);
        height = b.map(|b| b.max.y.max(1.0)).unwrap_or(842.0);
    }

    Ok(SvgImport {
        width,
        height,
        elements: ctx.elements,
        images: ctx.images,
        text_fixes: ctx.text_fixes,
        next_id: ctx.next_id,
        skipped: ctx.skipped,
    })
}

/// Setzt importierte Texte mit BoxDocs Schriftmetrik an ihren Platz.
///
/// 1. **Breite**: so breit wie die längste Zeile (plus etwas Luft), damit
///    nichts umbricht, was im SVG auf einer Zeile stand.
/// 2. **Lage**: die Box so verschieben, dass die erste Grundlinie und der
///    `text-anchor`-Punkt wieder genau dort liegen, wo das SVG sie hatte —
///    auch bei gedrehtem Text.
pub fn fit_texts(ctx: &egui::Context, elements: &mut [Element], fixes: &[TextFix]) {
    if fixes.is_empty() {
        return;
    }
    ctx.fonts_mut(|fonts| {
        for fix in fixes {
            let Some(el) = elements.iter_mut().find(|e| e.id == fix.id) else {
                continue;
            };
            if el.kind != ElementKind::Text {
                continue;
            }
            if !fix.keep_layout {
                let natural = crate::text_layout::natural_width(fonts, el);
                el.w = (el.indent + natural + (natural * 0.02).clamp(0.5, 6.0)).max(1.0);
                if let Some(align) = detect_align(fonts, el, &fix.line_dx) {
                    el.align = align;
                }
            }
            let layout = crate::text_layout::layout(fonts, el, 1.0);
            if el.auto_height {
                el.h = layout.height.max(1.0);
            }
            let (line_x, line_w, baseline) = layout
                .lines
                .first()
                .map(|l| (l.x, l.width, l.baseline_y))
                .unwrap_or((0.0, 0.0, el.font_size * 0.8));
            let ax = match fix.anchor {
                Anchor::Start => line_x,
                Anchor::Middle => line_x + line_w / 2.0,
                Anchor::End => line_x + line_w,
            };
            // Im ungedrehten Rahmen liegt die linke obere Ecke bei
            // (-ax, -baseline) vom Ankerpunkt aus. Gedreht wird um die
            // Boxmitte, also die Mitte relativ zum Anker mitdrehen.
            let center_rel = Vec2::new(el.w / 2.0 - ax, el.h / 2.0 - baseline);
            let c = fix.anchor_pt + geometry::rotate_vec(center_rel, el.rotation);
            el.x = c.x - el.w / 2.0;
            el.y = c.y - el.h / 2.0;
        }
    });
}

/// Ausrichtung aus den Zeilenanfängen: Stehen die linken Kanten, die Mitten
/// oder die rechten Kanten übereinander? `None`, wenn es nichts zu erkennen
/// gibt oder die Zeilen zu keinem Muster passen.
fn detect_align(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    el: &Element,
    line_dx: &[f32],
) -> Option<TextAlign> {
    if line_dx.len() < 2 {
        return None;
    }
    let mut probe = el.clone();
    probe.align = TextAlign::Left;
    let layout = crate::text_layout::layout(fonts, &probe, 1.0);
    if layout.lines.len() != line_dx.len() {
        return None; // umgebrochen — die Zeilen passen nicht mehr zusammen
    }
    let spread = |v: Vec<f32>| {
        let lo = v.iter().copied().fold(f32::MAX, f32::min);
        let hi = v.iter().copied().fold(f32::MIN, f32::max);
        hi - lo
    };
    let widths: Vec<f32> = layout.lines.iter().map(|l| l.width).collect();
    let tol = (el.font_size * 0.15).max(0.5);
    let left = spread(line_dx.to_vec());
    let center = spread(line_dx.iter().zip(&widths).map(|(x, w)| x + w / 2.0).collect());
    let right = spread(line_dx.iter().zip(&widths).map(|(x, w)| x + w).collect());
    if left <= tol {
        Some(TextAlign::Left)
    } else if center <= tol {
        Some(TextAlign::Center)
    } else if right <= tol {
        Some(TextAlign::Right)
    } else {
        None
    }
}

// ===========================================================================
// Mini-DOM
// ===========================================================================

/// Der Baum muss vollständig im Speicher liegen, weil SVG vorwärts
/// verweist: Ein `<use>` oder ein `fill="url(#verlauf)"` darf auf ein Element
/// zeigen, das erst weiter unten in der Datei steht.
struct Dom {
    nodes: Vec<Node>,
    ids: HashMap<String, usize>,
}

struct Node {
    /// Elementname ohne Namensraum-Präfix bei SVG-Elementen; fremde
    /// Namensräume (`sodipodi:namedview`) behalten ihr Präfix und werden
    /// dadurch übersprungen.
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Child>,
}

enum Child {
    El(usize),
    Text(String),
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn element_children(&self) -> Vec<usize> {
        self.children
            .iter()
            .filter_map(|c| match c {
                Child::El(i) => Some(*i),
                Child::Text(_) => None,
            })
            .collect()
    }

    /// `href` oder das ältere `xlink:href`.
    fn href(&self) -> Option<&str> {
        self.attr("href").or_else(|| self.attr("xlink:href"))
    }
}

impl Dom {
    fn parse(svg: &str) -> Result<Dom, String> {
        use quick_xml::events::Event;

        let mut reader = quick_xml::Reader::from_str(svg);
        reader.config_mut().check_end_names = false;

        let mut nodes: Vec<Node> = Vec::new();
        let mut ids = HashMap::new();
        let mut stack: Vec<usize> = Vec::new();

        loop {
            let ev = reader
                .read_event()
                .map_err(|e| format!("Kein gültiges SVG (Byte {}): {e}", reader.buffer_position()))?;
            // `Empty` (`<rect/>`) hat kein End-Event und kommt deshalb nicht
            // auf den Stapel.
            let (e, opens) = match ev {
                Event::Start(e) => (Some(e), true),
                Event::Empty(e) => (Some(e), false),
                other => {
                    match other {
                        Event::End(_) => {
                            stack.pop();
                        }
                        Event::Text(t) => {
                            if let Some(&top) = stack.last() {
                                let s = match t.unescape() {
                                    Ok(s) => s.to_string(),
                                    // Unbekannte Entities (`&nbsp;` aus
                                    // HTML-Gewohnheit) sind kein Grund, die
                                    // ganze Datei abzulehnen.
                                    Err(_) => String::from_utf8_lossy(&t.into_inner())
                                        .replace("&nbsp;", "\u{a0}"),
                                };
                                nodes[top].children.push(Child::Text(s));
                            }
                        }
                        Event::CData(t) => {
                            if let Some(&top) = stack.last() {
                                let s = String::from_utf8_lossy(&t.into_inner()).to_string();
                                nodes[top].children.push(Child::Text(s));
                            }
                        }
                        Event::Eof => break,
                        _ => {}
                    }
                    (None, false)
                }
            };
            if let Some(e) = e {
                {
                    let name = qualified_name(e.name().as_ref());
                    let mut attrs = Vec::new();
                    for a in e.attributes().with_checks(false).flatten() {
                        let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                        if key == "xmlns" || key.starts_with("xmlns:") {
                            continue;
                        }
                        let val = match a.unescape_value() {
                            Ok(v) => v.to_string(),
                            Err(_) => String::from_utf8_lossy(&a.value).to_string(),
                        };
                        attrs.push((key, val));
                    }
                    let idx = nodes.len();
                    if let Some((_, id)) = attrs.iter().find(|(k, _)| k == "id") {
                        ids.entry(id.clone()).or_insert(idx);
                    }
                    nodes.push(Node {
                        name,
                        attrs,
                        children: Vec::new(),
                    });
                    if let Some(&parent) = stack.last() {
                        nodes[parent].children.push(Child::El(idx));
                    }
                    if opens {
                        stack.push(idx);
                    }
                }
            }
        }
        Ok(Dom { nodes, ids })
    }

    fn find_root(&self) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == "svg")
    }

    fn by_id(&self, id: &str) -> Option<usize> {
        self.ids.get(id).copied()
    }

    /// Der reine Textinhalt eines Elements (für `<style>`).
    fn text_of(&self, idx: usize) -> String {
        let mut s = String::new();
        for c in &self.nodes[idx].children {
            match c {
                Child::Text(t) => s.push_str(t),
                Child::El(i) => s.push_str(&self.text_of(*i)),
            }
        }
        s
    }
}

/// `svg:rect` → `rect`, `xlink:href` bleibt; fremde Präfixe bleiben stehen.
fn qualified_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).to_string();
    match s.split_once(':') {
        Some(("svg", local)) => local.to_string(),
        _ => s,
    }
}

// ===========================================================================
// CSS (nur das, was SVG-Programme tatsächlich schreiben)
// ===========================================================================

struct CssRule {
    /// (Tag, Klassen, ID) — alle Teile müssen passen.
    tag: Option<String>,
    classes: Vec<String>,
    id: Option<String>,
    specificity: u32,
    order: usize,
    decls: Vec<(String, String)>,
}

struct CssRules {
    rules: Vec<CssRule>,
}

impl CssRules {
    fn collect(dom: &Dom) -> CssRules {
        let mut rules = Vec::new();
        for (i, n) in dom.nodes.iter().enumerate() {
            if n.name == "style" {
                parse_css(&dom.text_of(i), &mut rules);
            }
        }
        rules.sort_by_key(|r| (r.specificity, r.order));
        CssRules { rules }
    }

    /// Deklarationen, die auf diesen Knoten passen — schwächste zuerst, damit
    /// spätere beim Einsammeln gewinnen.
    fn matching<'a>(&'a self, node: &Node) -> Vec<&'a (String, String)> {
        if self.rules.is_empty() {
            return Vec::new();
        }
        let classes: Vec<&str> = node
            .attr("class")
            .map(|c| c.split_whitespace().collect())
            .unwrap_or_default();
        let id = node.attr("id");
        let mut out = Vec::new();
        for r in &self.rules {
            if let Some(t) = &r.tag {
                if t != &node.name {
                    continue;
                }
            }
            if let Some(rid) = &r.id {
                if id != Some(rid.as_str()) {
                    continue;
                }
            }
            if !r.classes.iter().all(|c| classes.contains(&c.as_str())) {
                continue;
            }
            out.extend(r.decls.iter());
        }
        out
    }
}

fn parse_css(src: &str, rules: &mut Vec<CssRule>) {
    // Kommentare raus.
    let mut s = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("/*") {
        s.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    s.push_str(rest);

    for block in s.split('}') {
        let Some((sel, body)) = block.split_once('{') else {
            continue;
        };
        let sel = sel.trim();
        // @-Regeln (@font-face, @media) überspringen. Bei @media steht der
        // innere Selektor nach einem zweiten `{` — den erwischt split_once
        // nicht, dafür fällt hier der ganze Block weg. Gewollt: Druck- oder
        // Dunkelmodus-Regeln sollen nicht still gewinnen.
        if sel.starts_with('@') || sel.contains('{') {
            continue;
        }
        let decls = parse_declarations(body);
        if decls.is_empty() {
            continue;
        }
        for one in sel.split(',') {
            let one = one.trim();
            if one.is_empty()
                || one.contains(|c: char| c.is_whitespace() || ">+~:[".contains(c))
            {
                continue; // Kombinatoren, Pseudoklassen, Attribute: nicht unterstützt
            }
            let mut tag = None;
            let mut classes = Vec::new();
            let mut id = None;
            let mut cur = String::new();
            let mut kind = 't';
            let flush = |kind: char,
                         cur: &mut String,
                         tag: &mut Option<String>,
                         classes: &mut Vec<String>,
                         id: &mut Option<String>| {
                if cur.is_empty() {
                    return;
                }
                match kind {
                    '.' => classes.push(std::mem::take(cur)),
                    '#' => *id = Some(std::mem::take(cur)),
                    _ => {
                        let t = std::mem::take(cur);
                        if t != "*" {
                            *tag = Some(t);
                        }
                    }
                }
            };
            for ch in one.chars() {
                if ch == '.' || ch == '#' {
                    flush(kind, &mut cur, &mut tag, &mut classes, &mut id);
                    kind = ch;
                } else {
                    cur.push(ch);
                }
            }
            flush(kind, &mut cur, &mut tag, &mut classes, &mut id);
            let specificity = id.is_some() as u32 * 100
                + classes.len() as u32 * 10
                + tag.is_some() as u32;
            let order = rules.len();
            rules.push(CssRule {
                tag,
                classes,
                id,
                specificity,
                order,
                decls: decls.clone(),
            });
        }
    }
}

fn parse_declarations(body: &str) -> Vec<(String, String)> {
    body.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim().trim_end_matches("!important").trim().to_string();
            if k.is_empty() || v.is_empty() {
                None
            } else {
                Some((k, v))
            }
        })
        .collect()
}

// ===========================================================================
// Stil
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
enum Paint {
    None,
    Color([u8; 3], f32),
}

/// Vererbter Zustand beim Abstieg durch den Baum.
#[derive(Debug, Clone)]
struct Style {
    fill: Paint,
    stroke: Paint,
    stroke_width: f32,
    fill_opacity: f32,
    stroke_opacity: f32,
    /// Produkt aller `opacity`-Werte der Vorfahren. Eigentlich gilt Deckkraft
    /// für die Gruppe als Ganzes; BoxDoc hat keine Gruppen, also geht sie in
    /// die Farben jedes einzelnen Objekts.
    opacity: f32,
    color: [u8; 3],
    font_family: String,
    font_size: f32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    anchor: Anchor,
    visible: bool,
    /// `display: none` — wird nicht vererbt, sondern beendet den Abstieg.
    display: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            fill: Paint::Color([0, 0, 0], 1.0),
            stroke: Paint::None,
            stroke_width: 1.0,
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            opacity: 1.0,
            color: [0, 0, 0],
            font_family: String::new(),
            font_size: 16.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            anchor: Anchor::Start,
            visible: true,
            display: true,
        }
    }
}

/// Eigenschaften, die auch als Attribut stehen dürfen.
const PRESENTATION: &[&str] = &[
    "fill",
    "stroke",
    "stroke-width",
    "fill-opacity",
    "stroke-opacity",
    "opacity",
    "color",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "text-decoration",
    "text-anchor",
    "visibility",
    "display",
];

// ===========================================================================
// Affine Abbildung
// ===========================================================================

/// `x' = a·x + c·y + e`, `y' = b·x + d·y + f` — dieselbe Reihenfolge wie
/// SVGs `matrix(a b c d e f)`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Affine {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Affine {
    const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn translate(x: f32, y: f32) -> Affine {
        Affine {
            e: x,
            f: y,
            ..Affine::IDENTITY
        }
    }

    fn scale(x: f32, y: f32) -> Affine {
        Affine {
            a: x,
            d: y,
            ..Affine::IDENTITY
        }
    }

    fn rotate(deg: f32) -> Affine {
        let (s, c) = deg.to_radians().sin_cos();
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    /// `self ∘ inner`: erst `inner`, dann `self` — so wie SVG eine Liste von
    /// Transformationen von rechts nach links anwendet.
    fn then_inner(&self, m: &Affine) -> Affine {
        Affine {
            a: self.a * m.a + self.c * m.b,
            b: self.b * m.a + self.d * m.b,
            c: self.a * m.c + self.c * m.d,
            d: self.b * m.c + self.d * m.d,
            e: self.a * m.e + self.c * m.f + self.e,
            f: self.b * m.e + self.d * m.f + self.f,
        }
    }

    fn apply(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    fn det(&self) -> f32 {
        self.a * self.d - self.b * self.c
    }

    /// Mittlerer Maßstab — für Linienstärken.
    fn mean_scale(&self) -> f32 {
        self.det().abs().sqrt()
    }

    /// Zerlegt in Drehung und Skalierung: `R(θ)·diag(sx, sy)`.
    ///
    /// `None`, wenn die Abbildung schert — dann gibt es keine Drehung, die
    /// ein Rechteck wieder zu einem Rechteck macht. Spiegelungen kommen als
    /// negatives `sy` heraus.
    fn decompose(&self) -> Option<(f32, f32, f32)> {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        if sx < 1e-9 {
            return None;
        }
        let sy = self.det() / sx;
        if sy.abs() < 1e-9 {
            return None;
        }
        let col2 = (self.c * self.c + self.d * self.d).sqrt();
        let dot = self.a * self.c + self.b * self.d;
        if dot.abs() > 1e-3 * sx * col2 {
            return None; // Scherung
        }
        let rot = self.b.atan2(self.a).to_degrees();
        Some((rot, sx, sy))
    }
}

fn parse_transform(s: &str) -> Affine {
    let mut m = Affine::IDENTITY;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open]
            .trim()
            .trim_start_matches(',')
            .trim()
            .to_ascii_lowercase();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args = numbers(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
        let arg = |i: usize| args.get(i).copied().unwrap_or(0.0);
        let t = match name.as_str() {
            "matrix" if args.len() >= 6 => Affine {
                a: arg(0),
                b: arg(1),
                c: arg(2),
                d: arg(3),
                e: arg(4),
                f: arg(5),
            },
            "translate" => Affine::translate(arg(0), arg(1)),
            "scale" => {
                let sx = args.first().copied().unwrap_or(1.0);
                let sy = args.get(1).copied().unwrap_or(sx);
                Affine::scale(sx, sy)
            }
            "rotate" => {
                let r = Affine::rotate(arg(0));
                if args.len() >= 3 {
                    Affine::translate(arg(1), arg(2))
                        .then_inner(&r)
                        .then_inner(&Affine::translate(-arg(1), -arg(2)))
                } else {
                    r
                }
            }
            "skewx" => Affine {
                c: arg(0).to_radians().tan(),
                ..Affine::IDENTITY
            },
            "skewy" => Affine {
                b: arg(0).to_radians().tan(),
                ..Affine::IDENTITY
            },
            _ => Affine::IDENTITY,
        };
        m = m.then_inner(&t);
    }
    m
}

/// viewBox → Nutzereinheiten des Elternelements, nach `preserveAspectRatio`.
fn view_box_transform(vb: [f32; 4], w: f32, h: f32, par: &str) -> Affine {
    let sx = w / vb[2];
    let sy = h / vb[3];
    let par = par.trim();
    if par.starts_with("none") {
        return Affine::scale(sx, sy).then_inner(&Affine::translate(-vb[0], -vb[1]));
    }
    let slice = par.contains("slice");
    let s = if slice { sx.max(sy) } else { sx.min(sy) };
    let (ax, ay) = align_factors(par);
    let tx = (w - vb[2] * s) * ax;
    let ty = (h - vb[3] * s) * ay;
    Affine::translate(tx, ty)
        .then_inner(&Affine::scale(s, s))
        .then_inner(&Affine::translate(-vb[0], -vb[1]))
}

/// `xMinYMid` usw. → Anteile 0 / 0,5 / 1. Ohne Angabe: mittig.
fn align_factors(par: &str) -> (f32, f32) {
    let ax = if par.contains("xMin") {
        0.0
    } else if par.contains("xMax") {
        1.0
    } else {
        0.5
    };
    let ay = if par.contains("YMin") {
        0.0
    } else if par.contains("YMax") {
        1.0
    } else {
        0.5
    };
    (ax, ay)
}

// ===========================================================================
// Zahlen, Längen, Farben
// ===========================================================================

/// Alle Zahlen in einer Liste — auch in der kompakten Schreibweise, die
/// Optimierer erzeugen: `1.5.5-2` sind drei Zahlen (1.5, .5, -2).
fn numbers(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut lx = NumLexer::new(s);
    while let Some(v) = lx.number() {
        out.push(v);
    }
    out
}

struct NumLexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> NumLexer<'a> {
    fn new(s: &'a str) -> Self {
        NumLexer { s: s.as_bytes(), i: 0 }
    }

    fn skip_sep(&mut self) {
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',')
        {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_sep();
        self.s.get(self.i).copied()
    }

    fn at_number(&mut self) -> bool {
        matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.')
    }

    fn number(&mut self) -> Option<f32> {
        self.skip_sep();
        let start = self.i;
        let s = self.s;
        let mut i = self.i;
        if i < s.len() && (s[i] == b'-' || s[i] == b'+') {
            i += 1;
        }
        let mut digits = false;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
            digits = true;
        }
        if i < s.len() && s[i] == b'.' {
            i += 1;
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
                digits = true;
            }
        }
        if !digits {
            // Kein Zahlzeichen: nicht weiterkommen, statt endlos zu kreiseln.
            if i == start && start < s.len() && !s[start].is_ascii_alphabetic() {
                self.i = start + 1;
                return self.number();
            }
            return None;
        }
        if i < s.len() && (s[i] == b'e' || s[i] == b'E') {
            let mut j = i + 1;
            if j < s.len() && (s[j] == b'-' || s[j] == b'+') {
                j += 1;
            }
            if j < s.len() && s[j].is_ascii_digit() {
                while j < s.len() && s[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
            }
        }
        self.i = i;
        std::str::from_utf8(&s[start..i]).ok()?.parse().ok()
    }

    /// Ein Bogen-Flag: genau eine `0` oder `1`, auch ohne Trenner
    /// (`a1 1 0 01 5 5` ist gültig).
    fn flag(&mut self) -> Option<bool> {
        match self.peek()? {
            b'0' => {
                self.i += 1;
                Some(false)
            }
            b'1' => {
                self.i += 1;
                Some(true)
            }
            _ => None,
        }
    }
}

fn parse_view_box(s: &str) -> Option<[f32; 4]> {
    let v = numbers(s);
    (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| [v[0], v[1], v[2], v[3]])
}

/// Länge in Nutzereinheiten (px). `percent_base` ist die Bezugsgröße für `%`.
fn parse_length(s: &str, font_size: f32, percent_base: f32) -> Option<f32> {
    let s = s.trim();
    let split = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(s.len());
    let v: f32 = s[..split].trim().parse().ok()?;
    let unit = s[split..].trim().to_ascii_lowercase();
    let f = match unit.as_str() {
        "" | "px" => 1.0,
        "pt" => 4.0 / 3.0,
        "pc" => 16.0,
        "mm" => 96.0 / 25.4,
        "cm" => 96.0 / 2.54,
        "in" => 96.0,
        "em" => font_size,
        "ex" => font_size / 2.0,
        "%" => percent_base / 100.0,
        _ => 1.0,
    };
    Some(v * f)
}

fn parse_opacity(s: &str) -> Option<f32> {
    let s = s.trim();
    let v = if let Some(p) = s.strip_suffix('%') {
        p.trim().parse::<f32>().ok()? / 100.0
    } else {
        s.parse::<f32>().ok()?
    };
    Some(v.clamp(0.0, 1.0))
}

/// Farbe als RGB plus eigenem Alpha (aus `rgba()`/`#rrggbbaa`).
fn parse_color(s: &str, current: [u8; 3]) -> Option<([u8; 3], f32)> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if lower == "currentcolor" {
        return Some((current, 1.0));
    }
    if let Some(hex) = lower.strip_prefix('#') {
        let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        let b = hex.as_bytes();
        let all: Option<Vec<u8>> = b.iter().map(|c| digit(*c)).collect();
        let d = all?;
        return match d.len() {
            3 => Some(([d[0] * 17, d[1] * 17, d[2] * 17], 1.0)),
            4 => Some(([d[0] * 17, d[1] * 17, d[2] * 17], d[3] as f32 * 17.0 / 255.0)),
            6 => Some(([d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]], 1.0)),
            8 => Some((
                [d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5]],
                (d[6] * 16 + d[7]) as f32 / 255.0,
            )),
            _ => None,
        };
    }
    if let Some(open) = lower.find('(') {
        let func = lower[..open].trim();
        let inner = lower[open + 1..].trim_end_matches(')');
        // Prozentwerte erkennen, bevor `numbers` das %-Zeichen verschluckt.
        let parts: Vec<&str> = inner
            .split(|c: char| c == ',' || c == '/' || c.is_whitespace())
            .filter(|p| !p.is_empty())
            .collect();
        let val = |i: usize, scale: f32| -> f32 {
            parts
                .get(i)
                .and_then(|p| {
                    if let Some(pc) = p.strip_suffix('%') {
                        pc.parse::<f32>().ok().map(|v| v / 100.0 * scale)
                    } else {
                        p.parse::<f32>().ok()
                    }
                })
                .unwrap_or(0.0)
        };
        let alpha = if parts.len() >= 4 {
            let p = parts[3];
            if let Some(pc) = p.strip_suffix('%') {
                pc.parse::<f32>().unwrap_or(100.0) / 100.0
            } else {
                p.parse::<f32>().unwrap_or(1.0)
            }
        } else {
            1.0
        };
        let to8 = |v: f32| v.round().clamp(0.0, 255.0) as u8;
        return match func {
            "rgb" | "rgba" => Some((
                [to8(val(0, 255.0)), to8(val(1, 255.0)), to8(val(2, 255.0))],
                alpha.clamp(0.0, 1.0),
            )),
            "hsl" | "hsla" => {
                let h = val(0, 360.0).rem_euclid(360.0) / 360.0;
                let sat = val(1, 1.0).clamp(0.0, 1.0);
                let l = val(2, 1.0).clamp(0.0, 1.0);
                let (r, g, b) = hsl_to_rgb(h, sat, l);
                Some(([to8(r * 255.0), to8(g * 255.0), to8(b * 255.0)], alpha.clamp(0.0, 1.0)))
            }
            _ => None,
        };
    }
    named_color(&lower).map(|c| (c, 1.0))
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0))
}

fn named_color(name: &str) -> Option<[u8; 3]> {
    const NAMES: &[(&str, u32)] = &[
        ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff),
        ("aquamarine", 0x7fffd4), ("azure", 0xf0ffff), ("beige", 0xf5f5dc),
        ("bisque", 0xffe4c4), ("black", 0x000000), ("blanchedalmond", 0xffebcd),
        ("blue", 0x0000ff), ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a),
        ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00),
        ("chocolate", 0xd2691e), ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed),
        ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c), ("cyan", 0x00ffff),
        ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b),
        ("darkgray", 0xa9a9a9), ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9),
        ("darkkhaki", 0xbdb76b), ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f),
        ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc), ("darkred", 0x8b0000),
        ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b),
        ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1),
        ("darkviolet", 0x9400d3), ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff),
        ("dimgray", 0x696969), ("dimgrey", 0x696969), ("dodgerblue", 0x1e90ff),
        ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22),
        ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc), ("ghostwhite", 0xf8f8ff),
        ("gold", 0xffd700), ("goldenrod", 0xdaa520), ("gray", 0x808080),
        ("grey", 0x808080), ("green", 0x008000), ("greenyellow", 0xadff2f),
        ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
        ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c),
        ("lavender", 0xe6e6fa), ("lavenderblush", 0xfff0f5), ("lawngreen", 0x7cfc00),
        ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6), ("lightcoral", 0xf08080),
        ("lightcyan", 0xe0ffff), ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3),
        ("lightgreen", 0x90ee90), ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1),
        ("lightsalmon", 0xffa07a), ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa),
        ("lightslategray", 0x778899), ("lightslategrey", 0x778899), ("lightsteelblue", 0xb0c4de),
        ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32),
        ("linen", 0xfaf0e6), ("magenta", 0xff00ff), ("maroon", 0x800000),
        ("mediumaquamarine", 0x66cdaa), ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3),
        ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371), ("mediumslateblue", 0x7b68ee),
        ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc), ("mediumvioletred", 0xc71585),
        ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1),
        ("moccasin", 0xffe4b5), ("navajowhite", 0xffdead), ("navy", 0x000080),
        ("oldlace", 0xfdf5e6), ("olive", 0x808000), ("olivedrab", 0x6b8e23),
        ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6),
        ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98), ("paleturquoise", 0xafeeee),
        ("palevioletred", 0xdb7093), ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9),
        ("peru", 0xcd853f), ("pink", 0xffc0cb), ("plum", 0xdda0dd),
        ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
        ("red", 0xff0000), ("rosybrown", 0xbc8f8f), ("royalblue", 0x4169e1),
        ("saddlebrown", 0x8b4513), ("salmon", 0xfa8072), ("sandybrown", 0xf4a460),
        ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee), ("sienna", 0xa0522d),
        ("silver", 0xc0c0c0), ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd),
        ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa),
        ("springgreen", 0x00ff7f), ("steelblue", 0x4682b4), ("tan", 0xd2b48c),
        ("teal", 0x008080), ("thistle", 0xd8bfd8), ("tomato", 0xff6347),
        ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("wheat", 0xf5deb3),
        ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00),
        ("yellowgreen", 0x9acd32),
    ];
    NAMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| [(v >> 16) as u8, (v >> 8) as u8, *v as u8])
}

/// Schriftfamilie aus dem SVG → BoxDoc-Schriftschlüssel.
///
/// Die Liste wird von vorn durchgegangen wie im Browser. Verglichen wird ohne
/// Leerzeichen, Bindestriche und Groß/klein, damit auch PDF-Konverter-Namen
/// wie `Roboto-Regular` oder `ArialMT` ihre Schrift finden.
fn map_font(family: &str) -> String {
    let norm = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(|c| c.to_lowercase())
            .collect::<String>()
    };
    for raw in family.split(',') {
        let name = norm(raw.trim().trim_matches(|c| c == '"' || c == '\''));
        if name.is_empty() {
            continue;
        }
        for f in crate::model::FONT_CHOICES.iter().skip(1) {
            let d = norm(f.display);
            if name == d || name == f.key || name.starts_with(&d) {
                return f.key.to_string();
            }
        }
        let alias = match name.as_str() {
            "helvetica" | "helveticaneue" | "liberationsans" | "arimo" => Some("arial"),
            "serif" | "timesnewroman" | "times" => Some("lora"),
            "monospace" | "couriernew" | "courier" | "dejavusansmono" => Some("jetbrains"),
            "sansserif" | "systemui" => Some("default"),
            _ => None,
        };
        if let Some(a) = alias {
            return a.to_string();
        }
    }
    String::from("default")
}

// ===========================================================================
// Pfaddaten
// ===========================================================================

/// Ein Teilpfad in Nutzereinheiten, noch nicht transformiert.
#[derive(Debug, Clone)]
struct SubPath {
    nodes: Vec<PathNode>,
    closed: bool,
}

fn parse_path_data(d: &str) -> Vec<SubPath> {
    let mut subs: Vec<SubPath> = Vec::new();
    let mut cur: Option<SubPath> = None;
    let mut pos = Pos2::ZERO;
    let mut start = Pos2::ZERO;
    // Letzter Kontrollpunkt für die glatten Befehle S/T.
    let mut last_cubic: Option<Pos2> = None;
    let mut last_quad: Option<Pos2> = None;

    let mut lx = NumLexer::new(d);
    let mut cmd = b'M';

    fn finish(cur: &mut Option<SubPath>, subs: &mut Vec<SubPath>) {
        if let Some(s) = cur.take() {
            if s.nodes.len() >= 2 {
                subs.push(s);
            }
        }
    }

    // Einen Knoten an den laufenden Teilpfad hängen; startet einen neuen,
    // falls nach einem `Z` ohne `M` weitergezeichnet wird.
    fn ensure<'a>(cur: &'a mut Option<SubPath>, at: Pos2) -> &'a mut SubPath {
        cur.get_or_insert_with(|| SubPath {
            nodes: vec![PathNode::corner(at)],
            closed: false,
        })
    }

    loop {
        let Some(c) = lx.peek() else { break };
        if c.is_ascii_alphabetic() {
            cmd = c;
            lx.i += 1;
            if cmd == b'Z' || cmd == b'z' {
                if let Some(s) = cur.as_mut() {
                    s.closed = true;
                    // Endet der Zug auf seinem Start, ist der letzte Knoten
                    // doppelt: Sein Eingangsgriff gehört dem ersten Knoten,
                    // das Schlusssegment übernimmt BoxDoc selbst. Bei weniger
                    // als drei Knoten zeichnet BoxDoc kein Schlusssegment —
                    // dann bleibt die Dopplung stehen.
                    let n = s.nodes.len();
                    if n > 3 && (s.nodes[n - 1].anchor - s.nodes[0].anchor).length() < 1e-4 {
                        let last = s.nodes.pop().unwrap();
                        s.nodes[0].in_h = last.in_h;
                    }
                }
                finish(&mut cur, &mut subs);
                pos = start;
                last_cubic = None;
                last_quad = None;
            }
            continue;
        }
        if !lx.at_number() {
            lx.i += 1; // unbekanntes Zeichen überspringen
            continue;
        }
        let rel = cmd.is_ascii_lowercase();
        let off = if rel { pos.to_vec2() } else { Vec2::ZERO };
        let mut num = || lx.number();
        match cmd.to_ascii_uppercase() {
            b'M' => {
                let (Some(x), Some(y)) = (num(), num()) else { break };
                finish(&mut cur, &mut subs);
                pos = Pos2::new(x, y) + off;
                start = pos;
                cur = Some(SubPath {
                    nodes: vec![PathNode::corner(pos)],
                    closed: false,
                });
                // Weitere Koordinatenpaare nach M sind implizite L-Befehle.
                cmd = if rel { b'l' } else { b'L' };
                last_cubic = None;
                last_quad = None;
            }
            b'L' => {
                let (Some(x), Some(y)) = (num(), num()) else { break };
                let p = Pos2::new(x, y) + off;
                ensure(&mut cur, pos).nodes.push(PathNode::corner(p));
                pos = p;
                last_cubic = None;
                last_quad = None;
            }
            b'H' => {
                let Some(x) = num() else { break };
                let p = Pos2::new(if rel { pos.x + x } else { x }, pos.y);
                ensure(&mut cur, pos).nodes.push(PathNode::corner(p));
                pos = p;
                last_cubic = None;
                last_quad = None;
            }
            b'V' => {
                let Some(y) = num() else { break };
                let p = Pos2::new(pos.x, if rel { pos.y + y } else { y });
                ensure(&mut cur, pos).nodes.push(PathNode::corner(p));
                pos = p;
                last_cubic = None;
                last_quad = None;
            }
            b'C' | b'S' => {
                let smooth = cmd.to_ascii_uppercase() == b'S';
                let c1 = if smooth {
                    last_cubic.map(|c| pos + (pos - c)).unwrap_or(pos)
                } else {
                    let (Some(x), Some(y)) = (num(), num()) else { break };
                    Pos2::new(x, y) + off
                };
                let (Some(x2), Some(y2), Some(x), Some(y)) = (num(), num(), num(), num()) else {
                    break;
                };
                let c2 = Pos2::new(x2, y2) + off;
                let p = Pos2::new(x, y) + off;
                cubic_to(ensure(&mut cur, pos), c1, c2, p);
                pos = p;
                last_cubic = Some(c2);
                last_quad = None;
            }
            b'Q' | b'T' => {
                let smooth = cmd.to_ascii_uppercase() == b'T';
                let q = if smooth {
                    last_quad.map(|c| pos + (pos - c)).unwrap_or(pos)
                } else {
                    let (Some(x), Some(y)) = (num(), num()) else { break };
                    Pos2::new(x, y) + off
                };
                let (Some(x), Some(y)) = (num(), num()) else { break };
                let p = Pos2::new(x, y) + off;
                // Quadratisch → kubisch: die Kontrollpunkte liegen bei 2/3.
                let c1 = pos + (q - pos) * (2.0 / 3.0);
                let c2 = p + (q - p) * (2.0 / 3.0);
                cubic_to(ensure(&mut cur, pos), c1, c2, p);
                pos = p;
                last_quad = Some(q);
                last_cubic = None;
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(phi)) = (lx.number(), lx.number(), lx.number())
                else {
                    break;
                };
                let (Some(large), Some(sweep)) = (lx.flag(), lx.flag()) else { break };
                let (Some(x), Some(y)) = (lx.number(), lx.number()) else { break };
                let p = Pos2::new(x, y) + off;
                let sub = ensure(&mut cur, pos);
                for (c1, c2, e) in arc_to_cubics(pos, rx, ry, phi, large, sweep, p) {
                    cubic_to(sub, c1, c2, e);
                }
                if (p - pos).length() > 0.0 && (rx == 0.0 || ry == 0.0) {
                    sub.nodes.push(PathNode::corner(p));
                }
                pos = p;
                last_cubic = None;
                last_quad = None;
            }
            _ => {
                lx.number();
            }
        }
    }
    finish(&mut cur, &mut subs);
    subs
}

fn cubic_to(sub: &mut SubPath, c1: Pos2, c2: Pos2, p: Pos2) {
    if let Some(last) = sub.nodes.last_mut() {
        last.out_h = c1;
    }
    sub.nodes.push(PathNode {
        anchor: p,
        in_h: c2,
        out_h: p,
    });
}

/// Elliptischer Bogen → kubische Kurven, je höchstens 90°.
///
/// Umrechnung von der Endpunkt- in die Mittelpunktdarstellung nach
/// SVG 1.1, Anhang F.6.5/F.6.6 (inklusive Vergrößern zu kleiner Radien).
fn arc_to_cubics(
    p1: Pos2,
    rx: f32,
    ry: f32,
    phi_deg: f32,
    large: bool,
    sweep: bool,
    p2: Pos2,
) -> Vec<(Pos2, Pos2, Pos2)> {
    let (mut rx, mut ry) = (rx.abs() as f64, ry.abs() as f64);
    if (p1 - p2).length() < 1e-6 || rx < 1e-9 || ry < 1e-9 {
        return Vec::new(); // Gerade (siehe Aufrufer) oder nichts
    }
    let (x1, y1, x2, y2) = (p1.x as f64, p1.y as f64, p2.x as f64, p2.y as f64);
    let phi = (phi_deg as f64).to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cp * dx + sp * dy;
    let y1p = -sp * dx + cp * dy;
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut coef = if den > 0.0 { (num / den).max(0.0).sqrt() } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (x1 + x2) / 2.0;
    let cy = sp * cxp + cp * cyp + (y1 + y2) / 2.0;

    let angle = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = angle(1.0, 0.0, ux, uy);
    let mut dtheta = angle(ux, uy, vx, vy);
    let tau = std::f64::consts::TAU;
    if !sweep && dtheta > 0.0 {
        dtheta -= tau;
    } else if sweep && dtheta < 0.0 {
        dtheta += tau;
    }

    let segs = (dtheta.abs() / (std::f64::consts::FRAC_PI_2) - 1e-9).ceil().max(1.0) as usize;
    let step = dtheta / segs as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |t: f64| {
        let (st, ct) = t.sin_cos();
        let x = rx * ct;
        let y = ry * st;
        (cp * x - sp * y + cx, sp * x + cp * y + cy)
    };
    let deriv = |t: f64| {
        let (st, ct) = t.sin_cos();
        let x = -rx * st;
        let y = ry * ct;
        (cp * x - sp * y, sp * x + cp * y)
    };
    let mut out = Vec::with_capacity(segs);
    let mut t = theta1;
    for i in 0..segs {
        let t2 = t + step;
        let (ax, ay) = point(t);
        let (bx, by) = if i + 1 == segs { (x2, y2) } else { point(t2) };
        let (dax, day) = deriv(t);
        let (dbx, dby) = deriv(t2);
        out.push((
            Pos2::new((ax + k * dax) as f32, (ay + k * day) as f32),
            Pos2::new((bx - k * dbx) as f32, (by - k * dby) as f32),
            Pos2::new(bx as f32, by as f32),
        ));
        t = t2;
    }
    out
}

/// Ellipse als vier kubische Viertelbögen — für Ellipsen, die durch eine
/// Scherung keine Ellipse mit Drehung mehr sind.
fn ellipse_nodes(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<PathNode> {
    const K: f32 = 0.552_284_8;
    let (kx, ky) = (rx * K, ry * K);
    vec![
        PathNode {
            anchor: Pos2::new(cx + rx, cy),
            in_h: Pos2::new(cx + rx, cy - ky),
            out_h: Pos2::new(cx + rx, cy + ky),
        },
        PathNode {
            anchor: Pos2::new(cx, cy + ry),
            in_h: Pos2::new(cx + kx, cy + ry),
            out_h: Pos2::new(cx - kx, cy + ry),
        },
        PathNode {
            anchor: Pos2::new(cx - rx, cy),
            in_h: Pos2::new(cx - rx, cy + ky),
            out_h: Pos2::new(cx - rx, cy - ky),
        },
        PathNode {
            anchor: Pos2::new(cx, cy - ry),
            in_h: Pos2::new(cx - kx, cy - ry),
            out_h: Pos2::new(cx + kx, cy - ry),
        },
    ]
}

/// Abgerundetes Rechteck als Pfad (ungleiche Radien oder Scherung).
fn rect_nodes(x: f32, y: f32, w: f32, h: f32, rx: f32, ry: f32) -> Vec<PathNode> {
    if rx <= 0.0 || ry <= 0.0 {
        return vec![
            PathNode::corner(Pos2::new(x, y)),
            PathNode::corner(Pos2::new(x + w, y)),
            PathNode::corner(Pos2::new(x + w, y + h)),
            PathNode::corner(Pos2::new(x, y + h)),
        ];
    }
    const K: f32 = 0.552_284_8;
    let (kx, ky) = (rx * (1.0 - K), ry * (1.0 - K));
    let n = |ax: f32, ay: f32, ix: f32, iy: f32, ox: f32, oy: f32| PathNode {
        anchor: Pos2::new(ax, ay),
        in_h: Pos2::new(ix, iy),
        out_h: Pos2::new(ox, oy),
    };
    let (r, b) = (x + w, y + h);
    vec![
        n(x + rx, y, x + kx, y, x + rx, y),
        n(r - rx, y, r - rx, y, r - kx, y),
        n(r, y + ry, r, y + ky, r, y + ry),
        n(r, b - ry, r, b - ry, r, b - ky),
        n(r - rx, b, r - kx, b, r - rx, b),
        n(x + rx, b, x + rx, b, x + kx, b),
        n(x, b - ry, x, b - ky, x, b - ry),
        n(x, y + ry, x, y + ry, x, y + ky),
    ]
}

// ===========================================================================
// Baum ablaufen
// ===========================================================================

struct Walker<'a> {
    dom: &'a Dom,
    css: &'a CssRules,
    next_id: u64,
    elements: Vec<Element>,
    images: Vec<ImportedImage>,
    text_fixes: Vec<TextFix>,
    skipped: usize,
    load_file: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    /// Bezugsgröße für `%`-Längen (Nutzereinheiten).
    viewport: (f32, f32),
}

impl<'a> Walker<'a> {
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Stil eines Knotens: Attribut < `<style>`-Regel < `style="…"`.
    fn style_for(&self, idx: usize, parent: &Style) -> Style {
        let node = &self.dom.nodes[idx];
        let mut props: Vec<(String, String)> = Vec::new();
        for (k, v) in &node.attrs {
            if PRESENTATION.contains(&k.as_str()) {
                props.push((k.clone(), v.clone()));
            }
        }
        for (k, v) in self.css.matching(node) {
            props.push((k.clone(), v.clone()));
        }
        if let Some(st) = node.attr("style") {
            props.extend(parse_declarations(st));
        }

        let mut s = parent.clone();
        s.display = true;
        // `opacity` gilt für dieses Element, nicht vererbt — multipliziert
        // sich aber mit dem, was die Eltern schon hatten.
        let mut own_opacity = 1.0;
        // `color` zuerst: `currentColor` in fill/stroke bezieht sich darauf.
        if let Some((_, v)) = props.iter().rev().find(|(k, _)| k == "color") {
            if let Some((c, _)) = parse_color(v, parent.color) {
                s.color = c;
            }
        }
        // Spätere Einträge gewinnen.
        for (k, v) in &props {
            let v = v.trim();
            if v == "inherit" {
                continue;
            }
            match k.as_str() {
                "fill" => s.fill = self.paint(v, s.color).unwrap_or(s.fill),
                "stroke" => s.stroke = self.paint(v, s.color).unwrap_or(s.stroke),
                "stroke-width" => {
                    if let Some(w) = parse_length(v, s.font_size, self.viewport.0) {
                        s.stroke_width = w.max(0.0);
                    }
                }
                "fill-opacity" => s.fill_opacity = parse_opacity(v).unwrap_or(s.fill_opacity),
                "stroke-opacity" => {
                    s.stroke_opacity = parse_opacity(v).unwrap_or(s.stroke_opacity)
                }
                "opacity" => own_opacity = parse_opacity(v).unwrap_or(1.0),
                "font-family" => s.font_family = v.to_string(),
                "font-size" => {
                    let named = match v {
                        "xx-small" => Some(9.0),
                        "x-small" => Some(10.0),
                        "small" => Some(13.0),
                        "medium" => Some(16.0),
                        "large" => Some(18.0),
                        "x-large" => Some(24.0),
                        "xx-large" => Some(32.0),
                        _ => None,
                    };
                    if let Some(sz) = named.or_else(|| parse_length(v, parent.font_size, parent.font_size * 100.0)) {
                        if sz > 0.0 {
                            s.font_size = sz;
                        }
                    }
                }
                "font-weight" => {
                    s.bold = match v {
                        "bold" | "bolder" => true,
                        "normal" | "lighter" => false,
                        n => n.parse::<f32>().map(|w| w >= 600.0).unwrap_or(s.bold),
                    }
                }
                "font-style" => s.italic = v == "italic" || v.starts_with("oblique"),
                "text-decoration" | "text-decoration-line" => {
                    s.underline = v.contains("underline");
                    s.strike = v.contains("line-through");
                }
                "text-anchor" => {
                    s.anchor = match v {
                        "middle" => Anchor::Middle,
                        "end" => Anchor::End,
                        _ => Anchor::Start,
                    }
                }
                "visibility" => s.visible = v == "visible",
                "display" => s.display = v != "none",
                _ => {}
            }
        }
        s.opacity = parent.opacity * own_opacity;
        s
    }

    /// `fill`/`stroke`-Wert. `None` = unverständlich (Elternwert bleibt).
    fn paint(&self, v: &str, current: [u8; 3]) -> Option<Paint> {
        let lower = v.trim().to_ascii_lowercase();
        if lower == "none" || lower == "transparent" {
            return Some(Paint::None);
        }
        if let Some(rest) = lower.strip_prefix("url(") {
            let id = rest
                .split(')')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim_start_matches('#');
            // Original-Schreibweise der ID (IDs unterscheiden Groß/klein).
            let orig_id = v
                .find('#')
                .map(|i| v[i + 1..].split(')').next().unwrap_or("").trim_matches(|c| c == '"' || c == '\''))
                .unwrap_or(id);
            if let Some(c) = self.gradient_color(orig_id, 0) {
                return Some(c);
            }
            // Rückfallfarbe hinter der URL: `url(#x) red`.
            if let Some(fb) = v.split(')').nth(1).map(str::trim).filter(|s| !s.is_empty()) {
                return self.paint(fb, current);
            }
            return Some(Paint::Color([128, 128, 128], 1.0));
        }
        parse_color(v, current).map(|(c, a)| Paint::Color(c, a))
    }

    /// Mischfarbe eines Verlaufs: der Durchschnitt seiner Stopps.
    fn gradient_color(&self, id: &str, depth: usize) -> Option<Paint> {
        let idx = self.dom.by_id(id)?;
        let node = &self.dom.nodes[idx];
        if node.name != "linearGradient" && node.name != "radialGradient" {
            return None;
        }
        let stops: Vec<usize> = node
            .element_children()
            .into_iter()
            .filter(|&c| self.dom.nodes[c].name == "stop")
            .collect();
        if stops.is_empty() {
            // Stopps können von einem anderen Verlauf geerbt sein.
            if depth < MAX_USE_DEPTH {
                if let Some(h) = node.href() {
                    return self.gradient_color(h.trim_start_matches('#'), depth + 1);
                }
            }
            return None;
        }
        let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        for &s in &stops {
            let st = &self.dom.nodes[s];
            let mut color = st.attr("stop-color").map(str::to_string);
            let mut opacity = st.attr("stop-opacity").map(str::to_string);
            if let Some(decl) = st.attr("style") {
                for (k, v) in parse_declarations(decl) {
                    match k.as_str() {
                        "stop-color" => color = Some(v),
                        "stop-opacity" => opacity = Some(v),
                        _ => {}
                    }
                }
            }
            let (c, ca) = color
                .as_deref()
                .and_then(|c| parse_color(c, [0, 0, 0]))
                .unwrap_or(([0, 0, 0], 1.0));
            let o = opacity.as_deref().and_then(parse_opacity).unwrap_or(1.0) * ca;
            r += c[0] as f32;
            g += c[1] as f32;
            b += c[2] as f32;
            a += o;
        }
        let n = stops.len() as f32;
        Some(Paint::Color(
            [(r / n).round() as u8, (g / n).round() as u8, (b / n).round() as u8],
            a / n,
        ))
    }

    fn walk(&mut self, idx: usize, m: &Affine, parent: &Style, depth: usize) {
        let node = &self.dom.nodes[idx];
        let name = node.name.clone();
        match name.as_str() {
            // Nicht gezeichnet, nur referenziert — oder reine Metadaten.
            "defs" | "clipPath" | "mask" | "pattern" | "linearGradient" | "radialGradient"
            | "marker" | "symbol" | "metadata" | "title" | "desc" | "style" | "script"
            | "filter" | "stop" => return,
            _ => {}
        }
        if name.contains(':') {
            return; // Editor-Metadaten (sodipodi:namedview, inkscape:…)
        }

        let style = self.style_for(idx, parent);
        if !style.display {
            return;
        }
        let mut m = *m;
        if let Some(t) = node.attr("transform") {
            m = m.then_inner(&parse_transform(t));
        }

        match name.as_str() {
            "g" | "a" | "switch" => {
                for c in node.element_children() {
                    self.walk(c, &m, &style, depth);
                }
            }
            "svg" => {
                // Verschachteltes SVG: eigener Ursprung und ggf. viewBox.
                let len = |k: &str, base: f32| {
                    node.attr(k)
                        .and_then(|v| parse_length(v, style.font_size, base))
                };
                let x = len("x", self.viewport.0).unwrap_or(0.0);
                let y = len("y", self.viewport.1).unwrap_or(0.0);
                let mut inner = m.then_inner(&Affine::translate(x, y));
                if let Some(vb) = node.attr("viewBox").and_then(parse_view_box) {
                    let w = len("width", self.viewport.0).unwrap_or(vb[2]);
                    let h = len("height", self.viewport.1).unwrap_or(vb[3]);
                    let par = node.attr("preserveAspectRatio").unwrap_or("");
                    inner = inner.then_inner(&view_box_transform(vb, w, h, par));
                }
                for c in node.element_children() {
                    self.walk(c, &inner, &style, depth);
                }
            }
            "use" => {
                if depth >= MAX_USE_DEPTH {
                    return;
                }
                let Some(target) = node
                    .href()
                    .and_then(|h| h.strip_prefix('#'))
                    .and_then(|id| self.dom.by_id(id))
                else {
                    return;
                };
                let x = self.num_attr(idx, "x", &style, self.viewport.0);
                let y = self.num_attr(idx, "y", &style, self.viewport.1);
                let inner = m.then_inner(&Affine::translate(x, y));
                if self.dom.nodes[target].name == "symbol" {
                    let tstyle = self.style_for(target, &style);
                    for c in self.dom.nodes[target].element_children() {
                        self.walk(c, &inner, &tstyle, depth + 1);
                    }
                } else {
                    self.walk(target, &inner, &style, depth + 1);
                }
            }
            "rect" => self.rect(idx, &m, &style),
            "circle" | "ellipse" => self.ellipse(idx, &m, &style),
            "line" => self.line(idx, &m, &style),
            "polyline" | "polygon" => {
                let pts = numbers(node.attr("points").unwrap_or(""));
                let nodes: Vec<PathNode> = pts
                    .chunks_exact(2)
                    .map(|p| PathNode::corner(Pos2::new(p[0], p[1])))
                    .collect();
                if nodes.len() >= 2 {
                    let sub = SubPath {
                        nodes,
                        closed: name == "polygon",
                    };
                    self.emit_paths(vec![sub], &m, &style);
                }
            }
            "path" => {
                let subs = parse_path_data(node.attr("d").unwrap_or(""));
                self.emit_paths(subs, &m, &style);
            }
            "text" => self.text(idx, &m, &style),
            "image" => self.image(idx, &m, &style),
            _ => self.skipped += 1,
        }
    }

    fn num_attr(&self, idx: usize, key: &str, style: &Style, base: f32) -> f32 {
        self.dom.nodes[idx]
            .attr(key)
            .and_then(|v| parse_length(v, style.font_size, base))
            .unwrap_or(0.0)
    }

    /// Füll- und Konturwerte auf ein Element übertragen.
    fn apply_paint(&self, el: &mut Element, style: &Style, m: &Affine, fillable: bool) {
        el.fill_color = match (fillable, style.fill) {
            (true, Paint::Color(c, a)) => rgba(c, a * style.fill_opacity * style.opacity),
            _ => [0, 0, 0, 0],
        };
        match style.stroke {
            Paint::Color(c, a) if style.stroke_width > 0.0 => {
                el.stroke_color = rgba(c, a * style.stroke_opacity * style.opacity);
                el.stroke_width = style.stroke_width * m.mean_scale();
            }
            _ => {
                // Kein Rahmen. Die Farbe bleibt ein sichtbares Grau, damit ein
                // Rahmen, den man später aufzieht, nicht unsichtbar ist.
                el.stroke_width = 0.0;
                el.stroke_color = [40, 40, 40, 255];
            }
        }
    }

    fn has_paint(&self, style: &Style, fillable: bool) -> bool {
        style.visible
            && ((fillable && style.fill != Paint::None)
                || (style.stroke != Paint::None && style.stroke_width > 0.0))
    }

    fn rect(&mut self, idx: usize, m: &Affine, style: &Style) {
        let (vw, vh) = self.viewport;
        let x = self.num_attr(idx, "x", style, vw);
        let y = self.num_attr(idx, "y", style, vh);
        let w = self.num_attr(idx, "width", style, vw);
        let h = self.num_attr(idx, "height", style, vh);
        if w <= 0.0 || h <= 0.0 || !self.has_paint(style, true) {
            return;
        }
        let node = &self.dom.nodes[idx];
        let rx_a = node.attr("rx").and_then(|v| parse_length(v, style.font_size, vw));
        let ry_a = node.attr("ry").and_then(|v| parse_length(v, style.font_size, vh));
        let rx = rx_a.or(ry_a).unwrap_or(0.0).clamp(0.0, w / 2.0);
        let ry = ry_a.or(rx_a).unwrap_or(0.0).clamp(0.0, h / 2.0);

        match m.decompose() {
            Some((rot, sx, sy)) if (rx * sx - ry * sy.abs()).abs() < 0.01 => {
                let id = self.id();
                let c = m.apply(x + w / 2.0, y + h / 2.0);
                let (bw, bh) = (w * sx, h * sy.abs());
                let mut el = Element::new_rectangle(id, c.x - bw / 2.0, c.y - bh / 2.0);
                el.w = bw;
                el.h = bh;
                el.rotation = rot;
                el.corner_radius = rx * sx;
                self.apply_paint(&mut el, style, m, true);
                self.elements.push(el);
            }
            _ => {
                let sub = SubPath {
                    nodes: rect_nodes(x, y, w, h, rx, ry),
                    closed: true,
                };
                self.emit_paths(vec![sub], m, style);
            }
        }
    }

    fn ellipse(&mut self, idx: usize, m: &Affine, style: &Style) {
        let (vw, vh) = self.viewport;
        let node = &self.dom.nodes[idx];
        let cx = self.num_attr(idx, "cx", style, vw);
        let cy = self.num_attr(idx, "cy", style, vh);
        let (rx, ry) = if node.name == "circle" {
            let diag = (vw * vw + vh * vh).sqrt() / std::f32::consts::SQRT_2;
            let r = self.num_attr(idx, "r", style, diag);
            (r, r)
        } else {
            let rx_a = node.attr("rx").and_then(|v| parse_length(v, style.font_size, vw));
            let ry_a = node.attr("ry").and_then(|v| parse_length(v, style.font_size, vh));
            (rx_a.or(ry_a).unwrap_or(0.0), ry_a.or(rx_a).unwrap_or(0.0))
        };
        if rx <= 0.0 || ry <= 0.0 || !self.has_paint(style, true) {
            return;
        }
        match m.decompose() {
            Some((rot, sx, sy)) => {
                let id = self.id();
                let c = m.apply(cx, cy);
                let (bw, bh) = (2.0 * rx * sx, 2.0 * ry * sy.abs());
                let mut el = Element::new_ellipse(id, c.x - bw / 2.0, c.y - bh / 2.0);
                el.w = bw;
                el.h = bh;
                el.rotation = rot;
                self.apply_paint(&mut el, style, m, true);
                self.elements.push(el);
            }
            None => {
                let sub = SubPath {
                    nodes: ellipse_nodes(cx, cy, rx, ry),
                    closed: true,
                };
                self.emit_paths(vec![sub], m, style);
            }
        }
    }

    fn line(&mut self, idx: usize, m: &Affine, style: &Style) {
        if !self.has_paint(style, false) {
            return;
        }
        let (vw, vh) = self.viewport;
        let a = m.apply(
            self.num_attr(idx, "x1", style, vw),
            self.num_attr(idx, "y1", style, vh),
        );
        let b = m.apply(
            self.num_attr(idx, "x2", style, vw),
            self.num_attr(idx, "y2", style, vh),
        );
        let len = (b - a).length();
        if len < 1e-4 {
            return;
        }
        let id = self.id();
        let c = a + (b - a) / 2.0;
        let mut el = Element::new_line(id, c.x - len / 2.0, c.y);
        el.w = len;
        el.h = 0.0;
        el.rotation = (b.y - a.y).atan2(b.x - a.x).to_degrees();
        self.apply_paint(&mut el, style, m, false);
        self.elements.push(el);
    }

    /// Teilpfade als Pfad-Elemente.
    ///
    /// SVG füllt auch offene Teilpfade (als wären sie geschlossen), BoxDoc
    /// füllt sie nie. Damit das Bild gleich bleibt: Hat ein offener Teilpfad
    /// keine Kontur, wird er geschlossen — die Schlusskante sieht man ohne
    /// Kontur nicht. Hat er beides, wird er geteilt in eine geschlossene
    /// Fläche ohne Kontur und darüber die offene Kontur ohne Füllung.
    fn emit_paths(&mut self, subs: Vec<SubPath>, m: &Affine, style: &Style) {
        if !self.has_paint(style, true) {
            return;
        }
        let has_fill = style.fill != Paint::None;
        let has_stroke = style.stroke != Paint::None && style.stroke_width > 0.0;
        for sub in subs {
            let nodes: Vec<PathNode> = sub
                .nodes
                .iter()
                .map(|n| PathNode {
                    anchor: m.apply(n.anchor.x, n.anchor.y),
                    in_h: m.apply(n.in_h.x, n.in_h.y),
                    out_h: m.apply(n.out_h.x, n.out_h.y),
                })
                .collect();
            if sub.closed || !has_fill {
                self.push_path(&nodes, sub.closed, style, m, true, true);
            } else if !has_stroke {
                self.push_path(&nodes, true, style, m, true, false);
            } else {
                self.push_path(&nodes, true, style, m, true, false);
                self.push_path(&nodes, false, style, m, false, true);
            }
        }
    }

    fn push_path(
        &mut self,
        nodes: &[PathNode],
        closed: bool,
        style: &Style,
        m: &Affine,
        with_fill: bool,
        with_stroke: bool,
    ) {
        let id = self.id();
        let mut el = geometry::path_from_nodes(id, nodes, closed);
        let mut st = style.clone();
        if !with_fill {
            st.fill = Paint::None;
        }
        if !with_stroke {
            st.stroke = Paint::None;
        }
        self.apply_paint(&mut el, &st, m, closed);
        self.elements.push(el);
    }

    fn text(&mut self, idx: usize, m: &Affine, style: &Style) {
        let (vw, vh) = self.viewport;
        let first = |v: Option<&str>, base: f32, fs: f32| {
            v.and_then(|s| {
                s.split(|c: char| c == ',' || c.is_whitespace())
                    .find(|p| !p.is_empty())
                    .and_then(|p| parse_length(p, fs, base))
            })
        };
        let node = &self.dom.nodes[idx];
        let x0 = first(node.attr("x"), vw, style.font_size).unwrap_or(0.0)
            + first(node.attr("dx"), vw, style.font_size).unwrap_or(0.0);
        let y0 = first(node.attr("y"), vh, style.font_size).unwrap_or(0.0)
            + first(node.attr("dy"), vh, style.font_size).unwrap_or(0.0);
        let preserve = node.attr("xml:space") == Some("preserve");

        // Zeilen sammeln. Ein <tspan> mit eigener y-Position, die merklich
        // von der laufenden abweicht, beginnt eine neue Zeile — so schreiben
        // BoxDoc, Inkscape und Illustrator mehrzeiligen Text.
        let mut acc = TextAcc {
            lines: vec![String::new()],
            line_x: vec![None],
            cur_y: y0,
            anchor_pos: None,
            style: style.clone(),
        };
        self.collect_text(idx, style, preserve, (x0, y0), &mut acc, true);
        let text_style = acc.style;
        let anchor_pos = acc.anchor_pos;
        let lines: Vec<String> = acc
            .lines
            .into_iter()
            .map(|l| if preserve { l } else { l.trim().to_string() })
            .collect();
        // Leere Zeilen am Anfang/Ende verwerfen; innen bleiben sie stehen.
        let start = lines.iter().position(|l| !l.is_empty());
        let end = lines.iter().rposition(|l| !l.is_empty());
        let (Some(start), Some(end)) = (start, end) else {
            return;
        };
        let content = lines[start..=end].join("\n");
        if !style.visible || text_style.fill == Paint::None {
            return;
        }

        let (ax, ay) = anchor_pos.unwrap_or((x0, y0));
        let p = m.apply(ax, ay);
        let (rot, sx, scale) = match m.decompose() {
            Some((rot, sx, sy)) => (rot, sx, sy.abs()),
            None => (0.0, m.mean_scale(), m.mean_scale()),
        };
        // Zeilenanfänge relativ zur ersten Zeile, in pt entlang der
        // Textrichtung — daraus erkennt `fit_texts` die Ausrichtung.
        let line_dx: Vec<f32> = match acc.line_x[start..=end]
            .iter()
            .copied()
            .collect::<Option<Vec<f32>>>()
        {
            Some(xs) if text_style.anchor == Anchor::Start && xs.len() > 1 => {
                xs.iter().map(|x| (x - xs[0]) * sx).collect()
            }
            _ => Vec::new(),
        };

        let id = self.id();
        let mut el = Element::new_text(id, p.x, p.y);
        el.text = content;
        el.font_size = (text_style.font_size * scale).max(1.0);
        el.font = map_font(&text_style.font_family);
        el.color = match text_style.fill {
            Paint::Color(c, a) => rgba(c, a * text_style.fill_opacity * text_style.opacity),
            Paint::None => [0, 0, 0, 0],
        };
        el.bold = text_style.bold;
        el.italic = text_style.italic;
        el.underline = text_style.underline;
        el.strikethrough = text_style.strike;
        el.align = match text_style.anchor {
            Anchor::Start => TextAlign::Left,
            Anchor::Middle => TextAlign::Center,
            Anchor::End => TextAlign::Right,
        };
        el.valign = VAlign::Top;
        el.rotation = rot;
        el.auto_height = true;
        el.fill_color = [0, 0, 0, 0];
        el.stroke_width = 0.0;

        // Aus BoxDoc exportiert? Dann Originaltext samt weicher Umbrüche,
        // Boxbreite und Ausrichtung zurückholen (siehe `svg::text`).
        let keep_layout = match boxdoc_text_layout(&self.dom.nodes[idx], &el.text) {
            Some((text, w, align)) => {
                el.text = text;
                el.w = w * sx;
                el.align = align;
                true
            }
            None => false,
        };

        // Grobe Vorbelegung ohne Schriftmetrik; `fit_texts` macht es genau.
        if !keep_layout {
            let longest = el.text.lines().map(|l| l.chars().count()).max().unwrap_or(1);
            el.w = (longest as f32 * el.font_size * 0.55).max(el.font_size);
        }
        el.h = el.text.lines().count().max(1) as f32 * el.font_size * 1.2;
        let ax_off = match text_style.anchor {
            Anchor::Start => 0.0,
            Anchor::Middle => el.w / 2.0,
            Anchor::End => el.w,
        };
        let center_rel = Vec2::new(el.w / 2.0 - ax_off, el.h / 2.0 - el.font_size * 0.8);
        let c = p + geometry::rotate_vec(center_rel, rot);
        el.x = c.x - el.w / 2.0;
        el.y = c.y - el.h / 2.0;

        self.text_fixes.push(TextFix {
            id,
            anchor_pt: p,
            anchor: text_style.anchor,
            line_dx: if keep_layout { Vec::new() } else { line_dx },
            keep_layout,
        });
        self.elements.push(el);
    }

    /// Text eines `<text>` samt `<tspan>`s einsammeln.
    ///
    /// `acc.style` übernimmt den Stil des ersten Stücks mit sichtbarem
    /// Inhalt — BoxDoc kennt nur einen Stil je Textblock, und das erste Wort
    /// ist der beste Vertreter.
    fn collect_text(
        &self,
        idx: usize,
        style: &Style,
        preserve: bool,
        pos: (f32, f32),
        acc: &mut TextAcc,
        is_root: bool,
    ) {
        let (vw, vh) = self.viewport;
        let mut pos = pos;
        for child in &self.dom.nodes[idx].children {
            match child {
                Child::Text(t) => {
                    let piece = if preserve {
                        t.replace(['\n', '\r', '\t'], " ")
                    } else {
                        collapse_ws(t)
                    };
                    let line = acc.lines.last_mut().unwrap();
                    if piece.trim().is_empty() {
                        if !piece.is_empty() && !line.is_empty() && !line.ends_with(' ') {
                            line.push(' ');
                        }
                        continue;
                    }
                    if acc.anchor_pos.is_none() {
                        acc.anchor_pos = Some(pos);
                        if !is_root {
                            acc.style = style.clone();
                        }
                    }
                    let lx = acc.line_x.last_mut().unwrap();
                    if lx.is_none() {
                        *lx = Some(pos.0);
                    }
                    if line.ends_with(' ') && piece.starts_with(' ') {
                        line.push_str(piece.trim_start());
                    } else {
                        line.push_str(&piece);
                    }
                }
                Child::El(c) => {
                    let n = &self.dom.nodes[*c];
                    if n.name != "tspan" && n.name != "a" && n.name != "textPath" {
                        continue;
                    }
                    let st = self.style_for(*c, style);
                    if !st.display {
                        continue;
                    }
                    let first = |v: Option<&str>, base: f32| {
                        v.and_then(|s| {
                            s.split(|c: char| c == ',' || c.is_whitespace())
                                .find(|p| !p.is_empty())
                                .and_then(|p| parse_length(p, st.font_size, base))
                        })
                    };
                    let mut new_y = acc.cur_y;
                    if let Some(y) = first(n.attr("y"), vh) {
                        new_y = y;
                    }
                    if let Some(dy) = first(n.attr("dy"), vh) {
                        new_y += dy;
                    }
                    let mut new_x = pos.0;
                    if let Some(x) = first(n.attr("x"), vw) {
                        new_x = x;
                    }
                    if let Some(dx) = first(n.attr("dx"), vw) {
                        new_x += dx;
                    }
                    if (new_y - acc.cur_y).abs() > st.font_size * 0.3 {
                        if !acc.lines.last().unwrap().trim().is_empty() || acc.anchor_pos.is_some()
                        {
                            acc.lines.push(String::new());
                            acc.line_x.push(None);
                        }
                        acc.cur_y = new_y;
                    }
                    pos = (new_x, new_y);
                    let preserve_c = match n.attr("xml:space") {
                        Some("preserve") => true,
                        Some(_) => false,
                        None => preserve,
                    };
                    self.collect_text(*c, &st, preserve_c, pos, acc, false);
                }
            }
        }
        if is_root && acc.anchor_pos.is_none() {
            acc.style = style.clone();
        }
    }

    fn image(&mut self, idx: usize, m: &Affine, style: &Style) {
        if !style.visible {
            return;
        }
        let node = &self.dom.nodes[idx];
        let Some(href) = node.href() else { return };
        let Some(bytes) = image_bytes(href, self.load_file) else {
            self.skipped += 1;
            return;
        };
        let Ok(img) = image::load_from_memory(&bytes) else {
            self.skipped += 1;
            return;
        };
        let dim = (img.width(), img.height());
        if dim.0 == 0 || dim.1 == 0 {
            return;
        }
        // Der ImageStore heißt nicht zufällig `png`: Export und Speichern
        // gehen davon aus. Fremde Formate werden einmal umgewandelt.
        let png = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            bytes
        } else {
            let mut out = std::io::Cursor::new(Vec::new());
            if img.write_to(&mut out, image::ImageFormat::Png).is_err() {
                self.skipped += 1;
                return;
            }
            out.into_inner()
        };

        let (vw, vh) = self.viewport;
        let x = self.num_attr(idx, "x", style, vw);
        let y = self.num_attr(idx, "y", style, vh);
        let w = node
            .attr("width")
            .and_then(|v| parse_length(v, style.font_size, vw))
            .unwrap_or(dim.0 as f32);
        let h = node
            .attr("height")
            .and_then(|v| parse_length(v, style.font_size, vh))
            .unwrap_or(dim.1 as f32);
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        // D: wo das ganze Bild liegt. V: was davon sichtbar ist.
        let par = node.attr("preserveAspectRatio").unwrap_or("").trim();
        let box_r = egui::Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
        let d = if par.starts_with("none") {
            box_r
        } else {
            let (iw, ih) = (dim.0 as f32, dim.1 as f32);
            let s = if par.contains("slice") {
                (w / iw).max(h / ih)
            } else {
                (w / iw).min(h / ih)
            };
            let (dw, dh) = (iw * s, ih * s);
            let (fx, fy) = align_factors(par);
            egui::Rect::from_min_size(
                Pos2::new(x + (w - dw) * fx, y + (h - dh) * fy),
                Vec2::new(dw, dh),
            )
        };
        let mut v = d.intersect(box_r);
        if let Some(clip) = self.clip_rect(idx) {
            v = v.intersect(clip);
        }
        if v.width() <= 0.0 || v.height() <= 0.0 {
            return;
        }

        let (rot, sx, sy) = m.decompose().unwrap_or((0.0, m.mean_scale(), m.mean_scale()));
        let id = self.id();
        let c = m.apply(v.center().x, v.center().y);
        let mut el = Element::new_image(id, 0, 0, dim.0, dim.1);
        el.w = v.width() * sx;
        el.h = v.height() * sy.abs();
        el.x = c.x - el.w / 2.0;
        el.y = c.y - el.h / 2.0;
        el.rotation = rot;
        el.crop = crate::model::Crop {
            x: (v.min.x - d.min.x) / d.width(),
            y: (v.min.y - d.min.y) / d.height(),
            w: v.width() / d.width(),
            h: v.height() / d.height(),
        }
        .clamp();
        self.images.push(ImportedImage { id, png, dim });
        self.elements.push(el);
    }

    /// Ein Schnittpfad, der aus genau einem ungedrehten Rechteck besteht —
    /// so schreibt BoxDoc selbst den Bildausschnitt. Alles andere wird
    /// ignoriert (die Form bleibt dann unbeschnitten).
    fn clip_rect(&self, idx: usize) -> Option<egui::Rect> {
        let node = &self.dom.nodes[idx];
        let mut val = node.attr("clip-path").map(str::to_string);
        if let Some(st) = node.attr("style") {
            if let Some((_, v)) = parse_declarations(st).into_iter().find(|(k, _)| k == "clip-path") {
                val = Some(v);
            }
        }
        let val = val?;
        let id = val
            .trim()
            .strip_prefix("url(")?
            .trim_end_matches(')')
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .strip_prefix('#')?
            .to_string();
        let cp = &self.dom.nodes[self.dom.by_id(&id)?];
        if cp.name != "clipPath"
            || cp.attr("transform").is_some()
            || cp.attr("clipPathUnits") == Some("objectBoundingBox")
        {
            return None;
        }
        let kids = cp.element_children();
        if kids.len() != 1 {
            return None;
        }
        let r = &self.dom.nodes[kids[0]];
        if r.name != "rect" || r.attr("transform").is_some() {
            return None;
        }
        let g = |k: &str| r.attr(k).and_then(|v| parse_length(v, 16.0, 0.0)).unwrap_or(0.0);
        Some(egui::Rect::from_min_size(
            Pos2::new(g("x"), g("y")),
            Vec2::new(g("width"), g("height")),
        ))
    }
}

/// Originaltext, Boxbreite (Nutzereinheiten) und Ausrichtung aus den
/// `data-boxdoc-*`-Attributen, die BoxDocs SVG-Export schreibt.
///
/// Nur, wenn der Originaltext noch zu dem passt, was im SVG zu sehen ist
/// (verglichen ohne Leerraum, denn der Umbruch hat ihn verschoben): Wurde der
/// Text inzwischen in Inkscape geändert, gewinnt die Änderung, und die
/// veralteten Angaben werden ignoriert.
fn boxdoc_text_layout(node: &Node, visible: &str) -> Option<(String, f32, TextAlign)> {
    let text = node.attr("data-boxdoc-text")?;
    let w: f32 = node.attr("data-boxdoc-w")?.trim().parse().ok()?;
    let align = match node.attr("data-boxdoc-align")? {
        "center" => TextAlign::Center,
        "right" => TextAlign::Right,
        _ => TextAlign::Left,
    };
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    (w > 0.0 && squash(text) == squash(visible)).then(|| (text.to_string(), w, align))
}

/// Zwischenstand beim Einsammeln eines `<text>`.
struct TextAcc {
    lines: Vec<String>,
    /// x-Anfang jeder Zeile (Nutzereinheiten), sobald sie Text hat.
    line_x: Vec<Option<f32>>,
    cur_y: f32,
    /// Position des ersten sichtbaren Zeichens.
    anchor_pos: Option<(f32, f32)>,
    style: Style,
}

fn rgba(c: [u8; 3], alpha: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (alpha.clamp(0.0, 1.0) * 255.0).round() as u8]
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !ws {
                out.push(' ');
            }
            ws = true;
        } else {
            out.push(ch);
            ws = false;
        }
    }
    out
}

/// Bilddaten zu einem `href`: data-URI (base64) oder eine Datei.
fn image_bytes(href: &str, load_file: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    let href = href.trim();
    if let Some(rest) = href.strip_prefix("data:") {
        let (meta, data) = rest.split_once(',')?;
        if !meta.ends_with(";base64") {
            return None; // URL-kodierte Daten — bei Rasterbildern unüblich
        }
        let clean: String = data.chars().filter(|c| !c.is_whitespace()).collect();
        use base64::Engine;
        return base64::engine::general_purpose::STANDARD
            .decode(clean.as_bytes())
            .ok();
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return None; // Nichts aus dem Netz nachladen.
    }
    load_file(href.strip_prefix("file://").unwrap_or(href))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_files(_: &str) -> Option<Vec<u8>> {
        None
    }

    fn import(svg: &str) -> SvgImport {
        parse(svg, 1, &no_files).expect("SVG sollte lesbar sein")
    }

    #[test]
    fn zahlen_in_kompakter_schreibweise() {
        assert_eq!(numbers("1.5.5-2"), vec![1.5, 0.5, -2.0]);
        assert_eq!(numbers("1e2,3E-1"), vec![100.0, 0.3]);
    }

    #[test]
    fn farben() {
        assert_eq!(parse_color("#abc", [0; 3]), Some(([0xaa, 0xbb, 0xcc], 1.0)));
        assert_eq!(parse_color("rgb(10, 20, 30)", [0; 3]), Some(([10, 20, 30], 1.0)));
        assert_eq!(parse_color("red", [0; 3]), Some(([255, 0, 0], 1.0)));
        let (c, a) = parse_color("rgba(0,0,0,0.5)", [0; 3]).unwrap();
        assert_eq!(c, [0, 0, 0]);
        assert!((a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn leinwand_in_pt_und_px() {
        let i = import(r#"<svg xmlns="http://www.w3.org/2000/svg" width="595pt" height="842pt" viewBox="0 0 595 842"/>"#);
        assert!((i.width - 595.0).abs() < 0.01 && (i.height - 842.0).abs() < 0.01);
        // 96 px = 1 Zoll = 72 pt
        let i = import(r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="48"/>"#);
        assert!((i.width - 72.0).abs() < 0.01 && (i.height - 36.0).abs() < 0.01);
    }

    #[test]
    fn rechteck_mit_drehung_bleibt_rechteck() {
        let i = import(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="200pt" height="200pt" viewBox="0 0 200 200">
              <rect x="10" y="20" width="100" height="50" rx="5" fill="#ff0000" fill-opacity="0.5"
                    stroke="blue" stroke-width="2" transform="rotate(30 60 45)"/>
            </svg>"##,
        );
        assert_eq!(i.elements.len(), 1);
        let el = &i.elements[0];
        assert_eq!(el.kind, ElementKind::Rectangle);
        assert!((el.x - 10.0).abs() < 0.01 && (el.y - 20.0).abs() < 0.01, "{el:?}");
        assert!((el.w - 100.0).abs() < 0.01 && (el.h - 50.0).abs() < 0.01);
        assert!((el.rotation - 30.0).abs() < 0.01);
        assert!((el.corner_radius - 5.0).abs() < 0.01);
        assert_eq!(el.fill_color, [255, 0, 0, 128]);
        assert_eq!(el.stroke_color, [0, 0, 255, 255]);
        assert!((el.stroke_width - 2.0).abs() < 0.01);
    }

    #[test]
    fn pfad_mit_kurven_und_bogen() {
        let i = import(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100pt" height="100pt" viewBox="0 0 100 100">
              <path d="M10 10 C 20 0 40 0 50 10 A 20 20 0 0 1 90 10 L 90 90 Z" fill="none" stroke="black"/>
            </svg>"#,
        );
        assert_eq!(i.elements.len(), 1);
        let el = &i.elements[0];
        assert_eq!(el.kind, ElementKind::Path);
        assert!(el.path_closed);
        assert!(el.path_is_curved());
        assert_eq!(el.handles.len(), el.points.len());
        // Box umschließt die Kurve: der Bogen geht bis y = -10 hoch
        // (Halbkreis um (70,10) mit r=20), unten bis 90.
        assert!(el.y < -9.0 && el.y > -11.0, "{}", el.y);
        assert!((el.y + el.h - 90.0).abs() < 0.1);
    }

    #[test]
    fn mehrere_teilpfade_werden_einzelne_pfade() {
        let i = import(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
              <path d="M0 0 L10 0 L10 10 Z M20 20 l10 0 l0 10 z"/>
            </svg>"#,
        );
        assert_eq!(i.elements.len(), 2);
        assert!(i.elements.iter().all(|e| e.path_closed));
    }

    #[test]
    fn offener_gefuellter_pfad_wird_geschlossen() {
        let i = import(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
              <path d="M0 0 L10 0 L10 10" fill="red"/>
            </svg>"#,
        );
        assert_eq!(i.elements.len(), 1);
        assert!(i.elements[0].path_closed);
    }

    #[test]
    fn css_klassen_und_gruppen_vererben() {
        let i = import(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
              <style>.a { fill: #00ff00 } #b { stroke: red; stroke-width: 3 }</style>
              <g opacity="0.5"><circle id="b" class="a" cx="50" cy="50" r="10"/></g>
            </svg>"#,
        );
        let el = &i.elements[0];
        assert_eq!(el.kind, ElementKind::Ellipse);
        assert_eq!(el.fill_color, [0, 255, 0, 128]);
        assert_eq!(el.stroke_color, [255, 0, 0, 128]);
    }

    #[test]
    fn text_mit_zeilen() {
        let i = import(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="200pt" height="200pt" viewBox="0 0 200 200">
              <text font-family="Lora, serif" font-size="12" font-weight="bold" text-anchor="middle">
                <tspan x="100" y="50">Erste Zeile</tspan>
                <tspan x="100" y="65">Zweite &amp; letzte</tspan>
              </text>
            </svg>"#,
        );
        let el = &i.elements[0];
        assert_eq!(el.kind, ElementKind::Text);
        assert_eq!(el.text, "Erste Zeile\nZweite & letzte");
        assert_eq!(el.font, "lora");
        assert!(el.bold);
        assert_eq!(el.align, TextAlign::Center);
        assert_eq!(i.text_fixes.len(), 1);
        assert!((i.text_fixes[0].anchor_pt.x - 100.0).abs() < 0.01);
        assert!((i.text_fixes[0].anchor_pt.y - 50.0).abs() < 0.01);
    }

    #[test]
    fn hintergrund_rechteck_wird_seitenhintergrund() {
        let i = import(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="595pt" height="842pt" viewBox="0 0 595 842">
              <rect x="0" y="0" width="595" height="842" fill="#fff6e6"/>
              <rect x="10" y="10" width="20" height="20"/>
            </svg>"##,
        );
        let (doc, _) = i.into_document("test");
        assert_eq!(doc.format, PaperFormat::A4);
        assert_eq!(doc.background, Some([0xff, 0xf6, 0xe6, 255]));
        assert_eq!(doc.pages[0].elements.len(), 1);
    }

    #[test]
    fn eigenes_format_bei_fremder_groesse() {
        let i = import(r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm"/>"#);
        let (doc, _) = i.into_document("Logo");
        match &doc.format {
            PaperFormat::Custom(c) => {
                assert_eq!(c.name, "Logo");
                assert!((c.w_mm - 100.0).abs() < 0.02 && (c.h_mm - 50.0).abs() < 0.02);
            }
            f => panic!("erwartet Custom, war {f:?}"),
        }
        assert_eq!(doc.background, None);
    }

    #[test]
    fn use_und_unsichtbares() {
        let i = import(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100">
              <defs><rect id="r" width="10" height="10"/></defs>
              <use xlink:href="#r" x="5" y="5"/>
              <use href="#r" x="50" y="50"/>
              <rect width="10" height="10" display="none"/>
            </svg>"##,
        );
        assert_eq!(i.elements.len(), 2);
        assert!((i.elements[1].x - 37.5).abs() < 0.01); // 50 px = 37,5 pt
    }

    #[test]
    fn ungueltiges_xml_wird_abgelehnt() {
        assert!(parse("<svg><rect></svg", 1, &no_files).is_err() || parse("kein svg", 1, &no_files).is_err());
        assert!(parse("<html/>", 1, &no_files).is_err());
    }
}
