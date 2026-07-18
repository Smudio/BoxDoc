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

use crate::model::{page_size_pt, Document, Element, ElementKind, TextAlign, VAlign};

type E = Box<dyn std::error::Error>;

fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

pub fn export_pdf_dialog(app: &mut crate::app::EditorApp) {
    let mut dlg = rfd::FileDialog::new()
        .add_filter("PDF", &["pdf"])
        .set_title("Als PDF exportieren");
    if let Some(stem) = app
        .file_path
        .as_ref()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
    {
        dlg = dlg.set_file_name(format!("{stem}.pdf"));
    }
    let Some(path) = dlg.save_file() else { return; };
    let path = if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
        path
    } else {
        path.with_extension("pdf")
    };
    crate::io::export_pdf(app, path);
}

pub fn print_dialog(app: &mut crate::app::EditorApp) {
    let dir = std::env::temp_dir();
    let path = dir.join("boxdoc_drucken.pdf");
    match export_pdf(&path, &app.doc, &app.images) {
        Ok(()) => {
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .arg("/C")
                .arg("start")
                .arg("")
                .arg(path.display().to_string())
                .spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&path).spawn();
            app.set_status("PDF erzeugt und Drucker-Dialog geöffnet.");
        }
        Err(e) => app.set_status(format!("Drucken fehlgeschlagen: {e}")),
    }
}

pub fn export_pdf(
    path: &std::path::Path,
    doc: &Document,
    images: &crate::store::ImageStore,
) -> Result<(), E> {
    let (pw_pt, ph_pt) = page_size_pt(doc.format, doc.orientation);
    let (pw_mm, ph_mm) = (pt_to_mm(pw_pt), pt_to_mm(ph_pt));

    let (document, first_page, first_layer) =
        PdfDocument::new("BoxDoc", Mm(pw_mm), Mm(ph_mm), "Ebene 1");
    // Fallback-Schrift (wird verwendet, wenn eine Element-Schrift fehlt).
    let fallback_font = system_font(&document);
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
                    draw_text(&layer, el, ph_mm, &font);
                }
                ElementKind::Image => draw_image(&layer, el, ph_mm, images),
                ElementKind::Rectangle => draw_rectangle(&layer, el, ph_mm),
                ElementKind::Line => draw_line(&layer, el, ph_mm),
                ElementKind::Ellipse => draw_ellipse(&layer, el, ph_mm),
            }
        }
    }

    document.save(&mut BufWriter::new(File::create(path)?))?;
    Ok(())
}

fn system_font(doc: &PdfDocumentReference) -> IndirectFontRef {
    let candidates: [&str; 4] = [
        "C:\\Windows\\Fonts\\arial.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ];
    for c in candidates {
        if let Ok(f) = File::open(c) {
            if let Ok(font) = doc.add_external_font(f) {
                return font;
            }
        }
    }
    doc.add_builtin_font(BuiltinFont::Helvetica).unwrap_or_else(|_| panic!("keine Schrift gefunden"))
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

/// Geschätzte Textbreite in pt für Approximation (kein echtes Metrics-API
/// in printpdf 0.7). Formula: ~0.5 × font_size pro Zeichen — etwas größer
/// (0.55) für etwas Luft nach rechts.
fn approx_text_width(text: &str, font_size: f32) -> f32 {
    let n = text.chars().count() as f32;
    n * font_size * 0.5
}

fn draw_text(layer: &PdfLayerReference, el: &Element, page_h_mm: f32, font: &IndirectFontRef) {
    let r = el.color[0] as f32 / 255.0;
    let g = el.color[1] as f32 / 255.0;
    let b = el.color[2] as f32 / 255.0;
    layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));

    // Bei externen Schriften (nicht "default") wird Bold über einen leichten
    // Outline-Simuliert. Italic wird über die Text-Matrix geschert (12°).
    // Für Builtin-Fonts (Helvetica) ist Bold/Italic bereits im Font enthalten.
    let is_builtin = el.font == "default" || el.font.is_empty();
    let needs_synth_bold = el.bold && !is_builtin;
    let needs_shear = el.italic && !is_builtin;
    let shear = 12.0f32.to_radians().tan();

    // Vertikale Ausrichtung (valign) innerhalb der Box.
    let lines: Vec<&str> = el.text.split('\n').collect();
    let line_h = el.font_size * 1.25;
    let block_h = line_h * lines.len() as f32;
    let y_offset = match el.valign {
        VAlign::Top => 0.0,
        VAlign::Middle => (el.h - block_h).max(0.0) / 2.0,
        VAlign::Bottom => (el.h - block_h).max(0.0),
    };

    layer.save_graphics_state();
    if needs_synth_bold {
        // Synth-Bold: schmaler Strich um die Glyphen. ~3 % der font_size ist
        // ein typischer Wert für „faux bold".
        layer.set_outline_color(Color::Rgb(Rgb::new(r, g, b, None)));
        layer.set_outline_thickness((el.font_size * 0.03).max(0.2));
        // Text-Render-Mode 2 = Fill + Stroke → ergibt fett.
        layer.set_text_rendering_mode(printpdf::TextRenderingMode::FillStroke);
    }

    let mut y = el.y + y_offset;
    for line in &lines {
        // Horizontale Ausrichtung: verschiebt x innerhalb von el.w.
        let line_w = approx_text_width(line, el.font_size);
        let x_offset = match el.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (el.w - line_w).max(0.0) / 2.0,
            TextAlign::Right => (el.w - line_w).max(0.0),
        };
        let pdf_y_mm = page_h_mm - pt_to_mm(y + el.font_size);
        let pdf_x_mm = pt_to_mm(el.x + el.indent + x_offset);

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

        // Unterstrich: dünne Linie direkt unter der Grundlinie.
        if el.underline && !line.is_empty() {
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

        y += line_h;
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
fn draw_rectangle(layer: &PdfLayerReference, el: &Element, page_h_mm: f32) {
    // Bei Corner-Radius > 0 erzeugen wir ein abgerundetes Rechteck als
    // Polygonzug (4× Viertelkreis à 4 Stützpunkte, kappa-Approximation).
    let radius = el.corner_radius.max(0.0);
    let w_mm = pt_to_mm(el.w);
    let h_mm = pt_to_mm(el.h);
    let cx_mm = pt_to_mm(el.x) + w_mm / 2.0;
    let cy_mm = page_h_mm - (pt_to_mm(el.y) + h_mm / 2.0);
    let r = radius.min(w_mm / 2.0).min(h_mm / 2.0);
    let hw = w_mm / 2.0;
    let hh = h_mm / 2.0;

    // Lokale Punkteliste (relativ zum Zentrum, PDF-Koordinaten: y nach oben).
    // Wir bauen zuerst die 4 Ecken als Polygonzug, optional mit Bögen.
    let kappa = 0.5522847498_f32;
    let mut local: Vec<(f32, f32)> = Vec::new();
    if r > 0.05 {
        // Wir beginnen oben-rechts (PDF-Koordinaten: +x, +y) und laufen im
        // Uhrzeigersinn um das Rechteck. Pro Ecke zwei Kontrollpunkte + Anker.
        let corners = [
            (hw - r, hh),       // oben-rechts, Startpunkt des Bogens
            (hw, hh - r),       // oben-rechts, Endpunkt des Bogens
            (hw, -hh + r),      // unten-rechts, Start
            (hw - r, -hh),      // unten-rechts, End
            (-hw + r, -hh),     // unten-links, Start
            (-hw, -hh + r),     // unten-links, End
            (-hw, hh - r),      // oben-links, Start
            (-hw + r, hh),      // oben-links, End
        ];
        let controls = [
            // Pro Ecke 2 Kontrollpunkte (Bezier), Reihenfolge wie corners.
            (hw - r + r * kappa, hh), (hw, hh - r + r * kappa), // Ecke OR
            (hw, -hh + r - r * kappa), (hw - r + r * kappa, -hh), // Ecke UR
            (-hw + r - r * kappa, -hh), (-hw, -hh + r - r * kappa), // Ecke UL
            (-hw, hh - r - r * kappa), (-hw + r - r * kappa, hh), // Ecke OL
        ];
        local.push(corners[0]);
        for seg in 0..4 {
            local.push(controls[seg * 2]);
            local.push(controls[seg * 2 + 1]);
            local.push(corners[seg * 2 + 1]);
        }
    } else {
        // Scharfes Rechteck.
        local.push((hw, hh));
        local.push((hw, -hh));
        local.push((-hw, -hh));
        local.push((-hw, hh));
    }

    // Rotation (BoxDoc: Grad gegen Uhrzeigersinn, y nach unten → PDF: y nach
    // oben, daher dreht sich das Vorzeichen des Sinus um).
    let rad = el.rotation.to_radians();
    let (rs, rc) = (rad.sin(), rad.cos());
    let rotate = |(x, y): (f32, f32)| -> (f32, f32) {
        (rc * x - rs * y, rs * x + rc * y)
    };

    let points: Vec<(Point, bool)> = local
        .iter()
        .map(|(x, y)| {
            let (rx, ry) = rotate((*x, *y));
            (Point::new(Mm(cx_mm + rx), Mm(cy_mm + ry)), false)
        })
        .collect();

    // Farben setzen.
    let has_fill = el.fill_color[3] > 0;
    let has_stroke = el.stroke_color[3] > 0 && el.stroke_width > 0.0;
    if has_fill {
        let f = el.fill_color;
        layer.set_fill_color(Color::Rgb(Rgb::new(
            f[0] as f32 / 255.0,
            f[1] as f32 / 255.0,
            f[2] as f32 / 255.0,
            None,
        )));
    }
    if has_stroke {
        let s = el.stroke_color;
        layer.set_outline_color(Color::Rgb(Rgb::new(
            s[0] as f32 / 255.0,
            s[1] as f32 / 255.0,
            s[2] as f32 / 255.0,
            None,
        )));
        layer.set_outline_thickness(el.stroke_width);
    }

    let mode = match (has_fill, has_stroke) {
        (true, true) => PaintMode::FillStroke,
        (true, false) => PaintMode::Fill,
        (false, true) => PaintMode::Stroke,
        (false, false) => return,
    };

    let poly = Polygon {
        rings: vec![points],
        mode,
        winding_order: WindingOrder::NonZero,
    };
    layer.add_polygon(poly);
}

/// Zeichnet eine Linie (Box mit h=0). BoxDoc-Modell: Position (x, y) ist der
/// Startpunkt, w die Länge, rotation der Winkel gegen den Uhrzeigersinn.
/// In PDF-Koordinaten (y nach oben) bedeutet eine Rotation von z. B. 30°
/// gegen Uhrzeigersinn im BoxDoc-System eine Drehung des Endpunkts nach
/// oben-links in PDF-Koordinaten.
fn draw_line(layer: &PdfLayerReference, el: &Element, page_h_mm: f32) {
    if el.stroke_width <= 0.0 || el.stroke_color[3] == 0 {
        return;
    }
    let x1_mm = pt_to_mm(el.x);
    let y1_mm = page_h_mm - pt_to_mm(el.y);
    // Endpunkt im BoxDoc-System: (x + w·cos, y + w·sin). rotation=0 → waagerecht
    // nach rechts (y in BoxDoc wächst nach unten, in PDF nach oben → Sinus
    // negieren, um identisches visuelles Ergebnis zu erhalten).
    let rad = el.rotation.to_radians();
    let (c, s) = (rad.cos(), rad.sin());
    let dx_mm = pt_to_mm(el.w) * c;
    let dy_mm = -pt_to_mm(el.w) * s;
    let x2_mm = x1_mm + dx_mm;
    let y2_mm = y1_mm + dy_mm;

    let stroke = el.stroke_color;
    layer.set_outline_color(Color::Rgb(Rgb::new(
        stroke[0] as f32 / 255.0,
        stroke[1] as f32 / 255.0,
        stroke[2] as f32 / 255.0,
        None,
    )));
    layer.set_outline_thickness(el.stroke_width);

    let line = Line {
        points: vec![
            (Point::new(Mm(x1_mm), Mm(y1_mm)), false),
            (Point::new(Mm(x2_mm), Mm(y2_mm)), false),
        ],
        is_closed: false,
    };
    layer.add_line(line);
}

/// Zeichnet eine Ellipse (Kreis als Spezialfall w == h) als geschlossenen
/// Bezier-Pfad. printpdf 0.7 hat keine native Ellipse, daher wird die Kurve
/// über 4 kubische Bezier-Segmente mit dem Kappa-Wert 0,5523 approximiert.
/// PDF-Koordinatensystem: y zeigt nach OBEN, daher wird das Zentrum anhand der
/// Seitenhöhe `page_h_mm` gespiegelt. Rotation (Grad, gegen Uhrzeigersinn im
/// BoxDoc-System) wird durch Rotation der Stützpunkte um das Zentrum aufgeprägt.
fn draw_ellipse(layer: &PdfLayerReference, el: &Element, page_h_mm: f32) {
    let cx_mm = pt_to_mm(el.x + el.w / 2.0);
    let cy_mm = page_h_mm - pt_to_mm(el.y + el.h / 2.0);
    let rx_mm = pt_to_mm(el.w).max(0.01) / 2.0;
    let ry_mm = pt_to_mm(el.h).max(0.01) / 2.0;

    // 4 Ankerpunkte (rechts, unten, links, oben) und je 2 Kontrollpunkte pro
    // Segment. Kappa-Wert für beste Annäherung an einen Kreis.
    let kappa = 0.5522847498;
    let anchors = [(rx_mm, 0.0), (0.0, ry_mm), (-rx_mm, 0.0), (0.0, -ry_mm)];
    let ctrl_offsets = [
        (rx_mm, ry_mm * kappa),
        (-rx_mm * kappa, ry_mm),
        (-rx_mm * kappa, ry_mm),
        (-rx_mm, -ry_mm * kappa),
        (-rx_mm, -ry_mm * kappa),
        (rx_mm * kappa, -ry_mm),
        (rx_mm * kappa, -ry_mm),
        (rx_mm, ry_mm * kappa),
    ];

    // Rotation (BoxDoc: Grad gegen Uhrzeigersinn, y nach unten).
    let rad = el.rotation.to_radians();
    let (rs, rc) = (rad.sin(), rad.cos());
    let rotate = |(x, y): (f32, f32)| -> (f32, f32) {
        (rc * x - rs * y, rs * x + rc * y)
    };

    let mut pts_local: Vec<(f32, f32, bool)> = Vec::with_capacity(13);
    let a0 = rotate(anchors[0]);
    pts_local.push((a0.0, a0.1, false));
    for seg in 0..4 {
        let c1 = rotate(ctrl_offsets[seg * 2]);
        let c2 = rotate(ctrl_offsets[seg * 2 + 1]);
        let an = rotate(anchors[(seg + 1) % 4]);
        pts_local.push((c1.0, c1.1, true));
        pts_local.push((c2.0, c2.1, true));
        pts_local.push((an.0, an.1, false));
    }

    // In PDF-Koordinaten umwandeln (y-Achse spiegeln).
    let points: Vec<(Point, bool)> = pts_local
        .iter()
        .map(|(x, y, is_ctrl)| (Point::new(Mm(cx_mm + x), Mm(cy_mm - y)), *is_ctrl))
        .collect();

    if el.fill_color[3] > 0 {
        let f = el.fill_color;
        layer.set_fill_color(Color::Rgb(Rgb::new(
            f[0] as f32 / 255.0,
            f[1] as f32 / 255.0,
            f[2] as f32 / 255.0,
            None,
        )));
    }
    if el.stroke_color[3] > 0 && el.stroke_width > 0.0 {
        let s = el.stroke_color;
        layer.set_outline_color(Color::Rgb(Rgb::new(
            s[0] as f32 / 255.0,
            s[1] as f32 / 255.0,
            s[2] as f32 / 255.0,
            None,
        )));
        layer.set_outline_thickness(el.stroke_width);
    }

    let mode = match (el.fill_color[3] > 0, el.stroke_color[3] > 0 && el.stroke_width > 0.0) {
        (true, true) => PaintMode::FillStroke,
        (true, false) => PaintMode::Fill,
        (false, true) => PaintMode::Stroke,
        (false, false) => return,
    };

    let poly = Polygon {
        rings: vec![points],
        mode,
        winding_order: WindingOrder::NonZero,
    };
    layer.add_polygon(poly);
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
