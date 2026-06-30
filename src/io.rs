//! Datei-Dialoge sowie Speichern/Laden des Projektformats (.boxdoc).
//!
//! Auf Native: rfd-Dialoge + std::fs.
//! Auf Web: Drag&Drop (läuft über egui) + Browser-Download.

use serde::{Deserialize, Serialize};

use crate::app::EditorApp;
use crate::model::Document;
use crate::store::ImageStore;

#[derive(Serialize, Deserialize)]
pub struct ProjectImage {
    pub id: u64,
    pub png_base64: String,
}

/// Vollständige AI-Anleitung, die in jede .boxdoc-Datei eingebettet wird,
/// damit opencode (und jeder andere Agent) sofort weiß, wie das Format
/// funktioniert und wie man es bearbeitet. BoxDoc ignoriert dieses Feld beim
/// Laden; es dient ausschließlich der Maschinen-Lesbarkeit.
pub const AI_HINT: &str = r#"BoxDoc-Dokument-Format (.boxdoc) — Anleitung für KI-Agenten
=======================================================

Diese Datei ist die komplette AI-Schnittstelle. Es gibt kein zusätzliches
Protokoll, keine API, keine Sockets. Du (die KI) liest und bearbeitest die
Datei direkt mit deinen normalen Datei-Werkzeugen — wie eine Quellcode-Datei.

Wenn BoxDoc läuft und diese Datei geöffnet hat, übernimmt es jede externe
Änderung automatisch (≤ 300 ms) als Undo-Schritt. Der Nutzer sieht sie live
und kann sie mit Strg+Z zurückrollen.

DATEIFORMAT
-----------
{
  "doc": {
    "format": "A4" | "A3" | "A5" | "Letter" | "Legal",
    "orientation": "Portrait" | "Landscape",
    "pages": [ { "elements": [ <Element>, ... ] } ]
  },
  "images": [ { "id": <u64>, "png_base64": "<base64-PNG-Bytes>" } ]
}

ELEMENT (je nach "kind" sind verschiedene Felder relevant)
---------------------------------------------------------
{
  "id": <u64>,                       // stabil, niemals ändern beim Update
  "kind": "Text" | "Image" | "Rectangle" | "Line",
  "x": <f32 pt>,                     // linke obere Ecke (unrotiert)
  "y": <f32 pt>,
  "w": <f32 pt>,                     // Breite
  "h": <f32 pt>,                     // Höhe (bei "Line" = 0)
  "rotation": <Grad>,                // gegen Uhrzeigersinn
  "text": "<Inhalt>",                // kann \n enthalten
  "font_size": <pt>,
  "font": "default"|"inter"|"roboto"|"lora"|"jetbrains"|"pacifico",
  "color": [r, g, b, a],             // 0..255; a=255 deckend
  "bold": <bool>, "italic": <bool>, "underline": <bool>,
  "align": "Left"|"Center"|"Right", "valign": "Top"|"Middle"|"Bottom",
  "indent": <pt>,
  "crop": { "x":0.0, "y":0.0, "w":1.0, "h":1.0 },  // Image; normalisiert 0..1
  "image_w": <px>, "image_h": <px>,                // Image
  "fill_color": [r,g,b,a],           // Shape; Alpha 0 = transparent
  "stroke_width": <pt>,              // Shape; 0 = kein Rahmen
  "stroke_color": [r,g,b,a],         // Shape
  "corner_radius": <pt>              // Rectangle
}

ELEMENT-TYPEN
-------------
Text       : text, font_size, font, color, bold, italic, underline, align, valign
Rectangle  : fill_color, stroke_width, stroke_color, corner_radius
Line       : stroke_width, stroke_color (Linie = Box mit h=0 + rotation)
Image      : id (verweist auf images[].id), crop, image_w, image_h

KOOORDINATENSYSTEM
------------------
- Maßeinheit: Punkt (1 pt = 1/72 Zoll; 1 Zoll = 25,4 mm)
- Ursprung: oben-links, y zeigt nach UNTEN
- A4 Hochformat: 595 x 842 pt | A4 Querformat: 842 x 595 pt
- 1 cm ≈ 28,3 pt | 1 mm ≈ 2,83 pt
- Z-Order: Elemente weiter hinten im Array liegen OBEN (werden zuletzt gezeichnet)

REGELN FÜR DIE KI
-----------------
1. Lies die Datei als JSON, verstehe das Dokument.
2. Ändere Felder direkt mit deinen normalen Edit-Tools.
3. IDs sind u64 und stabil. Beim Aktualisieren nur existierende IDs verwenden.
   Für neue Elemente: höchste vorhandene ID + 1.
4. Bilder (images, png_base64, image_w, image_h) UNVERÄNDERT lassen.
5. Nach jedem Speichern übernimmt BoxDoc die Änderung automatisch (≤ 300 ms).
6. Der Nutzer kann mit Strg+Z zurückrollen; BoxDoc schreibt den alten Stand zurück.
7. Ungültiges JSON wird still ignoriert — teilschreibende Dateien sind unkritisch.

DESIGN-LEITFADEN
----------------
- Titel 28-36pt bold, Überschrift 18-22pt bold, Fließtext 10-12pt, Footer 8-9pt
- Zeilenabstand: ca. 1,3x Schriftgröße
- Seitenrand: mindestens ~50 pt (≈ 1,8 cm)
- Eine Hauptfarbe + eine Akzentfarbe für ein ruhiges Bild
- Aufzählungen mit "• " prefixen, eine Zeile pro Punkt
- Z-Order: Dekorationen (Hintergrundbalken) VORNE im Array, Text HINTEN

BEISPIEL: Neues Text-Element hinzufügen (an elements anhängen)
--------------------------------------------------------------
{
  "id": <nächste freie ID>, "kind": "Text",
  "x": 100.0, "y": 200.0, "w": 400.0, "h": 40.0, "rotation": 0.0,
  "text": "Neuer Absatz", "font_size": 14.0, "font": "default",
  "color": [20,20,20,255], "bold": false, "italic": false, "underline": false,
  "align": "Left", "valign": "Top", "indent": 0.0,
  "crop": {"x":0,"y":0,"w":1,"h":1}, "image_w": 0, "image_h": 0,
  "fill_color": [80,140,220,60], "stroke_width": 2.0,
  "stroke_color": [40,100,180,255], "corner_radius": 0.0
}

Hinweis: Dieses Feld (_ai_hint) wird von BoxDoc beim Laden ignoriert.
Du kannst es beim Bearbeiten unverändert lassen oder löschen — BoxDoc wird
es beim nächsten Speichern wieder automatisch einfügen.
"#;

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub doc: Document,
    pub images: Vec<ProjectImage>,
    /// Selbst-Dokumentation für KI-Agenten. Wird beim Speichern automatisch
    /// eingefügt und beim Laden ignoriert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _ai_hint: Option<String>,
}

impl Project {
    /// Erzeugt ein Project mit eingebettetem AI-Hint (für den regulären
    /// Speichern-Fluss).
    pub fn for_save(doc: Document, images: Vec<ProjectImage>) -> Self {
        Project {
            doc,
            images,
            _ai_hint: Some(AI_HINT.to_string()),
        }
    }
}

// ===========================================================================
// Native
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::PathBuf;

    use base64::Engine;

    use super::{Document, EditorApp, ImageStore, Project, ProjectImage};

    type IoResult<T> = std::io::Result<T>;

    pub fn open_project_dialog(app: &mut EditorApp) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("BoxDoc-Projekt", &["boxdoc"])
            .set_title("Dokument öffnen")
            .pick_file()
        else {
            return;
        };
        match load_project(&path) {
            Ok((doc, images, next_id)) => {
                app.doc = doc;
                app.images = images;
                app.page_index = 0;
                app.next_id = next_id;
                app.clear_selection();
                app.editing = None;
                app.crop_mode = false;
                app.interaction = crate::app::Interaction::None;
                app.file_path = Some(path);
                app.modified = false;
                app.set_status("Dokument geöffnet.");
            }
            Err(e) => app.set_status(format!("Fehler beim Öffnen: {e}")),
        }
    }

    pub fn save_project_dialog(app: &mut EditorApp, save_as: bool) {
        let path = if !save_as {
            app.file_path.clone()
        } else {
            None
        };
        let path = match path {
            Some(p) => p,
            None => {
                let mut dlg = rfd::FileDialog::new()
                    .add_filter("BoxDoc-Projekt", &["boxdoc"])
                    .set_title("Dokument speichern");
                if let Some(start) = default_name(app) {
                    dlg = dlg.set_file_name(start);
                }
                match dlg.save_file() {
                    Some(p) => ensure_ext(p, "boxdoc"),
                    None => return,
                }
            }
        };

        match save_project(&path, app) {
            Ok(()) => {
                app.file_path = Some(path);
                app.modified = false;
                app.set_status("Gespeichert.");
            }
            Err(e) => app.set_status(format!("Fehler beim Speichern: {e}")),
        }
    }

    pub fn open_image_dialog(app: &mut EditorApp) {
        let files = rfd::FileDialog::new()
            .add_filter("Bild", &["png", "jpg", "jpeg", "bmp", "webp", "ico"])
            .set_title("Bild auswählen")
            .pick_files();
        for f in files.unwrap_or_default() {
            if let Ok(bytes) = std::fs::read(&f) {
                app.add_image_from_bytes(bytes, None);
            }
        }
    }

    /// Liefert ein Bild aus der Zwischenablage (Windows).
    #[cfg(target_os = "windows")]
    pub fn poll_clipboard_image() -> Option<Vec<u8>> {
        // Clipboard-Bild als BMP über eine temporäre Datei via PowerShell lesen.
        let temp = std::env::temp_dir().join("boxdoc_clip.png");
        let ps = format!(
            r#"$ErrorActionPreference='SilentlyContinue'; Add-Type -AssemblyName System.Windows.Forms; $img=[System.Windows.Forms.Clipboard]::GetImage(); if ($img) {{ $img.Save('{}') }}"#,
            temp.display()
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .output();
        if temp.exists() {
            let bytes = std::fs::read(&temp).ok();
            let _ = std::fs::remove_file(&temp);
            bytes.filter(|b| b.starts_with(&[0x89, b'P', b'N', b'G']))
        } else {
            None
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn poll_clipboard_image() -> Option<Vec<u8>> {
        None
    }

    pub fn export_odt_dialog(app: &mut EditorApp) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("OpenDocument", &["odt"])
            .set_title("Als ODT exportieren");
        if let Some(start) = default_name(app) {
            let n = format!("{}.odt", start.trim_end_matches(".boxdoc"));
            dlg = dlg.set_file_name(n);
        }
        let Some(path) = dlg.save_file().map(|p| ensure_ext(p, "odt")) else {
            return;
        };
        match crate::odt::export(&path, &app.doc, &app.images) {
            Ok(()) => app.set_status(format!("ODT exportiert: {}", path.display())),
            Err(e) => app.set_status(format!("ODT-Export fehlgeschlagen: {e}")),
        }
    }

    pub fn import_odt_dialog(app: &mut EditorApp) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("OpenDocument", &["odt"])
            .set_title("ODT öffnen")
            .pick_file()
        else {
            return;
        };
        match crate::odt::import(&path) {
            Ok((doc, images, next_id)) => {
                app.doc = doc;
                app.images = images;
                app.page_index = 0;
                app.next_id = next_id;
                app.clear_selection();
                app.editing = None;
                app.crop_mode = false;
                app.interaction = crate::app::Interaction::None;
                app.file_path = Some(path);
                app.modified = false;
                app.set_status("ODT geöffnet.");
            }
            Err(e) => app.set_status(format!("Fehler beim ODT-Lesen: {e}")),
        }
    }

    pub fn export_pdf(app: &mut EditorApp, path: PathBuf) {
        match crate::printing::export_pdf(&path, &app.doc, &app.images) {
            Ok(()) => app.set_status(format!("PDF exportiert: {}", path.display())),
            Err(e) => app.set_status(format!("PDF-Export fehlgeschlagen: {e}")),
        }
    }

    fn default_name(app: &EditorApp) -> Option<String> {
        Some(
            app.file_path
                .as_ref()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                .unwrap_or_else(|| String::from("dokument")),
        )
    }

    fn ensure_ext(path: PathBuf, ext: &str) -> PathBuf {
        if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            path
        } else {
            path.with_extension(ext)
        }
    }

    pub fn save_project(path: &std::path::Path, app: &EditorApp) -> std::io::Result<()> {
        let images: Vec<ProjectImage> = app
            .images
            .map
            .iter()
            .map(|(id, e)| ProjectImage {
                id: *id,
                png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
            })
            .collect();
        let project = Project::for_save(app.doc.clone(), images);
        let json = serde_json::to_string_pretty(&project)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    pub fn load_project(path: &std::path::Path) -> std::io::Result<(Document, ImageStore, u64)> {
        let json = std::fs::read_to_string(path)?;
        let project: Project = serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut images = ImageStore::default();
        let mut max_id = 0u64;
        for img in project.images {
            let png = base64::engine::general_purpose::STANDARD
                .decode(&img.png_base64)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let dim = image::load_from_memory(&png)
                .map(|i| (i.width(), i.height()))
                .unwrap_or((0, 0));
            images.insert(img.id, png, dim);
        }
        for page in &project.doc.pages {
            for el in &page.elements {
                max_id = max_id.max(el.id);
            }
        }
        Ok((project.doc, images, max_id + 1))
    }
}

#[cfg(target_arch = "wasm32")]
mod web_impl {
    use super::{EditorApp, Project, ProjectImage};
    use base64::Engine;

    pub fn open_project_dialog(app: &mut EditorApp) {
        super::trigger_project_file_input();
        app.set_status(".boxdoc-Datei auswählen…");
    }

    pub fn save_project_dialog(app: &mut EditorApp, _save_as: bool) {
        let images: Vec<ProjectImage> = app
            .images
            .map
            .iter()
            .map(|(id, e)| ProjectImage {
                id: *id,
                png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
            })
            .collect();
        let project = Project::for_save(app.doc.clone(), images);
        match serde_json::to_string_pretty(&project) {
            Ok(json) => {
                download_file(&json, "dokument.boxdoc", "application/json");
                app.modified = false;
                app.set_status("Dokument heruntergeladen.");
            }
            Err(e) => app.set_status(format!("Fehler: {e}")),
        }
    }

    pub fn open_image_dialog(app: &mut EditorApp) {
        super::trigger_file_input();
        app.set_status("Bild auswählen…");
    }

    pub fn poll_clipboard_image() -> Option<Vec<u8>> {
        None // auf Web übernimmt der paste-Listener + take_pending_image
    }

    pub fn export_odt_dialog(app: &mut EditorApp) {
        app.set_status("ODT-Export wird auf Web noch nicht unterstützt.");
    }

    pub fn import_odt_dialog(app: &mut EditorApp) {
        app.set_status("ODT-Import wird auf Web noch nicht unterstützt.");
    }

    pub fn export_pdf(app: &mut EditorApp, _path: std::path::PathBuf) {
        app.set_status("PDF-Export wird auf Web noch nicht unterstützt.");
    }

    fn download_file(content: &str, filename: &str, mime: &str) {
        use js_sys::Uint8Array;
        use wasm_bindgen::JsCast;
        use web_sys::{Blob, BlobPropertyBag};

        let bytes = content.as_bytes();
        let array = Uint8Array::new_with_length(bytes.len() as u32);
        array.copy_from(bytes);

        let mut props = BlobPropertyBag::new();
        props.type_(mime);
        let blob = Blob::new_with_u8_array_sequence_and_options(
            &js_sys::Array::of1(&array.into()),
            &props,
        )
        .unwrap();

        let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        let anchor = document
            .create_element("a")
            .unwrap()
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .unwrap();
        anchor.set_href(&url);
        anchor.set_download(filename);
        anchor.click();
        web_sys::Url::revoke_object_url(&url).ok();
    }
}

// ===========================================================================
// Öffentliche API — dispatch je nach Plattform
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
pub use web_impl::*;

// ===========================================================================
// Web: Globaler Puffer für asynchron geladene Dateien
// ===========================================================================

#[cfg(target_arch = "wasm32")]
static PENDING_IMAGE: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

#[cfg(target_arch = "wasm32")]
static PENDING_PROJECT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(target_arch = "wasm32")]
fn trigger_file_input() {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlInputElement;

    let document = web_sys::window().unwrap().document().unwrap();
    let input = document
        .create_element("input")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    input.set_type("file");
    input.set_accept("image/png,image/jpeg,image/bmp,image/webp");
    input.set_multiple(false);

    let onchange: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> =
        wasm_bindgen::closure::Closure::new(move |event: web_sys::Event| {
            let input: Option<HtmlInputElement> = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
            let Some(input) = input else { return };
            let Some(file) = input.files().and_then(|f| f.get(0)) else {
                return;
            };

            let reader = web_sys::FileReader::new().unwrap();
            let _ = reader.read_as_array_buffer(&file);

            let onload: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> = {
                let reader = reader.clone();
                wasm_bindgen::closure::Closure::new(move |_e: web_sys::Event| {
                    if let Ok(result) = reader.result() {
                        let uint8 = js_sys::Uint8Array::new(&result).to_vec();
                        if let Ok(mut p) = PENDING_IMAGE.lock() {
                            *p = Some(uint8);
                        }
                    }
                })
            };
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
        });

    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    input.click();
}

#[cfg(target_arch = "wasm32")]
pub fn trigger_project_file_input() {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlInputElement;

    let document = web_sys::window().unwrap().document().unwrap();
    let input = document
        .create_element("input")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    input.set_type("file");
    input.set_accept(".boxdoc,application/json");
    input.set_multiple(false);

    let onchange: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> =
        wasm_bindgen::closure::Closure::new(move |event: web_sys::Event| {
            let input: Option<HtmlInputElement> = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
            let Some(input) = input else { return };
            let Some(file) = input.files().and_then(|f| f.get(0)) else {
                return;
            };

            let reader = web_sys::FileReader::new().unwrap();
            let _ = reader.read_as_text(&file);

            let onload: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> = {
                let reader = reader.clone();
                wasm_bindgen::closure::Closure::new(move |_e: web_sys::Event| {
                    if let Ok(result) = reader.result() {
                        if let Some(text) = result.as_string() {
                            if let Ok(mut p) = PENDING_PROJECT.lock() {
                                *p = Some(text);
                            }
                        }
                    }
                })
            };
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
        });

    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    input.click();
}

/// Registriert einen paste-Listener auf Window-Ebene, der Bilder aus der
/// Zwischenablage abfängt. Muss einmal beim Start aufgerufen werden.
#[cfg(target_arch = "wasm32")]
pub fn install_clipboard_paste_listener() {
    use wasm_bindgen::JsCast;
    let cb: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> =
        wasm_bindgen::closure::Closure::new(|event: web_sys::Event| {
            let Some(evt) = event.dyn_ref::<web_sys::ClipboardEvent>() else {
                return;
            };
            let Some(data) = evt.clipboard_data() else {
                return;
            };
            let Some(files) = data.files() else { return };
            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };
                if file.type_().starts_with("image/") {
                    let reader = web_sys::FileReader::new().unwrap();
                    let _ = reader.read_as_array_buffer(&file);
                    let onload: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> = {
                        let reader = reader.clone();
                        wasm_bindgen::closure::Closure::new(move |_e: web_sys::Event| {
                            if let Ok(result) = reader.result() {
                                let bytes = js_sys::Uint8Array::new(&result).to_vec();
                                if let Ok(mut p) = PENDING_IMAGE.lock() {
                                    *p = Some(bytes);
                                }
                            }
                        })
                    };
                    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                    onload.forget();
                    break;
                }
            }
        });
    let window = web_sys::window().unwrap();
    let _ = window.add_event_listener_with_callback("paste", cb.as_ref().unchecked_ref());
    cb.forget();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn install_clipboard_paste_listener() {}

/// Auf Web: gibt die zuletzt geladene Projekt-JSON zurück und leert den Puffer.
#[cfg(target_arch = "wasm32")]
pub fn take_pending_project() -> Option<String> {
    PENDING_PROJECT.lock().unwrap().take()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_pending_project() -> Option<String> {
    None
}

/// Auf Web: gibt die zuletzt geladenen Bild-Bytes zurück (falls vorhanden)
/// und leert den Puffer. Auf Native immer `None`.
#[cfg(target_arch = "wasm32")]
pub fn take_pending_image() -> Option<Vec<u8>> {
    PENDING_IMAGE.lock().unwrap().take()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_pending_image() -> Option<Vec<u8>> {
    None
}
