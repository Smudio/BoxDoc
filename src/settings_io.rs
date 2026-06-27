//! Persistenz der Benutzereinstellungen (Theme, Panel-Position).
//!
//! Native: JSON-Datei im Benutzer-Verzeichnis.
//! Web: localStorage.

use crate::model::Settings;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::Settings;

    fn settings_path() -> Option<std::path::PathBuf> {
        let dir = directories();
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir.join("settings.json"))
    }

    fn directories() -> std::path::PathBuf {
        if let Some(home) = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .or_else(|| std::env::var_os("APPDATA"))
        {
            let p = std::path::PathBuf::from(home);
            let p = p.join(".boxdoc");
            if p.exists() || std::fs::create_dir_all(&p).is_ok() {
                return p;
            }
        }
        std::env::temp_dir().join("boxdoc")
    }

    pub fn load() -> Settings {
        let Some(path) = settings_path() else {
            return Settings::default();
        };
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(settings: &Settings) {
        let Some(path) = settings_path() else {
            return;
        };
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::Settings;

    const KEY: &str = "boxdoc_settings";

    pub fn load() -> Settings {
        let Some(window) = web_sys::window() else {
            return Settings::default();
        };
        let Ok(Some(storage)) = window.local_storage() else {
            return Settings::default();
        };
        let Ok(Some(json)) = storage.get_item(KEY) else {
            return Settings::default();
        };
        serde_json::from_str(&json).unwrap_or_default()
    }

    pub fn save(settings: &Settings) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(Some(storage)) = window.local_storage() else {
            return;
        };
        if let Ok(json) = serde_json::to_string(settings) {
            let _ = storage.set_item(KEY, &json);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{load, save};

#[cfg(target_arch = "wasm32")]
pub use web::{load, save};

/// Erstellt die Settings: geladene Werte oder Mobile-Default (Panel unten).
pub fn load_or_detect() -> Settings {
    let mut s = load();
    // Wenn noch keine Settings gespeichert waren, auf Mobile initial "Unten".
    if is_mobile() && s.panel_side == crate::model::PanelSide::Right {
        s.panel_side = crate::model::PanelSide::Bottom;
    }
    s
}

/// Grobe Mobile-Erkennung (Viewport-Seitenverhältnis oder Touch).
pub fn is_mobile() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(window) = web_sys::window() {
            // Heuristik: schmaler als 768px oder Touch-Gerät.
            let w = window.inner_width().map(|v| v.as_f64().unwrap_or(1000.0)).unwrap_or(1000.0);
            let h = window.inner_height().map(|v| v.as_f64().unwrap_or(1000.0)).unwrap_or(1000.0);
            let narrow = w < 768.0;
            let portrait = h > w;
            return narrow && portrait;
        }
    }
    false
}
