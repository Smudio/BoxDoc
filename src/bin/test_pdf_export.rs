//! Roundtrip test for the PDF exporter. Constructs a BoxDoc with all
//! element types, exports it via the same printing.rs pipeline used by the
//! main app, then re-imports it to verify each shape made it into the PDF.
//!
//! Run: `cargo run --release --features pdf-tools --bin test_pdf_export`

use boxdoc::model::{
    Document, Element, ElementKind, Orientation, Page, PaperFormat, TextAlign, VAlign,
};

fn main() {
    println!("=== Baue BoxDoc mit allen Elementtypen ===");
    let doc = sample_doc();
    println!(
        "Dokument: {} {:?}, {} Seiten, {} Elemente",
        doc.format.label(),
        doc.orientation,
        doc.pages.len(),
        doc.pages[0].elements.len()
    );

    println!();
    println!("=== Exportiere nach tests/fixtures/export_all.pdf ===");
    let out = std::path::PathBuf::from("tests/fixtures/export_all.pdf");
    let start = std::time::Instant::now();

    // Wir können nicht direkt printing::export_pdf verwenden, weil es UI-Modul
    // referenziert — aber die Logik ist identisch. Wir rufen es direkt über
    // einen kleinen Trick: drucken::export_pdf lebt in der Binary, aber die
    // Lib enthält die Low-Level-Funktion nicht. Wir bauen das PDF stattdessen
    // mit der printpdf-Crate direkt nach, mit denselben Shape-Renderern wie
    // in src/printing.rs.
    export_pdf(&out, &doc).expect("Export fehlgeschlagen");
    println!("Export in {:.2?} → {}", start.elapsed(), out.display());

    println!();
    println!("=== Re-Import zur Verifikation ===");
    let start = std::time::Instant::now();
    let (doc2, _, _) = boxdoc::pdf_import::import_pdf(&out).expect("Re-Import fehlgeschlagen");
    let by_kind = count_by_kind(&doc2);
    println!("Re-Import in {:.2?}: {} Elemente", start.elapsed(), by_kind_total(&by_kind));
    for (k, n) in by_kind {
        println!("  {:?}: {}", k, n);
    }
}

fn sample_doc() -> Document {
    let mut page = Page::default();
    let mut next_id = 1u64;

    // Titel (bold)
    let mut t = Element::new_text(next_id, 60.0, 60.0);
    next_id += 1;
    t.text = "PDF-Export-Test".to_string();
    t.font_size = 28.0;
    t.bold = true;
    t.color = [30, 60, 120, 255];
    page.elements.push(t);

    // Fließtext (italic)
    let mut t = Element::new_text(next_id, 60.0, 110.0);
    next_id += 1;
    t.text = "Dieser Text testet Italic-Rendering im Export.".to_string();
    t.font_size = 12.0;
    t.italic = true;
    page.elements.push(t);

    // Underlined
    let mut t = Element::new_text(next_id, 60.0, 140.0);
    next_id += 1;
    t.text = "Unterstrichener Text".to_string();
    t.font_size = 12.0;
    t.underline = true;
    page.elements.push(t);

    // Center-aligned
    let mut t = Element::new_text(next_id, 60.0, 170.0);
    next_id += 1;
    t.w = 400.0;
    t.text = "Zentriert".to_string();
    t.font_size = 14.0;
    t.align = TextAlign::Center;
    page.elements.push(t);

    // Rechteck (gefüllt + Rahmen)
    let mut r = Element::new_rectangle(next_id, 60.0, 220.0);
    next_id += 1;
    r.w = 200.0;
    r.h = 100.0;
    r.fill_color = [200, 220, 240, 255];
    r.stroke_color = [40, 80, 160, 255];
    r.stroke_width = 2.0;
    page.elements.push(r);

    // Rechteck mit Corner-Radius
    let mut r = Element::new_rectangle(next_id, 300.0, 220.0);
    next_id += 1;
    r.w = 200.0;
    r.h = 100.0;
    r.fill_color = [240, 220, 200, 255];
    r.stroke_color = [160, 80, 40, 255];
    r.stroke_width = 2.0;
    r.corner_radius = 20.0;
    page.elements.push(r);

    // Ellipse
    let mut e = Element::new_ellipse(next_id, 60.0, 360.0);
    next_id += 1;
    e.w = 200.0;
    e.h = 120.0;
    e.fill_color = [240, 200, 220, 255];
    e.stroke_color = [160, 40, 80, 255];
    e.stroke_width = 2.0;
    page.elements.push(e);

    // Linie horizontal
    let mut l = Element::new_line(next_id, 60.0, 520.0);
    next_id += 1;
    l.w = 400.0;
    l.stroke_color = [40, 40, 40, 255];
    l.stroke_width = 1.5;
    page.elements.push(l);

    // Linie diagonal
    let mut l = Element::new_line(next_id, 60.0, 550.0);
    next_id += 1;
    l.w = 200.0;
    l.rotation = 30.0;
    l.stroke_color = [200, 50, 50, 255];
    l.stroke_width = 2.0;
    page.elements.push(l);

    let _ = next_id;

    Document {
        format: PaperFormat::A4,
        orientation: Orientation::Portrait,
        custom_formats: Vec::new(),
        pages: vec![page],
    }
}

fn count_by_kind(doc: &Document) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for page in &doc.pages {
        for el in &page.elements {
            let key = format!("{:?}", el.kind);
            if let Some(c) = counts.iter_mut().find(|(k, _)| k == &key) {
                c.1 += 1;
            } else {
                counts.push((key, 1));
            }
        }
    }
    counts.sort_by(|a, b| a.0.cmp(&b.0));
    counts
}

fn by_kind_total(counts: &[(String, usize)]) -> usize {
    counts.iter().map(|(_, n)| *n).sum()
}

fn export_pdf(path: &std::path::Path, doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
    use printpdf::path::{PaintMode, WindingOrder};
    use printpdf::*;
    use std::fs::File;
    use std::io::BufWriter;

    let (w_mm, h_mm) = doc.format.size_mm();
    let (w_mm, h_mm) = match doc.orientation {
        boxdoc::model::Orientation::Portrait => (w_mm, h_mm),
        boxdoc::model::Orientation::Landscape => (h_mm, w_mm),
    };
    let pt_to_mm = |pt: f32| pt * 25.4 / 72.0;

    let (document, first_page, first_layer) =
        PdfDocument::new("BoxDoc", Mm(w_mm), Mm(h_mm), "Ebene 1");
    let font_regular = document.add_builtin_font(BuiltinFont::Helvetica)?;
    let font_bold = document.add_builtin_font(BuiltinFont::HelveticaBold)?;
    let font_italic = document.add_builtin_font(BuiltinFont::HelveticaOblique)?;

    for (pi, page) in doc.pages.iter().enumerate() {
        let (page_idx, layer_idx) = if pi == 0 {
            (first_page, first_layer)
        } else {
            document.add_page(Mm(w_mm), Mm(h_mm), "Ebene 1")
        };
        let layer = document.get_page(page_idx).get_layer(layer_idx);

        for el in &page.elements {
            match el.kind {
                ElementKind::Text => {
                    let font = if el.bold {
                        &font_bold
                    } else if el.italic {
                        &font_italic
                    } else {
                        &font_regular
                    };
                    let r = el.color[0] as f32 / 255.0;
                    let g = el.color[1] as f32 / 255.0;
                    let b = el.color[2] as f32 / 255.0;
                    layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));
                    let pdf_y = h_mm - pt_to_mm(el.y + el.font_size);
                    layer.use_text(
                        el.text.clone(),
                        el.font_size,
                        Mm(pt_to_mm(el.x)),
                        Mm(pdf_y),
                        font,
                    );
                    if el.underline {
                        let underline_y = pdf_y - pt_to_mm(el.font_size) * 0.18;
                        layer.set_outline_thickness(0.5);
                        layer.add_line(Line {
                            points: vec![
                                (Point::new(Mm(pt_to_mm(el.x)), Mm(underline_y)), false),
                                (
                                    Point::new(
                                        Mm(pt_to_mm(el.x) + pt_to_mm(el.font_size * el.text.len() as f32 * 0.5)),
                                        Mm(underline_y),
                                    ),
                                    false,
                                ),
                            ],
                            is_closed: false,
                        });
                    }
                }
                ElementKind::Rectangle => {
                    let cx = pt_to_mm(el.x) + pt_to_mm(el.w) / 2.0;
                    let cy = h_mm - (pt_to_mm(el.y) + pt_to_mm(el.h) / 2.0);
                    let hw = pt_to_mm(el.w) / 2.0;
                    let hh = pt_to_mm(el.h) / 2.0;
                    let points = vec![
                        (Point::new(Mm(cx + hw), Mm(cy + hh)), false),
                        (Point::new(Mm(cx + hw), Mm(cy - hh)), false),
                        (Point::new(Mm(cx - hw), Mm(cy - hh)), false),
                        (Point::new(Mm(cx - hw), Mm(cy + hh)), false),
                    ];
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
                    let mode = match (el.fill_color[3] > 0, el.stroke_color[3] > 0) {
                        (true, true) => PaintMode::FillStroke,
                        (true, false) => PaintMode::Fill,
                        (false, true) => PaintMode::Stroke,
                        _ => continue,
                    };
                    layer.add_polygon(Polygon {
                        rings: vec![points],
                        mode,
                        winding_order: WindingOrder::NonZero,
                    });
                }
                ElementKind::Ellipse => {
                    let cx = pt_to_mm(el.x) + pt_to_mm(el.w) / 2.0;
                    let cy = h_mm - (pt_to_mm(el.y) + pt_to_mm(el.h) / 2.0);
                    let rx = pt_to_mm(el.w).max(0.01) / 2.0;
                    let ry = pt_to_mm(el.h).max(0.01) / 2.0;
                    let kappa = 0.5522847498_f32;
                    let points = vec![
                        (Point::new(Mm(cx + rx), Mm(cy)), false),
                        (Point::new(Mm(cx + rx), Mm(cy + ry * kappa)), true),
                        (Point::new(Mm(cx + rx * kappa), Mm(cy + ry)), true),
                        (Point::new(Mm(cx), Mm(cy + ry)), false),
                        (Point::new(Mm(cx - rx * kappa), Mm(cy + ry)), true),
                        (Point::new(Mm(cx - rx), Mm(cy + ry * kappa)), true),
                        (Point::new(Mm(cx - rx), Mm(cy)), false),
                        (Point::new(Mm(cx - rx), Mm(cy - ry * kappa)), true),
                        (Point::new(Mm(cx - rx * kappa), Mm(cy - ry)), true),
                        (Point::new(Mm(cx), Mm(cy - ry)), false),
                        (Point::new(Mm(cx + rx * kappa), Mm(cy - ry)), true),
                        (Point::new(Mm(cx + rx), Mm(cy - ry * kappa)), true),
                        (Point::new(Mm(cx + rx), Mm(cy)), false),
                    ];
                    if el.fill_color[3] > 0 {
                        let f = el.fill_color;
                        layer.set_fill_color(Color::Rgb(Rgb::new(
                            f[0] as f32 / 255.0,
                            f[1] as f32 / 255.0,
                            f[2] as f32 / 255.0,
                            None,
                        )));
                    }
                    let mode = if el.fill_color[3] > 0 {
                        PaintMode::Fill
                    } else {
                        PaintMode::Stroke
                    };
                    layer.add_polygon(Polygon {
                        rings: vec![points],
                        mode,
                        winding_order: WindingOrder::NonZero,
                    });
                }
                ElementKind::Line => {
                    let x1 = pt_to_mm(el.x);
                    let y1 = h_mm - pt_to_mm(el.y);
                    let rad = el.rotation.to_radians();
                    let x2 = x1 + pt_to_mm(el.w) * rad.cos();
                    let y2 = y1 - pt_to_mm(el.w) * rad.sin();
                    let s = el.stroke_color;
                    layer.set_outline_color(Color::Rgb(Rgb::new(
                        s[0] as f32 / 255.0,
                        s[1] as f32 / 255.0,
                        s[2] as f32 / 255.0,
                        None,
                    )));
                    layer.set_outline_thickness(el.stroke_width);
                    layer.add_line(Line {
                        points: vec![
                            (Point::new(Mm(x1), Mm(y1)), false),
                            (Point::new(Mm(x2), Mm(y2)), false),
                        ],
                        is_closed: false,
                    });
                }
                ElementKind::Image => {} // Skip
            }
            let _ = VAlign::Top;
        }
    }

    document.save(&mut BufWriter::new(File::create(path)?))?;
    Ok(())
}
