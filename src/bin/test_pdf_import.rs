//! Manual PDF import test. Reads a PDF and prints a summary.
//!
//! Run: `cargo run --release --bin test_pdf_import -- <path-to-pdf>`

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = if args.len() >= 2 {
        PathBuf::from(&args[1])
    } else {
        PathBuf::from("tests/fixtures/invoice.pdf")
    };

    println!("Importiere {} …", path.display());

    let start = std::time::Instant::now();
    match boxdoc::pdf_import::import_pdf(&path) {
        Ok((doc, images, next_id)) => {
            let elapsed = start.elapsed();
            println!("OK in {:.2?}", elapsed);
            println!(
                "  Format: {} {:?}, Seiten: {}",
                doc.format.label(),
                doc.orientation,
                doc.pages.len()
            );
            println!("  Bilder: {}, nächste ID: {}", images.map.len(), next_id);
            for (i, page) in doc.pages.iter().enumerate() {
                println!("  Seite {}: {} Elemente", i + 1, page.elements.len());
                for el in &page.elements {
                    let preview: String = el.text.chars().take(60).collect();
                    let label = if el.text.is_empty() {
                        String::new()
                    } else {
                        format!("\"{}\"", preview)
                    };
                    println!(
                        "    [{:>10}] {:>20} ({:>6.1}x{:<6.1} @ {:>6.1},{:<6.1}) {}{}",
                        format!("{:?}", el.kind),
                        label,
                        el.w,
                        el.h,
                        el.x,
                        el.y,
                        if el.bold { "[B]" } else { "" },
                        if el.italic { "[I]" } else { "" }
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("FEHLER: {e}");
            std::process::exit(1);
        }
    }
}
