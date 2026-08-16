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

use crate::io::{Project, ProjectFont, ProjectImage};
use crate::model::Document;
use crate::store::{FontStore, ImageStore};

// -----------------------------------------------------------------------------
// Event-Queue (Brücke async → sync ui-Loop)
// -----------------------------------------------------------------------------

/// Events, die vom Hintergrund-Task an den ui-Loop gesendet werden.
pub enum WebEvent {
    /// Initiales Laden — der Stand wird unverändert übernommen.
    Loaded { json: String, version: u64 },
    /// Der Server hat einen neueren Stand. Muss mit dem lokalen Stand
    /// **zusammengeführt** werden, nicht ihn ersetzen.
    RemoteChanged { json: String, version: u64 },
    /// Speichern war erfolgreich. `version` ist der neue Serverstand.
    Saved { version: u64 },
    /// Speichern abgelehnt, weil jemand anders zuerst war (HTTP 409).
    /// Der mitgelieferte Serverstand wird gemergt und erneut gespeichert.
    SaveConflict { json: String, version: u64 },
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
    /// Serverseitige Versionsnummer des zuletzt gesehenen Standes.
    ///
    /// Grundlage der optimistischen Nebenläufigkeit: Beim Speichern wird sie
    /// mitgeschickt. Hat der Server inzwischen eine höhere Version, lehnt er
    /// mit 409 ab, statt fremde Arbeit zu überschreiben.
    pub version: u64,
    /// Der zuletzt mit dem Server abgeglichene Dokumentstand.
    ///
    /// Das ist die `base` des Drei-Wege-Merges — ohne sie ließe sich nicht
    /// unterscheiden, ob ein Unterschied eine eigene Änderung oder eine fremde
    /// ist.
    pub base_doc: Option<Document>,
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

        // Pfad: /<slug> (saubere URL, z. B. boxdoc.at/lebenslauf)
        let path = location.pathname().ok()?;
        let mut slug = String::new();

        // Letztes Pfad-Segment prüfen, falls es wie ein Slug aussieht.
        if let Some(rest) = path.rsplit('/').next() {
            if rest.len() >= 4 && rest.len() <= 32
                && rest.chars().all(|c| c.is_ascii_alphanumeric())
                && rest.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
            {
                slug = rest.to_string();
            }
        }

        // Query-Parameter ?doc=<slug> hat Vorrang (Fallback ohne Rewriting)
        let query = location.search().unwrap_or_default();
        if let Some(qs) = url_param(&query, "doc") {
            slug = qs;
        }

        if slug.is_empty() || !is_valid_slug(&slug) {
            return None;
        }

        let token = url_param(&query, "t").unwrap_or_default();

        // Basis-URL: Origin (Server-Root). api.php liegt im Root.
        let base = location.origin().ok().unwrap_or_default();

        Some(WebDoc {
            slug,
            token,
            base,
            last_known_modified: 0.0,
            last_content_hash: 0,
            version: 0,
            base_doc: None,
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
            version: 0,
            base_doc: None,
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

    /// Geteilbare URL für Nutzer (saubere URL: boxdoc.at/<slug>).
    pub fn share_url(&self) -> String {
        if self.token.is_empty() {
            format!("{}/{}", self.base, self.slug)
        } else {
            format!("{}/{}?t={}", self.base, self.slug, self.token)
        }
    }

    /// Lädt das Dokument vom Server. Liefert (Document, ImageStore, FontStore, next_id).
    pub async fn load(&mut self) -> Result<(Document, ImageStore, FontStore, u64), String> {
        let url = self.api_get_url();
        let text = fetch_text(&url).await?;

        let project: Project = serde_json::from_str(&text)
            .map_err(|e| format!("JSON-Parser: {}", e))?;

        let (doc, images, fonts, next_id) = decode_project(project);

        self.last_content_hash = hash_str(&text);
        Ok((doc, images, fonts, next_id))
    }

    /// Speichert das Dokument serverseitig (PUT, debounced vom Aufrufer).
    pub async fn save(
        &mut self,
        doc: &Document,
        images: &ImageStore,
        fonts: &FontStore,
    ) -> Result<(), String> {
        let body = serialize_project(doc, images, fonts)?;
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

    /// Startet den initialen Lade-Vorgang.
    /// Bevorzugt: eingebettetes JSON aus der HTML-Seite lesen (sofort da,
    /// kein extra Fetch). Fallback: fetch an die API.
    pub fn spawn_initial_load(&self) {
        // 1. Eingebettetes JSON aus <script id="boxdoc-content"> lesen.
        //    Version 0, damit der erste Poll die echte Version nachzieht.
        if let Some(embedded) = read_embedded_content() {
            if !embedded.trim().is_empty() && embedded.trim() != "{}" {
                push_event(WebEvent::Loaded {
                    json: embedded,
                    version: 0,
                });
                return;
            }
        }
        // 2. Fallback: per fetch laden, inklusive Version.
        let url = self.api_get_url();
        let meta_url = format!("{}/api.php?meta={}&t={}", self.base, self.slug, self.token);
        wasm_bindgen_futures::spawn_local(async move {
            let version = fetch_text(&meta_url)
                .await
                .ok()
                .and_then(|t| serde_json::from_str::<VersionResponse>(&t).ok())
                .map(|m| m.version)
                .unwrap_or(0);
            match fetch_text(&url).await {
                Ok(text) => push_event(WebEvent::Loaded {
                    json: text,
                    version,
                }),
                Err(e) => push_event(WebEvent::Error(format!("Laden fehlgeschlagen: {}", e))),
            }
        });
    }

    /// Fragt beim Server nur die **Versionsnummer** ab und lädt das Dokument
    /// ausschließlich dann, wenn es sich tatsächlich geändert hat.
    ///
    /// Vorher wurde alle 2 Sekunden das komplette Dokument samt aller
    /// base64-Bilder geladen — bei einem 3-MB-Dokument rund 1,5 MB/s pro
    /// offenem Tab. Jetzt kostet ein unveränderter Poll ein paar Dutzend Bytes.
    pub fn spawn_poll(&self) {
        let meta_url = format!(
            "{}/api.php?meta={}&t={}",
            self.base, self.slug, self.token
        );
        let get_url = self.api_get_url();
        let known_version = self.version;

        wasm_bindgen_futures::spawn_local(async move {
            let Ok(meta_text) = fetch_text(&meta_url).await else {
                // Polling-Fehler still ignorieren, nächster Versuch später.
                return;
            };
            let Ok(meta) = serde_json::from_str::<VersionResponse>(&meta_text) else {
                return;
            };
            if meta.version <= known_version {
                return; // nichts Neues
            }
            // Erst jetzt das eigentliche Dokument holen.
            if let Ok(text) = fetch_text(&get_url).await {
                push_event(WebEvent::RemoteChanged {
                    json: text,
                    version: meta.version,
                });
            }
        });
    }

    /// Startet einen asynchronen PUT mit `If-Match`-Semantik.
    ///
    /// Der Server akzeptiert nur, wenn die mitgeschickte Version noch die
    /// aktuelle ist. Andernfalls antwortet er mit 409 und dem neueren Stand —
    /// der wird dann gemergt statt überschrieben.
    pub fn spawn_save(
        &self,
        doc: Document,
        images: ImageStore,
        fonts: FontStore,
        version: u64,
    ) {
        let url = format!("{}&version={}", self.api_put_url(), version);
        wasm_bindgen_futures::spawn_local(async move {
            let body = match serialize_project(&doc, &images, &fonts) {
                Ok(b) => b,
                Err(e) => {
                    push_event(WebEvent::Error(format!("Serialize: {}", e)));
                    return;
                }
            };
            match put_text(&url, &body).await {
                Ok(PutOutcome::Ok { version }) => {
                    push_event(WebEvent::Saved { version });
                }
                Ok(PutOutcome::Conflict { json, version }) => {
                    push_event(WebEvent::SaveConflict { json, version });
                }
                Err(e) => push_event(WebEvent::Error(format!("Speichern fehlgeschlagen: {}", e))),
            }
        });
    }
}

/// Antwort des `?meta=`-Endpunkts — bewusst winzig.
#[derive(Serialize, Deserialize)]
struct VersionResponse {
    version: u64,
}

/// Antwort auf einen erfolgreichen PUT.
#[derive(Serialize, Deserialize)]
struct PutResponse {
    #[serde(default)]
    version: u64,
}

/// Ergebnis eines PUT-Versuchs.
pub enum PutOutcome {
    /// Gespeichert; `version` ist der neue Serverstand.
    Ok { version: u64 },
    /// Abgelehnt (409) — jemand anders war schneller. `json` ist der aktuelle
    /// Serverstand, den der Client jetzt einmergen muss.
    Conflict { json: String, version: u64 },
}

// -----------------------------------------------------------------------------
// Projekt-Encode/Decode (Shared zwischen WebDoc und App)
// -----------------------------------------------------------------------------

/// Dekodiert ein serialisiertes Project (Bilder + Fonts + max-id).
pub fn decode_project(project: Project) -> (Document, ImageStore, FontStore, u64) {
    let mut images = ImageStore::default();
    let mut fonts = FontStore::default();
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
    for pf in project.fonts {
        let ttf = base64::engine::general_purpose::STANDARD
            .decode(&pf.ttf_base64)
            .unwrap_or_default();
        fonts.insert(pf.name, ttf);
    }
    for page in &project.doc.pages {
        for el in &page.elements {
            max_id = max_id.max(el.id);
        }
    }
    (project.doc, images, fonts, max_id + 1)
}

/// Serialisiert den aktuellen Stand zu JSON.
pub fn serialize_project(
    doc: &Document,
    images: &ImageStore,
    fonts: &FontStore,
) -> Result<String, String> {
    let project_fonts: Vec<ProjectFont> = fonts
        .map
        .iter()
        .map(|(name, e)| ProjectFont {
            name: name.clone(),
            ttf_base64: base64::engine::general_purpose::STANDARD.encode(&e.ttf),
        })
        .collect();
    let project_images: Vec<ProjectImage> = images
        .map
        .iter()
        .map(|(id, e)| ProjectImage {
            id: *id,
            png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
        })
        .collect();
    let project = Project::for_save(doc.clone(), project_fonts, project_images);
    serde_json::to_string(&project).map_err(|e| format!("serialize: {}", e))
}

// -----------------------------------------------------------------------------
// Hilfsfunktionen
// -----------------------------------------------------------------------------

fn is_valid_slug(s: &str) -> bool {
    s.len() >= 4
        && s.len() <= 32
        && s.chars().all(|c| c.is_ascii_alphanumeric())
        && s.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
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

pub fn detect_base() -> String {
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

async fn put_text(url: &str, body: &str) -> Result<PutOutcome, String> {
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

    let status = resp.status();
    let text = JsFuture::from(resp.text().map_err(|e| format!("text: {:?}", e))?)
        .await
        .map_err(|e| format!("text await: {:?}", e))?
        .as_string()
        .unwrap_or_default();

    // 409 = jemand anders hat zwischenzeitlich gespeichert. Der Body enthält
    // den aktuellen Serverstand, damit der Client sofort mergen kann, ohne
    // einen weiteren Roundtrip.
    if status == 409 {
        let version = serde_json::from_str::<ConflictResponse>(&text)
            .map(|c| c.version)
            .unwrap_or(0);
        let doc_json = serde_json::from_str::<ConflictResponse>(&text)
            .ok()
            .and_then(|c| c.document)
            .unwrap_or(text);
        return Ok(PutOutcome::Conflict {
            json: doc_json,
            version,
        });
    }

    if !(200..300).contains(&status) {
        return Err(format!("HTTP {}", status));
    }

    let version = serde_json::from_str::<PutResponse>(&text)
        .map(|r| r.version)
        .unwrap_or(0);
    Ok(PutOutcome::Ok { version })
}

/// Antwort des Servers bei einem Versionskonflikt (HTTP 409).
#[derive(Serialize, Deserialize)]
struct ConflictResponse {
    #[serde(default)]
    version: u64,
    /// Der aktuelle Serverstand als JSON-String.
    #[serde(default)]
    document: Option<String>,
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

// -----------------------------------------------------------------------------
// File-Listing (für den Datei-Browser)
// -----------------------------------------------------------------------------

static FILE_LIST: Mutex<Option<String>> = Mutex::new(None);

/// Startet einen asynchronen Fetch der Dokumentenliste (?list=1).
pub fn spawn_file_list(base: &str) {
    let url = format!("{}/api.php?list=1", base);
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(text) = fetch_text(&url).await {
            if let Ok(mut slot) = FILE_LIST.lock() {
                *slot = Some(text);
            }
        }
    });
}

/// Nimmt das Ergebnis von spawn_file_list ab (JSON-String), falls bereit.
pub fn take_file_list() -> Option<String> {
    FILE_LIST.lock().ok()?.take()
}

/// Navigiert den Browser zu einem Dokument-Slug (?reload, lädt neu).
pub fn navigate_to(slug: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_href(&format!("/{}", slug));
    }
}
