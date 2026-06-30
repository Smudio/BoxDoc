//! Laden und Registrieren der kuratierten Schriften bei egui.
//!
//! Eingebettete Schriften (bundled) sind via `include_bytes!` in der Binary
//! und funktionieren auf Desktop und im Browser (WASM).
//! System-Schriften werden nur auf Desktop vom Dateisystem geladen.

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

use crate::model::FONT_CHOICES;

/// Menge der erfolgreich registrierten Schrift-Schlüssel.
static REGISTERED: OnceLock<std::sync::Mutex<Vec<String>>> = OnceLock::new();

fn registered() -> &'static std::sync::Mutex<Vec<String>> {
    REGISTERED.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Liefert die eingebetteten Bytes für einen bundled Font-Key.
fn bundled_bytes(key: &str) -> Option<&'static [u8]> {
    Some(match key {
        "inter" => include_bytes!("../assets/fonts/Inter-Regular.ttf"),
        "roboto" => include_bytes!("../assets/fonts/Roboto-Regular.ttf"),
        "lora" => include_bytes!("../assets/fonts/Lora-Regular.ttf"),
        "jetbrains" => include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
        "pacifico" => include_bytes!("../assets/fonts/Pacifico-Regular.ttf"),
        _ => return None,
    })
}

/// Registriert alle auffindbaren Schriften bei egui.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    for def in FONT_CHOICES {
        if def.key == "default" {
            continue;
        }

        let bytes: Option<Vec<u8>> = if def.bundled {
            bundled_bytes(def.key).map(|b| b.to_vec())
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                def.paths
                    .iter()
                    .find(|p| std::fs::metadata(p).is_ok())
                    .and_then(|p| std::fs::read(p).ok())
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        };

        if let Some(bytes) = bytes {
            let family = FontFamily::Name(def.key.into());
            fonts
                .font_data
                .insert(def.key.to_owned(), Arc::new(FontData::from_owned(bytes)));
            fonts
                .families
                .entry(family)
                .or_default()
                .push(def.key.to_owned());
            registered().lock().unwrap().push(def.key.to_string());
        }
    }

    // Alias-Familien für Bold/Italic beim Default-Font (Proportional).
    // canvas.rs nutzt FontFamily::Name("Bold"|"Italics"|"Bold Italic") für
    // Default-Font-Elemente mit bold/italic. Diese müssen gebunden sein,
    // sonst panicert egui beim Text-Shaping.
    let prop_fonts: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if !prop_fonts.is_empty() {
        for alias in ["Bold", "Italics", "Bold Italic"] {
            fonts
                .families
                .entry(FontFamily::Name(alias.into()))
                .or_default()
                .extend(prop_fonts.iter().cloned());
        }
    }

    ctx.set_fonts(fonts);
}

/// Liefert die `FontFamily` für einen Element-Schlüssel.
pub fn family_for(key: &str) -> FontFamily {
    if key == "default" || key.is_empty() {
        return FontFamily::Proportional;
    }
    let list = registered().lock().unwrap();
    if list.iter().any(|k| k == key) {
        FontFamily::Name(key.into())
    } else {
        FontFamily::Proportional
    }
}
