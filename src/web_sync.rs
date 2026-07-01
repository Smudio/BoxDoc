//! Web-Sync: Anbindung an das PHP-Dokumenten-Backend.
//!
//! Browser-seitige Entsprechung zu `file_watch` auf Native. Statt einer Datei
//! auf Platte beobachten wir eine Dokumenten-URL: laden, pollen, speichern.
//!
//! Funktioniert nur im Browser (WASM).

#![cfg(target_arch = "wasm32")]
#![allow(dead_code)]

use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode, Response};

use crate::io::{Project, ProjectImage};
use crate::model::Document;
use crate::store::ImageStore;

// -----------------------------------------------------------------------------
// Event-Queue (Brücke async → sync ui-Loop)
// -----------------------------------------------------------------------------

/// Events, die vom Hintergrund-Task an den ui-Loop gesendet werden.
pub enum WebEvent {
    /// Initiales Laden oder Polling hat neuen Inhalt geliefert.
    Loaded(String),
    /// Speichern war erfolgreich.
    Saved,
    /// Ein Fehler ist aufgetreten.
    Error(String),
}

static WEB_QUEUE: Mutex<Vec<WebEvent>> = Mutex::new(Vec::new());

fn push_event(ev: WebEvent) {
    if let Ok(mut q) = WEB_QUEUE.lock() {
        q.push(ev);
    }
}

/// Liefert das nächste ausstehende Event, falls vorhanden (sync).
pub fn next_event() -> Option<WebEvent> {
    WEB_QUEUE.lock().ok()?.drain(..).next()
}

/// Repräsentiert ein serverseitig gespeichertes BoxDoc-Dokument.
pub struct WebDoc {
    pub slug: String,
    pub token: String,
    /// Basis-URL inkl. Pfad zum Backend (z. B. https://boxdoc.at).
    pub base: String,
    /// Zeitpunkt des letzten Ladens/Speicherns (Client-seitig, Sekunden).
    pub last_known_modified: f64,
    /// Hash des zuletzt geladenen Inhalts — zur Änderungserkennung beim Pollen.
    pub last_content_hash: u64,
}

#[derive(Serialize, Deserialize)]
struct MetaResponse {
    slug: String,
    api_get: String,
    api_put: String,
    modified: f64,
}

#[derive(Serialize, Deserialize)]
struct CreateResponse {
    slug: String,
    token: String,
    url: String,
}

impl WebDoc {
    /// Erzeugt ein WebDoc aus der aktuellen URL, falls ein Doc-Parameter
    /// vorhanden ist. Liefert None auf der Startseite.
    pub fn from_url() -> Option<Self> {
        let window = web_sys::window()?;
        let location = window.location();

        // Pfad: /d/<slug> ODER ?doc=<slug>
        let path = location.pathname().ok()?;
        let mut slug = String::new();

        if let Some(rest) = path.strip_prefix("/d/") {
            // /d/<slug>
            slug = rest.trim_end_matches('/').to_string();
        } else if let Some(rest) = path.rsplit('/').next() {
            // Letztes Pfad-Segment, falls es wie ein Slug aussieht.
            if rest.len() >= 8 && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                slug = rest.to_string();
            }
        }

        // Query-String: location.search() gibt Result<String, JsValue>.
        let query = location.search().unwrap_or_default();
        // Query-Parameter ?doc=<slug> hat Vorrang
        if let Some(qs) = url_param(&query, "doc") {
            slug = qs;
        }

        if slug.is_empty() || !is_valid_slug(&slug) {
            return None;
        }

        let token = url_param(&query, "t").unwrap_or_default();

        // Basis-URL: Protokoll + Host (+ evtl. Pfad-Präfix)
        let origin = location.origin().ok()?;
        let base = if path.starts_with("/d/") || path.ends_with(&slug) {
            // Wir sind unter /d/... — Basis ist der Root.
            origin.clone()
        } else {
            // Index.php liegt evtl. in einem Unterverzeichnis.
            let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            if dir.is_empty() {
                origin.clone()
            } else {
                format!("{}{}", origin, dir)
            }
        };

        Some(WebDoc {
            slug,
            token,
            base,
            last_known_modified: 0.0,
            last_content_hash: 0,
        })
    }

    /// Erzeugt ein neues Dokument serverseitig (POST ?new=1) und liefert
    /// das initialisierte WebDoc zurück.
    pub async fn create_new() -> Result<Self, String> {
        let base = detect_base();
        let url = format!("{}/api.php?new=1", base);

        let mut opts = RequestInit::new();
        opts.method("POST");
        opts.mode(RequestMode::Cors);

        let request = Request::new_with_str_and_init(&url, &opts)
            .map_err(|e| format!("Request fehlgeschlagen: {:?}", e))?;

        let window = web_sys::window().ok_or("kein window")?;
        let resp_val = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(|e| format!("fetch: {:?}", e))?;
        let resp: Response = resp_val.dyn_into().map_err(|e| format!("cast: {:?}", e))?;

        if !resp.ok() {
            return Err(format!("Server antwortet {}", resp.status()));
        }

        let json_text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
            .await
            .map_err(|e| format!("text await: {:?}", e))?
            .as_string()
            .ok_or("text nicht als String")?;

        let parsed: CreateResponse = serde_json::from_str(&json_text)
            .map_err(|e| format!("parse: {}", e))?;

        Ok(WebDoc {
            slug: parsed.slug,
            token: parsed.token,
            base,
            last_known_modified: 0.0,
            last_content_hash: 0,
        })
    }

    /// Liefert die GET-URL für die API.
    pub fn api_get_url(&self) -> String {
        format!("{}/api.php?get={}&t={}", self.base, self.slug, self.token)
    }

    /// Liefert die PUT-URL für die API.
    pub fn api_put_url(&self) -> String {
        format!("{}/api.php?put={}&t={}", self.base, self.slug, self.token)
    }

    /// Geteilbare URL für Nutzer.
    pub fn share_url(&self) -> String {
        format!("{}/d/{}?t={}", self.base, self.slug, self.token)
    }

    /// Lädt das Dokument vom Server. Liefert (Document, ImageStore, next_id).
    pub async fn load(&mut self) -> Result<(Document, ImageStore, u64), String> {
        let url = self.api_get_url();
        let text = fetch_text(&url).await?;

        let project: Project = serde_json::from_str(&text)
            .map_err(|e| format!("JSON-Parser: {}", e))?;

        let mut images = ImageStore::default();
        let mut max_id = 0u64;
        for img in project.images {
            let png = base64::engine::general_purpose::STANDARD
                .decode(&img.png_base64)
                .unwrap_or_default();
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

        self.last_content_hash = hash_str(&text);
        Ok((project.doc, images, max_id + 1))
    }

    /// Speichert das Dokument serverseitig (PUT, debounced vom Aufrufer).
    pub async fn save(
        &mut self,
        doc: &Document,
        images: &ImageStore,
    ) -> Result<(), String> {
        let project_images: Vec<ProjectImage> = images
            .map
            .iter()
            .map(|(id, e)| ProjectImage {
                id: *id,
                png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
            })
            .collect();
        let project = Project::for_save(doc.clone(), project_images);
        let body = serde_json::to_string(&project)
            .map_err(|e| format!("serialize: {}", e))?;

        put_text(&self.api_put_url(), &body).await?;

        // Hash aktualisieren, damit der nächste Poll unsere eigene Save nicht
        // als externe Änderung missversteht.
        self.last_content_hash = hash_str(&body);
        Ok(())
    }

    /// Prüft, ob sich das Dokument serverseitig geändert hat. Liefert true,
    /// wenn ein Reload nötig ist.
    pub async fn poll(&self) -> bool {
        let url = format!("{}/api.php?get={}&t={}", self.base, self.slug, self.token);
        let Ok(text) = fetch_text(&url).await else {
            return false;
        };
        hash_str(&text) != self.last_content_hash
    }

    // --- Spawn-Helfer (starten Hintergrund-Tasks, pushen in die Queue) ---

    /// Startet den initialen Lade-Vorgang per fetch.
    pub fn spawn_initial_load(&self) {
        let url = self.api_get_url();
        wasm_bindgen_futures::spawn_local(async move {
            match fetch_text(&url).await {
                Ok(text) => push_event(WebEvent::Loaded(text)),
                Err(e) => push_event(WebEvent::Error(format!("Laden fehlgeschlagen: {}", e))),
            }
        });
    }

    /// Startet einen Polling-Request. Bei Änderung wird `Loaded` gepusht.
    pub fn spawn_poll(&self) {
        let url = self.api_get_url();
        let known_hash = self.last_content_hash;
        wasm_bindgen_futures::spawn_local(async move {
            match fetch_text(&url).await {
                Ok(text) => {
                    if hash_str(&text) != known_hash {
                        push_event(WebEvent::Loaded(text));
                    }
                }
                Err(_e) => { /* Polling-Fehler still ignorieren, nächster Versuch später */ }
            }
        });
    }

    /// Startet einen asynchronen PUT. Bei Erfolg wird `Saved` gepusht.
    pub fn spawn_save(&self, doc: Document, images: ImageStore) {
        let url = self.api_put_url();
        wasm_bindgen_futures::spawn_local(async move {
            let project_images: Vec<ProjectImage> = images
                .map
                .iter()
                .map(|(id, e)| ProjectImage {
                    id: *id,
                    png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
                })
                .collect();
            let project = Project::for_save(doc, project_images);
            let body = match serde_json::to_string(&project) {
                Ok(b) => b,
                Err(e) => {
                    push_event(WebEvent::Error(format!("Serialize: {}", e)));
                    return;
                }
            };
            match put_text(&url, &body).await {
                Ok(_) => {
                    // Hash updaten, damit der nächste Poll unsere eigene Save
                    // nicht als externe Änderung fehlinterpretiert.
                    let _ = body; // hash wird im Haupt-Thread gesetzt
                    push_event(WebEvent::Saved);
                }
                Err(e) => push_event(WebEvent::Error(format!("Speichern fehlgeschlagen: {}", e))),
            }
        });
    }
}

// -----------------------------------------------------------------------------
// Hilfsfunktionen
// -----------------------------------------------------------------------------

fn is_valid_slug(s: &str) -> bool {
    s.len() >= 8 && s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn url_param(query: &str, key: &str) -> Option<String> {
    let q = query.trim_start_matches('?');
    for pair in q.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next()? == key {
            let v = it.next().unwrap_or("");
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                16,
            ) {
                out.push(b as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn detect_base() -> String {
    if let Some(w) = web_sys::window() {
        if let Ok(loc) = w.location().origin() {
            return loc;
        }
    }
    String::from(".")
}

pub fn hash_str(s: &str) -> u64 {
    // FNV-1a 64-bit — simpel, deterministisch, reicht für Änderungserkennung.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Öffentlicher Alias für App-Aufrufe.
pub fn hash_of(s: &str) -> u64 {
    hash_str(s)
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let window = web_sys::window().ok_or("kein window")?;
    let mut opts = RequestInit::new();
    opts.method("GET");
    opts.mode(RequestMode::Cors);

    let request = Request::new_with_str_and_init(url, &opts)
        .map_err(|e| format!("request: {:?}", e))?;

    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("cast: {:?}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("text await: {:?}", e))?
        .as_string()
        .ok_or("text nicht als String")?;
    Ok(text)
}

async fn put_text(url: &str, body: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or("kein window")?;
    let mut opts = RequestInit::new();
    opts.method("PUT");
    opts.mode(RequestMode::Cors);
    opts.body(Some(&js_sys::JsString::from(body).into()));

    let request = Request::new_with_str_and_init(url, &opts)
        .map_err(|e| format!("request: {:?}", e))?;

    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("fetch: {:?}", e))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("cast: {:?}", e))?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    Ok(())
}

/// Liest das eingebettete JSON aus der Seite, falls von PHP injiziert.
/// Ohne SSR-Seite liefert dies None — dann wird per fetch geladen.
pub fn read_embedded_content() -> Option<String> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let el = document.get_element_by_id("boxdoc-content")?;
    let script: web_sys::HtmlScriptElement = el.dyn_into().ok()?;
    let text = script.text().unwrap_or_default();
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}
