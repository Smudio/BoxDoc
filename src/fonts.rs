//! Laden und Registrieren der kuratierten Schriften bei egui.
//!
//! Eingebettete Schriften (bundled) sind via `include_bytes!` in der Binary
//! und funktionieren auf Desktop und im Browser (WASM).
//! System-Schriften werden nur auf Desktop vom Dateisystem geladen.

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

use crate::model::FONT_CHOICES;
use crate::store::FontStore;

/// Menge der erfolgreich registrierten Schrift-Schlüssel.
static REGISTERED: OnceLock<std::sync::Mutex<Vec<String>>> = OnceLock::new();

fn registered() -> &'static std::sync::Mutex<Vec<String>> {
    REGISTERED.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Push nur, wenn der Schlüssel noch nicht vorhanden ist — verhindert
/// Duplikate, wenn `install` und `install_with_custom` nacheinander laufen.
fn remember(key: &str) {
    let mut list = registered().lock().unwrap();
    if !list.iter().any(|k| k == key) {
        list.push(key.to_string());
    }
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

/// Registriert einen Font unter `key` in den FontDefinitions und merkt ihn
/// in der `registered()`-Liste vor (ohne Duplikate).
fn register(fonts: &mut FontDefinitions, key: &str, bytes: Vec<u8>) {
    let family = FontFamily::Name(key.into());
    fonts
        .font_data
        .insert(key.to_owned(), Arc::new(FontData::from_owned(bytes)));
    fonts.families.entry(family).or_default().push(key.to_owned());
    remember(key);
}

/// Alias-Familien für Bold/Italic beim Default-Font (Proportional).
/// canvas.rs nutzt FontFamily::Name("Bold"|"Italics"|"Bold Italic") für
/// Default-Font-Elemente mit bold/italic. Diese müssen gebunden sein,
/// sonst panicert egui beim Text-Shaping.
fn rebuild_proportional_aliases(fonts: &mut FontDefinitions) {
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
}

/// Registriert alle auffindbaren Schriften bei egui (ohne Custom-Fonts).
pub fn install(ctx: &egui::Context) {
    install_with_custom(ctx, &FontStore::default());
}

/// Registriert alle bundled/system-Schriften PLUS die übergebenen Custom-Fonts
/// bei egui. Bundled-Keys haben Vorrang — ein Custom-Font mit kollidierendem
/// Namen wird übersprungen.
pub fn install_with_custom(ctx: &egui::Context, custom: &FontStore) {
    let mut fonts = FontDefinitions::default();

    // Bundled + System-Schriften.
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
            register(&mut fonts, def.key, bytes);
        }
    }

    // Custom-Fonts. Bundled-Keys haben Vorrang — Kollisionen überspringen.
    let bundled_keys: std::collections::HashSet<&str> =
        FONT_CHOICES.iter().map(|f| f.key).collect();
    for name in custom.names() {
        if bundled_keys.contains(name.as_str()) {
            continue;
        }
        if let Some(entry) = custom.map.get(&name) {
            register(&mut fonts, &name, entry.ttf.clone());
        }
    }

    rebuild_proportional_aliases(&mut fonts);

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
