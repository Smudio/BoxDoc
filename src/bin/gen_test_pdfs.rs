//! Generate small test PDFs for the roundtrip QA workflow.
//!
//! Run: `cargo run --release --bin gen_test_pdfs`
//!
//! Writes three PDFs into `tests/fixtures/`:
//!   - `invoice.pdf` — text-only invoice
//!   - `flyer.pdf` — text + rectangles + ellipse + line
//!   - `report.pdf` — multi-page text
//!
//! Used for manual QA of the PDF importer in phase2-pdf.

use printpdf::path::{PaintMode, WindingOrder};
use printpdf::*;
use std::fs::File;
use std::io::BufWriter;

fn main() {
    let out_dir = std::path::Path::new("tests/fixtures");
    std::fs::create_dir_all(out_dir).expect("create tests/fixtures");

    write_invoice(out_dir.join("invoice.pdf"));
    write_flyer(out_dir.join("flyer.pdf"));
    write_report(out_dir.join("report.pdf"));

    println!("Test-PDFs in {} erzeugt.", out_dir.display());
}

fn write_invoice(path: std::path::PathBuf) {
    let (doc, first_page, first_layer) =
        PdfDocument::new("Invoice", Mm(210.0), Mm(297.0), "Ebene 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).unwrap();
    let layer = doc.get_page(first_page).get_layer(first_layer);

    layer.use_text("RECHNUNG".to_string(), 24.0, Mm(25.0), Mm(270.0), &bold);
    layer.use_text("Rechnung Nr. 2024-001".to_string(), 12.0, Mm(25.0), Mm(260.0), &font);
    layer.use_text("Kunde: Mustermann GmbH".to_string(), 12.0, Mm(25.0), Mm(250.0), &font);
    layer.use_text("Datum: 18. Juli 2026".to_string(), 12.0, Mm(25.0), Mm(240.0), &font);

    // Trennlinie
    let line = Line {
        points: vec![
            (Point::new(Mm(25.0), Mm(230.0)), false),
            (Point::new(Mm(185.0), Mm(230.0)), false),
        ],
        is_closed: false,
    };
    layer.set_outline_thickness(0.5);
    layer.add_line(line);

    layer.use_text("Position 1: Dienstleistung".to_string(), 11.0, Mm(25.0), Mm(220.0), &font);
    layer.use_text("Betrag: 1.200,00 EUR".to_string(), 11.0, Mm(120.0), Mm(220.0), &font);

    layer.use_text("Position 2: Hardware".to_string(), 11.0, Mm(25.0), Mm(210.0), &font);
    layer.use_text("Betrag: 480,00 EUR".to_string(), 11.0, Mm(120.0), Mm(210.0), &font);

    layer.use_text("Gesamtbetrag: 1.680,00 EUR".to_string(), 13.0, Mm(120.0), Mm(190.0), &bold);

    doc.save(&mut BufWriter::new(File::create(&path).unwrap())).unwrap();
    println!("  • {}", path.display());
}

fn write_flyer(path: std::path::PathBuf) {
    let (doc, first_page, first_layer) =
        PdfDocument::new("Flyer", Mm(210.0), Mm(297.0), "Ebene 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).unwrap();
    let layer = doc.get_page(first_page).get_layer(first_layer);

    // Hintergrund-Rechteck (Rahmen).
    layer.set_fill_color(Color::Rgb(Rgb::new(0.95, 0.95, 0.95, None)));
    layer.set_outline_color(Color::Rgb(Rgb::new(0.3, 0.5, 0.8, None)));
    layer.set_outline_thickness(2.0);
    let rect = Polygon {
        rings: vec![vec![
            (Point::new(Mm(20.0), Mm(20.0)), false),
            (Point::new(Mm(190.0), Mm(20.0)), false),
            (Point::new(Mm(190.0), Mm(277.0)), false),
            (Point::new(Mm(20.0), Mm(277.0)), false),
        ]],
        mode: PaintMode::FillStroke,
        winding_order: WindingOrder::NonZero,
    };
    layer.add_polygon(rect);

    // Ellipse als Deko.
    layer.set_fill_color(Color::Rgb(Rgb::new(0.4, 0.7, 0.9, None)));
    layer.set_outline_color(Color::Rgb(Rgb::new(0.1, 0.3, 0.6, None)));
    layer.set_outline_thickness(1.0);
    let kappa = 0.5522847498_f32;
    let cx = 105.0;
    let cy = 200.0;
    let rx = 40.0f32;
    let ry = 25.0f32;
    let ellipse = Polygon {
        rings: vec![vec![
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
        ]],
        mode: PaintMode::FillStroke,
        winding_order: WindingOrder::NonZero,
    };
    layer.add_polygon(ellipse);

    // Titel
    layer.use_text("SOMMERAKTION".to_string(), 28.0, Mm(50.0), Mm(260.0), &bold);
    layer.use_text("20% auf alles!".to_string(), 18.0, Mm(60.0), Mm(250.0), &font);

    // Linie
    layer.set_outline_thickness(1.5);
    layer.set_outline_color(Color::Rgb(Rgb::new(0.1, 0.3, 0.6, None)));
    let line = Line {
        points: vec![
            (Point::new(Mm(60.0), Mm(240.0)), false),
            (Point::new(Mm(150.0), Mm(240.0)), false),
        ],
        is_closed: false,
    };
    layer.add_line(line);

    layer.use_text(
        "Nur im Juli — solange Vorrat reicht.".to_string(),
        10.0,
        Mm(50.0),
        Mm(150.0),
        &font,
    );

    doc.save(&mut BufWriter::new(File::create(&path).unwrap())).unwrap();
    println!("  • {}", path.display());
}

fn write_report(path: std::path::PathBuf) {
    let (doc, first_page, first_layer) =
        PdfDocument::new("Report", Mm(210.0), Mm(297.0), "Ebene 1");
    let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold).unwrap();
    let italic = doc.add_builtin_font(BuiltinFont::HelveticaOblique).unwrap();

    let (p2, l2) = doc.add_page(Mm(210.0), Mm(297.0), "Ebene 1");
    let (p3, l3) = doc.add_page(Mm(210.0), Mm(297.0), "Ebene 1");

    // Seite 1
    let layer = doc.get_page(first_page).get_layer(first_layer);
    layer.use_text("Quartalsbericht Q2/2026".to_string(), 22.0, Mm(25.0), Mm(260.0), &bold);
    layer.use_text("Zusammenfassung".to_string(), 14.0, Mm(25.0), Mm(245.0), &bold);
    layer.use_text(
        "Das zweite Quartal 2026 war geprägt von stabilem Wachstum".to_string(),
        11.0,
        Mm(25.0),
        Mm(235.0),
        &font,
    );
    layer.use_text(
        "in den Kernbereichen und einer Erweiterung des Portfolios.".to_string(),
        11.0,
        Mm(25.0),
        Mm(225.0),
        &font,
    );

    // Seite 2
    let layer = doc.get_page(p2).get_layer(l2);
    layer.use_text("Kennzahlen".to_string(), 16.0, Mm(25.0), Mm(270.0), &bold);
    layer.use_text("Umsatz: 4,2 Mio. EUR".to_string(), 11.0, Mm(25.0), Mm(255.0), &font);
    layer.use_text("Mitarbeiter: 142".to_string(), 11.0, Mm(25.0), Mm(245.0), &font);
    layer.use_text(
        "Hinweis: Vorläufige Zahlen, vorbehaltlich Abschlussprüfung.".to_string(),
        9.0,
        Mm(25.0),
        Mm(230.0),
        &italic,
    );

    // Seite 3
    let layer = doc.get_page(p3).get_layer(l3);
    layer.use_text("Ausblick".to_string(), 16.0, Mm(25.0), Mm(270.0), &bold);
    layer.use_text(
        "Für Q3 erwarten wir ein moderates Wachstum von 5-7%.".to_string(),
        11.0,
        Mm(25.0),
        Mm(255.0),
        &font,
    );

    doc.save(&mut BufWriter::new(File::create(&path).unwrap())).unwrap();
    println!("  • {}", path.display());
}
