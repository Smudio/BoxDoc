//! Bild-Speicher: hält die PNG-Bytes und die egui-Texturen vor.

use std::collections::HashMap;

use egui::{ColorImage, Context, TextureHandle};

pub struct ImageEntry {
    pub png: Vec<u8>,
    pub dim: (u32, u32),
    pub texture: Option<TextureHandle>,
}

#[derive(Default)]
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
