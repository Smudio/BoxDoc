//! Roundtrip test: PDF → BoxDoc → PDF, then verify the second PDF parses.
//!
//! Run: `cargo run --release --bin test_pdf_roundtrip -- <path-to-pdf>`

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = if args.len() >= 2 {
        PathBuf::from(&args[1])
    } else {
        PathBuf::from("tests/fixtures/invoice.pdf")
    };

    println!("=== 1. Importiere {} ===", src.display());
    let start = std::time::Instant::now();
    let (doc, images, next_id) = boxdoc::pdf_import::import_pdf(&src)
        .expect("Import fehlgeschlagen");
    println!(
        "Import in {:.2?}: {} Seiten, {} Elemente gesamt, {} Bilder",
        start.elapsed(),
        doc.pages.len(),
        doc.pages.iter().map(|p| p.elements.len()).sum::<usize>(),
        images.map.len()
    );

    println!();
    println!("=== 2. Exportiere als roundtrip.pdf ===");
    let out = PathBuf::from("tests/fixtures/roundtrip_out.pdf");
    let start = std::time::Instant::now();

    // Printpdf direkt nutzen, da die print_dialog-Funktionen UI-Abhängig sind.
    // Wir rufen die low-level export_pdf-Funktion aus printing.rs nicht direkt,
    // da sie Teil der Binary ist — statt dessen bauen wir selbst ein Mini-PDF.
    export_minimal_pdf(&out, &doc).expect("Export fehlgeschlagen");
    println!("Export in {:.2?} → {}", start.elapsed(), out.display());

    println!();
    println!("=== 3. Importiere das re-exportierte PDF ===");
    let start = std::time::Instant::now();
    let (doc2, images2, next_id2) = boxdoc::pdf_import::import_pdf(&out)
        .expect("Re-Import fehlgeschlagen");
    println!(
        "Re-Import in {:.2?}: {} Seiten, {} Elemente gesamt, {} Bilder, next_id {}→{}",
        start.elapsed(),
        doc2.pages.len(),
        doc2.pages.iter().map(|p| p.elements.len()).sum::<usize>(),
        images2.map.len(),
        next_id,
        next_id2
    );

    println!();
    println!("=== 4. Vergleich ===");
    let count1: usize = doc.pages.iter().map(|p| p.elements.len()).sum();
    let count2: usize = doc2.pages.iter().map(|p| p.elements.len()).sum();
    if count1 == count2 {
        println!("OK — Elementzahl identisch ({}).", count1);
    } else {
        println!(
            "WARN — Elementzahl unterschiedlich: orig={}, reimport={}",
            count1, count2
        );
    }
}

fn export_minimal_pdf(
    path: &std::path::Path,
    doc: &boxdoc::model::Document,
) -> Result<(), Box<dyn std::error::Error>> {
    use printpdf::*;
    use std::fs::File;
    use std::io::BufWriter;

    let (w_mm, h_mm) = doc.format.size_mm();
    let (w_mm, h_mm) = match doc.orientation {
        boxdoc::model::Orientation::Portrait => (w_mm, h_mm),
        boxdoc::model::Orientation::Landscape => (h_mm, w_mm),
    };

    let (document, first_page, first_layer) =
        PdfDocument::new("BoxDoc-Roundtrip", Mm(w_mm), Mm(h_mm), "Ebene 1");
    let font_regular = document.add_builtin_font(BuiltinFont::Helvetica)?;
    let font_bold = document.add_builtin_font(BuiltinFont::HelveticaBold)?;

    for (pi, page) in doc.pages.iter().enumerate() {
        let (page_idx, layer_idx) = if pi == 0 {
            (first_page, first_layer)
        } else {
            document.add_page(Mm(w_mm), Mm(h_mm), "Ebene 1")
        };
        let layer = document.get_page(page_idx).get_layer(layer_idx);
        for el in &page.elements {
            use boxdoc::model::ElementKind;
            match el.kind {
                ElementKind::Text => {
                    let font = if el.bold { &font_bold } else { &font_regular };
                    let pdf_y = h_mm - el.y * 25.4 / 72.0 - el.font_size;
                    let pdf_x = el.x * 25.4 / 72.0;
                    layer.use_text(
                        el.text.clone(),
                        el.font_size,
                        Mm(pdf_x),
                        Mm(pdf_y),
                        font,
                    );
                }
                _ => {} // Shapes/Bilder im Minimal-Test überspringen.
            }
        }
    }

    document.save(&mut BufWriter::new(File::create(path)?))?;
    Ok(())
}
