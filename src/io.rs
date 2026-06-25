//! Datei-Dialoge sowie Speichern/Laden des nativen Projektformats (.boxdoc).

use std::path::PathBuf;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::app::EditorApp;
use crate::model::Document;
use crate::odt;
use crate::printing;
use crate::store::ImageStore;

#[derive(Serialize, Deserialize)]
struct ProjectImage {
    id: u64,
    png_base64: String,
}

#[derive(Serialize, Deserialize)]
struct Project {
    doc: Document,
    images: Vec<ProjectImage>,
}

/// Öffnet einen Datei-Dialog und lädt ein Projekt.
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
    match odt::export(&path, &app.doc, &app.images) {
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
    match odt::import(&path) {
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
    match printing::export_pdf(&path, &app.doc, &app.images) {
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

fn save_project(path: &std::path::Path, app: &EditorApp) -> std::io::Result<()> {
    let images: Vec<ProjectImage> = app
        .images
        .map
        .iter()
        .map(|(id, e)| ProjectImage {
            id: *id,
            png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
        })
        .collect();
    let project = Project { doc: app.doc.clone(), images };
    let json = serde_json::to_string_pretty(&project)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, json)
}

fn load_project(path: &std::path::Path) -> std::io::Result<(Document, ImageStore, u64)> {
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
