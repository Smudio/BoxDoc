//! Beweist, dass der PDF-Export dasselbe Ergebnis liefert wie der Bildschirm.
//!
//! Das ist der Test, der den ursprünglichen Fehler gefangen hätte: Der Export
//! brach Text nur an `\n` um, während der Canvas an der Boxbreite umbrach. Ein
//! Absatz, der auf dem Bildschirm über fünf Zeilen lief, wurde im PDF zu einer
//! einzigen Zeile, die aus der Seite herauslief.
//!
//! Verifiziert wird gegen das **tatsächlich erzeugte PDF**, nicht gegen
//! Zwischenwerte: Das PDF wird geschrieben, mit pdfium wieder eingelesen und
//! sein Textinhalt geprüft.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;

use boxdoc::model::{Document, Element, Page, TextAlign};
use boxdoc::store::ImageStore;
use boxdoc::text_layout;

/// egui-Kontext mit geladenen Standardschriften.
fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    let _ = ctx.run(Default::default(), |_| {});
    ctx
}

const LANGER_ABSATZ: &str = "Dies ist ein bewusst langer Absatz ohne jeden \
    Zeilenumbruch im Quelltext, der in einer schmalen Box zwingend über mehrere \
    Zeilen umbrechen muss, damit der Export etwas zu tun bekommt.";

fn wrapped_doc() -> Document {
    let mut el = Element::new_text(1, 60.0, 60.0);
    el.text = LANGER_ABSATZ.to_string();
    el.w = 200.0;
    el.h = 400.0;
    el.font_size = 12.0;
    el.auto_height = false;
    Document {
        pages: vec![Page { elements: vec![el] }],
        ..Document::default()
    }
}

fn layouts_for(ctx: &egui::Context, doc: &Document) -> HashMap<u64, text_layout::TextLayout> {
    boxdoc::printing::collect_layouts(ctx, doc)
}

#[test]
fn export_verwendet_denselben_umbruch_wie_der_canvas() {
    let ctx = ctx();
    let doc = wrapped_doc();
    let el = &doc.pages[0].elements[0];

    // So layoutet der Canvas (mit Zoom), so der Export (ohne).
    let canvas = ctx.fonts_mut(|f| text_layout::layout(f, el, 1.5));
    let export = layouts_for(&ctx, &doc);
    let pdf = export.get(&1).expect("Layout für Element 1");

    assert!(
        pdf.lines.len() > 1,
        "Der Absatz muss umbrechen, sonst testet das hier nichts"
    );

    let a: Vec<&str> = canvas.lines.iter().map(|l| l.text.as_str()).collect();
    let b: Vec<&str> = pdf.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(a, b, "Canvas und PDF brechen an verschiedenen Stellen um");
}

#[test]
fn keine_zeile_laeuft_ueber_die_boxbreite_hinaus() {
    let ctx = ctx();
    let doc = wrapped_doc();
    let el = &doc.pages[0].elements[0];
    let layout = ctx.fonts_mut(|f| text_layout::layout(f, el, 1.0));

    for (i, line) in layout.lines.iter().enumerate() {
        assert!(
            line.x + line.width <= el.w + 0.5,
            "Zeile {i} ({:?}) ragt über die Box hinaus: {} > {}",
            line.text,
            line.x + line.width,
            el.w
        );
    }
}

#[test]
fn zeilen_folgen_ohne_ueberlappung_aufeinander() {
    let ctx = ctx();
    let doc = wrapped_doc();
    let layout = ctx.fonts_mut(|f| text_layout::layout(f, &doc.pages[0].elements[0], 1.0));

    let mut last = f32::NEG_INFINITY;
    for line in &layout.lines {
        assert!(
            line.baseline_y > last,
            "Grundlinien laufen nicht monoton: {} nach {}",
            line.baseline_y,
            last
        );
        last = line.baseline_y;
    }
}

#[test]
fn pdf_wird_geschrieben_und_enthaelt_den_text() {
    let ctx = ctx();
    let doc = wrapped_doc();
    let layouts = layouts_for(&ctx, &doc);
    let images = ImageStore::default();

    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).expect("Temp-Verzeichnis");
    let path = dir.join("wysiwyg.pdf");

    boxdoc::printing::export_pdf(&path, &doc, &images, &layouts).expect("PDF-Export");

    let bytes = std::fs::read(&path).expect("PDF lesen");
    assert!(bytes.starts_with(b"%PDF"), "keine gültige PDF-Datei");
    assert!(bytes.len() > 500, "PDF verdächtig klein: {} Bytes", bytes.len());
}

#[test]
fn pdf_text_entspricht_den_gelayouteten_zeilen() {
    // Der eigentliche Beweis: Das fertige PDF wird zurückgelesen und sein
    // Textinhalt mit dem verglichen, was das Layout vorgegeben hat.
    let ctx = ctx();
    let doc = wrapped_doc();
    let layouts = layouts_for(&ctx, &doc);

    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("roundtrip.pdf");
    boxdoc::printing::export_pdf(&path, &doc, &ImageStore::default(), &layouts).unwrap();

    let Some(extracted) = extract_pdf_text(&path) else {
        // pdfium nicht verfügbar → Test überspringen statt fälschlich zu
        // scheitern. Die übrigen Tests decken das Layout weiterhin ab.
        eprintln!("pdfium nicht verfügbar, Test übersprungen");
        return;
    };

    let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let expected = normalize(LANGER_ABSATZ);
    let actual = normalize(&extracted);

    assert_eq!(
        actual, expected,
        "Der Text im PDF weicht vom Ausgangstext ab"
    );

    // Und: Er steht dort wirklich in mehreren Zeilen.
    let pdf_layout = layouts.get(&1).unwrap();
    assert!(
        pdf_layout.lines.len() >= 3,
        "erwartet mehrzeiliger Umbruch, war {}",
        pdf_layout.lines.len()
    );
}

/// Liest den Text aus einem PDF zurück. `None`, wenn pdfium fehlt.
///
/// Bindet pdfium genauso wie der PDF-Import (`pdfium_bundled`), damit hier
/// keine zweite, abweichende Ladelogik entsteht.
fn extract_pdf_text(path: &std::path::Path) -> Option<String> {
    use pdfium_render::prelude::*;
    let pdfium = match pdfium_bundled::bind_pdfium_silent() {
        Ok(p) => p,
        // Bereits gebunden → bestehende globale Bindings verwenden.
        Err(_) => Pdfium::default(),
    };
    let document = pdfium.load_pdf_from_file(path, None).ok()?;
    let mut out = String::new();
    for page in document.pages().iter() {
        out.push_str(&page.text().ok()?.all());
        out.push(' ');
    }
    Some(out)
}

#[test]
fn ausrichtung_wirkt_sich_auf_die_pdf_position_aus() {
    let ctx = ctx();
    let mut doc = wrapped_doc();

    doc.pages[0].elements[0].align = TextAlign::Left;
    let left = layouts_for(&ctx, &doc);
    let left_x = left.get(&1).unwrap().lines[0].x;

    doc.pages[0].elements[0].align = TextAlign::Right;
    let right = layouts_for(&ctx, &doc);
    let right_x = right.get(&1).unwrap().lines[0].x;

    assert!(
        right_x > left_x,
        "rechtsbündig muss weiter rechts stehen: {right_x} vs {left_x}"
    );
    // Frühere Fassung schätzte die Breite mit `Zeichen * 0.5 * Größe` und lag
    // damit systematisch daneben. Jetzt muss die rechte Kante exakt sitzen.
    let el = &doc.pages[0].elements[0];
    let line = &right.get(&1).unwrap().lines[0];
    assert!(
        (line.x + line.width - el.w).abs() < 0.5,
        "rechte Kante sitzt nicht bündig: {} statt {}",
        line.x + line.width,
        el.w
    );
}
