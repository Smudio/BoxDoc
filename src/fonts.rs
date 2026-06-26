//! Laden und Registrieren der kuratierten Schriften bei egui.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

use crate::model::FONT_CHOICES;

/// Registriert alle auffindbaren Schriften aus `FONT_CHOICES` bei egui.
/// Schriften, deren Datei nicht existiert, werden übersprungen.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    for def in FONT_CHOICES {
        if def.key == "default" {
            continue;
        }
        // Auf Web: keine System-Schriften verfügbar, nur Native.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = def.paths.iter().find(|p| std::fs::metadata(p).is_ok()) {
                if let Ok(bytes) = std::fs::read(path) {
                    let family = FontFamily::Name(def.key.into());
                    fonts.font_data.insert(
                        def.key.to_owned(),
                        Arc::new(FontData::from_owned(bytes)),
                    );
                    fonts.families.entry(family).or_default().push(def.key.to_owned());
                }
            }
        }
    }

    ctx.set_fonts(fonts);
}

/// Liefert die `FontFamily` für einen Element-Schlüssel.
/// Für "default" → `FontFamily::Proportional` (egui-Default).
pub fn family_for(key: &str) -> FontFamily {
    if key == "default" {
        FontFamily::Proportional
    } else {
        FontFamily::Name(key.into())
    }
}
