//! Speichern/Öffnen im OpenDocument-Format (ODT).
//!
//! ODT ist eine ZIP-Datei mit XML-Inhalt. Wir erzeugen ein minimales, aber
//! gültiges Dokument, das LibreOffice/OpenOffice öffnen können, und lesen
//! einfache ODT-Dateien bestmöglich wieder ein.

use std::fs::File;
use std::io::{Read, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::model::{Document, Element, ElementKind, Orientation, PaperFormat, Page};
use crate::store::ImageStore;

type E = Box<dyn std::error::Error>;

const MIMETYPE: &str = "application/vnd.oasis.opendocument.text";

/// Maximale Größe einer einzelnen entpackten Datei (100 MB) — Schutz vor
/// ZIP-Bomben. Wird beim Lesen jedes Archiveintrags geprüft.
const MAX_EXTRACT_SIZE: u64 = 100 * 1024 * 1024;
/// Maximale Summe aller unkomprimierten Dateien im Archiv (400 MB). Verhindert
/// ZIP-Bomben mit vielen kleinen Dateien unterhalb des Einzel-Limits.
const MAX_ARCHIVE_TOTAL_SIZE: u64 = 4 * MAX_EXTRACT_SIZE;

const NS: &str = concat!(
    " xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\"",
    " xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\"",
    " xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\"",
    " xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\"",
    " xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
    " xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\"",
    " xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\"",
    " xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\""
);

fn pt_to_cm(pt: f32) -> f32 {
    pt * 2.54 / 72.0
}
fn cm_to_pt(cm: f32) -> f32 {
    cm * 72.0 / 2.54
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn export(path: &std::path::Path, doc: &Document, images: &ImageStore) -> Result<(), E> {
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // mimetype (muss zuerst, unkomprimiert)
    zip.start_file("mimetype", stored)?;
    zip.write_all(MIMETYPE.as_bytes())?;

    // Bilder sammeln und schreiben
    let mut image_files: Vec<(u64, String, &'static str)> = Vec::new();
    for (id, entry) in &images.map {
        let (ext, mime) = if entry.png.starts_with(&[0x89, b'P', b'N', b'G']) {
            ("png", "image/png")
        } else if entry.png.starts_with(&[0xFF, 0xD8, 0xFF]) {
            ("jpg", "image/jpeg")
        } else {
            ("png", "image/png")
        };
        let name = format!("Pictures/{id}.{ext}");
        zip.start_file(&name, deflated)?;
        zip.write_all(&entry.png)?;
        image_files.push((*id, name, mime));
    }

    // content.xml
    let mut content = String::new();
    content.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    content.push_str(&format!(
        "<office:document-content {NS} office:version=\"1.2\">\n"
    ));
    content.push_str(" <office:automatic-styles/>\n");
    content.push_str(" <office:body>\n  <office:text>\n");

    for (pi, page) in doc.pages.iter().enumerate() {
        if pi > 0 {
            content.push_str("   <text:p text:style-name=\"Standard\"/>\n");
        }
        for el in &page.elements {
            content.push_str(&frame_xml(el, &image_files));
        }
    }

    content.push_str("  </office:text>\n </office:body>\n</office:document-content>\n");
    zip.start_file("content.xml", deflated)?;
    zip.write_all(content.as_bytes())?;

    // styles.xml
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <office:document-styles {NS} office:version=\"1.2\">\n\
         <office:styles><style:style style:name=\"Standard\" style:family=\"paragraph\"/></office:styles>\n\
         <office:automatic-styles/>\n</office:document-styles>\n"
    );
    zip.start_file("styles.xml", deflated)?;
    zip.write_all(styles.as_bytes())?;

    // meta.xml
    let meta = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <office:document-meta {NS} office:version=\"1.2\">\n\
         <office:meta><meta:generator>BoxDoc</meta:generator></office:meta>\n\
         </office:document-meta>\n"
    );
    zip.start_file("meta.xml", deflated)?;
    zip.write_all(meta.as_bytes())?;

    // manifest.xml
    let mut manifest = String::new();
    manifest.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    manifest.push_str("<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\n");
    manifest.push_str(" <manifest:file-entry manifest:media-type=\"application/vnd.oasis.opendocument.text\" manifest:full-path=\"/\"/>\n");
    manifest.push_str(" <manifest:file-entry manifest:media-type=\"text/xml\" manifest:full-path=\"content.xml\"/>\n");
    manifest.push_str(" <manifest:file-entry manifest:media-type=\"text/xml\" manifest:full-path=\"styles.xml\"/>\n");
    manifest.push_str(" <manifest:file-entry manifest:media-type=\"text/xml\" manifest:full-path=\"meta.xml\"/>\n");
    for (_, name, mime) in &image_files {
        manifest.push_str(&format!(
            " <manifest:file-entry manifest:media-type=\"{mime}\" manifest:full-path=\"{name}\"/>\n"
        ));
    }
    manifest.push_str("</manifest:manifest>\n");
    zip.start_file("META-INF/manifest.xml", deflated)?;
    zip.write_all(manifest.as_bytes())?;

    zip.finish()?;
    Ok(())
}

fn frame_xml(el: &Element, image_files: &[(u64, String, &'static str)]) -> String {
    let x = format_pt(pt_to_cm(el.x));
    let y = format_pt(pt_to_cm(el.y));
    let w = format_pt(pt_to_cm(el.w));
    let h = format_pt(pt_to_cm(el.h));
    let mut transform = String::new();
    if el.rotation.abs() > 0.01 {
        let cx = pt_to_cm(el.x + el.w / 2.0);
        let cy = pt_to_cm(el.y + el.h / 2.0);
        transform = format!(
            " draw:transform=\"rotate ({} {} {})\"",
            format_rad(el.rotation.to_radians()),
            format_pt(cx),
            format_pt(cy)
        );
    }

    let inner = match el.kind {
        ElementKind::Text => {
            let mut s = String::from("<draw:text-box>");
            for line in el.text.split('\n') {
                if line.is_empty() {
                    s.push_str("<text:p text:style-name=\"Standard\"/>");
                } else {
                    s.push_str(&format!(
                        "<text:p text:style-name=\"Standard\">{}</text:p>",
                        esc(line)
                    ));
                }
            }
            s.push_str("</draw:text-box>");
            s
        }
        ElementKind::Image => {
            let href = image_files
                .iter()
                .find(|(id, _, _)| *id == el.id)
                .map(|(_, n, _)| n.clone())
                .unwrap_or_default();
            format!(
                "<draw:image xlink:href=\"{href}\" xlink:type=\"simple\" xlink:actuate=\"onLoad\"/>"
            )
        }
        ElementKind::Ellipse => {
            // Eigenständiges draw:ellipse-Element innerhalb des Rahmens
            // (Breite/Höhe aus dem Frame, draw:ellipse ohne eigene Geometrie).
            String::from("<draw:ellipse/>")
        }
        ElementKind::Path => {
            // ODF kennt Polygone mit eigenem Koordinatenraum: `svg:viewBox`
            // spannt ihn auf, `draw:points` liegt darin. Wir wählen 0..1000,
            // damit die normalisierten Stützpunkte ganzzahlig werden.
            const VB: f32 = 1000.0;
            let pts = el
                .points
                .iter()
                .map(|[x, y]| format!("{},{}", (x * VB) as i32, (y * VB) as i32))
                .collect::<Vec<_>>()
                .join(" ");
            let tag = if el.path_closed { "polygon" } else { "polyline" };
            format!(
                "<draw:{tag} svg:viewBox=\"0 0 {vb} {vb}\" draw:points=\"{pts}\"/>",
                vb = VB as i32
            )
        }
        ElementKind::Rectangle | ElementKind::Line => String::new(),
    };

    format!(
        "   <draw:frame text:anchor-type=\"page\" svg:x=\"{x}cm\" svg:y=\"{y}cm\" svg:width=\"{w}cm\" svg:height=\"{h}cm\"{transform}>\n    {inner}\n   </draw:frame>\n"
    )
}

fn format_pt(v: f32) -> String {
    format!("{:.3}", v)
}
fn format_rad(v: f32) -> String {
    format!("{:.4}", v)
}

/// Best-Effort-Import einer ODT-Datei.
pub fn import(path: &std::path::Path) -> Result<(Document, ImageStore, u64), E> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;

    // SW-003: Summen-Check über alle Einträge. Blockiert ZIP-Bomben mit vielen
    // kleinen Dateien, die einzeln unter `MAX_EXTRACT_SIZE` liegen.
    let total_uncompressed: u64 = (0..archive.len())
        .map(|i| {
            archive
                .by_index(i)
                .map(|f| f.size())
                .unwrap_or(0)
        })
        .sum();
    if total_uncompressed > MAX_ARCHIVE_TOTAL_SIZE {
        return Err(format!(
            "ODT-Archiv zu groß: entpackt {} Bytes (Limit: {} Bytes)",
            total_uncompressed, MAX_ARCHIVE_TOTAL_SIZE
        )
        .into());
    }

    let content = String::from_utf8(read_entry(&mut archive, "content.xml")?)?;

    let mut images = ImageStore::default();
    let mut pages: Vec<Page> = Vec::new();
    let mut current = Page::default();
    let mut next_id: u64 = 1;

    for raw in content.split("<draw:frame").skip(1) {
        let block = match raw.split("</draw:frame>").next() {
            Some(b) => b,
            None => continue,
        };
        let x = parse_cm(attr(block, "svg:x"));
        let y = parse_cm(attr(block, "svg:y"));
        let w = parse_cm(attr(block, "svg:width")).max(2.0);
        let h = parse_cm(attr(block, "svg:height")).max(2.0);
        let rotation = parse_transform_rotate(block);

        if let Some(href) = attr(block, "xlink:href") {
            // Bild
            let name = href.trim_start_matches("./").trim_start_matches('/');
            if let Ok(bytes) = read_entry(&mut archive, name) {
                let dim = image::load_from_memory(&bytes)
                    .map(|i| (i.width(), i.height()))
                    .unwrap_or((w as u32, h as u32));
                let id = next_id;
                next_id += 1;
                images.insert(id, bytes, dim);
                let mut el = Element::new_image(id, x as u32, y as u32, dim.0, dim.1);
                el.x = x;
                el.y = y;
                el.w = w;
                el.h = h;
                el.rotation = rotation;
                current.elements.push(el);
            }
        } else {
            // Text
            let text = extract_text(block);
            if !text.trim().is_empty() {
                let id = next_id;
                next_id += 1;
                let mut el = Element::new_text(id, x, y);
                el.w = w;
                el.h = h;
                el.rotation = rotation;
                el.text = text;
                current.elements.push(el);
            }
        }
    }
    pages.push(current);

    let doc = Document {
        format: PaperFormat::A4,
        orientation: Orientation::Portrait,
        pages,
    };
    Ok((doc, images, next_id))
}

fn read_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, E> {
    let mut f = archive.by_name(name)?;
    // SW-003: Einzel-Limit pro Archiveintrag (Schutz vor ZIP-Bomben).
    if f.size() > MAX_EXTRACT_SIZE {
        return Err(format!(
            "Archiveintrag '{}' zu groß: {} Bytes (Limit: {} Bytes)",
            name,
            f.size(),
            MAX_EXTRACT_SIZE
        )
        .into());
    }
    let mut buf = Vec::with_capacity((f.size() as usize).min(MAX_EXTRACT_SIZE as usize));
    let mut total: u64 = 0;
    loop {
        let n = {
            let mut chunk = (&mut f).take(8 * 1024);
            chunk.read_to_end(&mut buf)?
        };
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_EXTRACT_SIZE {
            return Err(format!(
                "Archiveintrag '{}' überschreitet Maximalgröße von {} Bytes beim Entpacken",
                name, MAX_EXTRACT_SIZE
            )
            .into());
        }
    }
    Ok(buf)
}

fn attr(block: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = block.find(&needle)? + needle.len();
    let rest = &block[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_cm(v: Option<String>) -> f32 {
    let Some(v) = v else { return 0.0 };
    let num: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
    let n: f32 = num.parse().unwrap_or(0.0);
    if v.contains("cm") {
        cm_to_pt(n)
    } else {
        n
    }
}

fn parse_transform_rotate(block: &str) -> f32 {
    let Some(tf) = attr(block, "draw:transform") else {
        return 0.0;
    };
    let Some(idx) = tf.find("rotate") else {
        return 0.0;
    };
    let rest = &tf[idx..];
    let Some(open) = rest.find('(') else {
        return 0.0;
    };
    let after = &rest[open + 1..];
    let Some(close) = after.find(')') else {
        return 0.0;
    };
    let inner = &after[..close];
    let first: String = inner.split_whitespace().next().unwrap_or("0").to_string();
    let rad: f32 = first.parse().unwrap_or(0.0);
    rad.to_degrees()
}

fn extract_text(block: &str) -> String {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(start) = rest.find("<text:p") {
        rest = &rest[start..];
        let Some(gt) = rest.find('>') else { break };
        let inner = &rest[gt + 1..];
        let Some(end) = inner.find("</text:p>") else { break };
        let line = &inner[..end];
        out.push(unescape(line.trim()));
        rest = &inner[end + "</text:p>".len()..];
    }
    out.join("\n")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}
