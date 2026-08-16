//! Laden und Registrieren der kuratierten Schriften bei egui.
//!
//! Eingebettete Schriften (bundled) sind via `include_bytes!` in der Binary
//! und funktionieren auf Desktop und im Browser (WASM).
//! System-Schriften werden nur auf Desktop vom Dateisystem geladen.

use std::sync::{Arc, OnceLock};

use egui::{FontData, FontDefinitions, FontFamily};

use crate::model::{FontStyle, FONT_CHOICES};
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
///
/// Öffentlich, weil der PDF-Export dieselben Bytes braucht: Diese Schriften
/// liegen nur in der Binary, nicht auf der Platte. Wer sie über einen Pfad
/// sucht, findet nichts.
pub fn bundled_bytes(key: &str) -> Option<&'static [u8]> {
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

/// Alias-Familien "Bold"/"Italics"/"Bold Italic" für den Default-Font.
///
/// **Sie enthalten keinen eigenen Schnitt** — nur dieselbe Schriftliste wie
/// `Proportional`. Genau darum wählt [`family_for_style`] sie nicht mehr aus
/// und meldet [`has_style`] für die Standardschrift `false`: Über sie gesetzter
/// Text sah magerem Text auf den Punkt genau gleich.
///
/// Angelegt bleiben sie trotzdem: egui panickt beim Shaping, sobald eine
/// unbekannte Familie angefragt wird, und eine leere Familie ist ein billiger
/// Fangschutz.
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

    // Bundled + System-Schriften, je Schnitt.
    //
    // Ein echter Fett-Schnitt ist nicht nur schöner als ein nachgeahmter, er
    // ist auch **anders breit**. Weil `text_layout` mit genau der Familie misst,
    // die hier registriert wird, sitzen die Zeilenumbrüche damit dort, wo sie
    // in der gedruckten Fassung auch sitzen.
    for def in FONT_CHOICES {
        if def.key == "default" {
            continue;
        }
        for style in FontStyle::all() {
            // Eingebettete Schriften liegen nur als Regular in der Binary.
            let bytes: Option<Vec<u8>> = if def.bundled {
                if style == FontStyle::Regular {
                    bundled_bytes(def.key).map(|b| b.to_vec())
                } else {
                    None
                }
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    def.paths_for(style)
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
                register(&mut fonts, &family_key(def.key, style), bytes);
            }
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

/// Familienname eines Schnitts: `"arial"`, `"arial:b"`, `"arial:i"`, `"arial:bi"`.
///
/// Der Regular-Schnitt behält den nackten Schlüssel — so bleibt jede bereits
/// registrierte Familie und jeder Aufruf von [`family_for`] unverändert gültig.
fn family_key(key: &str, style: FontStyle) -> String {
    format!("{key}{}", style.suffix())
}

/// Ist dieser Schnitt der Schrift wirklich geladen — oder müsste er
/// nachgeahmt werden?
///
/// Canvas und PDF-Export fragen beide hier, damit sie sich einig sind: Wo ein
/// echter Schnitt liegt, benutzen ihn beide; wo keiner liegt, ahmen ihn beide
/// nach. Ohne diese gemeinsame Auskunft könnte der Bildschirm einen echten
/// Schnitt zeigen, den das PDF nachahmt (oder umgekehrt).
pub fn has_style(key: &str, style: FontStyle) -> bool {
    if style == FontStyle::Regular {
        return true;
    }
    if key == "default" || key.is_empty() {
        // Nein — und das war lange eine stille Lüge. Die Familien "Bold",
        // "Italics" und "Bold Italic" existieren zwar, sind aber mit
        // demselben mageren Schriftschnitt gefüllt wie "Proportional"
        // (siehe `rebuild_proportional_aliases`). Fett gesetzter Text in der
        // Standardschrift maß deshalb auf den Punkt genau so breit wie
        // magerer und sah auf dem Bildschirm identisch aus, während im PDF
        // echtes Helvetica-Bold stand.
        return false;
    }
    let name = family_key(key, style);
    registered().lock().unwrap().iter().any(|k| *k == name)
}

/// Liefert die `FontFamily` für einen Element-Schlüssel (Regular-Schnitt).
pub fn family_for(key: &str) -> FontFamily {
    family_for_style(key, FontStyle::Regular)
}

/// Liefert die `FontFamily` für Schlüssel **und Schnitt**.
///
/// Fehlt der Schnitt, wird auf den Regular-Schnitt derselben Schrift
/// zurückgefallen — lieber dieselbe Schrift ohne Fettung als eine fremde
/// Schrift mit. Fehlt auch der, bleibt der egui-Standard.
pub fn family_for_style(key: &str, style: FontStyle) -> FontFamily {
    if key == "default" || key.is_empty() {
        // Immer Proportional: Die Alias-Familien "Bold"/"Italics" enthalten
        // denselben mageren Schnitt (siehe `has_style`), sie hier zu wählen
        // täuschte nur eine Fettung vor. Der Canvas ahmt sie stattdessen nach.
        return FontFamily::Proportional;
    }
    let list = registered().lock().unwrap();
    let styled = family_key(key, style);
    if list.iter().any(|k| *k == styled) {
        return FontFamily::Name(styled.into());
    }
    if list.iter().any(|k| k == key) {
        return FontFamily::Name(key.into());
    }
    FontFamily::Proportional
}
