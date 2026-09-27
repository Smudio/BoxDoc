//! PDF-Export und Drucken.
//!
//! Text wird vektoriell ausgegeben (mit eingebetteter Systemschrift, damit auch
//! Umlaute korrekt sind). Bilder werden zugeschnitten, gedreht und gerastert
//! ausgegeben, sodass Drehung und Crop exakt dem Bildschirm entsprechen.
//! Transparenz bleibt dabei erhalten: Der Alphakanal geht als Soft-Mask
//! (`/SMask`) mit ins PDF, statt gegen Weiß verrechnet zu werden.

#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::BufWriter;

use image::{ImageBuffer, RgbaImage};
use printpdf::{
    path::{PaintMode, WindingOrder},
    BuiltinFont, Color, ColorBits, ColorSpace, Image, ImageTransform, ImageXObject, IndirectFontRef,
    Line, Mm, PdfDocument, PdfDocumentReference, PdfLayerReference, Point, Polygon, Px, Rgb, SMask,
};

use crate::model::{Document, Element, ElementKind, FontStyle};

type E = Box<dyn std::error::Error>;

fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

/// Layoutet alle Textelemente des Dokuments vorab mit dem Schriftsystem der
/// GUI. Genau dieselbe Funktion nutzt der Canvas zum Zeichnen — dadurch stimmen
/// Bildschirm und PDF per Konstruktion überein.
///
/// Schlüssel ist die Element-ID, weil sie über das ganze Dokument eindeutig ist.
pub fn collect_layouts(
    ctx: &egui::Context,
    doc: &Document,
) -> std::collections::HashMap<u64, crate::text_layout::TextLayout> {
    let mut map = std::collections::HashMap::new();
    ctx.fonts_mut(|fonts| {
        for page in &doc.pages {
            for el in &page.elements {
                if el.kind == ElementKind::Text {
                    map.insert(el.id, crate::text_layout::layout(fonts, el, 1.0));
                }
            }
        }
    });
    map
}

/// Baut das PDF und gibt es als Bytes zurück.
///
/// Der eigentliche Export steckt hier und nicht in `export_pdf`, weil es das
/// Ziel in zwei Varianten gibt: Native schreibt eine Datei, der Browser bietet
/// einen Download an. Beide Wege sollen aber dieselben Bytes erzeugen — sonst
/// weicht das PDF aus der Web-Version irgendwann unbemerkt von dem der EXE ab.
pub fn pdf_bytes(
    doc: &Document,
    images: &crate::store::ImageStore,
    layouts: &std::collections::HashMap<u64, crate::text_layout::TextLayout>,
) -> Result<Vec<u8>, E> {
    let (pw_pt, ph_pt) = doc.page_size_pt();
    let (pw_mm, ph_mm) = (pt_to_mm(pw_pt), pt_to_mm(ph_pt));

    let (document, first_page, first_layer) =
        PdfDocument::new("BoxDoc", Mm(pw_mm), Mm(ph_mm), "Ebene 1");
    // Fallback-Schrift (wird verwendet, wenn eine Element-Schrift fehlt).
    let fallback_font = system_font(&document)?;
    // Cache: Schrift-Schlüssel + Schnitt → eingebetteter Font.
    let mut font_cache: std::collections::HashMap<String, ResolvedFont> =
        std::collections::HashMap::new();

    for (pi, page) in doc.pages.iter().enumerate() {
        let (page_idx, layer_idx) = if pi == 0 {
            (first_page, first_layer)
        } else {
            document.add_page(Mm(pw_mm), Mm(ph_mm), "Ebene 1")
        };
        let layer = document.get_page(page_idx).get_layer(layer_idx);
        // Seitenhintergrund als unterste Fläche. "Kein Hintergrund" (None)
        // lässt das Papier weiß; eine Farbe wird inklusive Deckkraft
        // (über Weiß gemischt, wie bei Formen) ganzseitig gefüllt.
        if let Some(bg) = doc.background {
            let mut bg_el = Element::new_rectangle(0, 0.0, 0.0);
            bg_el.w = pw_pt;
            bg_el.h = ph_pt;
            bg_el.fill_color = bg;
            bg_el.stroke_width = 0.0;
            draw_rectangle(&layer, &bg_el, ph_mm);
        }
        for el in &page.elements {
            match el.kind {
                ElementKind::Text => {
                    let font = resolve_text_font(
                        &document,
                        &el.font,
                        FontStyle::of(el),
                        &fallback_font,
                        &mut font_cache,
                    );
                    // Ohne vorberechnetes Layout wird der Text übersprungen,
                    // statt ihn falsch (unumgebrochen) zu setzen.
                    if let Some(layout) = layouts.get(&el.id) {
                        draw_text(&layer, el, ph_mm, &font, layout);
                    }
                }
                ElementKind::Image => draw_image(&layer, el, ph_mm, images),
                ElementKind::Rectangle => draw_rectangle(&layer, el, ph_mm),
                ElementKind::Line => draw_line(&layer, el, ph_mm),
                ElementKind::Ellipse => draw_ellipse(&layer, el, ph_mm),
                ElementKind::Path => draw_path(&layer, el, ph_mm),
            }
        }
    }

    Ok(document.save_to_bytes()?)
}

/// Schreibt das PDF in eine Datei (Native).
#[cfg(not(target_arch = "wasm32"))]
pub fn export_pdf(
    path: &std::path::Path,
    doc: &Document,
    images: &crate::store::ImageStore,
    layouts: &std::collections::HashMap<u64, crate::text_layout::TextLayout>,
) -> Result<(), E> {
    use std::io::Write;
    let bytes = pdf_bytes(doc, images, layouts)?;
    BufWriter::new(File::create(path)?).write_all(&bytes)?;
    Ok(())
}

/// Ermittelt die Fallback-Schrift für den Export.
///
/// Gibt einen Fehler zurück, statt zu panicken: Ein fehlgeschlagener PDF-Export
/// darf niemals die Anwendung mitsamt dem ungespeicherten Dokument abreißen.
/// Im Browser gibt es keine Systemschriften auf einer Platte. Dort ist
/// `default_font` (die in der Binary mitgelieferte egui-Schrift) der einzige
/// sinnvolle Fallback — und der bessere: sie ist genau die, die der Nutzer auf
/// dem Bildschirm sieht.
fn system_font(doc: &PdfDocumentReference) -> Result<IndirectFontRef, E> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let candidates: [&str; 4] = [
            "C:\\Windows\\Fonts\\arial.ttf",
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
        ];
        for c in candidates {
            if let Ok(f) = File::open(c) {
                if let Ok(font) = doc.add_external_font(f) {
                    return Ok(font);
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(font) = default_font(doc) {
            return Ok(font);
        }
    }
    doc.add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| -> E { format!("keine verwendbare Schrift gefunden: {e}").into() })
}

/// Lädt den Schnitt `style` der zu `key` gehörenden Schrift aus
/// `FONT_CHOICES`. `None`, wenn dieser Schnitt hier nicht beschaffbar ist.
fn load_font_by_key(
    doc: &PdfDocumentReference,
    key: &str,
    style: FontStyle,
) -> Option<IndirectFontRef> {
    let def = crate::model::FONT_CHOICES.iter().find(|f| f.key == key)?;

    // Eingebettete Schriften stehen in der Binary, nicht auf der Platte.
    //
    // Der Export suchte sie früher trotzdem über `paths` — eine Liste, die bei
    // genau diesen Schriften leer ist. Er fand also nie eine, fiel auf die
    // Fallback-Schrift zurück und setzte jedes Dokument in Arial. Im
    // Browser-Build, wo es *nur* diese Schriften gibt, betraf das alles.
    if def.bundled {
        // Nur der Regular-Schnitt ist eingebettet; der Rest wird nachgeahmt.
        if style != FontStyle::Regular {
            return None;
        }
        return doc.add_external_font(crate::fonts::bundled_bytes(key)?).ok();
    }

    // Nicht eingebettete Schriften liegen als Datei auf der Platte. Im Browser
    // gibt es die nicht — dort greift der Fallback des Aufrufers.
    #[cfg(target_arch = "wasm32")]
    {
        let _ = style;
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = def
            .paths_for(style)
            .iter()
            .find(|p| std::fs::metadata(p).is_ok())?;
        let f = File::open(path).ok()?;
        doc.add_external_font(f).ok()
    }
}

/// Bettet die Schrift ein, die egui für `FontFamily::Proportional` benutzt —
/// also die, die der Nutzer als "Standard" auf dem Bildschirm sieht.
///
/// `FontDefinitions::default()` trägt die Bytes der eingebauten Schriften bei
/// sich; wir nehmen den ersten Eintrag der Proportional-Familie, denn genau
/// den nimmt auch das Shaping. `None`, wenn egui ohne eingebaute Schriften
/// gebaut wurde.
fn default_font(doc: &PdfDocumentReference) -> Option<IndirectFontRef> {
    let defs = egui::FontDefinitions::default();
    let name = defs
        .families
        .get(&egui::FontFamily::Proportional)?
        .first()?
        .clone();
    let bytes = defs.font_data.get(&name)?.font.to_vec();
    doc.add_external_font(&bytes[..]).ok()
}

/// Eine für den Export aufgelöste Schrift — und was an ihr noch nachgeahmt
/// werden muss.
#[derive(Clone)]
struct ResolvedFont {
    font: IndirectFontRef,
    /// Kein echter Fett-Schnitt vorhanden: über einen Umriss nachahmen.
    synth_bold: bool,
    /// Kein echter Kursiv-Schnitt vorhanden: über die Textmatrix scheren.
    synth_italic: bool,
}

/// Wählt den Schrift-Slot passend zum Schnitt und meldet mit, was daran
/// nachgeahmt werden muss. Gecacht, damit jede Schrift nur einmal pro Dokument
/// eingebettet wird; Schlüssel ist `<font>|<schnitt>`.
///
/// Vorrang hat immer der **echte** Schnitt (`arialbd.ttf` für fettes Arial):
/// Er hat eigene Glyphenformen und eigene Breiten, und genau mit diesen
/// Breiten hat `text_layout` den Umbruch bereits gerechnet. Ein nachgeahmter
/// Fettdruck wäre an denselben Umbruchstellen zu breit.
fn resolve_text_font(
    doc: &PdfDocumentReference,
    font_key: &str,
    style: FontStyle,
    fallback: &IndirectFontRef,
    cache: &mut std::collections::HashMap<String, ResolvedFont>,
) -> ResolvedFont {
    let cache_key = format!("{font_key}|{}", style.suffix());
    if let Some(f) = cache.get(&cache_key) {
        return f.clone();
    }

    // Für "default" wird **genau die Schrift eingebettet, mit der egui auf dem
    // Bildschirm zeichnet**.
    //
    // Vorher stand hier Helvetica. Das ist eine andere Schrift mit anderen
    // Zeichenbreiten als die, mit der `text_layout` den Umbruch gerechnet hat:
    // Der Text lief im PDF ein Stück über seine gemessene Breite hinaus, und
    // die Unterstreichung — die genau diese gemessene Breite bekommt — endete
    // sichtbar vor dem letzten Buchstaben. Mit derselben Schriftdatei auf
    // beiden Seiten stimmen Breiten und Umbruch per Konstruktion.
    let resolved = if font_key == "default" || font_key.is_empty() {
        match default_font(doc) {
            // Kein echter Fett-/Kursiv-Schnitt vorhanden — genau wie auf dem
            // Bildschirm (siehe `fonts::has_style`), also beidseitig nachahmen.
            Some(font) => ResolvedFont {
                font,
                synth_bold: style.bold(),
                synth_italic: style.italic(),
            },
            // Ohne eingebaute egui-Schriften bleibt Helvetica; dessen echte
            // Schnitte sind dann das kleinere Übel gegenüber Nachahmung.
            None => {
                let variant = match style {
                    FontStyle::BoldItalic => BuiltinFont::HelveticaBoldOblique,
                    FontStyle::Bold => BuiltinFont::HelveticaBold,
                    FontStyle::Italic => BuiltinFont::HelveticaOblique,
                    FontStyle::Regular => BuiltinFont::Helvetica,
                };
                ResolvedFont {
                    font: doc
                        .add_builtin_font(variant)
                        .unwrap_or_else(|_| fallback.clone()),
                    synth_bold: false,
                    synth_italic: false,
                }
            }
        }
    } else if let Some(font) = load_font_by_key(doc, font_key, style) {
        // Echter Schnitt gefunden — nichts nachzuahmen.
        ResolvedFont {
            font,
            synth_bold: false,
            synth_italic: false,
        }
    } else {
        // Kein echter Schnitt: Regular einbetten und den Rest nachahmen.
        ResolvedFont {
            font: load_font_by_key(doc, font_key, FontStyle::Regular)
                .unwrap_or_else(|| fallback.clone()),
            synth_bold: style.bold(),
            synth_italic: style.italic(),
        }
    };

    cache.insert(cache_key, resolved.clone());
    resolved
}

/// Zeichnet ein Textelement anhand des **vorberechneten** Layouts.
///
/// Die Zeilen, ihre Breiten und ihre Grundlinien kommen aus
/// `crate::text_layout` — demselben Code, den der Canvas benutzt. Dieses
/// Modul rechnet bewusst nichts mehr selbst aus: Jede eigene Schätzung wäre
/// eine neue Quelle für Abweichungen zwischen Bildschirm und PDF.
fn draw_text(
    layer: &PdfLayerReference,
    el: &Element,
    page_h_mm: f32,
    resolved: &ResolvedFont,
    layout: &crate::text_layout::TextLayout,
) {
    // Textfarbe inkl. Alpha (siehe blend_over_white). r/g/b werden weiter
    // unten fuer den Synth-Bold-Umriss und die Auszeichnungslinien gebraucht.
    let blended = blend_over_white(el.color);
    let (r, g, b) = (blended.r, blended.g, blended.b);
    let font = &resolved.font;

    // Glyphen und Auszeichnungslinien laufen in **zwei getrennten Durchgängen**.
    //
    // Vorher standen sie ineinander: Der Unterstrich der ersten Zeile setzte
    // die Strichstärke auf seinen eigenen Wert, und ab der zweiten Zeile trug
    // der nachgeahmte Fettdruck damit fünfmal zu dick auf. Getrennte
    // Durchgänge haben je einen eigenen Grafikzustand, der den anderen nicht
    // mehr überschreiben kann.
    layer.save_graphics_state();
    layer.set_fill_color(Color::Rgb(blended));
    if resolved.synth_bold {
        // Synth-Bold: schmaler Strich um die Glyphen. ~3 % der font_size ist
        // ein typischer Wert für „faux bold".
        layer.set_outline_color(Color::Rgb(Rgb::new(r, g, b, None)));
        layer.set_outline_thickness((el.font_size * 0.03).max(0.2));
        // Text-Render-Mode 2 = Fill + Stroke → ergibt fett.
        layer.set_text_rendering_mode(printpdf::TextRenderingMode::FillStroke);
    }

    let shear = 12.0f32.to_radians().tan();
    for laid in &layout.lines {
        if laid.text.is_empty() {
            continue;
        }
        // PDF-Koordinaten: y zeigt nach oben, Ursprung unten links.
        let pdf_y_mm = page_h_mm - pt_to_mm(el.y + laid.baseline_y);
        let pdf_x_mm = pt_to_mm(el.x + laid.x);

        if resolved.synth_italic {
            // Scherung über die Text-Matrix. PDF-Text-Matrix:
            //   [a b c d e f]  →  a=1, b=0, c=tan(12°), d=1, e=x_pt, f=y_pt.
            // Positives c schert nach rechts (italic-Look). Position in Pt
            // (PDF-interne Einheit für die Text-Matrix).
            let x_pt = pdf_x_mm * 72.0 / 25.4;
            let y_pt = pdf_y_mm * 72.0 / 25.4;
            layer.begin_text_section();
            layer.set_font(font, el.font_size);
            layer.set_text_matrix(printpdf::TextMatrix::Raw([
                1.0, 0.0, shear, 1.0, x_pt, y_pt,
            ]));
            layer.write_text(laid.text.clone(), font);
            layer.end_text_section();
        } else {
            layer.use_text(
                laid.text.clone(),
                el.font_size,
                Mm(pdf_x_mm),
                Mm(pdf_y_mm),
                font,
            );
        }
    }
    layer.restore_graphics_state();

    if el.underline || el.strikethrough {
        draw_text_decorations(layer, el, page_h_mm, layout, Rgb::new(r, g, b, None));
    }
}

/// Zieht Unter- und Durchstreichung unter bzw. durch die bereits gesetzten
/// Zeilen.
///
/// Lage und Stärke kommen aus `text_layout::decoration_metrics` — demselben
/// Ort, aus dem Canvas und SVG-Export sie holen. Die Farbe wird hier
/// **ausdrücklich** gesetzt: Die Linienfarbe ist Teil des Seitenzustands, und
/// ohne eigene Zuweisung erbte ein Unterstrich schlicht die Randfarbe des
/// zuletzt gezeichneten Rechtecks.
fn draw_text_decorations(
    layer: &PdfLayerReference,
    el: &Element,
    page_h_mm: f32,
    layout: &crate::text_layout::TextLayout,
    color: Rgb,
) {
    let metrics = crate::text_layout::decoration_metrics(el.font_size);

    layer.save_graphics_state();
    layer.set_outline_color(Color::Rgb(color));
    layer.set_outline_thickness(metrics.thickness);

    for laid in &layout.lines {
        if laid.text.is_empty() {
            continue;
        }
        let x0 = pt_to_mm(el.x + laid.x);
        let x1 = pt_to_mm(el.x + laid.x + laid.width);
        // Auf der Seite zeigt y nach unten, im PDF nach oben — der Unterstrich
        // liegt unter der Grundlinie, also im PDF bei kleinerem y.
        let baseline_mm = page_h_mm - pt_to_mm(el.y + laid.baseline_y);

        let rule = |dy_mm: f32| {
            layer.add_line(Line {
                points: vec![
                    (Point::new(Mm(x0), Mm(baseline_mm + dy_mm)), false),
                    (Point::new(Mm(x1), Mm(baseline_mm + dy_mm)), false),
                ],
                is_closed: false,
            });
        };
        if el.underline {
            rule(-pt_to_mm(metrics.underline_dy));
        }
        if el.strikethrough {
            rule(pt_to_mm(metrics.strike_dy));
        }
    }
    layer.restore_graphics_state();
}

fn draw_image(
    layer: &PdfLayerReference,
    el: &Element,
    page_h_mm: f32,
    images: &crate::store::ImageStore,
) {
    let Some(entry) = images.map.get(&el.id) else { return };
    let Ok(dyn_img) = image::load_from_memory(&entry.png) else { return };
    let rgba = dyn_img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    if w == 0 || h == 0 {
        return;
    }
    let cx = ((el.crop.x * w as f32).round() as u32).min(w - 1);
    let cy = ((el.crop.y * h as f32).round() as u32).min(h - 1);
    let cw = (((el.crop.w * w as f32).round() as u32).max(1)).min(w - cx);
    let ch = (((el.crop.h * h as f32).round() as u32).max(1)).min(h - cy);
    let cropped = image::imageops::crop_imm(&rgba, cx, cy, cw, ch).to_image();
    let rotated = rotate_rgba(&cropped, el.rotation);

    let rad = el.rotation.to_radians();
    let (s, c) = (rad.sin(), rad.cos());
    let w_mm = pt_to_mm(el.w);
    let h_mm = pt_to_mm(el.h);
    let bbw = (w_mm * c.abs() + h_mm * s.abs()).abs();
    let bbh = (w_mm * s.abs() + h_mm * c.abs()).abs();

    let center_x = pt_to_mm(el.x + el.w / 2.0);
    let center_y = page_h_mm - pt_to_mm(el.y + el.h / 2.0);
    let tx = center_x - bbw / 2.0;
    let ty = center_y - bbh / 2.0;

    // Transparenz bleibt Transparenz: die Farbkanaele gehen unveraendert (also
    // *nicht* gegen Weiss verrechnet) ins PDF, der Alphakanal wird als
    // Soft-Mask mitgegeben. Damit deckt ein PNG mit Transparenz das ab, was
    // darunter liegt, nicht mehr weiss zu.
    let (rw, rh) = (rotated.width(), rotated.height());
    let mut rgb = Vec::with_capacity((rw * rh * 3) as usize);
    let mut alpha = Vec::with_capacity((rw * rh) as usize);
    for px in rotated.pixels() {
        rgb.push(px[0]);
        rgb.push(px[1]);
        rgb.push(px[2]);
        alpha.push(px[3] as i64);
    }
    let opaque = alpha.iter().all(|a| *a == 255);
    let smask = (!opaque).then(|| SMask {
        width: rw as i64,
        height: rh as i64,
        interpolate: false,
        bits_per_component: 8,
        matte: alpha,
    });

    let xobj = ImageXObject {
        width: Px(rw as usize),
        height: Px(rh as usize),
        color_space: ColorSpace::Rgb,
        bits_per_component: ColorBits::Bit8,
        interpolate: true,
        image_data: rgb,
        image_filter: None,
        clipping_bbox: None,
        smask,
    };
    let img = Image::from(xobj);

    let nat_w_mm = rw as f32 / 300.0 * 25.4;
    let nat_h_mm = rh as f32 / 300.0 * 25.4;
    let sx = if nat_w_mm > 0.0 { bbw / nat_w_mm } else { 1.0 };
    let sy = if nat_h_mm > 0.0 { bbh / nat_h_mm } else { 1.0 };

    img.add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(Mm(tx)),
            translate_y: Some(Mm(ty)),
            rotate: None,
            scale_x: Some(sx),
            scale_y: Some(sy),
            dpi: Some(300.0),
        },
    );
}

/// Zeichnet ein Rechteck (optional mit Eckradius) als geschlossenen Pfad.
/// PDF-Koordinatensystem: y zeigt nach OBEN. Rotation (BoxDoc: Grad gegen
/// Uhrzeigersinn, y nach unten) wird durch Rotation der Stützpunkte um das
/// Zentrum aufgeprägt.
/// Rechnet einen Punkt aus BoxDoc-Seitenkoordinaten (pt, y nach unten) in
/// PDF-Koordinaten um (mm, y nach oben).
///
/// Das ist die **einzige** Stelle, an der der PDF-Export ein Koordinatensystem
/// wechselt. Alle Formen kommen fertig aus `geometry` — dort in genau dem
/// System, in dem auch der Canvas rechnet.
fn to_pdf(p: egui::Pos2, page_h_mm: f32) -> Point {
    Point::new(Mm(pt_to_mm(p.x)), Mm(page_h_mm - pt_to_mm(p.y)))
}

/// Wandelt eine RGBA-Farbe in eine deckende PDF-Farbe um, indem sie über den
/// weißen Seitenhintergrund gemischt wird.
///
/// **Warum gemischt und nicht echt transparent:** printpdf 0.7 bietet keinen
/// Zugriff auf die PDF-Transparenz (`ExtGState` mit `ca`/`CA`) — die
/// Alpha-Werte im `ExtendedGraphicsState` sind zwar vorhanden, aber
/// `PdfLayerReference` stellt keine Methode bereit, sie zu setzen.
///
/// Vorher wurde Alpha deshalb einfach **ignoriert**: Eine zu 23 % deckende
/// Füllung kam im PDF knallig deckend heraus. Da BoxDoc-Seiten immer weiß
/// sind, liefert das Mischen über Weiß für Formen auf der Seite exakt das
/// Bild, das auch der Canvas zeigt.
///
/// **Bekannte Grenze:** Überlappen zwei halbtransparente Formen, sieht man im
/// PDF die untere nicht durchscheinen. Auf dem Bildschirm schon.
fn blend_over_white(rgba: [u8; 4]) -> Rgb {
    let a = rgba[3] as f32 / 255.0;
    let mix = |c: u8| (c as f32 / 255.0) * a + (1.0 - a);
    Rgb::new(mix(rgba[0]), mix(rgba[1]), mix(rgba[2]), None)
}

/// Setzt Füll- und Linienfarbe und liefert den passenden Paint-Modus.
/// `None` bedeutet: nichts zu zeichnen.
fn apply_shape_paint(layer: &PdfLayerReference, el: &Element) -> Option<PaintMode> {
    let has_fill = el.fill_color[3] > 0;
    let has_stroke = el.stroke_color[3] > 0 && el.stroke_width > 0.0;

    if has_fill {
        layer.set_fill_color(Color::Rgb(blend_over_white(el.fill_color)));
    }
    if has_stroke {
        layer.set_outline_color(Color::Rgb(blend_over_white(el.stroke_color)));
        layer.set_outline_thickness(el.stroke_width);
    }

    match (has_fill, has_stroke) {
        (true, true) => Some(PaintMode::FillStroke),
        (true, false) => Some(PaintMode::Fill),
        (false, true) => Some(PaintMode::Stroke),
        (false, false) => None,
    }
}

/// Zeichnet einen geschlossenen Umriss (Rechteck, Ellipse) aus
/// Seitenkoordinaten.
fn draw_outline(layer: &PdfLayerReference, el: &Element, page_h_mm: f32, outline: Vec<egui::Pos2>) {
    let Some(mode) = apply_shape_paint(layer, el) else {
        return;
    };
    let points: Vec<(Point, bool)> = outline
        .into_iter()
        .map(|p| (to_pdf(p, page_h_mm), false))
        .collect();
    layer.add_polygon(Polygon {
        rings: vec![points],
        mode,
        winding_order: WindingOrder::NonZero,
    });
}

fn draw_rectangle(layer: &PdfLayerReference, el: &Element, page_h_mm: f32) {
    draw_outline(layer, el, page_h_mm, crate::geometry::rect_outline(el));
}

fn draw_ellipse(layer: &PdfLayerReference, el: &Element, page_h_mm: f32) {
    draw_outline(layer, el, page_h_mm, crate::geometry::ellipse_outline(el));
}

/// Wandelt einen Pfad in die Punktfolge, die printpdf erwartet.
///
/// printpdf markiert Kurven nicht als eigene Segmente, sondern über ein
/// `bool` je Punkt. Der Leser (`Line::into_stream_op`) schaut auf **zwei
/// aufeinanderfolgende** gesetzte Flags und verbraucht dann vier Punkte als
/// kubische Kurve. Für eine Kurve von `A` nach `D` über `B`,`C` muss deshalb
/// **`A` und `B`** markiert sein — nicht etwa nur die Kontrollpunkte. Genau
/// das macht diese Funktion; ein gerades Segment bleibt ein einzelner Punkt
/// ohne Flag.
fn path_points(el: &Element, page_h_mm: f32) -> Vec<(Point, bool)> {
    let Some((start, segs)) = crate::geometry::path_segments(el) else {
        return Vec::new();
    };
    let mut out: Vec<(Point, bool)> = vec![(to_pdf(start, page_h_mm), false)];
    for seg in segs {
        match seg {
            crate::geometry::PathSeg::Line(p) => out.push((to_pdf(p, page_h_mm), false)),
            crate::geometry::PathSeg::Cubic(c1, c2, p) => {
                // printpdf verwechselt zwei **gleiche** Kontrollpunkte mit dem
                // Sonderfall „zweiter Kontrollpunkt = Endpunkt" und schreibt
                // dann die falsche Kurve. Ein unmerklicher Versatz umgeht das.
                let c2 = if (c2 - c1).length() < 1e-4 {
                    c2 + egui::Vec2::new(1e-3, 0.0)
                } else {
                    c2
                };
                // Der vorangehende Punkt eröffnet die Kurve und wird markiert.
                if let Some(last) = out.last_mut() {
                    last.1 = true;
                }
                out.push((to_pdf(c1, page_h_mm), true));
                out.push((to_pdf(c2, page_h_mm), false));
                out.push((to_pdf(p, page_h_mm), false));
            }
        }
    }
    out
}

/// Zeichnet einen freien Pfad — mit echten Kurven.
///
/// Geschlossen wird er wie jede andere Fläche behandelt; offen als Linienzug,
/// denn ein `Polygon` würde ihn stillschweigend schließen und damit eine Kante
/// erfinden, die auf dem Bildschirm nicht zu sehen ist.
///
/// Anders als Rechteck und Ellipse geht der Pfad **nicht** über
/// [`draw_outline`]: Der Bildschirm zeichnet die Kurve aufgelöst, das PDF
/// bekommt sie als Kurve. Nur so kommt eine aus einem PDF importierte Rundung
/// beim Export auch wieder als Rundung heraus statt als Vieleck mit
/// zweihundert Ecken.
fn draw_path(layer: &PdfLayerReference, el: &Element, page_h_mm: f32) {
    let points = path_points(el, page_h_mm);
    if points.len() < 2 {
        return;
    }
    if el.path_closed {
        let Some(mode) = apply_shape_paint(layer, el) else {
            return;
        };
        layer.add_polygon(Polygon {
            rings: vec![points],
            mode,
            winding_order: WindingOrder::NonZero,
        });
        return;
    }
    if el.stroke_width <= 0.0 || el.stroke_color[3] == 0 {
        return;
    }
    layer.set_outline_color(Color::Rgb(blend_over_white(el.stroke_color)));
    layer.set_outline_thickness(el.stroke_width);
    layer.add_line(Line {
        points,
        is_closed: false,
    });
}

/// Zeichnet eine Linie zwischen ihren beiden Endpunkten.
///
/// Vorher rechnete diese Funktion die Endpunkte selbst aus — und verankerte
/// die Linie dabei am Startpunkt statt am Mittelpunkt. Bei `rotation = 0` fiel
/// das nicht auf, bei jedem anderen Winkel stand die Linie im PDF an einer
/// völlig anderen Stelle als auf dem Bildschirm.
fn draw_line(layer: &PdfLayerReference, el: &Element, page_h_mm: f32) {
    if el.stroke_width <= 0.0 || el.stroke_color[3] == 0 {
        return;
    }
    let (a, b) = crate::geometry::line_endpoints(el);

    layer.set_outline_color(Color::Rgb(blend_over_white(el.stroke_color)));
    layer.set_outline_thickness(el.stroke_width);

    layer.add_line(Line {
        points: vec![
            (to_pdf(a, page_h_mm), false),
            (to_pdf(b, page_h_mm), false),
        ],
        is_closed: false,
    });
}

/// Rotiert ein RGBA-Bild um einen beliebigen Winkel (Bilinear-approximiert).
fn rotate_rgba(src: &RgbaImage, deg: f32) -> RgbaImage {
    let (w, h) = (src.width(), src.height());
    if deg.abs() < 0.05 {
        return src.clone();
    }
    let rad = deg.to_radians();
    let (s, c) = (rad.sin(), rad.cos());
    let nw = ((w as f32) * c.abs() + (h as f32) * s.abs()).ceil() as u32;
    let nh = ((w as f32) * s.abs() + (h as f32) * c.abs()).ceil() as u32;
    let nw = nw.max(1);
    let nh = nh.max(1);
    let mut out: RgbaImage = ImageBuffer::new(nw, nh);
    let cx = nw as f32 / 2.0;
    let cy = nh as f32 / 2.0;
    let sw = w as f32 / 2.0;
    let sh = h as f32 / 2.0;
    for oy in 0..nh {
        for ox in 0..nw {
            let dx = ox as f32 - cx;
            let dy = oy as f32 - cy;
            // inverse Rotation
            let sx = c * dx + s * dy + sw;
            let sy = -s * dx + c * dy + sh;
            if sx >= 0.0 && sy >= 0.0 && sx < w as f32 && sy < h as f32 {
                let p = src.get_pixel(sx as u32, sy as u32);
                out.put_pixel(ox, oy, *p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deckende_farbe_bleibt_unveraendert() {
        let c = blend_over_white([80, 140, 220, 255]);
        assert!((c.r - 80.0 / 255.0).abs() < 0.001);
        assert!((c.g - 140.0 / 255.0).abs() < 0.001);
        assert!((c.b - 220.0 / 255.0).abs() < 0.001);
    }

    #[test]
    fn voellig_transparente_farbe_wird_weiss() {
        let c = blend_over_white([0, 0, 0, 0]);
        assert!((c.r - 1.0).abs() < 0.001);
        assert!((c.g - 1.0).abs() < 0.001);
        assert!((c.b - 1.0).abs() < 0.001);
    }

    #[test]
    fn halbtransparente_farbe_liegt_dazwischen() {
        // Der eigentliche Fehlerfall: Alpha wurde ignoriert, eine zu 23 %
        // deckende Fuellung kam knallig deckend heraus.
        let default_fill = [80u8, 140, 220, 60];
        let blended = blend_over_white(default_fill);
        let opaque = blend_over_white([80, 140, 220, 255]);

        // Deutlich heller als die deckende Variante ...
        assert!(blended.r > opaque.r + 0.4, "kaum aufgehellt: {}", blended.r);
        // ... aber noch nicht weiss.
        assert!(blended.b < 0.999, "vollstaendig ausgebleicht");
        // Der Blaustich muss erhalten bleiben.
        assert!(blended.b > blended.r, "Farbton verloren");
    }

    #[test]
    fn mischung_ist_monoton_im_alpha() {
        let mut last = 2.0_f32;
        for a in [0u8, 60, 128, 200, 255] {
            let c = blend_over_white([0, 0, 0, a]);
            assert!(c.r < last, "Alpha {a} machte die Farbe nicht dunkler");
            last = c.r;
        }
    }
}
