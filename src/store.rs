//! Bild-Speicher: hält die PNG-Bytes und die egui-Texturen vor.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle};

use crate::model::{Crop, Element};

#[derive(Clone)]
pub struct ImageEntry {
    pub png: Vec<u8>,
    pub dim: (u32, u32),
    pub texture: Option<TextureHandle>,
}

#[derive(Default, Clone)]
pub struct ImageStore {
    pub map: HashMap<u64, ImageEntry>,
}

impl ImageStore {
    pub fn insert(&mut self, id: u64, png: Vec<u8>, dim: (u32, u32)) {
        self.map.insert(id, ImageEntry { png, dim, texture: None });
    }

    pub fn remove(&mut self, id: u64) {
        self.map.remove(&id);
    }

    /// Die Pixel eines Bild-Elements, **auf seinen Crop beschnitten**.
    ///
    /// Bewusst ohne Drehung: `rotation` beschreibt, wie das Bild auf der Seite
    /// liegt, nicht wie die Bilddatei aussieht. Wer ein Bild exportiert, will
    /// die Bilddatei — eine schräg gedrehte Kopie mit transparenten Ecken wäre
    /// in Paint & Co. nur im Weg. Der Crop dagegen *ist* der Bildinhalt, den
    /// der Nutzer sieht.
    ///
    /// `None`, wenn zur ID kein Bild existiert oder es nicht dekodierbar ist.
    pub fn cropped_rgba(&self, el: &Element) -> Option<image::RgbaImage> {
        let entry = self.map.get(&el.id)?;
        let rgba = image::load_from_memory(&entry.png).ok()?.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        if w == 0 || h == 0 {
            return None;
        }
        let crop = el.crop.clamp();
        // Dieselbe Rechnung wie im PDF- und SVG-Export — sonst zeigt die
        // gespeicherte Datei einen anderen Ausschnitt als das Dokument.
        let cx = ((crop.x * w as f32).round() as u32).min(w - 1);
        let cy = ((crop.y * h as f32).round() as u32).min(h - 1);
        let cw = (((crop.w * w as f32).round() as u32).max(1)).min(w - cx);
        let ch = (((crop.h * h as f32).round() as u32).max(1)).min(h - cy);
        Some(image::imageops::crop_imm(&rgba, cx, cy, cw, ch).to_image())
    }

    /// PNG-Bytes eines Bild-Elements.
    ///
    /// Ist das Bild unbeschnitten und liegt bereits als PNG vor, werden die
    /// Originalbytes durchgereicht. Ein erneutes Kodieren wäre nicht nur
    /// langsamer, es würde auch Farbprofile und Metadaten des Originals
    /// wegwerfen — beim reinen „Bild wieder herausspeichern" ein echter
    /// Verlust.
    pub fn element_png(&self, el: &Element) -> Option<Vec<u8>> {
        let entry = self.map.get(&el.id)?;
        if el.crop.clamp() == Crop::default()
            && image::guess_format(&entry.png).ok() == Some(image::ImageFormat::Png)
        {
            return Some(entry.png.clone());
        }
        encode_png(&self.cropped_rgba(el)?)
    }

    /// JPEG-Bytes eines Bild-Elements. Transparente Bereiche landen auf Weiß,
    /// weil JPEG kein Alpha kennt (sonst würden sie schwarz).
    pub fn element_jpeg(&self, el: &Element, quality: u8) -> Option<Vec<u8>> {
        encode_jpeg(&flatten_on_white(&self.cropped_rgba(el)?), quality)
    }

    /// Legt die Textur bei Bedarf an und gibt sie zurück.
    pub fn texture(&mut self, id: u64, ctx: &Context) -> Option<TextureHandle> {
        let entry = self.map.get_mut(&id)?;
        if entry.texture.is_none() {
            if let Ok(img) = image::load_from_memory(&entry.png) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let image = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                entry.texture = Some(ctx.load_texture(format!("boxdoc-img-{id}"), image, Default::default()));
            }
        }
        entry.texture.clone()
    }
}

/// Hat das Bild überhaupt transparente Pixel? Entscheidet, ob eine
/// Weiß-Variante nötig ist — für undurchsichtige Bilder wäre sie nur eine
/// zweite, identische Kodierung.
pub fn has_alpha(img: &image::RgbaImage) -> bool {
    img.pixels().any(|p| p.0[3] < 255)
}

/// Kopie mit weißem Untergrund — für Ziele ohne Alpha (JPEG, die
/// Windows-Zwischenablage, Paint).
pub fn flatten_on_white(img: &image::RgbaImage) -> image::RgbaImage {
    let mut out = img.clone();
    for p in out.pixels_mut() {
        let a = p.0[3] as f32 / 255.0;
        for c in 0..3 {
            let v = p.0[c] as f32 * a + 255.0 * (1.0 - a);
            p.0[c] = v.round().clamp(0.0, 255.0) as u8;
        }
        p.0[3] = 255;
    }
    out
}

pub fn encode_png(img: &image::RgbaImage) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img.clone())
        .write_to(&mut out, image::ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

pub fn encode_jpeg(img: &image::RgbaImage, quality: u8) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100))
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(out)
}

// ===========================================================================
// Custom-Fonts (in der .boxdoc-Datei eingebettet, analog zu Bildern)
// ===========================================================================

/// Ein eingebetteter Custom-Font (TTF/OTF-Bytes + eindeutiger Schlüssel).
#[derive(Clone)]
pub struct FontEntry {
    pub name: String,
    pub ttf: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct FontStore {
    pub map: HashMap<String, FontEntry>,
}

impl FontStore {
    pub fn insert(&mut self, name: String, ttf: Vec<u8>) {
        self.map.insert(name.clone(), FontEntry { name, ttf });
    }

    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// Sortierte Liste der Font-Namen (für UI und deterministische Speicherung).
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.map.keys().cloned().collect();
        v.sort();
        v
    }
}
