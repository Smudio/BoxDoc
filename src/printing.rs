//! PDF-Export und Drucken.
//!
//! Text wird vektoriell ausgegeben (mit eingebetteter Systemschrift, damit auch
//! Umlaute korrekt sind). Bilder werden zugeschnitten, gedreht und gerastert
//! ausgegeben, sodass Drehung und Crop exakt dem Bildschirm entsprechen.

use std::fs::File;
use std::io::BufWriter;

use image::{ImageBuffer, RgbaImage};
use printpdf::{
    BuiltinFont, Color, ColorBits, ColorSpace, Image, ImageTransform, ImageXObject, IndirectFontRef,
    Mm, PdfDocument, PdfDocumentReference, PdfLayerReference, Px, Rgb,
};

use crate::model::{page_size_pt, Document, Element, ElementKind};

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
                .args(["/C", "start", "", &path.display().to_string()])
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
                    let font = if el.font == "default" || el.font.is_empty() {
                        fallback_font.clone()
                    } else {
                        match font_cache.get(&el.font).cloned() {
                            Some(f) => f,
                            None => {
                                let f = load_font_by_key(&document, &el.font)
                                    .unwrap_or_else(|| fallback_font.clone());
                                font_cache.insert(el.font.clone(), f.clone());
                                f
                            }
                        }
                    };
                    draw_text(&layer, el, ph_mm, &font);
                }
                ElementKind::Image => draw_image(&layer, el, ph_mm, images),
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

fn draw_text(layer: &PdfLayerReference, el: &Element, page_h_mm: f32, font: &IndirectFontRef) {
    let r = el.color[0] as f32 / 255.0;
    let g = el.color[1] as f32 / 255.0;
    let b = el.color[2] as f32 / 255.0;
    layer.set_fill_color(Color::Rgb(Rgb::new(r, g, b, None)));

    let mut y = el.y;
    let line_h = el.font_size * 1.25;
    for line in el.text.split('\n') {
        let pdf_y = page_h_mm - pt_to_mm(y + el.font_size);
        layer.use_text(
            line.to_string(),
            el.font_size,
            Mm(pt_to_mm(el.x + el.indent)),
            Mm(pdf_y),
            font,
        );
        y += line_h;
    }
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
