//! SVG-Export — ganze Seiten oder eine Auswahl.
//!
//! Anders als der PDF-Export erzeugt dieses Modul **echte Vektorobjekte, die
//! ein Zeichenprogramm wieder auseinandernehmen kann**: Ein Rechteck bleibt
//! ein `<rect>`, eine Ellipse eine `<ellipse>`, eine Kurve ein `<path>` mit
//! kubischen Bézier-Segmenten. Nichts wird zu einem Vieleck aufgelöst.
//!
//! **Transparenz ist echt.** Der PDF-Export muss halbdurchsichtige Farben über
//! Weiß mischen, weil printpdf 0.7 keinen Zugriff auf `ExtGState` gibt (siehe
//! `printing::blend_over_white`). SVG kennt `fill-opacity` — hier bleibt eine
//! zu 23 % deckende Füllung zu 23 % deckend, und zwei überlappende Formen
//! scheinen durcheinander durch, genau wie auf dem Bildschirm.
//!
//! **Keine UI-Abhängigkeiten.** Das Modul erzeugt nur eine Zeichenkette; wer
//! sie wohin schreibt, entscheidet der Aufrufer. Dadurch ist der Export
//! vollständig testbar, und ein späterer Web-Export braucht kein zweites
//! Modul.

use std::collections::HashMap;

use crate::geometry;
use crate::model::{page_size_pt, Document, Element, ElementKind};
use crate::text_layout::TextLayout;

/// Rand um eine exportierte Auswahl, in Punkten.
///
/// Ohne Rand schneidet die Leinwand an der Kontur ab — bei einer dicken Linie
/// oder einem Pfad mit Rundungen sieht das aus wie ein Fehler.
pub const SELECTION_MARGIN: f32 = 8.0;

/// Was in die Datei soll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Eine ganze Seite: Leinwand ist das Seitenformat, mit weißem Grund.
    Page(usize),
    /// Nur diese Objekte einer Seite. Die Leinwand ist ihre gemeinsame
    /// Hüllbox plus [`SELECTION_MARGIN`], der Grund bleibt **durchsichtig** —
    /// eine exportierte Auswahl soll sich in ein anderes Dokument legen
    /// lassen, ohne einen weißen Kasten mitzubringen.
    Selection { page: usize, ids: Vec<u64> },
}

/// Baut das SVG-Dokument als Zeichenkette.
///
/// `layouts` kommt aus [`crate::printing::collect_layouts`] — dasselbe
/// Textlayout, das auch Bildschirm und PDF benutzen. Fehlt der Eintrag zu
/// einem Textelement, wird es übersprungen, statt mit geratenem Umbruch falsch
/// gesetzt zu werden.
pub fn svg_string(
    doc: &Document,
    images: &crate::store::ImageStore,
    layouts: &HashMap<u64, TextLayout>,
    scope: &Scope,
) -> Result<String, String> {
    let page_idx = match scope {
        Scope::Page(i) => *i,
        Scope::Selection { page, .. } => *page,
    };
    let page = doc
        .pages
        .get(page_idx)
        .ok_or_else(|| format!("Seite {} gibt es nicht", page_idx + 1))?;

    // Welche Elemente, und welche Leinwand?
    let (elements, view, opaque): (Vec<&Element>, egui::Rect, bool) = match scope {
        Scope::Page(_) => {
            let (w, h) = page_size_pt(doc.format, doc.orientation);
            (
                page.elements.iter().collect(),
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(w, h)),
                true,
            )
        }
        Scope::Selection { ids, .. } => {
            // Reihenfolge der Seite, nicht der Auswahl: Sonst hinge die
            // Stapelung davon ab, in welcher Reihenfolge angeklickt wurde.
            let els: Vec<&Element> = page
                .elements
                .iter()
                .filter(|el| ids.contains(&el.id))
                .collect();
            if els.is_empty() {
                return Err(String::from("Nichts ausgewählt."));
            }
            let b = geometry::elements_bounds(els.iter().copied())
                .ok_or_else(|| String::from("Nichts ausgewählt."))?;
            (els, b.expand(SELECTION_MARGIN), false)
        }
    };

    let mut out = String::with_capacity(4096);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" \
         width=\"{w}pt\" height=\"{h}pt\" viewBox=\"{vx} {vy} {w} {h}\">\n",
        w = n(view.width()),
        h = n(view.height()),
        vx = n(view.min.x),
        vy = n(view.min.y),
    ));
    out.push_str(&format!(
        "  <!-- BoxDoc {} -->\n",
        env!("CARGO_PKG_VERSION")
    ));
    if opaque {
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>\n",
            n(view.min.x),
            n(view.min.y),
            n(view.width()),
            n(view.height()),
        ));
    }

    for el in elements {
        match el.kind {
            ElementKind::Rectangle => rectangle(&mut out, el),
            ElementKind::Ellipse => ellipse(&mut out, el),
            ElementKind::Line => line(&mut out, el),
            ElementKind::Path => path(&mut out, el),
            ElementKind::Image => image(&mut out, el, images),
            ElementKind::Text => {
                if let Some(layout) = layouts.get(&el.id) {
                    text(&mut out, el, layout);
                }
            }
        }
    }

    out.push_str("</svg>\n");
    Ok(out)
}

/// Schreibt das SVG in eine Datei.
#[cfg(not(target_arch = "wasm32"))]
pub fn export_svg(
    path: &std::path::Path,
    doc: &Document,
    images: &crate::store::ImageStore,
    layouts: &HashMap<u64, TextLayout>,
    scope: &Scope,
) -> Result<(), String> {
    let svg = svg_string(doc, images, layouts, scope)?;
    std::fs::write(path, svg).map_err(|e| e.to_string())
}

// ===========================================================================
// Zahlen, Farben, Text
// ===========================================================================

/// Formatiert eine Zahl fürs SVG: höchstens drei Nachkommastellen, ohne
/// überflüssige Nullen.
///
/// Drei Stellen sind bei Punkten rund ein Tausendstel Punkt — weit unter allem,
/// was ein Drucker auflöst. Die Nullen wegzulassen halbiert die Dateigröße
/// eines pfadlastigen Dokuments beinahe.
fn n(v: f32) -> String {
    if !v.is_finite() {
        return String::from("0");
    }
    let s = format!("{v:.3}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    if s.is_empty() || s == "-0" {
        String::from("0")
    } else {
        s.to_string()
    }
}

fn hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Hängt `name` und `name-opacity` an, aber Letzteres nur, wenn die Farbe
/// wirklich durchscheinend ist — sonst bläht es jede Zeile auf.
fn paint(out: &mut String, name: &str, c: [u8; 4]) {
    out.push_str(&format!(" {name}=\"{}\"", hex(c)));
    if c[3] < 255 {
        out.push_str(&format!(
            " {name}-opacity=\"{}\"",
            n(c[3] as f32 / 255.0)
        ));
    }
}

/// Füll- und Kontur-Attribute einer Form.
///
/// `fillable` ist `false` für alles, was nie gefüllt wird (Linie, offener
/// Pfad) — dort muss ausdrücklich `fill="none"` stehen, weil SVG sonst
/// schwarz füllt.
fn shape_attrs(el: &Element, fillable: bool) -> String {
    let mut s = String::new();
    if fillable && el.fill_color[3] > 0 {
        paint(&mut s, "fill", el.fill_color);
    } else {
        s.push_str(" fill=\"none\"");
    }
    if el.stroke_color[3] > 0 && el.stroke_width > 0.0 {
        paint(&mut s, "stroke", el.stroke_color);
        s.push_str(&format!(" stroke-width=\"{}\"", n(el.stroke_width)));
    }
    s
}

/// Dreh-Transformation um den Elementmittelpunkt.
///
/// SVG dreht `rotate(a cx cy)` bei positivem `a` im Uhrzeigersinn — im
/// y-nach-unten-System dasselbe wie [`geometry::rotate_vec`]. Deshalb geht der
/// Winkel unverändert durch, ohne Vorzeichenwechsel.
fn rotation(el: &Element) -> String {
    if el.rotation.abs() < 0.001 {
        return String::new();
    }
    let c = geometry::element_center(el);
    format!(
        " transform=\"rotate({} {} {})\"",
        n(el.rotation),
        n(c.x),
        n(c.y)
    )
}

/// Maskiert die fünf Zeichen, die in XML-Text oder -Attributen nicht roh
/// stehen dürfen.
///
/// Ohne das macht ein Text wie `a < b & "c"` die Datei unlesbar — und zwar
/// still: Viewer zeigen dann gar nichts an statt einer Fehlermeldung.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

// ===========================================================================
// Die einzelnen Element-Arten
// ===========================================================================

fn rectangle(out: &mut String, el: &Element) {
    let r = el
        .corner_radius
        .max(0.0)
        .min(el.w.abs() / 2.0)
        .min(el.h.abs() / 2.0);
    out.push_str(&format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
        n(el.x),
        n(el.y),
        n(el.w),
        n(el.h)
    ));
    if r > 0.0 {
        out.push_str(&format!(" rx=\"{}\"", n(r)));
    }
    out.push_str(&shape_attrs(el, true));
    out.push_str(&rotation(el));
    out.push_str("/>\n");
}

fn ellipse(out: &mut String, el: &Element) {
    let c = geometry::element_center(el);
    out.push_str(&format!(
        "  <ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"",
        n(c.x),
        n(c.y),
        n(el.w.abs() / 2.0),
        n(el.h.abs() / 2.0)
    ));
    out.push_str(&shape_attrs(el, true));
    out.push_str(&rotation(el));
    out.push_str("/>\n");
}

/// Linie zwischen ihren beiden Endpunkten.
///
/// [`geometry::line_endpoints`] liefert sie bereits gedreht, deshalb steht hier
/// **keine** Transformation — sonst würde die Drehung zweimal angewandt.
fn line(out: &mut String, el: &Element) {
    if el.stroke_width <= 0.0 || el.stroke_color[3] == 0 {
        return;
    }
    let (a, b) = geometry::line_endpoints(el);
    out.push_str(&format!(
        "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"",
        n(a.x),
        n(a.y),
        n(b.x),
        n(b.y)
    ));
    let mut s = String::new();
    paint(&mut s, "stroke", el.stroke_color);
    out.push_str(&s);
    out.push_str(&format!(
        " stroke-width=\"{}\" stroke-linecap=\"round\"",
        n(el.stroke_width)
    ));
    out.push_str("/>\n");
}

/// Pfad als echte kubische Bézier-Kurve.
///
/// Das ist der Grund, warum dieses Modul nicht einfach `path_outline`
/// benutzt: Eine importierte Rundung soll im SVG eine Rundung sein und in
/// Illustrator oder Inkscape mit ihren vier Griffen wieder auftauchen — nicht
/// als Vieleck mit zweihundert Ecken.
///
/// Wie bei der Linie kommen die Punkte fertig gedreht aus `geometry`.
fn path(out: &mut String, el: &Element) {
    let Some((start, segs)) = geometry::path_segments(el) else {
        return;
    };
    let mut d = format!("M {} {}", n(start.x), n(start.y));
    for seg in segs {
        match seg {
            geometry::PathSeg::Line(p) => {
                d.push_str(&format!(" L {} {}", n(p.x), n(p.y)));
            }
            geometry::PathSeg::Cubic(c1, c2, p) => {
                d.push_str(&format!(
                    " C {} {} {} {} {} {}",
                    n(c1.x),
                    n(c1.y),
                    n(c2.x),
                    n(c2.y),
                    n(p.x),
                    n(p.y)
                ));
            }
        }
    }
    if el.path_closed {
        d.push_str(" Z");
    }
    out.push_str(&format!("  <path d=\"{d}\""));
    // Ein offener Pfad wird nie gefüllt — dieselbe Regel wie im PDF-Export
    // und auf dem Bildschirm.
    out.push_str(&shape_attrs(el, el.path_closed));
    out.push_str(" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\n");
}

/// Textblock als `<text>` mit einem `<tspan>` je Zeile.
///
/// Umbruch, Ausrichtung und Grundlinien kommen aus dem vorberechneten Layout.
/// Dieses Modul rechnet bewusst nichts selbst: Jede eigene Schätzung wäre eine
/// neue Quelle für Abweichungen zwischen Bildschirm, PDF und SVG.
fn text(out: &mut String, el: &Element, layout: &TextLayout) {
    let lines: Vec<&crate::text_layout::LaidLine> = layout
        .lines
        .iter()
        .filter(|l| !l.text.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return;
    }

    out.push_str(&format!(
        "  <text font-family=\"{}\" font-size=\"{}\"",
        esc(&svg_font_family(&el.font)),
        n(el.font_size)
    ));
    let mut s = String::new();
    paint(&mut s, "fill", el.color);
    out.push_str(&s);
    if el.bold {
        out.push_str(" font-weight=\"bold\"");
    }
    if el.italic {
        out.push_str(" font-style=\"italic\"");
    }
    // `text-decoration` nimmt beide Werte in einem Attribut auf; zweimal
    // notiert gewönne nur das letzte.
    match (el.underline, el.strikethrough) {
        (true, true) => out.push_str(" text-decoration=\"underline line-through\""),
        (true, false) => out.push_str(" text-decoration=\"underline\""),
        (false, true) => out.push_str(" text-decoration=\"line-through\""),
        (false, false) => {}
    }
    out.push_str(&rotation(el));
    out.push_str(">\n");

    for laid in lines {
        out.push_str(&format!(
            "    <tspan x=\"{}\" y=\"{}\">{}</tspan>\n",
            n(el.x + laid.x),
            n(el.y + laid.baseline_y),
            esc(&laid.text)
        ));
    }
    out.push_str("  </text>\n");
}

/// Schriftfamilie für das SVG.
///
/// Die Datei enthält die Schrift **nicht**; sie nennt sie nur. Deshalb bekommt
/// jeder Name eine generische Rückfallebene angehängt — öffnet jemand das SVG
/// auf einem Rechner ohne Cambria, soll dort eine Serifenschrift stehen und
/// nicht der Systemstandard.
fn svg_font_family(key: &str) -> String {
    if key.is_empty() || key == "default" {
        return String::from("sans-serif");
    }
    let display = crate::model::FONT_CHOICES
        .iter()
        .find(|f| f.key == key)
        .map(|f| f.display)
        .unwrap_or(key);
    let generic = match key {
        "lora" | "cambria" | "georgia" | "times" | "garamond" | "bookantiqua" => "serif",
        "jetbrains" | "consolas" | "couriernew" => "monospace",
        "pacifico" => "cursive",
        _ => "sans-serif",
    };
    format!("{display}, {generic}")
}

/// Bild als eingebettetes PNG (data-URI).
///
/// **Crop ohne Neuberechnung der Pixel:** Statt das Bild zuzuschneiden — wie es
/// der PDF-Export tun muss — wird das *ganze* PNG so groß und so versetzt
/// platziert, dass der sichtbare Ausschnitt genau die Element-Box füllt, und
/// dann auf die Box beschnitten. Die Bilddaten bleiben dabei unangetastet: Der
/// Ausschnitt ist im SVG nachträglich wieder verschiebbar, und es geht keine
/// Auflösung durch Umrechnen verloren.
fn image(out: &mut String, el: &Element, images: &crate::store::ImageStore) {
    let Some(entry) = images.map.get(&el.id) else {
        return;
    };
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(&entry.png);

    // Nenner absichern: Ein Crop der Breite 0 wäre eine Division durch null
    // und käme als unsichtbares Element mit `NaN`-Koordinaten heraus.
    let cw = if el.crop.w > 1e-6 { el.crop.w } else { 1.0 };
    let ch = if el.crop.h > 1e-6 { el.crop.h } else { 1.0 };
    let full_w = el.w / cw;
    let full_h = el.h / ch;
    let ix = el.x - el.crop.x / cw * el.w;
    let iy = el.y - el.crop.y / ch * el.h;

    let cropped = cw < 0.999 || ch < 0.999 || el.crop.x > 0.001 || el.crop.y > 0.001;
    let clip_id = format!("crop{}", el.id);

    out.push_str(&format!("  <g{}>\n", rotation(el)));
    if cropped {
        out.push_str(&format!(
            "    <defs><clipPath id=\"{clip_id}\">\
             <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\
             </clipPath></defs>\n",
            n(el.x),
            n(el.y),
            n(el.w),
            n(el.h)
        ));
    }
    out.push_str("    <image");
    if cropped {
        out.push_str(&format!(" clip-path=\"url(#{clip_id})\""));
    }
    out.push_str(&format!(
        " x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" \
         href=\"data:image/png;base64,{data}\"/>\n",
        n(ix),
        n(iy),
        n(full_w),
        n(full_h)
    ));
    out.push_str("  </g>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zahlen_verlieren_ihre_nullen() {
        assert_eq!(n(1.0), "1");
        assert_eq!(n(1.5), "1.5");
        assert_eq!(n(1.23456), "1.235");
        assert_eq!(n(0.0), "0");
        assert_eq!(n(-0.0001), "0", "-0 wäre gültig, sieht aber nach Fehler aus");
        assert_eq!(n(f32::NAN), "0");
    }

    #[test]
    fn sonderzeichen_werden_maskiert() {
        assert_eq!(esc("a < b & \"c\""), "a &lt; b &amp; &quot;c&quot;");
    }

    #[test]
    fn deckende_farbe_bekommt_kein_opacity_attribut() {
        let mut s = String::new();
        paint(&mut s, "fill", [17, 34, 51, 255]);
        assert_eq!(s, " fill=\"#112233\"");
    }

    #[test]
    fn halbdurchsichtige_farbe_behaelt_ihre_transparenz() {
        // Der PDF-Export muss über Weiß mischen; SVG kann es richtig.
        let mut s = String::new();
        paint(&mut s, "fill", [17, 34, 51, 128]);
        assert!(s.contains("fill-opacity=\"0.502\""), "{s}");
    }

    #[test]
    fn unbekannte_schrift_faellt_auf_sans_serif_zurueck() {
        assert_eq!(svg_font_family("default"), "sans-serif");
        assert_eq!(svg_font_family(""), "sans-serif");
        assert!(svg_font_family("lora").ends_with(", serif"));
        assert!(svg_font_family("jetbrains").ends_with(", monospace"));
    }
}
