//! Laden und Registrieren der kuratierten Schriften bei egui.

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

use crate::model::FONT_CHOICES;

/// Menge der erfolgreich registrierten Schrift-Schlüssel.
/// Wird beim Start in `install()` gefüllt.
static REGISTERED: OnceLock<std::sync::Mutex<Vec<String>>> = OnceLock::new();

fn registered() -> &'static std::sync::Mutex<Vec<String>> {
    REGISTERED.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Registriert alle auffindbaren Schriften aus `FONT_CHOICES` bei egui.
/// Schriften, deren Datei nicht existiert, werden übersprungen.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    #[cfg(not(target_arch = "wasm32"))]
    {
        for def in FONT_CHOICES {
            if def.key == "default" {
                continue;
            }
            if let Some(path) = def.paths.iter().find(|p| std::fs::metadata(p).is_ok()) {
                if let Ok(bytes) = std::fs::read(path) {
                    let family = FontFamily::Name(def.key.into());
                    fonts.font_data.insert(
                        def.key.to_owned(),
                        Arc::new(FontData::from_owned(bytes)),
                    );
                    fonts.families.entry(family).or_default().push(def.key.to_owned());
                    registered().lock().unwrap().push(def.key.to_string());
                }
            }
        }
    }

    ctx.set_fonts(fonts);
}

/// Liefert die `FontFamily` für einen Element-Schlüssel.
/// Für "default" → `FontFamily::Proportional` (egui-Default).
/// Für nicht registrierte Schriften → ebenfalls `Proportional` (Fallback).
pub fn family_for(key: &str) -> FontFamily {
    if key == "default" || key.is_empty() {
        return FontFamily::Proportional;
    }
    // Nur registrierte Schriften als Name verwenden; sonst Fallback.
    let list = registered().lock().unwrap();
    if list.iter().any(|k| k == key) {
        FontFamily::Name(key.into())
    } else {
        FontFamily::Proportional
    }
}
