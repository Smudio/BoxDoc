//! PDF-Export und Drucken.
//!
//! Text wird vektoriell ausgegeben (mit eingebetteter Systemschrift, damit auch
//! Umlaute korrekt sind). Bilder werden zugeschnitten, gedreht und gerastert
//! ausgegeben, sodass Drehung und Crop exakt dem Bildschirm entsprechen.

use std::fs::File;
use std::io::BufWriter;

use image::{ImageBuffer, RgbaImage};
use printpdf::{
    path::{PaintMode, WindingOrder},
    BuiltinFont, Color, ColorBits, ColorSpace, Image, ImageTransform, ImageXObject, IndirectFontRef,
    Line, Mm, PdfDocument, PdfDocumentReference, PdfLayerReference, Point, Polygon, Px, Rgb,
};

use crate::model::{page_size_pt, Document, Element, ElementKind};

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

pub fn export_pdf(
    path: &std::path::Path,
    doc: &Document,
    images: &crate::store::ImageStore,
    layouts: &std::collections::HashMap<u64, crate::text_layout::TextLayout>,
) -> Result<(), E> {
    let (pw_pt, ph_pt) = page_size_pt(doc.format, doc.orientation);
    let (pw_mm, ph_mm) = (pt_to_mm(pw_pt), pt_to_mm(ph_pt));

    let (document, first_page, first_layer) =
        PdfDocument::new("BoxDoc", Mm(pw_mm), Mm(ph_mm), "Ebene 1");
    // Fallback-Schrift (wird verwendet, wenn eine Element-Schrift fehlt).
    let fallback_font = system_font(&document)?;
    // Cache: Schrift-Schlüssel → eingebetteter Font.
    let mut font_cache: std::collections::HashMap<String, IndirectFontRef> =
        std::collections::HashMap::new();

    for (pi, page) in doc.pages.iter().enumerate() {
        let (page_idx, layer_idx) = if pi == 0 {
            (first_page, first_layer)
        } else {
            document.add_page(Mm(pw_mm), Mm(ph_mm), "Ebene 1")
        };
        let layer = document.get_page(page_idx).get_layer(layer_idx);
        for el in &page.elements {
            match el.kind {
                ElementKind::Text => {
                    let font = resolve_text_font(&document, &el.font, el.bold, el.italic, &fallback_font, &mut font_cache);
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

    document.save(&mut BufWriter::new(File::create(path)?))?;
    Ok(())
}

/// Ermittelt die Fallback-Schrift für den Export.
///
/// Gibt einen Fehler zurück, statt zu panicken: Ein fehlgeschlagener PDF-Export
/// darf niemals die Anwendung mitsamt dem ungespeicherten Dokument abreißen.
fn system_font(doc: &PdfDocumentReference) -> Result<IndirectFontRef, E> {
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
    doc.add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| -> E { format!("keine verwendbare Schrift gefunden: {e}").into() })
}

/// Lädt die zu `key` gehörende Schrift aus `FONT_CHOICES`. Liefert `None`,
/// wenn die Datei fehlt oder nicht lesbar ist.
fn load_font_by_key(doc: &PdfDocumentReference, key: &str) -> Option<IndirectFontRef> {
    let def = crate::model::FONT_CHOICES.iter().find(|f| f.key == key)?;
    let path = def.paths.iter().find(|p| std::fs::metadata(p).is_ok())?;
    let f = File::open(path).ok()?;
    doc.add_external_font(f).ok()
}

/// Wählt den Schrift-Slot passend zu `bold`/`italic`. Die kursiven/fetten
/// Varianten werden gecacht, sodass sie nur einmal pro Dokument geladen werden.
/// Schlüssel im Cache ist `<font>|<bold>|<italic>`.
fn resolve_text_font(
    doc: &PdfDocumentReference,
    font_key: &str,
    bold: bool,
    italic: bool,
    fallback: &IndirectFontRef,
    cache: &mut std::collections::HashMap<String, IndirectFontRef>,
) -> IndirectFontRef {
    // Für "default" nutzen wir die PDF-Builtin-Varianten von Helvetica, die
    // Bold/Italic direkt unterstützen — kein externer Font nötig.
    if font_key == "default" || font_key.is_empty() {
        let cache_key = format!("__builtin|{bold}|{italic}");
        if let Some(f) = cache.get(&cache_key) {
            return f.clone();
        }
        let variant = match (bold, italic) {
            (true, true) => BuiltinFont::HelveticaBoldOblique,
            (true, false) => BuiltinFont::HelveticaBold,
            (false, true) => BuiltinFont::HelveticaOblique,
            (false, false) => BuiltinFont::Helvetica,
        };
        let f = doc
            .add_builtin_font(variant)
            .unwrap_or_else(|_| fallback.clone());
        cache.insert(cache_key, f.clone());
        return f;
    }

    // Externe Schrift: Wir versuchen zuerst eine passend benannte
    // Variante (z. B. arialbd.ttf für bold) zu finden; fällt das schief,
    // verwenden wir die Regular-Schrift und simulieren Bold nach.
    let cache_key = format!("{font_key}|{bold}|{italic}");
    if let Some(f) = cache.get(&cache_key) {
        return f.clone();
    }

    let regular = load_font_by_key(doc, font_key).unwrap_or_else(|| fallback.clone());
    let f = if bold || italic {
        // Pragmatische Lösung: Wir laden nur die Regular-Variante und
        // simulieren Bold/Italic beim Zeichnen (siehe draw_text). Damit
        // bleibt der Cache schlüssel-stabil — wir melden die Regular zurück.
        regular
    } else {
        regular
    };
    cache.insert(cache_key, f.clone());
    f
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
    font: &IndirectFontRef,
    layout: &crate::text_layout::TextLayout,
) {
    // Textfarbe inkl. Alpha (siehe blend_over_white). r/g/b werden weiter
    // unten fuer den Synth-Bold-Umriss gebraucht.
    let blended = blend_over_white(el.color);
    let (r, g, b) = (blended.r, blended.g, blended.b);
    layer.set_fill_color(Color::Rgb(blended));

    // Bei externen Schriften (nicht "default") wird Bold über einen leichten
    // Outline simuliert. Italic wird über die Text-Matrix geschert (12°).
    // Für Builtin-Fonts (Helvetica) ist Bold/Italic bereits im Font enthalten.
    let is_builtin = el.font == "default" || el.font.is_empty();
    let needs_synth_bold = el.bold && !is_builtin;
    let needs_shear = el.italic && !is_builtin;
    let shear = 12.0f32.to_radians().tan();

    layer.save_graphics_state();
    if needs_synth_bold {
        // Synth-Bold: schmaler Strich um die Glyphen. ~3 % der font_size ist
        // ein typischer Wert für „faux bold".
        layer.set_outline_color(Color::Rgb(Rgb::new(r, g, b, None)));
        layer.set_outline_thickness((el.font_size * 0.03).max(0.2));
        // Text-Render-Mode 2 = Fill + Stroke → ergibt fett.
        layer.set_text_rendering_mode(printpdf::TextRenderingMode::FillStroke);
    }

    for laid in &layout.lines {
        if laid.text.is_empty() {
            continue;
        }
        let line = laid.text.as_str();
        let line_w = laid.width;
        // PDF-Koordinaten: y zeigt nach oben, Ursprung unten links.
        let pdf_y_mm = page_h_mm - pt_to_mm(el.y + laid.baseline_y);
        let pdf_x_mm = pt_to_mm(el.x + laid.x);

        if needs_shear {
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
            layer.write_text(line.to_string(), font);
            layer.end_text_section();
        } else {
            layer.use_text(
                line.to_string(),
                el.font_size,
                Mm(pdf_x_mm),
                Mm(pdf_y_mm),
                font,
            );
        }

        // Unterstrich: dünne Linie direkt unter der Grundlinie, genau so breit
        // wie die gemessene Zeile.
        if el.underline {
            let underline_y_mm = pdf_y_mm - pt_to_mm(el.font_size) * 0.18;
            let underline = Line {
                points: vec![
                    (Point::new(Mm(pdf_x_mm), Mm(underline_y_mm)), false),
                    (
                        Point::new(Mm(pdf_x_mm + pt_to_mm(line_w)), Mm(underline_y_mm)),
                        false,
                    ),
                ],
                is_closed: false,
            };
            layer.set_outline_thickness((el.font_size * 0.05).max(0.5));
            layer.add_line(underline);
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

    let (rw, rh) = (rotated.width(), rotated.height());
    let mut rgb = Vec::with_capacity((rw * rh * 3) as usize);
    for px in rotated.pixels() {
        let a = px[3] as f32 / 255.0;
        rgb.push((px[0] as f32 * a + 255.0 * (1.0 - a)) as u8);
        rgb.push((px[1] as f32 * a + 255.0 * (1.0 - a)) as u8);
        rgb.push((px[2] as f32 * a + 255.0 * (1.0 - a)) as u8);
    }

    let xobj = ImageXObject {
        width: Px(rw as usize),
        height: Px(rh as usize),
        color_space: ColorSpace::Rgb,
        bits_per_component: ColorBits::Bit8,
        interpolate: true,
        image_data: rgb,
        image_filter: None,
        clipping_bbox: None,
        smask: None,
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
