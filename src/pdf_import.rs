//! PDF-Import via pdfium-render.
//!
//! Liest ein PDF ein und konvertiert Seiten/Elemente in das BoxDoc-Modell.
//! Unterstützt werden Text-Runs (mit Position, Größe, Bold/Italic-Heuristik),
//! eingebettete Bilder (als PNG) und einfache Vektorpfade (Rechteck, Linie,
//! Ellipse). Komplexere Pfade (Polygone, Kurven) werden aktuell ignoriert.
//!
//! Native-only: PDFium ist eine C/C++-Bibliothek und steht auf WASM nicht zur
//! Verfügung. Web-Import via pdf.js ist für eine spätere Phase geplant.

use crate::model::{
    mm_to_pt, Document, Element, ElementKind, Orientation, Page, PaperFormat, TextAlign, VAlign,
};
use crate::store::ImageStore;
use pdfium_render::prelude::{
    PdfColor, PdfPageObject, PdfPageObjectCommon, PdfPageObjectsCommon, PdfPathSegments, Pdfium,
};
use pdfium_render::prelude::PdfPathSegmentType;

type E = Box<dyn std::error::Error>;

/// Maximale Anzahl Seiten im Import (Schutz vor pathologischen PDFs).
const MAX_PAGES: i32 = 200;
/// Maximale Anzahl Elemente pro Seite (Schutz vor pathologischen PDFs).
const MAX_ELEMENTS_PER_PAGE: usize = 4000;

/// Liefert eine `Pdfium`-Instanz. Beim allerersten Aufruf lädt `pdfium_bundled`
/// die native Bibliothek in ein Cache-Verzeichnis herunter und bindet sie
/// global ein. Jeder weitere Aufruf gibt eine dünne Pdfium-Instanz zurück,
/// die auf den bereits gebundenen globalen State zugreift.
fn open_pdfium() -> Result<Pdfium, String> {
    match pdfium_bundled::bind_pdfium_silent() {
        Ok(p) => Ok(p),
        Err(_) => {
            // Wahrscheinlich already initialized — Pdfium::default() findet
            // die bestehenden Bindings und gibt eine funktionierende
            // Instanz zurück.
            Ok(Pdfium::default())
        }
    }
}

/// Importiert eine PDF-Datei und wandelt sie in ein BoxDoc-`Document` um.
///
/// Rückgabe: `(Document, ImageStore, next_id)` — analog zu `odt::import`.
pub fn import_pdf(path: &std::path::Path) -> Result<(Document, ImageStore, u64), E> {
    let pdfium = open_pdfium().map_err(|e| -> E { e.into() })?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("PDF konnte nicht geladen werden: {e}"))?;

    let mut images = ImageStore::default();
    let mut next_id: u64 = 1;
    let mut pages: Vec<Page> = Vec::new();

    // Gesamtanzahl Seiten begrenzen (Schutz vor pathologischen PDFs).
    let page_count = document.pages().len().min(MAX_PAGES);

    // Papierformat anhand der ersten Seite raten. Wir merken es uns am
    // Dokument, auch wenn spätere Seiten abweichende Maße haben können —
    // BoxDoc unterstützt nur ein einheitliches Format pro Dokument.
    let (format, orientation) = if page_count > 0 {
        let first = document
            .pages()
            .get(0)
            .map_err(|e| -> E { e.to_string().into() })?;
        detect_paper_format(first.width().value, first.height().value)
    } else {
        (PaperFormat::A4, Orientation::Portrait)
    };

    // Über alle Seiten iterieren. PdfPagesCommon::iter liefert einen
    // PdfPagesIterator, den wir direkt verarbeiten können.
    let pages_iter = document.pages().iter();
    let mut processed = 0i32;
    for page in pages_iter {
        if processed >= page_count {
            break;
        }
        processed += 1;

        let page_h = page.height().value;

        let mut elements = Vec::new();
        extract_text(&page, page_h, &mut next_id, &mut elements);
        extract_images(&page, &document, page_h, &mut images, &mut next_id, &mut elements);
        extract_paths(&page, page_h, &mut next_id, &mut elements);

        if elements.len() > MAX_ELEMENTS_PER_PAGE {
            elements.truncate(MAX_ELEMENTS_PER_PAGE);
        }

        pages.push(Page { elements });
    }

    if pages.is_empty() {
        pages.push(Page::default());
    }

    let doc = Document {
        format,
        orientation,
        pages,
    };
    Ok((doc, images, next_id))
}

/// Erkennt das Papierformat aus einer Mediabox-Größe (in pt).
/// BoxDoc unterstützt nur ein Format pro Dokument — bei gemischten Größen
/// wird die erste Seite maßgeblich, der Rest ggf. beschnitten.
fn detect_paper_format(w_pt: f32, h_pt: f32) -> (PaperFormat, Orientation) {
    let (w, h) = (w_pt.max(h_pt), w_pt.min(h_pt)); // Hochformat-Normierung
    let tolerance = 6.0; // pt — Wikimedia/PDF-Encoder haben leichte Abweichungen

    for fmt in PaperFormat::all() {
        let (fw_mm, fh_mm) = fmt.size_mm();
        let (fw_pt, fh_pt) = (mm_to_pt(fw_mm), mm_to_pt(fh_mm));
        if (w - fw_pt).abs() < tolerance && (h - fh_pt).abs() < tolerance {
            let orientation = if w_pt >= h_pt {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            };
            return (fmt, orientation);
        }
    }
    let orientation = if w_pt >= h_pt {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    };
    (PaperFormat::A4, orientation)
}

/// Extrahiert Text aus der Seite. Verwendet `PdfPageText::segments()`, das
/// pdfium bereits zu logischen Text-Runs zusammenfasst (mit Leerzeichen
/// pro Wort). Wir übernehmen die Run-Grenzen und ergänzen Stil-Infos aus dem
/// ersten Zeichen jeder Run (Bold/Italic/Farbe/Schriftgröße).
fn extract_text(
    page: &pdfium_render::prelude::PdfPage,
    page_h: f32,
    next_id: &mut u64,
    out: &mut Vec<Element>,
) {
    let text = match page.text() {
        Ok(t) => t,
        Err(_) => return,
    };

    for segment in text.segments().iter() {
        let seg_text = segment.text();
        let trimmed = seg_text.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Stil aus dem ersten Zeichen der Run übernehmen.
        let (size, bold, italic, color) = segment
            .chars()
            .ok()
            .and_then(|chars| chars.first().ok())
            .map(|c| {
                let size = c.scaled_font_size().value.max(4.0);
                let bold = c.font_is_bold_reenforced();
                let italic = c.font_is_italic();
                let color = c
                    .fill_color()
                    .map(|fc| {
                        [
                            fc.red() as u8,
                            fc.green() as u8,
                            fc.blue() as u8,
                            fc.alpha() as u8,
                        ]
                    })
                    .unwrap_or([20, 20, 20, 255]);
                (size, bold, italic, color)
            })
            .unwrap_or((12.0, false, false, [20, 20, 20, 255]));

        // Position aus Bounding-Box (PDF: y oben = top, BoxDoc: y wächst nach
        // unten → y = page_h - top).
        let bounds = segment.bounds();
        let x = bounds.left().value;
        let top = bounds.top().value;
        let w = bounds.width().value.max(size * 0.5);
        let h = bounds.height().value.max(size * 1.0);
        let y = page_h - top;

        let id = *next_id;
        *next_id += 1;
        let mut el = Element::new_text(id, x, y);
        el.w = w;
        el.h = h;
        el.text = trimmed.to_string();
        el.font_size = size;
        el.bold = bold;
        el.italic = italic;
        el.color = color;
        el.align = TextAlign::Left;
        el.valign = VAlign::Top;
        out.push(el);
    }
}

/// Extrahiert Bild-Objekte als PNG und legt sie im ImageStore ab.
fn extract_images(
    page: &pdfium_render::prelude::PdfPage,
    document: &pdfium_render::prelude::PdfDocument,
    page_h: f32,
    images: &mut ImageStore,
    next_id: &mut u64,
    out: &mut Vec<Element>,
) {
    let objects = page.objects();
    for obj in objects.iter() {
        let PdfPageObject::Image(ref img) = obj else {
            continue;
        };
        // Bitmap als RGBA holen. `get_processed_bitmap` wendet alle
        // Transformationen (Skalierung, Farbprofil) an und liefert die
        // „anzeigefertige" Variante.
        let bitmap = match img.get_processed_bitmap(document) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let w = bitmap.width();
        let h = bitmap.height();
        if w == 0 || h == 0 {
            continue;
        }
        let rgba = bitmap.as_rgba_bytes();
        if rgba.is_empty() {
            continue;
        }
        let png = encode_rgba_to_png(&rgba, w as u32, h as u32);
        if png.is_empty() {
            continue;
        }

        // Bounding-Box des Bilds in PDF-Punkten.
        let bounds = match img.bounds() {
            Ok(b) => b,
            Err(_) => continue,
        };
        let x = bounds.left().value;
        let y = page_h - bounds.top().value;
        let w = bounds.width().value;
        let h = bounds.height().value;

        let id = *next_id;
        *next_id += 1;
        images.insert(id, png, (w as u32, h as u32));
        let mut el = Element::new_image(id, x as u32, y as u32, w as u32, h as u32);
        el.x = x;
        el.y = y;
        el.w = w;
        el.h = h;
        out.push(el);
    }
}

/// Klassifiziert Vektorpfade in Rectangle/Line/Ellipse anhand der Segmentzahl.
fn extract_paths(
    page: &pdfium_render::prelude::PdfPage,
    page_h: f32,
    next_id: &mut u64,
    out: &mut Vec<Element>,
) {
    let objects = page.objects();
    for obj in objects.iter() {
        let PdfPageObject::Path(ref path) = obj else {
            continue;
        };
        let bounds = match path.bounds() {
            Ok(b) => b,
            Err(_) => continue,
        };
        let x = bounds.left().value;
        let top = bounds.top().value;
        let w = bounds.width().value;
        let h = bounds.height().value;
        if w < 1.0 && h < 1.0 {
            continue;
        }

        // Segmente analysieren: MoveTo / LineTo / BezierTo zählen.
        let mut moves = 0usize;
        let mut lines = 0usize;
        let mut beziers = 0usize;
        let mut is_closed = false;
        for seg in path.segments().iter() {
            match seg.segment_type() {
                PdfPathSegmentType::MoveTo => moves += 1,
                PdfPathSegmentType::LineTo => lines += 1,
                PdfPathSegmentType::BezierTo => beziers += 1,
                PdfPathSegmentType::Unknown => {}
            }
            if seg.is_close() {
                is_closed = true;
            }
        }
        let _ = moves; // aktuell ohne Bedeutung für die Klassifikation

        // Klassifikation:
        //   - 4 BezierTo + geschlossen → Ellipse
        //   - ≥3 LineTo + geschlossen → Rectangle
        //   - 1 LineTo, nicht geschlossen → Line
        //   - sonst: ignorieren (zu komplex)
        let kind = if beziers >= 4 && is_closed {
            ElementKind::Ellipse
        } else if lines >= 3 && is_closed {
            ElementKind::Rectangle
        } else if lines == 1 && !is_closed {
            ElementKind::Line
        } else {
            continue;
        };

        let id = *next_id;
        *next_id += 1;

        // Farben übernehmen, falls gesetzt. PdfColor implementiert kein Default,
        // daher nutzen wir einen expliziten Transparenz-Wert als Fallback.
        let transparent = PdfColor::new(0, 0, 0, 0);
        let fill = path.fill_color().unwrap_or(transparent);
        let stroke = path.stroke_color().unwrap_or(transparent);
        let stroke_w = path.stroke_width().map(|p| p.value).unwrap_or(0.0);
        let has_stroke = path.is_stroked().unwrap_or(false) && stroke_w > 0.0;

        // BoxDoc-Position: y ist oben (PDF top ist oben, BoxDoc-y wächst
        // nach unten → y = page_h - top).
        let y = page_h - top;

        let mut el = match kind {
            ElementKind::Rectangle => Element::new_rectangle(id, x, y),
            ElementKind::Ellipse => Element::new_ellipse(id, x, y),
            ElementKind::Line => Element::new_line(id, x, y),
            _ => continue,
        };
        el.w = w;
        el.h = if matches!(kind, ElementKind::Line) { 0.0 } else { h };
        el.fill_color = [
            fill.red() as u8,
            fill.green() as u8,
            fill.blue() as u8,
            fill.alpha() as u8,
        ];
        if has_stroke {
            el.stroke_color = [
                stroke.red() as u8,
                stroke.green() as u8,
                stroke.blue() as u8,
                stroke.alpha() as u8,
            ];
            el.stroke_width = stroke_w;
        } else {
            el.stroke_width = 0.0;
        }
        out.push(el);
    }
}

/// Kodiert eine RGBA-Pixel-Liste als PNG-Bytes. Nutzt die `image`-Crate,
/// die bereits in BoxDoc gelinkt ist.
fn encode_rgba_to_png(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    use image::{ImageBuffer, RgbaImage};
    let needed = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if rgba.len() < needed {
        return Vec::new();
    }
    let img: RgbaImage = ImageBuffer::from_raw(w, h, rgba[..needed].to_vec())
        .unwrap_or_else(|| ImageBuffer::new(w, h));
    let mut buf = std::io::Cursor::new(Vec::new());
    if image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .is_err()
    {
        return Vec::new();
    }
    buf.into_inner()
}
