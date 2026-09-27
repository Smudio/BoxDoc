//! Die Anwendung: Zustand und egui-App-Implementierung.

use std::path::PathBuf;

use egui::{Align, Color32, Context, Frame, Layout, Vec2};
use serde::Deserialize;

use crate::canvas::show_canvas;
use crate::geometry::{local_corners, local_to_world};
use crate::model::{
    CustomFormat, Document, Element, ElementKind, mm_to_pt, Orientation, PageAlign, PaperFormat,
    pt_to_mm, ScrollMode, Settings, TextAlign, Units,
};
use crate::store::{FontStore, ImageStore};

/// Ankerpunkt der Bounding-Box für die Positionsanzeige.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BBoxAnchor {
    TopLeft,
    TopCenter,
    TopRight,
    MidLeft,
    Center,
    MidRight,
    BotLeft,
    BotCenter,
    BotRight,
}

impl Default for BBoxAnchor {
    fn default() -> Self {
        BBoxAnchor::TopLeft
    }
}

impl BBoxAnchor {
    /// (dx, dy) als Anteil von 0..1 innerhalb der Bounding-Box.
    pub fn frac(self) -> (f32, f32) {
        match self {
            BBoxAnchor::TopLeft => (0.0, 0.0),
            BBoxAnchor::TopCenter => (0.5, 0.0),
            BBoxAnchor::TopRight => (1.0, 0.0),
            BBoxAnchor::MidLeft => (0.0, 0.5),
            BBoxAnchor::Center => (0.5, 0.5),
            BBoxAnchor::MidRight => (1.0, 0.5),
            BBoxAnchor::BotLeft => (0.0, 1.0),
            BBoxAnchor::BotCenter => (0.5, 1.0),
            BBoxAnchor::BotRight => (1.0, 1.0),
        }
    }

    pub fn all() -> [BBoxAnchor; 9] {
        [
            BBoxAnchor::TopLeft,
            BBoxAnchor::TopCenter,
            BBoxAnchor::TopRight,
            BBoxAnchor::MidLeft,
            BBoxAnchor::Center,
            BBoxAnchor::MidRight,
            BBoxAnchor::BotLeft,
            BBoxAnchor::BotCenter,
            BBoxAnchor::BotRight,
        ]
    }
}

/// Ansicht (Zoom & Verschiebung) der Zeichenfläche.
#[derive(Clone)]
pub struct View {
    pub zoom: f32,
    pub pan: Vec2,
    /// Zeitpunkt des letzten Seitenwechsels durch Scrollen (egui-Zeit in s).
    /// Sperrt kurz weitere Wechsel, damit eine Mausrad-Raste nicht mehrere
    /// Seiten überspringt.
    pub last_page_flip: f64,
}

impl Default for View {
    fn default() -> Self {
        // Pan X = 0, denn die horizontale Ausrichtung wird dynamisch berechnet.
        View {
            zoom: 1.0,
            pan: Vec2::new(0.0, 24.0),
            last_page_flip: f64::NEG_INFINITY,
        }
    }
}

/// Welche Aktion gerade mit der Maus ausgeführt wird.
pub enum Interaction {
    None,
    /// Eines oder mehrere Objekte verschieben.
    DragBodies {
        start_pointer: egui::Pos2,
        /// (id, start_x, start_y) für jedes verschobene Objekt.
        starts: Vec<(u64, f32, f32)>,
    },
    /// Größe ändern; gegenüberliegende Ecke bleibt fix.
    Resize {
        id: u64,
        anchor: egui::Pos2,
        rotation: f32,
        start_aspect: f32,
    },
    /// Kante ziehen (Größe in einer Richtung ändern); gegenüberliegende Kante bleibt fix.
    ResizeEdge {
        id: u64,
        edge: CropEdge,
        rotation: f32,
        anchor: egui::Pos2,
    },
    /// Drehen.
    Rotate {
        id: u64,
    },
    /// Bild zuschneiden.
    Crop {
        id: u64,
        edge: CropEdge,
        start_crop: crate::model::Crop,
    },
    /// Auswahl-Rechteck ziehen.
    SelectionBox {
        start: egui::Pos2,
    },
    /// Linien-Endpunkt ziehen (id, true=start, false=end).
    LineEndpoint {
        id: u64,
        is_start: bool,
    },
    /// Stützpunkt eines Pfads ziehen (samt seiner Griffe).
    PathNode {
        id: u64,
        index: usize,
    },
    /// Kurvengriff eines Pfad-Knotens ziehen.
    PathHandle {
        id: u64,
        index: usize,
        /// `true` = Ausgangsgriff (in Zeichenrichtung), `false` = Eingangsgriff.
        outgoing: bool,
    },
}

/// Das aktive Werkzeug.
///
/// Alles außer [`Tool::Select`] fängt Klicks auf der Zeichenfläche ab, statt
/// Objekte auszuwählen. Genau **ein** Feld hält diesen Zustand — vorher war
/// der Linienmodus ein eigenes `Option`-Feld, und ein zweites Werkzeug daneben
/// hätte zwei Modi gleichzeitig aktiv sein lassen können.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// Auswählen und Bearbeiten (Standard).
    Select,
    /// Linie aus zwei Klicks.
    Line,
    /// Pfad aus gesetzten Knoten; Ziehen beim Klick erzeugt Kurvengriffe.
    Pen,
    /// Freihand-Zug, der beim Loslassen ausgedünnt und geglättet wird.
    Freehand,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Auswahl",
            Tool::Line => "Linie",
            Tool::Pen => "Pfad",
            Tool::Freehand => "Freihand",
        }
    }

    /// Hinweistext für die Statuszeile beim Wechsel auf das Werkzeug.
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Auswahl-Werkzeug.",
            Tool::Line => "Linie: Klicke den Startpunkt (Shift = 45°-Raster).",
            Tool::Pen => {
                "Pfad: Klicken setzt eine Ecke, Ziehen eine Kurve. \
                 Klick auf den Startpunkt schließt, Enter beendet, Esc bricht ab."
            }
            Tool::Freehand => "Freihand: Ziehen zum Zeichnen. Esc beendet das Werkzeug.",
        }
    }
}

/// Ein Pfad, der gerade gezeichnet wird — noch kein Element im Dokument.
///
/// Bewusst getrennt von [`Interaction`]: Der Entwurf gehört zu keinem
/// Element, hat keine ID und darf beim Abbrechen spurlos verschwinden.
#[derive(Default)]
pub struct PathDraft {
    /// Die bereits gesetzten Knoten, in Seitenkoordinaten (pt).
    pub nodes: Vec<crate::geometry::PathNode>,
    /// Zieht der Nutzer gerade die Griffe des zuletzt gesetzten Knotens
    /// heraus? (Maustaste seit dem Setzen noch nicht losgelassen.)
    pub dragging: bool,
    /// Rohspur des Freihand-Werkzeugs, ein Punkt je Frame.
    pub trace: Vec<egui::Pos2>,
}

/// Eine Pfad-Aktion aus dem Eigenschaften-Panel.
///
/// Wird dort nur vermerkt, weil das Panel bereits eine Ausleihe auf das
/// Element hält, die Aktion aber `&mut self` braucht.
#[derive(Clone, Copy, PartialEq)]
enum PathAction {
    /// Knotenbearbeitung ein-/ausschalten.
    ToggleEdit,
    /// Alle Knoten glätten.
    Smooth,
    /// Alle Kurven in Strecken zurückverwandeln.
    Sharpen,
    /// Pfad schließen oder öffnen.
    SetClosed(bool),
    /// Den Knoten mit diesem Index entfernen.
    RemoveNode(usize),
    /// Den Knoten mit diesem Index zwischen Ecke und Kurve umschalten.
    ToggleNode(usize),
}

/// Welcher Pfad gerade auf Knotenebene bearbeitet wird.
#[derive(Clone, Copy)]
pub struct PathEdit {
    pub id: u64,
    /// Zuletzt angefasster Knoten — Ziel von Entf und „Ecke/Kurve".
    pub node: Option<usize>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CropEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Wie ein Klick / Auswahl-Rechteck die bestehende Auswahl verändert.
/// Shift = [`Add`], Strg = [`Remove`], beides oder nichts = [`Replace`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SelectionOp {
    /// Auswahl ersetzen (normales Verhalten ohne Modifier).
    Replace,
    /// Zur bestehenden Auswahl hinzufügen (Shift).
    Add,
    /// Aus der bestehenden Auswahl entfernen (Strg).
    Remove,
}

impl SelectionOp {
    /// Modifier-Zustand zur Operation auflösen. Beide gleichzeitig heben
    /// sich auf → Replace.
    pub fn from_modifiers(shift: bool, ctrl: bool) -> Self {
        match (shift, ctrl) {
            (true, false) => SelectionOp::Add,
            (false, true) => SelectionOp::Remove,
            _ => SelectionOp::Replace,
        }
    }
}

/// Ausrichtungs-Operation für Mehrfachauswahl.
enum AlignOp {
    Left,
    Right,
    Top,
    Bottom,
    CenterX,
    CenterY,
}

/// Gleichmäßige Abstandsverteilung.
enum DistributeOp {
    /// Horizontal: gleiche Abstände zwischen den Objekten (X-Achse).
    Horizontal,
    /// Vertikal: gleiche Abstände zwischen den Objekten (Y-Achse).
    Vertical,
}

/// Ein Eintrag im Datei-Browser.
#[derive(Clone)]
struct FileEntry {
    name: String,
    modified: f64,
    size: u64,
    protected: bool,
}

#[derive(Deserialize)]
struct ListResponse {
    documents: Vec<ListDoc>,
}

#[derive(Deserialize)]
struct ListDoc {
    slug: String,
    modified: f64,
    size: u64,
    #[serde(default)]
    protected: bool,
}

/// Eine Aktion, die ungespeicherte Änderungen verwerfen würde und daher erst
/// nach Bestätigung ausgeführt wird.
///
/// Der gesamte Zweck dieses Typs ist es, zu verhindern, dass Nutzerarbeit
/// kommentarlos verloren geht. Jede Aktion, die `doc` ersetzt, muss über
/// `request_action` laufen — niemals direkt.
#[derive(Clone, PartialEq)]
pub enum PendingAction {
    /// Neues, leeres Dokument.
    NewDocument,
    /// Datei-Dialog zum Öffnen.
    OpenDialog,
    /// Konkrete Datei öffnen (Datei-Browser).
    OpenFile(PathBuf),
    /// Anwendung beenden.
    Quit,
}

impl PendingAction {
    /// Beschriftung für den Bestätigungsdialog.
    fn label(&self) -> &'static str {
        match self {
            PendingAction::NewDocument => "Neues Dokument anlegen",
            PendingAction::OpenDialog | PendingAction::OpenFile(_) => "Anderes Dokument öffnen",
            PendingAction::Quit => "BoxDoc beenden",
        }
    }
}

/// Die drei Ausgänge des „Ungespeicherte Änderungen"-Dialogs.
enum UnsavedDecision {
    /// Speichern, dann die geparkte Aktion ausführen.
    SaveThenContinue,
    /// Änderungen verwerfen und fortfahren.
    Discard,
    /// Nichts tun, Dokument unverändert lassen.
    Cancel,
}

/// Welche Seiten-Führungslinie(n) beim Ziehen aktiv sind (Snap-Visual).
/// Werte sind Seiten-Koordinaten in pt.
#[derive(Clone, Copy, Default)]
pub struct SnapLines {
    /// Vertikale Linie (Seiten-X), an die eine Außenkante oder der
    /// horizontale Mittelpunkt eines Objekts andockt.
    pub vertical: Option<f32>,
    /// Horizontale Linie (Seiten-Y), an die eine Außenkante oder der
    /// vertikale Mittelpunkt eines Objekts andockt.
    pub horizontal: Option<f32>,
}

/// Zulässiger Bereich für Format-Maße (10–2000 mm), ausgedrückt in der
/// gewählten Anzeige-Einheit.
fn custom_fmt_range(unit: Units) -> std::ops::RangeInclusive<f32> {
    let lo = unit.from_pt(mm_to_pt(10.0));
    let hi = unit.from_pt(mm_to_pt(2000.0));
    lo..=hi
}

/// Bearbeitungsstand des Dialogs für eigene Seitenformate. Bleibt nur im
/// UI bestehen; erst "Übernehmen" schreibt das Format ins Dokument (und
/// damit in die Datei). Breite/Höhe werden in der gewählten Einheit
/// angezeigt und erst beim Übernehmen nach mm umgerechnet.
#[derive(Debug, Clone, Default)]
pub struct CustomFormatDraft {
    pub name: String,
    /// Breite in der gewählten Einheit.
    pub w: f32,
    /// Höhe in der gewählten Einheit.
    pub h: f32,
    pub unit: Units,
}

pub struct EditorApp {
    pub doc: Document,
    pub page_index: usize,
    pub next_id: u64,
    /// Alle aktuell ausgewählten Element-IDs.
    pub selection: Vec<u64>,
    /// (id, Puffer) falls gerade Text bearbeitet wird.
    pub editing: Option<(u64, String)>,
    /// Beim Start der Textbearbeitung einmal Fokus anfordern.
    pub edit_focus: bool,
    pub interaction: Interaction,
    pub view: View,
    pub images: ImageStore,
    /// Eingebettete Custom-Fonts (analog zu `images`).
    pub fonts: FontStore,
    /// Flag: beim nächsten `update` die Fonts bei egui registrieren.
    /// Wird vom Lade-Code gesetzt, da dort kein `&egui::Context` verfügbar ist.
    pub fonts_dirty: bool,
    pub crop_mode: bool,
    pub file_path: Option<PathBuf>,
    pub modified: bool,
    pub status: String,
    pub settings: Settings,
    /// Ankerpunkt für die Positions-Anzeige.
    pub multi_anchor: BBoxAnchor,
    /// Gespeicherte Ankerposition (nur bei Anker-/Auswahlwechsel aktualisiert).
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_last_sel: Vec<u64>,
    /// Zwischenablage für Copy/Paste.
    pub clipboard: Vec<Element>,
    /// Ursprüngliche Positionen der kopierten Elemente (für Ghost + Snap).
    pub clip_origins: Vec<(f32, f32)>,
    /// Paste-Modus aktiv: Preview folgt dem Cursor, Klick platziert.
    pub pasting: bool,
    /// Snap-Visual: vertikale/horizontale Führungslinie(n) der Seite,
    /// an die mindestens eine Außenkante oder der Mittelpunkt eines
    /// gezogenen Objekts gerade andockt. Wert = Seitenkoordinate in pt;
    /// `None` = keine aktive Linie auf der jeweiligen Achse.
    pub snap_lines: SnapLines,
    /// Das aktive Werkzeug.
    pub tool: Tool,
    /// Offener Editor für ein eigenes Seitenformat (`None` = geschlossen).
    pub custom_fmt_edit: Option<CustomFormatDraft>,
    /// Startpunkt der Linie, sobald der erste Klick gesetzt ist.
    /// Nur beim Werkzeug [`Tool::Line`] belegt.
    pub line_drawing: Option<(f32, f32)>,
    /// Der Pfad, der gerade gezeichnet wird (Werkzeug Pfad/Freihand).
    pub path_draft: Option<PathDraft>,
    /// Welcher Pfad gerade auf Knotenebene bearbeitet wird.
    pub path_edit: Option<PathEdit>,
    /// Theme-Fade: Quell-Thema.
    pub theme_from: crate::model::Theme,
    /// Theme-Fade: Ziel-Thema (= settings.theme).
    pub theme_target: crate::model::Theme,
    /// Theme-Fade: Fortschritt 0..1.
    pub theme_anim: f32,
    /// Einmalig true: beim ersten ui()-Frame das gespeicherte Theme auf dem
    /// Live-Context re-applizen. `themes::apply()` in main.rs läuft während
    /// der Creation-Phase und das Style-Setting überlebt den Frame-Wechsel
    /// nicht zuverlässig — daher hier nochmal explizit anwenden.
    pub theme_init_pending: bool,
    /// Undo/Redo-History.
    pub history: crate::history::History,
    /// Flag: Snapshot beim nächsten DragValue-Focus-Gain machen.
    pub prop_snapshot_pending: bool,
    /// Zeitpunkt des letzten Pfeiltasten-Verschiebens (egui-time). Dient dazu,
    /// eine Serie von Tastendrücken zu EINEM Undo-Schritt zusammenzufassen.
    pub last_nudge_time: f64,

    // --- JSON-Editor (Rohtext-Ansicht des Dokuments) ---
    /// JSON-Editor-Fenster sichtbar?
    pub show_json: bool,
    /// Textpuffer des JSON-Editors.
    json_buf: String,
    /// Hat der JSON-Editor im letzten Frame Fokus? (Steuert Sync-Richtung.)
    json_focused: bool,

    // --- Datei-Browser ---
    /// Datei-Browser-Fenster sichtbar?
    pub show_files: bool,
    /// Aktuell geladene Datei-Liste (Web: Server-Docs; Native: lokale Dateien).
    files_entries: Vec<FileEntry>,
    /// Zeitstempel der letzten Aktualisierung (egui-time).
    files_last_refresh: f64,

    // --- File-Watch (AI-Schnittstelle) ---
    /// Lauft ein File-Watcher? Wird gedroppt → Watching stoppt.
    pub file_watcher: Option<crate::file_watch::FileWatcher>,
    /// Empfänger für externe Dateiänderungen.
    pub file_watch_rx: Option<std::sync::mpsc::Receiver<std::path::PathBuf>>,
    /// Der aktuell beobachtete Pfad (zum Erkennen von Pfadwechseln).
    pub watched_path: Option<std::path::PathBuf>,
    /// mtime/last-write der zuletzt geladenen oder selbst geschriebenen Datei.
    /// Dient dazu, eigene Schreibvorgänge vom Watcher zu unterscheiden.
    pub last_disk_write: Option<std::time::SystemTime>,
    /// Eine externe Änderung wurde erkannt, aber nicht angewendet, weil die
    /// GUI ungespeicherte eigene Änderungen hat → Konflikt-Dialog zeigen.
    pub pending_external_change: bool,
    /// Konflikt-Dialog anzeigen.
    pub show_conflict_dialog: bool,

    // --- Schutz vor Datenverlust ---
    /// Aktion, die auf die Bestätigung "ungespeicherte Änderungen verwerfen?"
    /// wartet. `None` = kein Dialog offen.
    pub pending_action: Option<PendingAction>,
    /// Wurde das Beenden bereits bestätigt? Verhindert, dass der Close-Guard
    /// den Schließvorgang ein zweites Mal abfängt.
    pub quit_confirmed: bool,
    /// Die App möchte das Fenster schließen (nach bestätigtem Beenden).
    /// Wird im nächsten `ui()` in ein `ViewportCommand::Close` übersetzt.
    #[cfg(not(target_arch = "wasm32"))]
    pub close_requested_by_app: bool,

    // --- Web-Sync (WASM-only, AI-Schnittstelle für den Browser) ---
    #[cfg(target_arch = "wasm32")]
    pub web_doc: Option<crate::web_sync::WebDoc>,
    /// Zeitstempel des letzten Polls (Sekunden, egui-time).
    #[cfg(target_arch = "wasm32")]
    pub web_last_poll: f64,
    /// Zeitpunkt der **ersten** Änderung seit dem letzten Speichern.
    ///
    /// Bewusst nicht „letzte Änderung": Ein in jedem Frame erneuertes
    /// Zeitfenster läuft nie ab (genau daran scheiterte die frühere Fassung —
    /// die Web-Version speicherte nie).
    #[cfg(target_arch = "wasm32")]
    pub web_dirty_at: Option<f64>,
    /// Kam während eines laufenden PUT eine weitere Änderung dazu?
    #[cfg(target_arch = "wasm32")]
    pub web_dirty_since_save: bool,
    /// Wurde das Dokument initial geladen?
    #[cfg(target_arch = "wasm32")]
    pub web_initial_loaded: bool,
    /// True, wenn gerade ein initialer GET läuft.
    #[cfg(target_arch = "wasm32")]
    pub web_load_in_flight: bool,
    /// True, wenn gerade ein PUT läuft.
    #[cfg(target_arch = "wasm32")]
    pub web_save_in_flight: bool,
    /// Status-Text für die Web-Sync-Anzeige.
    #[cfg(target_arch = "wasm32")]
    pub web_status: String,

    // --- Dialog „Am Server speichern" (WASM-only) ---
    /// Ist der Dialog offen?
    #[cfg(target_arch = "wasm32")]
    pub show_server_save: bool,
    /// Eingetippter Dokumentname (Slug).
    #[cfg(target_arch = "wasm32")]
    pub server_save_name: String,
    /// Soll das Dokument mit einem Token geschützt werden?
    ///
    /// Voreinstellung ist „nein": Ein öffentliches Dokument hat eine saubere,
    /// merkbare URL und steht im Datei-Browser. Der Token-Fall ist die
    /// Ausnahme und kostet einen Link, den man nicht verlieren darf.
    #[cfg(target_arch = "wasm32")]
    pub server_save_protected: bool,
    /// Fehlermeldung des Servers, direkt am Eingabefeld.
    #[cfg(target_arch = "wasm32")]
    pub server_save_error: String,
    /// Läuft gerade ein Anlege-Request?
    #[cfg(target_arch = "wasm32")]
    pub server_save_in_flight: bool,
}

impl EditorApp {
    /// Erstes ausgewähltes Element (für Resize/Rotate-Griffe etc.).
    pub fn primary(&self) -> Option<u64> {
        self.selection.first().copied()
    }

    pub fn is_selected(&self, id: u64) -> bool {
        self.selection.contains(&id)
    }

    pub fn select_only(&mut self, id: u64) {
        self.selection.clear();
        self.selection.push(id);
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Element zur Auswahl hinzufügen, falls noch nicht enthalten.
    /// Die Reihenfolge der bestehenden Auswahl bleibt unverändert.
    pub fn add_selected(&mut self, id: u64) {
        if !self.is_selected(id) {
            self.selection.push(id);
        }
    }

    /// Element ordnungserhaltend aus der Auswahl entfernen, falls enthalten.
    /// `retain` statt `swap_remove`, damit die Primary-Auswahl (`first`)
    /// nicht unerwartet auf ein anderes Element springt.
    pub fn remove_selected(&mut self, id: u64) {
        self.selection.retain(|&x| x != id);
    }
}

impl Default for EditorApp {
    fn default() -> Self {
        let settings = crate::settings_io::load_or_detect();
        let theme = settings.theme;
        EditorApp {
            doc: Document::default(),
            page_index: 0,
            next_id: 1,
            selection: Vec::new(),
            editing: None,
            edit_focus: false,
            interaction: Interaction::None,
            view: View::default(),
            images: ImageStore::default(),
            fonts: FontStore::default(),
            fonts_dirty: false,
            crop_mode: false,
            file_path: None,
            modified: false,
            status: String::from("Bereit. Tipp: Bild per Drag&Drop hereinziehen."),
            settings,
            multi_anchor: BBoxAnchor::default(),
            pos_x: 0.0,
            pos_y: 0.0,
            pos_last_sel: Vec::new(),
            clipboard: Vec::new(),
            clip_origins: Vec::new(),
            pasting: false,
            snap_lines: SnapLines::default(),
            tool: Tool::Select,
            custom_fmt_edit: None,
            line_drawing: None,
            path_draft: None,
            path_edit: None,
            theme_from: theme,
            theme_target: theme,
            theme_anim: 1.0,
            theme_init_pending: true,
            history: crate::history::History::default(),
            prop_snapshot_pending: false,
            last_nudge_time: 0.0,
            show_json: false,
            json_buf: String::new(),
            json_focused: false,
            show_files: false,
            files_entries: Vec::new(),
            files_last_refresh: 0.0,
            file_watcher: None,
            file_watch_rx: None,
            watched_path: None,
            last_disk_write: None,
            pending_external_change: false,
            show_conflict_dialog: false,
            pending_action: None,
            quit_confirmed: false,
            #[cfg(not(target_arch = "wasm32"))]
            close_requested_by_app: false,

            #[cfg(target_arch = "wasm32")]
            web_doc: crate::web_sync::WebDoc::from_url(),
            #[cfg(target_arch = "wasm32")]
            web_last_poll: 0.0,
            #[cfg(target_arch = "wasm32")]
            web_dirty_at: None,
            #[cfg(target_arch = "wasm32")]
            web_dirty_since_save: false,
            #[cfg(target_arch = "wasm32")]
            web_initial_loaded: false,
            #[cfg(target_arch = "wasm32")]
            web_load_in_flight: false,
            #[cfg(target_arch = "wasm32")]
            web_save_in_flight: false,
            #[cfg(target_arch = "wasm32")]
            web_status: String::new(),
            #[cfg(target_arch = "wasm32")]
            show_server_save: false,
            #[cfg(target_arch = "wasm32")]
            server_save_name: String::new(),
            #[cfg(target_arch = "wasm32")]
            server_save_protected: false,
            #[cfg(target_arch = "wasm32")]
            server_save_error: String::new(),
            #[cfg(target_arch = "wasm32")]
            server_save_in_flight: false,
        }
        .with_init_history()
    }
}

impl EditorApp {
    pub fn new_document(&mut self) {
        self.doc = Document::default();
        self.page_index = 0;
        self.next_id = 1;
        self.clear_selection();
        self.editing = None;
        self.edit_focus = false;
        self.interaction = Interaction::None;
        self.images = ImageStore::default();
        self.fonts = FontStore::default();
        self.fonts_dirty = true;
        self.crop_mode = false;
        self.file_path = None;
        self.modified = false;
        self.status = String::from("Neues Dokument.");
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// `at` wird interpretiert als:
    /// - `None`  → Seitenmitte (zentriert)
    /// - `Some(false, x, y)` → (x,y) ist die linke-obere Ecke
    /// - `Some(true, x, y)`  → (x,y) ist das Zentrum
    pub fn add_text(&mut self, at: Option<(bool, f32, f32)>) {
        self.push_history();
        let id = self.next_id();
        let (cx, cy) = match at {
            None => {
                let (w, h) = self.doc.page_size_pt();
                (w / 2.0, h / 2.0)
            }
            Some((true, x, y)) => (x, y),
            Some((false, x, y)) => (x, y),
        };
        let mut el = Element::new_text(id, 0.0, 0.0);
        // Für den Doppelklick: linke-obere Ecke genau an der Cursor-Position,
        // damit der Text-Nullpunkt (= Cursor) dort liegt.
        match at {
            Some((false, _, _)) => {
                el.x = cx;
                el.y = cy;
            }
            _ => {
                el.x = cx - el.w / 2.0;
                el.y = cy - el.h / 2.0;
            }
        }
        el.text = String::new();
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.crop_mode = false;
        self.modified = true;
        // Sofort in den Bearbeitungsmodus wechseln.
        self.editing = Some((id, String::new()));
        self.edit_focus = true;
        self.status = String::from("Text erstellt – tippe los.");
    }

    pub fn add_rectangle(&mut self, at: Option<(f32, f32)>) {
        self.push_history();
        let id = self.next_id();
        let (cx, cy) = match at {
            Some((x, y)) => (x, y),
            None => {
                let (w, h) = self.doc.page_size_pt();
                (w / 2.0, h / 2.0)
            }
        };
        let mut el = Element::new_rectangle(id, 0.0, 0.0);
        el.x = cx - el.w / 2.0;
        el.y = cy - el.h / 2.0;
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.crop_mode = false;
        self.modified = true;
        self.status = String::from("Rechteck hinzugefügt.");
    }

    pub fn add_line(&mut self, at: Option<(f32, f32)>) {
        self.push_history();
        let id = self.next_id();
        let (cx, cy) = match at {
            Some((x, y)) => (x, y),
            None => {
                let (w, h) = self.doc.page_size_pt();
                (w / 2.0, h / 2.0)
            }
        };
        let mut el = Element::new_line(id, 0.0, 0.0);
        el.x = cx - el.w / 2.0;
        el.y = cy - el.h / 2.0;
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.crop_mode = false;
        self.modified = true;
        self.status = String::from("Linie hinzugefügt.");
    }

    pub fn add_ellipse(&mut self, at: Option<(f32, f32)>) {
        self.push_history();
        let id = self.next_id();
        let (cx, cy) = match at {
            Some((x, y)) => (x, y),
            None => {
                let (w, h) = self.doc.page_size_pt();
                (w / 2.0, h / 2.0)
            }
        };
        let mut el = Element::new_ellipse(id, 0.0, 0.0);
        el.x = cx - el.w / 2.0;
        el.y = cy - el.h / 2.0;
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.crop_mode = false;
        self.modified = true;
        self.status = String::from("Ellipse hinzugefügt.");
    }

    /// Fügt einen fertigen Pfad ein — den Weg ohne Zeichnen.
    ///
    /// Das Pen-Werkzeug verlangt, dass man den Pfad erst aufzieht. Wer nur
    /// schnell eine bearbeitbare Kurve auf der Seite braucht, bekommt sie
    /// hier: eine flache Welle aus drei Knoten, mittig platziert wie jede
    /// andere eingefügte Form.
    ///
    /// Die Knotenbearbeitung wird gleich mit eingeschaltet. Ein frisch
    /// eingefügter Pfad ohne sichtbare Knoten sähe aus wie ein Strich, den
    /// man nur als Ganzes schieben kann — dabei ist genau das Gegenteil der
    /// Punkt an einem Pfad.
    pub fn add_path(&mut self, at: Option<(f32, f32)>) {
        use crate::geometry::PathNode;

        /// Größe des eingefügten Pfads in pt.
        const W: f32 = 240.0;
        const H: f32 = 80.0;

        self.push_history();
        let id = self.next_id();
        let (cx, cy) = match at {
            Some((x, y)) => (x, y),
            None => {
                let (w, h) = self.doc.page_size_pt();
                (w / 2.0, h / 2.0)
            }
        };

        // Eine Welle: zwei Ecken als Enden, ein glatter Knoten in der Mitte.
        // Die waagrechten Griffe machen den mittleren Knoten knickfrei.
        let (l, r) = (cx - W / 2.0, cx + W / 2.0);
        let (top, bot) = (cy - H / 2.0, cy + H / 2.0);
        let nodes = vec![
            PathNode::corner(egui::Pos2::new(l, bot)),
            PathNode {
                anchor: egui::Pos2::new(cx, top),
                in_h: egui::Pos2::new(cx - W / 4.0, top),
                out_h: egui::Pos2::new(cx + W / 4.0, top),
            },
            PathNode::corner(egui::Pos2::new(r, bot)),
        ];

        // Aussehen wie jede andere eingefügte Form. `path_from_nodes` liefert
        // die dünne, dunkle Kontur des PDF-Imports — passend zum Nachbilden
        // einer Vorlage, aber nicht zu einer Form, die der Nutzer eben selbst
        // eingefügt hat.
        let mut el = crate::geometry::path_from_nodes(id, &nodes, false);
        el.stroke_width = crate::model::default_stroke_width();
        el.stroke_color = crate::model::default_stroke_color();
        // Offen heißt: nie gefüllt. Die Füllfarbe steht trotzdem schon auf dem
        // üblichen Wert, damit „Geschlossen" im Panel sofort etwas zeigt.
        el.fill_color = crate::model::default_fill_color();

        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.crop_mode = false;
        self.tool = Tool::Select;
        self.cancel_draw();
        self.path_edit = Some(PathEdit { id, node: None });
        self.modified = true;
        self.status = String::from(
            "Pfad eingefügt. Knoten ziehen zum Verformen, Doppelklick auf ein Segment fügt \
             einen Knoten ein, Alt+Klick schaltet Ecke/Kurve um, N beendet.",
        );
    }

    /// Erstellt eine Linie zwischen zwei Punkten (start, end).
    pub fn add_line_between(&mut self, start: (f32, f32), end: (f32, f32)) {
        self.push_history();
        let id = self.next_id();
        let dx = end.0 - start.0;
        let dy = end.1 - start.1;
        let len = dx.hypot(dy).max(1.0);
        let rotation = dy.atan2(dx).to_degrees();
        let cx = (start.0 + end.0) / 2.0;
        let cy = (start.1 + end.1) / 2.0;
        let mut el = Element::new_line(id, 0.0, 0.0);
        el.x = cx - len / 2.0;
        el.y = cy;
        el.w = len;
        el.rotation = rotation;
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.modified = true;
        self.status = String::from("Linie gezeichnet.");
        // Werkzeug aktiv lassen für weitere Linien (AutoCAD-Verhalten); nur
        // der gesetzte Startpunkt wird zurückgenommen.
        self.line_drawing = None;
    }

    /// Wechselt das Werkzeug und räumt dabei auf, was zum alten gehörte.
    ///
    /// Ein halb gezeichneter Pfad oder eine angefangene Linie überleben den
    /// Wechsel nicht — sonst läge beim Zurückschalten ein Entwurf herum, an
    /// den sich niemand mehr erinnert.
    pub fn set_tool(&mut self, tool: Tool) {
        if self.tool == tool && tool != Tool::Select {
            // Dasselbe Werkzeug erneut → zurück zur Auswahl (Umschalter).
            self.set_tool(Tool::Select);
            return;
        }
        self.cancel_draw();
        self.tool = tool;
        if tool != Tool::Select {
            self.path_edit = None;
        }
        self.status = String::from(tool.hint());
    }

    /// Bricht jeden laufenden Zeichenvorgang ab, ohne das Werkzeug zu wechseln.
    pub fn cancel_draw(&mut self) {
        self.line_drawing = None;
        self.path_draft = None;
    }

    /// Macht aus dem aktuellen Entwurf ein Pfad-Element.
    ///
    /// Weniger als zwei Knoten ergeben keinen Pfad — der Entwurf wird dann
    /// verworfen statt ein unsichtbares Element zu hinterlassen.
    pub fn finish_path(&mut self, closed: bool) {
        let Some(draft) = self.path_draft.take() else {
            return;
        };
        if draft.nodes.len() < 2 {
            self.status = String::from("Pfad verworfen: zu wenige Punkte.");
            return;
        }
        self.push_history();
        let id = self.next_id();
        let el = crate::geometry::path_from_nodes(id, &draft.nodes, closed);
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.modified = true;
        self.status = format!(
            "Pfad mit {} Knoten erstellt ({}).",
            draft.nodes.len(),
            if closed { "geschlossen" } else { "offen" }
        );
    }

    /// Die ID des ausgewählten Pfads, falls genau einer ausgewählt ist.
    pub fn selected_path(&self) -> Option<u64> {
        if self.selection.len() != 1 {
            return None;
        }
        let id = self.selection[0];
        self.doc
            .current_page(self.page_index)?
            .elements
            .iter()
            .find(|e| e.id == id && e.kind == ElementKind::Path)
            .map(|e| e.id)
    }

    /// Führt eine im Eigenschaften-Panel angestoßene Pfad-Aktion aus.
    ///
    /// Alle Zweige nehmen zuerst einen Undo-Schnappschuss: „Glätten" verändert
    /// jeden Knoten des Pfads auf einmal — das muss sich mit einem Strg+Z
    /// zurückholen lassen.
    fn apply_path_action(&mut self, page_idx: usize, el_idx: usize, action: PathAction) {
        if action == PathAction::ToggleEdit {
            self.toggle_path_edit();
            return;
        }
        self.push_history();
        let Some(el) = self
            .doc
            .pages
            .get_mut(page_idx)
            .and_then(|p| p.elements.get_mut(el_idx))
        else {
            return;
        };
        match action {
            PathAction::Smooth => {
                crate::geometry::smooth_path(el);
                self.status = String::from("Pfad geglättet.");
            }
            PathAction::Sharpen => {
                crate::geometry::sharpen_path(el);
                self.status = String::from("Kurven in Strecken umgewandelt.");
            }
            PathAction::SetClosed(closed) => {
                el.path_closed = closed;
                // Die Box muss neu gelegt werden: Der Abschluss fügt ein
                // Segment hinzu, das über die bisherige Hülle hinausbeulen
                // kann — und beim Öffnen umgekehrt eines weg.
                let nodes = crate::geometry::path_nodes(el);
                crate::geometry::set_path_nodes(el, &nodes);
                self.status = String::from(if closed {
                    "Pfad geschlossen."
                } else {
                    "Pfad geöffnet."
                });
            }
            PathAction::RemoveNode(i) => {
                if crate::geometry::remove_node(el, i) {
                    // Der gelöschte Index zeigt jetzt auf einen anderen Knoten
                    // — die Auswahl aufheben, statt sie stillschweigend
                    // weiterwandern zu lassen.
                    self.path_edit = self.path_edit.map(|e| PathEdit { id: e.id, node: None });
                    self.status = String::from("Knoten gelöscht.");
                } else {
                    self.status = String::from("Ein Pfad braucht mindestens diese Knoten.");
                }
            }
            PathAction::ToggleNode(i) => {
                crate::geometry::toggle_node_smooth(el, i);
                self.status = String::from("Knoten umgeschaltet.");
            }
            PathAction::ToggleEdit => unreachable!("oben behandelt"),
        }
        self.touch();
    }

    /// Schaltet die Knotenbearbeitung des ausgewählten Pfads um.
    pub fn toggle_path_edit(&mut self) {
        if self.path_edit.is_some() {
            self.path_edit = None;
            self.status = String::from("Knotenbearbeitung beendet.");
            return;
        }
        if let Some(id) = self.selected_path() {
            self.tool = Tool::Select;
            self.cancel_draw();
            self.crop_mode = false;
            self.path_edit = Some(PathEdit { id, node: None });
            self.status = String::from(
                "Knoten: ziehen zum Verschieben, Doppelklick auf ein Segment fügt ein, \
                 Entf löscht, Alt+Klick schaltet Ecke/Kurve um.",
            );
        } else {
            self.status = String::from("Kein Pfad ausgewählt.");
        }
    }

    pub fn add_image_from_bytes(&mut self, bytes: Vec<u8>, at: Option<(f32, f32)>) {
        self.push_history();
        let id = self.next_id();
        let dims = match image::load_from_memory(&bytes) {
            Ok(img) => (img.width(), img.height()),
            Err(e) => {
                self.status = format!("Bild konnte nicht gelesen werden: {e}");
                return;
            }
        };
        let center = at.unwrap_or_else(|| {
            let (w, h) = self.doc.page_size_pt();
            (w / 2.0, h / 2.0)
        });
        let mut el = Element::new_image(id, 0, 0, dims.0, dims.1);
        el.x = center.0 - el.w / 2.0;
        el.y = center.1 - el.h / 2.0;
        self.images.insert(id, bytes, dims);
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.select_only(id);
        self.crop_mode = false;
        self.modified = true;
        self.status = format!("Bild hinzugefügt ({}×{}).", dims.0, dims.1);
    }

    /// Custom-Font aus Bytes hinzufügen. `name` = eindeutiger Schlüssel, der
    /// im Element als `font` gespeichert wird. Reservierte Namen ("default")
    /// und Bundled-Keys werden abgewiesen, ebenso Duplikate.
    pub fn add_font_from_bytes(&mut self, name: String, bytes: Vec<u8>) {
        if name.is_empty() || name == "default" {
            self.set_status(format!(
                "Font-Name '{}' ist reserviert oder leer.",
                name
            ));
            return;
        }
        if crate::model::FONT_CHOICES.iter().any(|f| f.key == name.as_str()) {
            self.set_status(format!("Font-Name '{}' ist bereits gebündelt.", name));
            return;
        }
        if self.fonts.contains(&name) {
            self.set_status(format!("Font '{}' existiert bereits.", name));
            return;
        }
        self.fonts.insert(name.clone(), bytes);
        self.fonts_dirty = true;
        self.modified = true;
        self.set_status(format!("Font '{}' geladen.", name));
    }

    pub fn delete_selected(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.push_history();
        let ids: Vec<u64> = self.selection.clone();
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.retain(|e| !ids.contains(&e.id));
        }
        // Images NICHT entfernen — bleiben für Undo erhalten.
        self.clear_selection();
        self.editing = None;
        self.crop_mode = false;
        self.interaction = Interaction::None;
        self.modified = true;
        self.status = String::from("Objekt(e) gelöscht.");
    }

    // ===================================================================
    // Z-Order (Reihenfolge der Elemente auf der Seite)
    // ===================================================================

    /// Index des Elements `id` auf der aktuellen Seite, oder `None`.
    fn element_index(&self, id: u64) -> Option<usize> {
        let page = self.doc.pages.get(self.page_index)?;
        page.elements.iter().position(|e| e.id == id)
    }

    /// Anzahl der Elemente auf der aktuellen Seite.
    fn element_count(&self) -> usize {
        self.doc
            .pages
            .get(self.page_index)
            .map(|p| p.elements.len())
            .unwrap_or(0)
    }

    /// Bewegt das Element `id` eine Position nach vorne (oben / zuletzt
    /// gezeichnet). Hat keinen Effekt am Anfang oder wenn nicht gefunden.
    pub fn bring_forward(&mut self, id: u64) {
        let len = self.element_count();
        let Some(idx) = self.element_index(id) else {
            return;
        };
        if idx + 1 >= len {
            return;
        }
        self.push_history();
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.swap(idx, idx + 1);
        }
        self.modified = true;
    }

    /// Bewegt das Element `id` eine Position nach hinten (unten / zuerst
    /// gezeichnet).
    pub fn send_backward(&mut self, id: u64) {
        let Some(idx) = self.element_index(id) else {
            return;
        };
        if idx == 0 {
            return;
        }
        self.push_history();
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.swap(idx, idx - 1);
        }
        self.modified = true;
    }

    /// Bringt das Element `id` ganz nach vorne (ganz oben).
    pub fn bring_to_front(&mut self, id: u64) {
        let len = self.element_count();
        let Some(idx) = self.element_index(id) else {
            return;
        };
        if idx + 1 >= len {
            return;
        }
        self.push_history();
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            let el = page.elements.remove(idx);
            page.elements.push(el);
        }
        self.modified = true;
    }

    /// Schickt das Element `id` ganz nach hinten (ganz unten).
    pub fn send_to_back(&mut self, id: u64) {
        let Some(idx) = self.element_index(id) else {
            return;
        };
        if idx == 0 {
            return;
        }
        self.push_history();
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            let el = page.elements.remove(idx);
            page.elements.insert(0, el);
        }
        self.modified = true;
    }

    pub fn add_page(&mut self) {
        self.doc.pages.push(crate::model::Page::default());
        self.page_index = self.doc.pages.len() - 1;
        self.clear_selection();
        self.modified = true;
    }

    pub fn current_elements_mut(&mut self) -> Option<&mut Vec<Element>> {
        self.doc
            .current_page_mut(self.page_index)
            .map(|p| &mut p.elements)
    }

    pub fn touch(&mut self) {
        self.modified = true;
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    /// Lädt ein Projekt aus einer JSON-Zeichenkette (Web File-Dialog).
    fn load_project_from_json(&mut self, json: &str) {
        match serde_json::from_str::<crate::io::Project>(json) {
            Ok(project) => {
                use base64::Engine;
                let mut images = crate::store::ImageStore::default();
                let mut fonts = crate::store::FontStore::default();
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
                self.doc = project.doc;
                self.images = images;
                self.fonts = fonts;
                self.fonts_dirty = true;
                self.page_index = 0;
                self.next_id = max_id + 1;
                self.clear_selection();
                self.editing = None;
                self.crop_mode = false;
                self.interaction = Interaction::None;
                self.modified = false;
                self.set_status("Dokument geöffnet.");
            }
            Err(e) => self.set_status(format!("Fehler beim Öffnen: {e}")),
        }
    }

    /// Übernimmt ein ODT aus dem Speicher — der Weg der Web-Version, wo der
    /// `FileReader` Bytes und keinen Pfad liefert.
    ///
    /// Setzt `file_path` bewusst **nicht**: Im Browser gibt es keinen Pfad, und
    /// ein erfundener würde beim Speichern eine Datei suggerieren, die es nicht
    /// gibt. Der Import ist ein Konvertierungsschritt, kein Öffnen.
    fn load_odt_from_bytes(&mut self, bytes: &[u8]) {
        match crate::odt::import_from_bytes(bytes) {
            Ok((doc, images, next_id)) => {
                self.doc = doc;
                self.images = images;
                self.fonts = Default::default();
                self.fonts_dirty = true;
                self.page_index = 0;
                self.next_id = next_id;
                self.clear_selection();
                self.editing = None;
                self.crop_mode = false;
                self.interaction = Interaction::None;
                self.modified = false;
                self.set_status("ODT geöffnet.");
            }
            Err(e) => self.set_status(format!("Fehler beim ODT-Lesen: {e}")),
        }
    }

    // ===================================================================
    // Schutz vor Datenverlust
    // ===================================================================

    /// Führt eine potenziell destruktive Aktion aus — aber nur, wenn nichts
    /// verloren gehen kann. Andernfalls wird sie geparkt und der
    /// Bestätigungsdialog geöffnet.
    ///
    /// **Jede** Stelle, die `self.doc` ersetzt oder die App beendet, muss
    /// hierüber gehen. Direktaufrufe von `new_document()` o. ä. sind ein Bug.
    pub fn request_action(&mut self, action: PendingAction) {
        if self.modified {
            self.pending_action = Some(action);
        } else {
            self.perform_action(action);
        }
    }

    /// Führt eine geparkte Aktion tatsächlich aus (nach Bestätigung oder wenn
    /// es nichts zu verlieren gab).
    fn perform_action(&mut self, action: PendingAction) {
        match action {
            PendingAction::NewDocument => self.new_document(),
            PendingAction::OpenDialog => crate::io::open_project_dialog(self),
            PendingAction::OpenFile(path) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.open_path(path);
                #[cfg(target_arch = "wasm32")]
                let _ = path;
            }
            PendingAction::Quit => {
                self.quit_confirmed = true;
                #[cfg(not(target_arch = "wasm32"))]
                self.request_close();
            }
        }
    }

    /// Sendet den Schließbefehl an das Fenster.
    #[cfg(not(target_arch = "wasm32"))]
    fn request_close(&mut self) {
        self.close_requested_by_app = true;
    }

    /// Zeigt den Bestätigungsdialog, falls eine Aktion wartet.
    ///
    /// Drei Wege heraus: speichern und fortfahren, verwerfen und fortfahren,
    /// oder abbrechen. „Speichern" ist der voreingestellte, sichere Weg.
    fn show_unsaved_dialog(&mut self, ctx: &Context) {
        let Some(action) = self.pending_action.clone() else {
            return;
        };

        let mut decision: Option<UnsavedDecision> = None;

        egui::Window::new("Ungespeicherte Änderungen")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(format!(
                    "„{}\u{201c} verwirft die ungespeicherten Änderungen an diesem Dokument.",
                    action.label()
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Speichern und fortfahren").clicked() {
                        decision = Some(UnsavedDecision::SaveThenContinue);
                    }
                    if ui.button("Verwerfen").clicked() {
                        decision = Some(UnsavedDecision::Discard);
                    }
                    if ui.button("Abbrechen").clicked() {
                        decision = Some(UnsavedDecision::Cancel);
                    }
                });
            });

        // Escape = abbrechen (der sichere Ausgang).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            decision = Some(UnsavedDecision::Cancel);
        }

        match decision {
            None => {}
            Some(UnsavedDecision::Cancel) => {
                self.pending_action = None;
                self.set_status("Abgebrochen.");
            }
            Some(UnsavedDecision::Discard) => {
                self.pending_action = None;
                self.modified = false;
                self.perform_action(action);
            }
            Some(UnsavedDecision::SaveThenContinue) => {
                crate::io::save_project_dialog(self, false);
                if self.modified {
                    // Speichern wurde abgebrochen oder ist fehlgeschlagen —
                    // die Aktion bleibt geparkt, damit nichts verloren geht.
                    self.set_status("Nicht gespeichert — Aktion abgebrochen.");
                    self.pending_action = None;
                } else {
                    self.pending_action = None;
                    self.perform_action(action);
                }
            }
        }
    }

    /// Globale Tastenkürzel. Wird einmal pro Frame vor dem Canvas ausgeführt.
    ///
    /// Wichtig: `consume_key` entfernt das Event, damit kein Widget es zusätzlich
    /// sieht. Kürzel, die im Textfeld eine andere Bedeutung haben (Strg+A/X/D),
    /// werden nur ausgelöst, wenn gerade nicht getippt wird.
    fn handle_shortcuts(&mut self, ctx: &Context) {
        // Bei offenem Bestätigungsdialog keine weiteren Aktionen auslösen.
        if self.pending_action.is_some() {
            return;
        }

        let typing = ctx.wants_keyboard_input();
        let ctrl = egui::Modifiers::COMMAND;
        let ctrl_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);

        // --- Datei: gelten immer, auch beim Tippen ---
        if ctx.input_mut(|i| i.consume_key(ctrl_shift, egui::Key::S)) {
            crate::io::save_project_dialog(self, true);
        } else if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::S)) {
            crate::io::save_project_dialog(self, false);
        }
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::O)) {
            self.request_action(PendingAction::OpenDialog);
        }
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::N)) {
            self.request_action(PendingAction::NewDocument);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::P)) {
            crate::io::print_dialog(self, ctx);
        }

        // --- Bearbeiten: nur wenn nicht getippt wird ---
        if typing {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::A)) {
            self.select_all();
        }
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::D)) {
            self.duplicate_selection();
        }
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::X)) {
            self.cut_selection();
        }
    }

    /// Alle Elemente der aktuellen Seite auswählen.
    pub fn select_all(&mut self) {
        let ids: Vec<u64> = self
            .doc
            .pages
            .get(self.page_index)
            .map(|p| p.elements.iter().map(|e| e.id).collect())
            .unwrap_or_default();
        let n = ids.len();
        self.selection = ids;
        self.set_status(format!("{n} Objekt(e) ausgewählt."));
    }

    /// Auswahl duplizieren — leicht versetzt, damit das Duplikat sichtbar ist.
    pub fn duplicate_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.push_history();
        const OFFSET: f32 = 12.0;
        let originals: Vec<Element> = self
            .doc
            .pages
            .get(self.page_index)
            .map(|p| {
                p.elements
                    .iter()
                    .filter(|e| self.selection.contains(&e.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        let mut new_ids = Vec::with_capacity(originals.len());
        for mut el in originals {
            let new_id = self.next_id();
            // Bilder teilen sich die Pixeldaten unter der neuen ID.
            if el.kind == ElementKind::Image {
                if let Some(entry) = self.images.map.get(&el.id).cloned() {
                    self.images.map.insert(new_id, entry);
                }
            }
            el.id = new_id;
            el.x += OFFSET;
            el.y += OFFSET;
            new_ids.push(new_id);
            if let Some(page) = self.doc.pages.get_mut(self.page_index) {
                page.elements.push(el);
            }
        }
        self.selection = new_ids;
        self.touch();
        self.set_status("Dupliziert.");
    }

    /// Ausschneiden = kopieren + löschen.
    pub fn cut_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.copy_selection();
        self.delete_selected();
        self.set_status("Ausgeschnitten.");
    }

    // ===================================================================
    // Undo / Redo
    // ===================================================================

    fn with_init_history(mut self) -> Self {
        self.history.init(self.snapshot());
        self
    }

    /// Erstellt einen Snapshot des aktuellen Zustands.
    pub fn snapshot(&self) -> crate::history::Snapshot {
        crate::history::Snapshot {
            doc: self.doc.clone(),
            selection: self.selection.clone(),
            page_index: self.page_index,
        }
    }

    /// Nimmt einen History-Snapshot auf (vor einer Mutation aufrufen).
    pub fn push_history(&mut self) {
        self.history.push(self.snapshot());
    }

    /// Undo: stellt vorherigen Zustand wieder her.
    pub fn undo(&mut self) {
        if let Some(snap) = self.history.undo() {
            self.doc = snap.doc.clone();
            self.selection = snap.selection.clone();
            self.page_index = snap.page_index;
            self.editing = None;
            self.crop_mode = false;
            self.interaction = Interaction::None;
            self.touch();
            self.set_status("Rückgängig.");
        }
    }

    /// Redo: stellt verworfenen Zustand wieder her.
    pub fn redo(&mut self) {
        if let Some(snap) = self.history.redo() {
            self.doc = snap.doc.clone();
            self.selection = snap.selection.clone();
            self.page_index = snap.page_index;
            self.editing = None;
            self.crop_mode = false;
            self.interaction = Interaction::None;
            self.touch();
            self.set_status("Wiederhergestellt.");
            // Undo/Redo schreibt den Stand zurück, damit Datei und GUI synchron
            // bleiben (KI-Agent sieht konsistenten Zustand).
            self.write_back_to_disk();
        }
    }

    // ===================================================================
    // File-Watch (AI-Schnittstelle)
    // ===================================================================

    /// Öffnet eine Datei beim Start (z. B. per Kommandozeilen-Argument).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_path(&mut self, path: std::path::PathBuf) {
        match crate::io::load_project(&path) {
            Ok((doc, images, fonts, next_id)) => {
                self.doc = doc;
                self.images = images;
                self.fonts = fonts;
                self.fonts_dirty = true;
                self.page_index = 0;
                self.next_id = next_id;
                self.clear_selection();
                self.editing = None;
                self.crop_mode = false;
                self.interaction = Interaction::None;
                self.file_path = Some(path);
                self.modified = false;
                self.last_disk_write = self.disk_mtime();
                self.history.init(self.snapshot());
                self.set_status("Dokument geöffnet (Datei wird beobachtet).");
            }
            Err(e) => self.set_status(format!("Öffnen fehlgeschlagen: {e}")),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn open_path(&mut self, _path: std::path::PathBuf) {}

    /// mtime der aktuell gebundenen Datei, falls vorhanden.
    fn disk_mtime(&self) -> Option<std::time::SystemTime> {
        let path = self.file_path.as_ref()?;
        std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
    }

    /// Gleicht den File-Watcher an `file_path` an (starten/stoppen/neustarten).
    /// Wird jeden Frame aufgerufen – idempotent.
    fn reconcile_watcher(&mut self) {
        let same = match (&self.file_path, &self.watched_path) {
            (Some(a), Some(b)) => a == b,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        // Alten Watcher verwerfen.
        self.file_watcher = None;
        self.file_watch_rx = None;
        self.watched_path = None;
        // Neuen starten, falls ein Pfad gesetzt ist.
        if let Some(path) = self.file_path.clone() {
            match crate::file_watch::FileWatcher::start(path.clone()) {
                Ok((w, rx)) => {
                    self.file_watcher = Some(w);
                    self.file_watch_rx = Some(rx);
                    self.watched_path = Some(path);
                }
                Err(_e) => {
                    self.set_status("Datei-Beobachtung nicht verfügbar.");
                }
            }
        }
    }

    /// Lädt die Datei neu und übernimmt sie als History-Schritt (Undo-fähig).
    fn reload_from_disk(&mut self) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let Ok(json) = std::fs::read_to_string(&path) else {
            self.set_status("Datei konnte nicht gelesen werden.");
            return;
        };
        // Nur gültiges JSON übernehmen – teilschreibende Agenten sind unkritisch.
        let project: crate::io::Project = match serde_json::from_str(&json) {
            Ok(p) => p,
            Err(_e) => {
                self.set_status("Datei enthält ungültiges JSON – Reload ignoriert.");
                return;
            }
        };
        use base64::Engine;
        let mut images = crate::store::ImageStore::default();
        let mut fonts = crate::store::FontStore::default();
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
        // Aktuellen Zustand als Undo-Schritt sichern.
        self.push_history();
        self.doc = project.doc;
        self.images = images;
        self.fonts = fonts;
        self.fonts_dirty = true;
        self.next_id = max_id + 1;
        self.modified = false;
        self.last_disk_write = self.disk_mtime();
        self.editing = None;
        self.crop_mode = false;
        self.interaction = Interaction::None;
        self.set_status("Dokument von extern aktualisiert.");
    }

    /// Schreibt den aktuellen GUI-Stand leise in die Datei (ohne Dialog),
    /// aktualisiert `last_disk_write`, sodass der Watcher das eigene
    /// Schreibereignis ignoriert. Setzt `modified` auf false.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_back_to_disk(&mut self) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if crate::io::save_project(&path, self).is_ok() {
            self.last_disk_write = self.disk_mtime();
            self.modified = false;
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn write_back_to_disk(&mut self) {}

    /// Pumpt ankommende Watcher-Ereignisse ab. Muss jeden Frame aufgerufen
    /// werden. Behandelt die Selbst-Schreib-Unterdrückung und Konflikte.
    fn poll_file_watcher(&mut self) {
        // Konflikt-Dialog blockiert automatische Anwendung weiterer Events
        // (nicht aber das Entgegennehmen).
        let mut events: Vec<std::path::PathBuf> = Vec::new();
        if let Some(rx) = &self.file_watch_rx {
            while let Ok(p) = rx.try_recv() {
                events.push(p);
            }
        }
        if events.is_empty() {
            return;
        }
        // Eigenes Schreiben ignorieren: wenn die mtime <= last_disk_write,
        // handelt es sich um unseren eigenen Schreibvorgang.
        if let Some(last) = self.last_disk_write {
            if let Some(m) = self.disk_mtime() {
                if m <= last {
                    return;
                }
            }
        }
        // Konflikt: GUI hat ungespeicherte eigene Änderungen.
        if self.modified {
            self.pending_external_change = true;
            self.show_conflict_dialog = true;
            return;
        }
        self.reload_from_disk();
    }

    /// Wendet eine anstehende externe Änderung an (Konflikt → "Übernehmen").
    pub fn accept_external_change(&mut self) {
        self.pending_external_change = false;
        self.show_conflict_dialog = false;
        self.reload_from_disk();
    }

    /// Behält den eigenen GUI-Stand und überschreibt die externe Datei
    /// (Konflikt → "Meine behalten").
    pub fn keep_local_version(&mut self) {
        self.pending_external_change = false;
        self.show_conflict_dialog = false;
        self.write_back_to_disk();
        self.set_status("Eigene Änderungen beibehalten.");
    }

    // ===================================================================
    // Web-Sync (WASM-only): Backend-Anbindung für die Browser-Version
    // ===================================================================

    #[cfg(target_arch = "wasm32")]
    fn tick_web_sync(&mut self, ctx: &egui::Context) {
        use crate::web_sync::{next_event, WebEvent};

        /// Ruhezeit nach der letzten Änderung, bevor gespeichert wird.
        const AUTOSAVE_DEBOUNCE_S: f64 = 2.0;
        /// Abstand zwischen zwei Versionsabfragen.
        const POLL_INTERVAL_S: f64 = 2.0;

        let now = ctx.input(|i| i.time);
        let needs_initial_load = self.web_doc.is_some() && !self.web_initial_loaded;

        // 1) Initiales Laden (einmalig pro WebDoc).
        if needs_initial_load && !self.web_load_in_flight {
            if let Some(w) = &self.web_doc {
                self.web_load_in_flight = true;
                w.spawn_initial_load();
                self.web_status = "Lade Dokument…".to_string();
            }
        }

        // 2) Events abpumpen (sync, vom Hintergrund-Task gepusht).
        while let Some(ev) = next_event() {
            match ev {
                WebEvent::Loaded { json, version } => {
                    self.web_load_in_flight = false;
                    self.web_initial_loaded = true;
                    self.adopt_web_doc(&json, version);
                }
                WebEvent::RemoteChanged { json, version } => {
                    // NICHT ersetzen — zusammenführen. Sonst geht alles
                    // verloren, was seit dem letzten Abgleich lokal entstand.
                    self.merge_web_doc(&json, version);
                }
                WebEvent::Saved { version } => {
                    self.web_save_in_flight = false;
                    // `modified` nur zurücksetzen, wenn seit dem Absenden
                    // nichts Neues dazugekommen ist.
                    if !self.web_dirty_since_save {
                        self.modified = false;
                        self.web_dirty_at = None;
                    }
                    if let Some(w) = &mut self.web_doc {
                        w.version = version;
                        w.base_doc = Some(self.doc.clone());
                    }
                    self.web_status = "Gespeichert.".to_string();
                }
                WebEvent::SaveConflict { json, version } => {
                    // Jemand anders war schneller. Serverstand einmergen und
                    // danach erneut speichern.
                    self.web_save_in_flight = false;
                    self.merge_web_doc(&json, version);
                    self.web_dirty_at = Some(0.0); // sofort erneut speichern
                }
                WebEvent::Created {
                    slug,
                    token,
                    version,
                } => {
                    self.adopt_created_doc(slug, token, version);
                }
                WebEvent::CreateFailed(msg) => {
                    // Der Dialog bleibt offen: Der Nutzer muss den Namen
                    // ändern können, ohne alles neu einzutippen.
                    self.server_save_in_flight = false;
                    self.server_save_error = translate_create_error(&msg);
                }
                WebEvent::Error(msg) => {
                    self.web_save_in_flight = false;
                    self.web_load_in_flight = false;
                    self.web_status = msg;
                }
            }
        }

        // 3) Auto-Save nach Ruhezeit.
        //
        // `web_dirty_at` markiert den Zeitpunkt der ERSTEN Änderung seit dem
        // letzten Speichern — nicht den letzten Frame. Genau daran scheiterte
        // die alte Fassung: Sie setzte den Zeitstempel in jedem Frame neu, in
        // dem `modified` true war, sodass die Ruhezeit nie ablief und die
        // Web-Version niemals speicherte.
        if self.modified && self.web_dirty_at.is_none() {
            self.web_dirty_at = Some(now);
        }
        if let Some(dirty_at) = self.web_dirty_at {
            if !self.web_save_in_flight
                && self.web_initial_loaded
                && now - dirty_at > AUTOSAVE_DEBOUNCE_S
            {
                if let Some(w) = &self.web_doc {
                    self.web_save_in_flight = true;
                    self.web_dirty_since_save = false;
                    self.web_status = "Speichern…".to_string();
                    w.spawn_save(
                        self.doc.clone(),
                        self.images.clone(),
                        self.fonts.clone(),
                        w.version,
                    );
                }
            }
        }

        // 4) Polling: nur die Versionsnummer abfragen (wenige Bytes).
        if self.web_initial_loaded && (now - self.web_last_poll) > POLL_INTERVAL_S {
            self.web_last_poll = now;
            if let Some(w) = &self.web_doc {
                w.spawn_poll();
            }
        }
    }

    /// Öffnet den Dialog „Am Server speichern".
    ///
    /// Der Namensvorschlag kommt aus dem, was schon da ist: der Slug eines
    /// bereits gebundenen Dokuments, sonst der Dateiname eines geöffneten
    /// .boxdoc. Ein leerer Vorschlag ist besser als ein erfundener — der Name
    /// wird Teil der URL und lässt sich später nicht mehr ändern.
    #[cfg(target_arch = "wasm32")]
    pub fn open_server_save_dialog(&mut self) {
        let suggestion = self
            .web_doc
            .as_ref()
            .map(|w| w.slug.clone())
            .or_else(|| {
                self.file_path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| crate::web_sync::slugify(&s.to_string_lossy()))
            })
            .unwrap_or_default();
        self.server_save_name = suggestion;
        self.server_save_error.clear();
        self.server_save_in_flight = false;
        self.show_server_save = true;
    }

    /// Speichert sofort am Server, ohne auf die Autosave-Ruhezeit zu warten.
    ///
    /// Ist noch kein Serverdokument gebunden, führt der Weg über den Dialog:
    /// „Speichern" ohne Ziel muss erst nach einem Ziel fragen.
    #[cfg(target_arch = "wasm32")]
    pub fn save_to_server_now(&mut self) {
        if self.web_doc.is_none() {
            self.open_server_save_dialog();
            return;
        }
        if self.web_save_in_flight {
            // Der laufende PUT trägt den aktuellen Stand noch nicht; `modified`
            // bleibt gesetzt, also holt die Autosave-Runde ihn gleich nach.
            self.set_status("Speichern läuft bereits…");
            return;
        }
        let Some(w) = &self.web_doc else { return };
        self.web_save_in_flight = true;
        self.web_dirty_since_save = false;
        self.web_dirty_at = None;
        w.spawn_save(
            self.doc.clone(),
            self.images.clone(),
            self.fonts.clone(),
            w.version,
        );
        self.web_status = "Speichern…".to_string();
        self.set_status("Am Server speichern…");
    }

    /// Bindet die Anwendung an ein frisch angelegtes Serverdokument.
    ///
    /// Das Dokument auf dem Server ist leer — der lokale Stand ist die erste
    /// Änderung darauf und muss unmittelbar hinauf. Deshalb `web_dirty_at`
    /// auf 0: die Autosave-Runde im selben Frame sieht die Ruhezeit als
    /// abgelaufen an und schickt den PUT sofort los.
    #[cfg(target_arch = "wasm32")]
    fn adopt_created_doc(&mut self, slug: String, token: String, version: u64) {
        crate::web_sync::update_url(&slug, &token);
        let share = if token.is_empty() {
            format!("{}/{}", crate::web_sync::detect_base(), slug)
        } else {
            format!("{}/{}?t={}", crate::web_sync::detect_base(), slug, token)
        };

        self.web_doc = Some(crate::web_sync::WebDoc::bind(slug, token, version));
        self.web_initial_loaded = true;
        self.web_load_in_flight = false;
        self.modified = true;
        self.web_dirty_since_save = false;
        self.web_dirty_at = Some(0.0);
        // Der Datei-Browser soll das neue Dokument sofort zeigen, nicht erst
        // nach der nächsten Fünf-Sekunden-Runde.
        self.files_last_refresh = 0.0;

        self.show_server_save = false;
        self.server_save_in_flight = false;
        self.server_save_error.clear();
        self.web_status = "Angelegt, wird hochgeladen…".to_string();
        self.set_status(format!("Am Server angelegt: {share}"));
    }

    /// Dialog „Am Server speichern": Name wählen, Zugriff wählen, anlegen.
    #[cfg(target_arch = "wasm32")]
    fn show_server_save_dialog(&mut self, ctx: &Context) {
        if !self.show_server_save {
            return;
        }

        // Die Eingabe wird beim Tippen bereinigt statt hinterher abgelehnt:
        // Wer „Mein Lebenslauf" eintippt, bekommt „meinlebenslauf" zu sehen und
        // weiß sofort, wie die URL aussehen wird.
        let mut submit = false;
        let mut cancel = false;

        egui::Window::new("Am Server speichern")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                ui.label("Name des Dokuments (wird Teil der Adresse):");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.server_save_name)
                        .hint_text("z. B. lebenslauf")
                        .desired_width(f32::INFINITY),
                );
                if resp.changed() {
                    self.server_save_name = crate::web_sync::slugify(&self.server_save_name);
                    self.server_save_error.clear();
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    submit = true;
                }

                let name = self.server_save_name.clone();
                let valid = crate::web_sync::is_valid_slug(&name);
                let base = crate::web_sync::detect_base();
                if valid {
                    ui.label(
                        egui::RichText::new(format!("{base}/{name}"))
                            .small()
                            .weak(),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("4 bis 32 Zeichen, nur a–z und 0–9.")
                            .small()
                            .weak(),
                    );
                }

                ui.add_space(6.0);
                ui.checkbox(
                    &mut self.server_save_protected,
                    "Mit Token schützen (nicht öffentlich)",
                )
                .on_hover_text(
                    "Ohne Token: saubere Adresse, im Datei-Browser sichtbar.
                     Mit Token: nur über den vollständigen Link erreichbar —                      geht der Link verloren, ist das Dokument nicht mehr                      aufzurufen.",
                );

                if !self.server_save_error.is_empty() {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 100, 90),
                        &self.server_save_error,
                    );
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let can_send = valid && !self.server_save_in_flight;
                    if ui
                        .add_enabled(can_send, egui::Button::new("Anlegen und speichern"))
                        .clicked()
                    {
                        submit = true;
                    }
                    if ui.button("Abbrechen").clicked() {
                        cancel = true;
                    }
                    if self.server_save_in_flight {
                        ui.spinner();
                    }
                });
            });

        if cancel {
            self.show_server_save = false;
            self.server_save_error.clear();
            return;
        }
        if submit
            && !self.server_save_in_flight
            && crate::web_sync::is_valid_slug(&self.server_save_name)
        {
            self.server_save_in_flight = true;
            self.server_save_error.clear();
            crate::web_sync::WebDoc::spawn_create(
                &self.server_save_name,
                !self.server_save_protected,
            );
        }
    }

    /// Übernimmt einen Serverstand unverändert (initiales Laden).
    #[cfg(target_arch = "wasm32")]
    fn adopt_web_doc(&mut self, json: &str, version: u64) {
        let Some((doc, images, fonts, next_id)) = self.decode_web_json(json) else {
            return;
        };
        self.doc = doc;
        self.images = images;
        self.fonts = fonts;
        self.fonts_dirty = true;
        self.next_id = next_id;
        self.modified = false;
        self.web_dirty_at = None;
        self.editing = None;
        self.crop_mode = false;
        self.interaction = Interaction::None;
        self.history.init(self.snapshot());
        if let Some(w) = &mut self.web_doc {
            w.version = version;
            w.last_content_hash = crate::web_sync::hash_of(json);
            w.base_doc = Some(self.doc.clone());
        }
        self.web_status = "Geladen.".to_string();
    }

    /// Führt einen neueren Serverstand mit dem lokalen zusammen.
    ///
    /// Das ist der Kern der Mehrbenutzer-Fähigkeit: Der lokale Stand wird
    /// nicht überschrieben, sondern gegen die gemeinsame Basis mit dem
    /// Serverstand gemergt. Was zwei Leute an verschiedenen Stellen getan
    /// haben, überlebt beides.
    #[cfg(target_arch = "wasm32")]
    fn merge_web_doc(&mut self, json: &str, version: u64) {
        let Some((remote_doc, images, fonts, next_id)) = self.decode_web_json(json) else {
            return;
        };

        // Ohne bekannte Basis ist kein Drei-Wege-Merge möglich. Dann gilt der
        // Serverstand als Basis und der lokale Stand als Änderung darauf —
        // das erhält lokale Arbeit immer noch besser als blindes Ersetzen.
        let base = self
            .web_doc
            .as_ref()
            .and_then(|w| w.base_doc.clone())
            .unwrap_or_else(|| remote_doc.clone());

        let (merged, report) = crate::merge::merge_documents(&base, &self.doc, &remote_doc);

        self.push_history();
        self.doc = merged;
        // Bilder und Fonts additiv übernehmen — sie sind unveränderlich und
        // per ID eindeutig, dürfen also nie verloren gehen.
        for (id, entry) in images.map {
            self.images.map.entry(id).or_insert(entry);
        }
        for (name, entry) in fonts.map {
            self.fonts.map.entry(name).or_insert(entry);
        }
        self.fonts_dirty = true;
        self.next_id = self.next_id.max(next_id);
        self.editing = None;
        self.interaction = Interaction::None;

        if let Some(w) = &mut self.web_doc {
            w.version = version;
            w.last_content_hash = crate::web_sync::hash_of(json);
            // Neue gemeinsame Basis ist der Serverstand — nur er ist beiden
            // Seiten bekannt.
            w.base_doc = Some(remote_doc);
        }

        // Nach dem Merge weicht der lokale Stand in der Regel vom Server ab
        // → als änderungsbedürftig markieren, damit er hochgeladen wird.
        self.modified = true;
        self.web_dirty_since_save = true;
        self.web_status = report.summary();
    }

    /// Dekodiert Server-JSON. Bei ungültigem JSON wird nur der Status gesetzt —
    /// ein kaputter Serverstand darf das lokale Dokument nicht beschädigen.
    #[cfg(target_arch = "wasm32")]
    fn decode_web_json(
        &mut self,
        json: &str,
    ) -> Option<(Document, crate::store::ImageStore, crate::store::FontStore, u64)> {
        match serde_json::from_str::<crate::io::Project>(json) {
            Ok(project) => Some(crate::web_sync::decode_project(project)),
            Err(e) => {
                self.web_status = format!("Ungültiges JSON vom Server: {}", e);
                None
            }
        }
    }

}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Custom-Fonts registrieren, falls nach einem Ladevorgang ausstehend.
        // Hier (statt im Loader), weil der Lade-Code keinen &egui::Context hat.
        if self.fonts_dirty {
            self.fonts_dirty = false;
            crate::fonts::install_with_custom(&ctx, &self.fonts);
        }

        // --- Close-Guard -------------------------------------------------
        // Muss VOR allem anderen laufen: wenn der Nutzer das Fenster schließt
        // und es ungespeicherte Änderungen gibt, wird der Schließvorgang
        // abgebrochen und stattdessen der Bestätigungsdialog gezeigt.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.close_requested_by_app {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if ctx.input(|i| i.viewport().close_requested())
                && self.modified
                && !self.quit_confirmed
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_action = Some(PendingAction::Quit);
            }
        }

        // Globale Tastenkürzel (Strg+S/O/N/A/D/X/P).
        self.handle_shortcuts(&ctx);

        // File-Watch: Watcher an file_path anpassen und ankommende Ereignisse
        // abpumpen (AI-Schnittstelle).
        self.reconcile_watcher();
        self.poll_file_watcher();

        // Web-Sync (WASM): Polling, Auto-Save, Event-Verarbeitung.
        #[cfg(target_arch = "wasm32")]
        self.tick_web_sync(&ctx);

        // Konflikt-Dialog (externe Änderung bei ungespeicherten eigenen Änderungen).
        if self.show_conflict_dialog {
            egui::Window::new("Datei extern geändert")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(&ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label("Die geöffnete Datei wurde von außen geändert");
                        ui.label("(z. B. durch eine KI), aber es gibt ungespeicherte");
                        ui.label("eigene Änderungen in der GUI.");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Externe übernehmen").clicked() {
                                self.accept_external_change();
                            }
                            if ui.button("Eigene behalten").clicked() {
                                self.keep_local_version();
                            }
                        });
                    });
                });
        }

        // Theme beim allerersten Frame einmalig auf dem Live-Context fixieren.
        // In main.rs wird themes::apply() schon während der Creation-Phase
        // gerufen, aber eframe 0.34 übernimmt das nicht zuverlässig in den
        // ersten Render-Frame (-> saved Theme wirkt wie nicht angewendet).
        if self.theme_init_pending {
            self.theme_init_pending = false;
            crate::themes::apply(&ctx, self.theme_target);
        }

        // Theme-Fade animieren.
        if self.theme_anim < 1.0 {
            let dt = ctx.input(|i| i.unstable_dt).min(0.1);
            self.theme_anim = (self.theme_anim + dt / crate::themes::fade_duration()).min(1.0);
            crate::themes::tick_fade(&ctx, self.theme_from, self.theme_target, self.theme_anim);
        }

        // Bestätigungsdialog für Aktionen, die Arbeit verwerfen würden.
        self.show_unsaved_dialog(&ctx);

        // Dialog „Am Server speichern" (Browser).
        #[cfg(target_arch = "wasm32")]
        self.show_server_save_dialog(&ctx);

        self.show_menu(&ctx);
        self.show_files_panel(&ctx);
        self.show_properties(&ctx);
        self.show_json_editor(&ctx);
        self.show_status(&ctx);

        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(0.0))
            .show(&ctx, |ui| {
                show_canvas(self, &ctx, ui);
            });

        // Dateien, die per Drag&Drop herein gezogen wurden.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut had_drops = false;
        for f in &dropped {
            let bytes = if let Some(bytes) = &f.bytes {
                // Web: Bytes direkt verfügbar.
                Some(bytes.to_vec())
            } else if let Some(path) = &f.path {
                // Native: Datei vom Pfad lesen.
                std::fs::read(path).ok()
            } else {
                None
            };

            if let Some(bytes) = bytes {
                let is_image = bytes.starts_with(&[0x89, b'P', b'N', b'G'])
                    || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
                    || bytes.starts_with(b"BM")
                    || f.mime.starts_with("image/");
                if is_image {
                    self.add_image_from_bytes(bytes, None);
                    had_drops = true;
                }
            }
        }
        if had_drops {
            ctx.input_mut(|i| i.raw.dropped_files.clear());
        }

        // Web: asynchron geladenes Bild aus dem File-Dialog oder Zwischenablage.
        if let Some(bytes) = crate::io::take_pending_image() {
            self.add_image_from_bytes(bytes, None);
        }

        // Web: asynchron geladene Projekt-Datei (.boxdoc).
        if let Some(json) = crate::io::take_pending_project() {
            self.load_project_from_json(&json);
        }

        // Web: asynchron geladene ODT-Datei.
        if let Some(bytes) = crate::io::take_pending_odt() {
            self.load_odt_from_bytes(&bytes);
        }

        // Web: asynchron geladene Schriftdatei.
        if let Some((name, bytes)) = crate::io::take_pending_font() {
            self.add_font_from_bytes(name, bytes);
        }
    }
}

impl EditorApp {
    fn show_menu(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Datei", |ui| {
                    // Destruktive Aktionen laufen über `request_action`, damit
                    // ungespeicherte Arbeit nie kommentarlos verloren geht.
                    if menu_entry(ui, "Neu", "Strg+N").clicked() {
                        self.request_action(PendingAction::NewDocument);
                        ui.close_menu();
                    }
                    if menu_entry(ui, "Öffnen…", "Strg+O").clicked() {
                        self.request_action(PendingAction::OpenDialog);
                        ui.close_menu();
                    }
                    // Speichern schreibt in das gebundene Ziel — auf Native
                    // eine Datei, im Browser ein Dokument auf dem Server.
                    // Derselbe Griff, dasselbe Versprechen; nur das Ziel ist
                    // ein anderes. Der Download bleibt im Browser als eigener
                    // Eintrag: Er ist ein Export, kein Speichern, denn was
                    // heruntergeladen wurde, bekommt keine weitere Änderung
                    // mehr mit.
                    if menu_entry(ui, "Speichern", "Strg+S").clicked() {
                        crate::io::save_project_dialog(self, false);
                        ui.close_menu();
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    if menu_entry(ui, "Speichern unter…", "Strg+Umschalt+S").clicked() {
                        crate::io::save_project_dialog(self, true);
                        ui.close_menu();
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        if menu_entry(ui, "Am Server speichern…", "Strg+Umschalt+S").clicked() {
                            crate::io::save_project_dialog(self, true);
                            ui.close_menu();
                        }
                        if ui
                            .button("Herunterladen (.boxdoc)")
                            .on_hover_text(
                                "Lädt eine Kopie als Datei herunter. Spätere \
                                 Änderungen landen nicht darin — dafür ist \
                                 der Eintrag Speichern da.",
                            )
                            .clicked()
                        {
                            crate::io::download_project(self);
                            ui.close_menu();
                        }
                    }
                    // Export/Import ist auf Web und Native derselbe Menübaum.
                    // Beide Plattformen erzeugen dieselben Bytes; nur das Ziel
                    // unterscheidet sich (Datei vs. Download) — und das steckt
                    // in `io.rs`, nicht hier. Nur die zwei Einträge, die im
                    // Browser wirklich nicht gehen (PDF-Import braucht pdfium,
                    // Drucken den Systemdialog), sind auf Web ausgegraut.
                    ui.separator();
                    if ui.button("ODT exportieren…").clicked() {
                        crate::io::export_odt_dialog(self);
                    }
                    if ui.button("ODT öffnen…").clicked() {
                        crate::io::import_odt_dialog(self);
                    }
                    ui.separator();
                    if ui.button("PDF exportieren…").clicked() {
                        crate::io::export_pdf_dialog(self, ctx);
                        ui.close_menu();
                    }
                    {
                        // Ausgegraut statt versteckt: Wer den Eintrag sucht,
                        // soll sehen, dass es ihn gibt — und woran es hakt.
                        let native = cfg!(not(target_arch = "wasm32"));
                        let btn = ui.add_enabled(native, egui::Button::new("PDF öffnen…"));
                        if btn
                            .on_disabled_hover_text(
                                "PDF-Import braucht pdfium (native Bibliothek) — \
                                 nur in der Desktop-Version.",
                            )
                            .clicked()
                        {
                            crate::io::import_pdf_dialog(self, ctx);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Seite als SVG exportieren…").clicked() {
                        crate::io::export_svg_dialog(self, ctx, false);
                        ui.close_menu();
                    }
                    let has_sel = !self.selection.is_empty();
                    let label = if has_sel {
                        format!("Auswahl als SVG exportieren… ({})", self.selection.len())
                    } else {
                        String::from("Auswahl als SVG exportieren…")
                    };
                    let btn = ui.add_enabled(has_sel, egui::Button::new(label));
                    if btn.on_disabled_hover_text("Erst Objekte auswählen").clicked() {
                        crate::io::export_svg_dialog(self, ctx, true);
                        ui.close_menu();
                    }
                    let img_n = self.selected_images().len();
                    let img_label = if img_n > 1 {
                        format!("Ausgewählte Bilder speichern… ({img_n})")
                    } else {
                        String::from("Ausgewähltes Bild speichern…")
                    };
                    let btn = ui.add_enabled(img_n > 0, egui::Button::new(img_label));
                    if btn
                        .on_hover_text(if cfg!(target_arch = "wasm32") {
                            "Lädt die Bilddatei selbst herunter (PNG), \
                             in Originalauflösung."
                        } else {
                            "Speichert die Bilddatei selbst — PNG oder JPEG, \
                             in Originalauflösung."
                        })
                        .on_disabled_hover_text("Erst ein Bild auswählen")
                        .clicked()
                    {
                        crate::io::export_image_dialog(self);
                        ui.close_menu();
                    }
                    ui.separator();
                    {
                        let native = cfg!(not(target_arch = "wasm32"));
                        if menu_entry_enabled(ui, native, "Drucken…", "Strg+P")
                            .on_disabled_hover_text(
                                "Im Browser: PDF exportieren und das PDF drucken.",
                            )
                            .clicked()
                        {
                            crate::io::print_dialog(self, ctx);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    ui.menu_button("Einstellungen", |ui| {
                        ui.label("Einheit:");
                        let mut u = self.settings.units;
                        for candidate in Units::all() {
                            ui.selectable_value(&mut u, candidate, candidate.label());
                        }
                        if u != self.settings.units {
                            self.settings.units = u;
                            crate::settings_io::save(&self.settings);
                        }

                        ui.separator();
                        ui.label("Seitenwechsel beim Scrollen:");
                        let mut sm = self.settings.scroll_mode;
                        ui.selectable_value(
                            &mut sm,
                            ScrollMode::Continuous,
                            "Fortlaufend (Scrollen wechselt Seite)",
                        );
                        ui.selectable_value(
                            &mut sm,
                            ScrollMode::PageByPage,
                            "Seitenweise (über Eigenschaften)",
                        );
                        if sm != self.settings.scroll_mode {
                            self.settings.scroll_mode = sm;
                            crate::settings_io::save(&self.settings);
                        }
                    });
                });

                ui.menu_button("Bearbeiten", |ui| {
                    // Ausgegraut, wenn nichts zu tun ist — der Nutzer sieht
                    // damit sofort, ob es noch etwas zurückzunehmen gibt.
                    ui.add_enabled_ui(self.history.can_undo(), |ui| {
                        if menu_entry(ui, "Rückgängig", "Strg+Z").clicked() {
                            self.undo();
                            ui.close_menu();
                        }
                    });
                    ui.add_enabled_ui(self.history.can_redo(), |ui| {
                        if menu_entry(ui, "Wiederholen", "Strg+Y").clicked() {
                            self.redo();
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    let has_sel = !self.selection.is_empty();
                    ui.add_enabled_ui(has_sel, |ui| {
                        if menu_entry(ui, "Ausschneiden", "Strg+X").clicked() {
                            self.cut_selection();
                            ui.close_menu();
                        }
                        if menu_entry(ui, "Kopieren", "Strg+C").clicked() {
                            self.copy_selection();
                            ui.close_menu();
                        }
                        if menu_entry(ui, "Duplizieren", "Strg+D").clicked() {
                            self.duplicate_selection();
                            ui.close_menu();
                        }
                    });
                    ui.add_enabled_ui(!self.clipboard.is_empty(), |ui| {
                        if menu_entry(ui, "Einfügen", "Strg+V").clicked() {
                            self.start_paste();
                            ui.close_menu();
                        }
                    });
                    ui.separator();
                    if menu_entry(ui, "Alles auswählen", "Strg+A").clicked() {
                        self.select_all();
                        ui.close_menu();
                    }
                    ui.add_enabled_ui(has_sel, |ui| {
                        if menu_entry(ui, "Löschen", "Entf").clicked() {
                            self.delete_selected();
                            ui.close_menu();
                        }
                    });
                });

                ui.menu_button("Einfügen", |ui| {
                    if ui.button("Text").clicked() {
                        self.add_text(None);
                        ui.close_menu();
                    }
                    if ui.button("Bild…").clicked() {
                        crate::io::open_image_dialog(self);
                        ui.close_menu();
                    }
                    if ui.button("Schrift laden…").clicked() {
                        crate::io::open_font_dialog(self);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Rechteck").clicked() {
                        self.add_rectangle(None);
                        ui.close_menu();
                    }
                    if ui.button("Ellipse").clicked() {
                        self.add_ellipse(None);
                        ui.close_menu();
                    }
                    if ui.button("Linie").clicked() {
                        self.add_line(None);
                        ui.close_menu();
                    }
                    if ui
                        .button("Pfad")
                        .on_hover_text(
                            "Fügt eine bearbeitbare Kurve ein und öffnet die Knotenbearbeitung.",
                        )
                        .clicked()
                    {
                        self.add_path(None);
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label("Zeichnen:");
                    if menu_entry(ui, "Linie zeichnen", "L").clicked() {
                        self.set_tool(Tool::Line);
                        ui.close_menu();
                    }
                    if menu_entry(ui, "Pfad zeichnen", "P").clicked() {
                        self.set_tool(Tool::Pen);
                        ui.close_menu();
                    }
                    if menu_entry(ui, "Freihand", "F").clicked() {
                        self.set_tool(Tool::Freehand);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Seite").clicked() {
                        self.add_page();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Ansicht", |ui| {
                    ui.label("Thema:");
                    let mut theme = self.settings.theme;
                    for t in crate::model::Theme::all() {
                        ui.selectable_value(&mut theme, t, t.label());
                    }
                    if theme != self.settings.theme {
                        self.theme_from = self.theme_target;
                        self.theme_target = theme;
                        self.theme_anim = 0.0;
                        self.settings.theme = theme;
                        crate::settings_io::save(&self.settings);
                    }

                    ui.separator();
                    ui.separator();
                    ui.label("Eigenschaften-Fenster:");
                    let mut side = self.settings.panel_side;
                    for s in crate::model::PanelSide::all() {
                        ui.selectable_value(&mut side, s, s.label());
                    }
                    if side != self.settings.panel_side {
                        self.settings.panel_side = side;
                        crate::settings_io::save(&self.settings);
                    }

                    ui.separator();
                    ui.checkbox(&mut self.show_files, "Datei-Browser");
                    ui.checkbox(&mut self.show_json, "JSON-Editor");

                    ui.separator();
                    ui.label("Seitenausrichtung:");
                    let mut align = self.settings.page_align;
                    ui.selectable_value(&mut align, PageAlign::Left, "Linksbündig");
                    ui.selectable_value(&mut align, PageAlign::Center, "Mittig");
                    ui.selectable_value(&mut align, PageAlign::Right, "Rechtsbündig");
                    if align != self.settings.page_align {
                        self.settings.page_align = align;
                        crate::settings_io::save(&self.settings);
                    }

                    ui.separator();
                    if ui.button("Ansicht zurücksetzen").clicked() {
                        self.view = View::default();
                        ui.close_menu();
                    }
                });

                ui.separator();
                ui.label("Format:");
                let mut fmt = self.doc.format.clone();
                let customs = self.doc.custom_formats.clone();
                let mut open_custom_dialog = false;
                egui::ComboBox::from_id_salt("format")
                    .selected_text(self.doc.format.label())
                    .show_ui(ui, |ui| {
                        for f in PaperFormat::all() {
                            let label = f.label();
                            ui.selectable_value(&mut fmt, f, label);
                        }
                        if !customs.is_empty() {
                            ui.separator();
                            for c in &customs {
                                let label =
                                    format!("{} ({:.0}\u{d7}{:.0} mm)", c.name, c.w_mm, c.h_mm);
                                let value = PaperFormat::Custom(c.clone());
                                ui.selectable_value(&mut fmt, value, label);
                            }
                        }
                        ui.separator();
                        if ui.selectable_label(false, "Benutzerdefiniert\u{2026}").clicked() {
                            open_custom_dialog = true;
                        }
                    });
                if fmt != self.doc.format {
                    self.custom_fmt_edit = None;
                    self.doc.format = fmt;
                    self.touch();
                }
                if open_custom_dialog && self.custom_fmt_edit.is_none() {
                    // Frisch öffnen: Startwerte = aktuelle Seitengröße
                    // (Hochformat), in der im Dokument eingestellten Einheit.
                    let (pw_pt, ph_pt) = self.doc.page_size_pt();
                    let (a, b) = (pt_to_mm(pw_pt), pt_to_mm(ph_pt));
                    let (w_mm, h_mm) = match self.doc.orientation {
                        Orientation::Portrait => (a, b),
                        Orientation::Landscape => (b, a),
                    };
                    let unit = self.settings.units;
                    let name = match &self.doc.format {
                        PaperFormat::Custom(c) => c.name.clone(),
                        _ => String::from("Eigenes Format"),
                    };
                    self.custom_fmt_edit = Some(CustomFormatDraft {
                        name,
                        w: unit.from_pt(mm_to_pt(w_mm)),
                        h: unit.from_pt(mm_to_pt(h_mm)),
                        unit,
                    });
                }

                // Kleines Fenster zum Anlegen/Bearbeiten eigener Formate.
                if self.custom_fmt_edit.is_some() {
                    let mut win_open = true;
                    egui::Window::new("Eigenes Format")
                        .open(&mut win_open)
                        .collapsible(false)
                        .resizable(false)
                        .show(ctx, |ui| {
                            let mut draft = match self.custom_fmt_edit.take() {
                                Some(d) => d,
                                None => return,
                            };
                            let active_is_custom =
                                matches!(self.doc.format, PaperFormat::Custom(_));
                            let mut apply = false;
                            let mut cancel = false;
                            let mut delete_active = false;
                            let mut apply_existing: Option<CustomFormat> = None;
                            let mut delete_existing: Option<String> = None;

                            // Einheit — Wechsel rechnet die aktuellen Werte mit um.
                            let prev_unit = draft.unit;
                            ui.horizontal(|ui| {
                                ui.label("Einheit:");
                                for u in Units::all() {
                                    ui.selectable_value(&mut draft.unit, u, u.label());
                                }
                            });
                            if draft.unit != prev_unit {
                                let (pw, ph) =
                                    (prev_unit.to_pt(draft.w), prev_unit.to_pt(draft.h));
                                draft.w = draft.unit.from_pt(pw);
                                draft.h = draft.unit.from_pt(ph);
                            }

                            egui::Grid::new("eigenes_format_felder")
                                .num_columns(2)
                                .spacing([8.0, 4.0])
                                .show(ui, |ui| {
                                    ui.label("Name:");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut draft.name)
                                            .desired_width(130.0),
                                    );
                                    ui.end_row();
                                    ui.label("Breite:");
                                    ui.add(
                                        egui::DragValue::new(&mut draft.w)
                                            .speed(draft.unit.drag_speed())
                                            .range(custom_fmt_range(draft.unit))
                                            .suffix(format!(" {}", draft.unit.label())),
                                    );
                                    ui.end_row();
                                    ui.label("Höhe:");
                                    ui.add(
                                        egui::DragValue::new(&mut draft.h)
                                            .speed(draft.unit.drag_speed())
                                            .range(custom_fmt_range(draft.unit))
                                            .suffix(format!(" {}", draft.unit.label())),
                                    );
                                    ui.end_row();
                                });

                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button("Übernehmen").clicked() {
                                    apply = true;
                                }
                                if ui.button("Abbrechen").clicked() {
                                    cancel = true;
                                }
                                if active_is_custom && ui.button("Löschen").clicked() {
                                    delete_active = true;
                                }
                            });

                            // Bereits gespeicherte eigene Formate: anklicken
                            // zum Verwenden, ✕ entfernt sie aus dem Dokument.
                            if !customs.is_empty() {
                                ui.separator();
                                ui.label("Gespeicherte Formate:");
                                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                                    for c in &customs {
                                        ui.horizontal(|ui| {
                                            let label = format!(
                                                "{} ({:.1}\u{d7}{:.1} mm)",
                                                c.name, c.w_mm, c.h_mm
                                            );
                                            if ui.selectable_label(false, &label).clicked() {
                                                apply_existing = Some(c.clone());
                                            }
                                            if ui.small_button("\u{2715}").clicked() {
                                                delete_existing = Some(c.name.clone());
                                            }
                                        });
                                    }
                                });
                            }

                            if apply {
                                let name = draft.name.trim().to_string();
                                let name = if name.is_empty() {
                                    String::from("Eigenes Format")
                                } else {
                                    name
                                };
                                let to_mm = |v: f32| pt_to_mm(draft.unit.to_pt(v));
                                let fmt = CustomFormat {
                                    name: name.clone(),
                                    w_mm: to_mm(draft.w).clamp(10.0, 2000.0),
                                    h_mm: to_mm(draft.h).clamp(10.0, 2000.0),
                                };
                                self.doc.apply_custom_format(fmt);
                                self.touch();
                                self.set_status(format!(
                                    "Eigenes Format '{name}' übernommen."
                                ));
                            } else if delete_active {
                                if let PaperFormat::Custom(c) = &self.doc.format {
                                    let name = c.name.clone();
                                    self.doc.remove_custom_format(&name);
                                    self.touch();
                                    self.set_status(format!(
                                        "Eigenes Format '{name}' gelöscht."
                                    ));
                                }
                            } else if let Some(c) = apply_existing {
                                self.doc.format = PaperFormat::Custom(c);
                                self.touch();
                            } else if let Some(name) = delete_existing {
                                self.doc.remove_custom_format(&name);
                                self.touch();
                            } else if !cancel {
                                // Offen lassen — Stand zurückschreiben. Wird
                                // das Fenster per ✕ geschlossen, räumt die
                                // Prüfung nach show() auf.
                                self.custom_fmt_edit = Some(draft);
                            }
                        });
                    if !win_open {
                        self.custom_fmt_edit = None;
                    }
                }

                ui.label("Ausrichtung:");
                let mut orient = self.doc.orientation;
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut orient, Orientation::Portrait, "Hoch");
                    ui.selectable_value(&mut orient, Orientation::Landscape, "Quer");
                });
                if orient != self.doc.orientation {
                    self.doc.orientation = orient;
                    self.touch();
                }

                ui.separator();
                ui.label(format!("{:.0}%", self.view.zoom * 100.0));
                if ui.button("-").clicked() {
                    self.view.zoom = (self.view.zoom * 0.9).max(0.1);
                }
                if ui.button("+").clicked() {
                    self.view.zoom = (self.view.zoom * 1.1).min(6.0);
                }
            });
        });
    }

    fn show_properties(&mut self, ctx: &Context) {
        let panel_side = self.settings.panel_side;

        // Der Inhalt wird in einer Closure gekapselt, damit alle drei
        // Panel-Varianten denselben Inhalt anzeigen.
        let content = |ui: &mut egui::Ui, app: &mut EditorApp, ctx: &Context| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.heading("Eigenschaften");
                    ui.separator();

                    // Seitennavigation
                    ui.label(format!(
                        "Seite {} / {}",
                        app.page_index + 1,
                        app.doc.pages.len()
                    ));
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(app.page_index > 0, |ui| {
                            if ui.button("<").clicked() {
                                app.page_index -= 1;
                                app.clear_selection();
                            }
                        });
                        if ui.button("+ Seite").clicked() {
                            app.add_page();
                        }
                        ui.add_enabled_ui(app.page_index + 1 < app.doc.pages.len(), |ui| {
                            if ui.button(">").clicked() {
                                app.page_index += 1;
                                app.clear_selection();
                            }
                        });
                    });
                    ui.separator();

                    // --- Werkzeuge ---
                    ui.label("Werkzeug:");
                    ui.horizontal_wrapped(|ui| {
                        for t in [Tool::Select, Tool::Line, Tool::Pen, Tool::Freehand] {
                            if ui.selectable_label(app.tool == t, t.label()).clicked() {
                                app.set_tool(t);
                            }
                        }
                    });
                    ui.separator();

                    let Some(sel) = app.primary() else {
                        ui.label(
                            "Kein Objekt ausgewählt.\nKlicke oder ziehe ein Auswahl-Rechteck.",
                        );
                        return;
                    };
                    if app.selection.len() == 1 {
                        app.properties_for(ui, sel);
                    } else {
                        app.position_section(ui);
                        ui.separator();
                        app.align_section(ui);
                        ui.separator();
                        app.multi_text_section(ui);
                        ui.separator();
                        if ui.button("Alle löschen").clicked() {
                            app.delete_selected();
                        }
                    }

                    // Für beide Fälle — ein einzelnes Objekt zu exportieren
                    // ist genauso sinnvoll wie eine Gruppe.
                    app.export_section(ui, ctx);
                });

            // Undo-Snapshot: einmal pro Editier-Session.
            let any_focused = ctx.memory(|m| m.focused()).is_some();
            if any_focused && app.prop_snapshot_pending {
                app.push_history();
                app.prop_snapshot_pending = false;
            } else if !any_focused {
                app.prop_snapshot_pending = true;
            }
        };

        match panel_side {
            crate::model::PanelSide::Right => {
                egui::SidePanel::right("properties")
                    .resizable(true)
                    .default_width(240.0)
                    .width_range(180.0..=360.0)
                    .show(ctx, |ui| content(ui, self, ctx));
            }
            crate::model::PanelSide::Left => {
                egui::SidePanel::left("properties")
                    .resizable(true)
                    .default_width(240.0)
                    .width_range(180.0..=360.0)
                    .show(ctx, |ui| content(ui, self, ctx));
            }
            crate::model::PanelSide::Bottom => {
                egui::TopBottomPanel::bottom("properties")
                    .resizable(true)
                    .default_height(200.0)
                    .height_range(120.0..=500.0)
                    .show(ctx, |ui| content(ui, self, ctx));
            }
        }
    }

    fn properties_for(&mut self, ui: &mut egui::Ui, sel: u64) {
        let page_idx = self.page_index;
        let Some(el_idx) = self.doc.pages[page_idx]
            .elements
            .iter()
            .position(|e| e.id == sel)
        else {
            return;
        };

        // Custom-Font-Namen vor dem Borrow von `el` einsammeln (sonst
        // Borrow-Konflikt mit &mut self.doc...).
        let custom_names = self.fonts.names();

        // --- Position: Ursprung abhängig von align/valign ---
        let unit = self.settings.units;
        let suffix = unit.label();
        ui.heading("Objekt");
        ui.separator();

        {
            let el = &mut self.doc.pages[page_idx].elements[el_idx];

            // Ursprungs-Offset aus horizontaler/vertikaler Ausrichtung.
            let (ox, oy) = origin_offset(el);

            // Angezeigte X/Y = Element-Position + Ursprungs-Offset.
            let mut x_d = unit.from_pt(el.x + ox);
            let mut y_d = unit.from_pt(el.y + oy);
            let mut w_d = unit.from_pt(el.w);
            let mut h_d = unit.from_pt(el.h);

            ui.horizontal(|ui| {
                ui.label("X:");
                ui.add(egui::DragValue::new(&mut x_d).speed(0.1).suffix(suffix));
                ui.label("Y:");
                ui.add(egui::DragValue::new(&mut y_d).speed(0.1).suffix(suffix));
            });
            ui.horizontal(|ui| {
                ui.label("B:");
                ui.add(
                    egui::DragValue::new(&mut w_d)
                        .range(0.01..=2000.0)
                        .speed(0.1)
                        .suffix(suffix),
                );
                // Linien haben keine Höhe (h = 0) — Feld ausblenden.
                if el.kind != ElementKind::Line {
                    ui.label("H:");
                    ui.add(
                        egui::DragValue::new(&mut h_d)
                            .range(0.01..=2000.0)
                            .speed(0.1)
                            .suffix(suffix),
                    );
                }
            });

            // Zurückschreiben: Element-Position = eingegebener Wert − Offset.
            el.x = unit.to_pt(x_d) - ox;
            el.y = unit.to_pt(y_d) - oy;
            el.w = unit.to_pt(w_d);
            if el.kind != ElementKind::Line {
                el.h = unit.to_pt(h_d);
            }
        }

        // --- Element-spezifische Eigenschaften ---
        //
        // Pfad-Aktionen brauchen `&mut self` (Undo-Schnappschuss, Statuszeile),
        // `el` hält aber bereits eine Ausleihe darauf. Deshalb wird die Aktion
        // hier nur vermerkt und unten ausgeführt.
        let path_edit_id = self.path_edit.map(|e| e.id);
        let path_edit_node = self.path_edit.and_then(|e| e.node);
        let mut path_action: Option<PathAction> = None;
        let el = &mut self.doc.pages[page_idx].elements[el_idx];
        ui.separator();

        match el.kind {
            ElementKind::Text => {
                ui.label("Text:");
                ui.add(
                    egui::TextEdit::multiline(&mut el.text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(4),
                );
                ui.label("Schrift:");
                ui.horizontal_wrapped(|ui| {
                    let mut chosen: Option<String> = None;
                    for def in crate::model::FONT_CHOICES {
                        let selected = el.font == def.key;
                        let text = egui::RichText::new(def.display)
                            .family(crate::fonts::family_for(def.key));
                        if ui.selectable_label(selected, text).clicked() {
                            chosen = Some(def.key.to_string());
                        }
                    }
                    // Custom-Fonts (in der .boxdoc-Datei eingebettet).
                    for name in &custom_names {
                        let selected = el.font == name.as_str();
                        let text = egui::RichText::new(name)
                            .family(crate::fonts::family_for(name));
                        if ui.selectable_label(selected, text).clicked() {
                            chosen = Some(name.clone());
                        }
                    }
                    if let Some(k) = chosen {
                        el.font = k;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Schriftgröße:");
                    ui.add(
                        egui::DragValue::new(&mut el.font_size)
                            .range(4.0..=400.0)
                            .speed(0.5)
                            .suffix("pt"),
                    );
                });
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(el.bold, egui::RichText::new("B").strong())
                        .on_hover_text("Fett")
                        .clicked()
                    {
                        el.bold = !el.bold;
                    }
                    if ui
                        .selectable_label(el.italic, egui::RichText::new("I").italics())
                        .on_hover_text("Kursiv")
                        .clicked()
                    {
                        el.italic = !el.italic;
                    }
                    if ui
                        .selectable_label(el.underline, egui::RichText::new("U").underline())
                        .on_hover_text("Unterstrichen")
                        .clicked()
                    {
                        el.underline = !el.underline;
                    }
                    if ui
                        .selectable_label(
                            el.strikethrough,
                            egui::RichText::new("S").strikethrough(),
                        )
                        .on_hover_text("Durchgestrichen")
                        .clicked()
                    {
                        el.strikethrough = !el.strikethrough;
                    }
                    // Ohne echten Schnitt wird fett/kursiv nur nachgeahmt. Das
                    // sieht anders aus als ein gesetzter Schnitt — der Hinweis
                    // erspart die Suche nach dem vermeintlichen Fehler.
                    let style = crate::model::FontStyle::of(el);
                    if style != crate::model::FontStyle::Regular
                        && !crate::fonts::has_style(&el.font, style)
                    {
                        ui.label(egui::RichText::new("≈").weak()).on_hover_text(
                            "Diese Schrift bringt den Schnitt nicht mit — \
                             fett und kursiv werden nachgeahmt.",
                        );
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Einzug:");
                    ui.add(
                        egui::DragValue::new(&mut el.indent)
                            .range(0.0..=400.0)
                            .speed(0.5)
                            .suffix("pt"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Farbe:");
                    let mut c = Color32::from_rgba_unmultiplied(
                        el.color[0],
                        el.color[1],
                        el.color[2],
                        el.color[3],
                    );
                    ui.color_edit_button_srgba(&mut c);
                    el.color = c.to_srgba_unmultiplied();
                });
                ui.horizontal(|ui| {
                    ui.label("Horizontal:");
                    ui.selectable_value(&mut el.align, TextAlign::Left, "Links");
                    ui.selectable_value(&mut el.align, TextAlign::Center, "Mitte");
                    ui.selectable_value(&mut el.align, TextAlign::Right, "Rechts");
                });
                ui.horizontal(|ui| {
                    ui.label("Vertikal:");
                    ui.selectable_value(&mut el.valign, crate::model::VAlign::Top, "Oben");
                    ui.selectable_value(&mut el.valign, crate::model::VAlign::Middle, "Mitte");
                    ui.selectable_value(&mut el.valign, crate::model::VAlign::Bottom, "Unten");
                });
            }
            ElementKind::Image => {
                ui.label(format!("Bildgröße: {}×{}", el.image_w, el.image_h));
                ui.horizontal(|ui| {
                    ui.label("Drehung:");
                    ui.add(
                        egui::DragValue::new(&mut el.rotation)
                            .range(-360.0..=360.0)
                            .speed(0.5)
                            .suffix("°"),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.button("Crop-Modus").clicked() {
                        self.crop_mode = !self.crop_mode;
                    }
                    if ui.button("Zurücksetzen").clicked() {
                        el.crop = crate::model::Crop::default();
                        el.rotation = 0.0;
                    }
                });
                if ui.button("90° drehen").clicked() {
                    el.rotation += 90.0;
                }
            }
            ElementKind::Rectangle
            | ElementKind::Line
            | ElementKind::Ellipse
            | ElementKind::Path => {
                ui.heading(match el.kind {
                    ElementKind::Rectangle => "Rechteck",
                    ElementKind::Line => "Linie",
                    ElementKind::Ellipse => "Ellipse",
                    ElementKind::Path => "Pfad",
                    _ => "",
                });
                if el.kind == ElementKind::Path {
                    let curved = el.path_is_curved();
                    ui.label(format!(
                        "{} Knoten · {} · {}",
                        el.points.len(),
                        if el.path_closed {
                            "geschlossen"
                        } else {
                            "offen"
                        },
                        if curved { "Kurven" } else { "Strecken" }
                    ));
                    let editing = path_edit_id == Some(el.id);
                    if ui
                        .selectable_label(editing, "Knoten bearbeiten (N)")
                        .on_hover_text("Oder Doppelklick auf den Pfad.")
                        .clicked()
                    {
                        path_action = Some(PathAction::ToggleEdit);
                    }
                    if editing {
                        // Alles, was die Knotenbearbeitung kann, auch ohne
                        // Tastatur und ohne Alt-Klick erreichbar machen — sonst
                        // ist „Punkt löschen" eine Funktion, die nur kennt, wer
                        // die Doku gelesen hat.
                        ui.group(|ui| {
                            match path_edit_node {
                                Some(i) => {
                                    ui.label(format!("Knoten {} von {}", i + 1, el.points.len()));
                                    ui.horizontal(|ui| {
                                        let min = if el.path_closed { 3 } else { 2 };
                                        let removable = el.points.len() > min;
                                        if ui
                                            .add_enabled(
                                                removable,
                                                egui::Button::new("Knoten löschen"),
                                            )
                                            .on_hover_text(if removable {
                                                "Entfernt den ausgewählten Knoten (Taste: Entf)."
                                            } else {
                                                "Ein Pfad braucht mindestens diese Knoten."
                                            })
                                            .clicked()
                                        {
                                            path_action = Some(PathAction::RemoveNode(i));
                                        }
                                        if ui
                                            .button("Ecke / Kurve")
                                            .on_hover_text(
                                                "Schaltet den Knoten um (Alt+Klick auf den Knoten).",
                                            )
                                            .clicked()
                                        {
                                            path_action = Some(PathAction::ToggleNode(i));
                                        }
                                    });
                                }
                                None => {
                                    ui.label("Klicke einen Knoten an, um ihn zu löschen oder umzuschalten.");
                                }
                            }
                            ui.label(
                                egui::RichText::new(
                                    "Ziehen verschiebt · Doppelklick auf ein Segment fügt einen \
                                     Knoten ein · Alt+Griff macht eine Spitze",
                                )
                                .weak()
                                .small(),
                            );
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Glätten").on_hover_text(
                            "Legt durch alle Knoten eine weiche Kurve.").clicked()
                        {
                            path_action = Some(PathAction::Smooth);
                        }
                        if ui
                            .add_enabled(curved, egui::Button::new("Ecken"))
                            .on_hover_text("Macht aus allen Kurven wieder Strecken.")
                            .clicked()
                        {
                            path_action = Some(PathAction::Sharpen);
                        }
                    });
                    let mut closed = el.path_closed;
                    if ui.checkbox(&mut closed, "Geschlossen").changed() {
                        path_action = Some(PathAction::SetClosed(closed));
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Drehung:");
                    ui.add(
                        egui::DragValue::new(&mut el.rotation)
                            .range(-360.0..=360.0)
                            .speed(0.5)
                            .suffix("°"),
                    );
                });
                ui.horizontal(|ui| {
                    let is_line = el.kind == ElementKind::Line;
                    ui.label(if is_line {
                        "Linienfarbe:"
                    } else {
                        "Rahmenfarbe:"
                    });
                    let mut c = Color32::from_rgba_unmultiplied(
                        el.stroke_color[0],
                        el.stroke_color[1],
                        el.stroke_color[2],
                        el.stroke_color[3],
                    );
                    ui.color_edit_button_srgba(&mut c);
                    el.stroke_color = c.to_srgba_unmultiplied();
                });
                ui.horizontal(|ui| {
                    let is_line = el.kind == ElementKind::Line;
                    ui.label(if is_line {
                        "Linienstärke:"
                    } else {
                        "Rahmenstärke:"
                    });
                    ui.add(
                        egui::DragValue::new(&mut el.stroke_width)
                            .range(0.0..=50.0)
                            .speed(0.2)
                            .suffix("pt"),
                    );
                });
                if matches!(
                    el.kind,
                    ElementKind::Rectangle | ElementKind::Ellipse | ElementKind::Path
                ) {
                    ui.horizontal(|ui| {
                        ui.label("Füllfarbe:");
                        let mut c = Color32::from_rgba_unmultiplied(
                            el.fill_color[0],
                            el.fill_color[1],
                            el.fill_color[2],
                            el.fill_color[3],
                        );
                        ui.color_edit_button_srgba(&mut c);
                        el.fill_color = c.to_srgba_unmultiplied();
                    });
                }
                // Eckradius nur für Rechtecke (für Ellipse ohne Bedeutung).
                if el.kind == ElementKind::Rectangle {
                    ui.horizontal(|ui| {
                        ui.label("Eckradius:");
                        ui.add(
                            egui::DragValue::new(&mut el.corner_radius)
                                .range(0.0..=100.0)
                                .speed(0.2)
                                .suffix("pt"),
                        );
                    });
                }
                if ui.button("90° drehen").clicked() {
                    el.rotation += 90.0;
                }
            }
        }

        // Die oben vermerkte Pfad-Aktion ausführen, jetzt ohne Ausleihe.
        if let Some(action) = path_action {
            self.apply_path_action(page_idx, el_idx, action);
        }

        // --- Z-Order (Anordnung) ---
        // Höherer Index = weiter oben (zuletzt gezeichnet). Anzeige 1-basiert:
        // 1 = ganz hinten, `total` = ganz vorne.
        let total = self.doc.pages[page_idx].elements.len();
        let z_pos = el_idx + 1;
        let can_front = z_pos < total;
        let can_forward = z_pos < total;
        let can_backward = z_pos > 1;
        let can_back = z_pos > 1;
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Anordnung:");
            ui.label(format!("{}/{}", z_pos, total));
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(can_front, |ui| {
                if ui.button("⤒ Ganz nach vorne").clicked() {
                    self.bring_to_front(sel);
                }
            });
            ui.add_enabled_ui(can_forward, |ui| {
                if ui.button("↑ Nach vorne").clicked() {
                    self.bring_forward(sel);
                }
            });
            ui.add_enabled_ui(can_backward, |ui| {
                if ui.button("↓ Nach hinten").clicked() {
                    self.send_backward(sel);
                }
            });
            ui.add_enabled_ui(can_back, |ui| {
                if ui.button("⤓ Ganz nach hinten").clicked() {
                    self.send_to_back(sel);
                }
            });
        });

        ui.separator();
        if ui.button("Objekt löschen").clicked() {
            self.delete_selected();
        }
    }

    /// Positions-Editor für Mehrfachauswahl mit Anker-Raster.
    fn position_section(&mut self, ui: &mut egui::Ui) {
        let unit = self.settings.units;
        let suffix = unit.label();

        ui.heading(format!("{} Objekte", self.selection.len()));
        ui.separator();

        let Some((bx, by, bw, bh)) = self.selection_bbox() else {
            ui.label("Keine gültige Auswahl.");
            return;
        };

        // --- Ankerpunkt-Raster (3×3) ---
        // ui.horizontal + ui.vertical => jedes Array ist eine visuelle Spalte.
        ui.label("Referenzpunkt:");
        let anchor_clicked = ui
            .horizontal(|ui| {
                let columns = [
                    [
                        BBoxAnchor::TopLeft,
                        BBoxAnchor::MidLeft,
                        BBoxAnchor::BotLeft,
                    ],
                    [
                        BBoxAnchor::TopCenter,
                        BBoxAnchor::Center,
                        BBoxAnchor::BotCenter,
                    ],
                    [
                        BBoxAnchor::TopRight,
                        BBoxAnchor::MidRight,
                        BBoxAnchor::BotRight,
                    ],
                ];
                let mut changed = false;
                for col in &columns {
                    ui.vertical(|ui| {
                        for anchor in col {
                            let sel = self.multi_anchor == *anchor;
                            if ui.selectable_label(sel, "•").clicked() {
                                self.multi_anchor = *anchor;
                                changed = true;
                            }
                        }
                    });
                }
                changed
            })
            .inner;

        // Ankerposition berechnen.
        let (fx, fy) = self.multi_anchor.frac();
        let anchor_x = bx + bw * fx;
        let anchor_y = by + bh * fy;

        // Bei Ankerwechsel oder Auswahlwechsel: Puffer synchronisieren.
        if anchor_clicked || self.pos_last_sel != self.selection {
            self.pos_x = anchor_x;
            self.pos_y = anchor_y;
            self.pos_last_sel = self.selection.clone();
        }

        // DragValues an den Puffer binden (nicht an die Live-Berechnung).
        let mut dx = unit.from_pt(self.pos_x);
        let mut dy = unit.from_pt(self.pos_y);

        let before_x = dx;
        let before_y = dy;

        ui.horizontal(|ui| {
            ui.label("X:");
            ui.add(egui::DragValue::new(&mut dx).speed(0.1).suffix(suffix));
            ui.label("Y:");
            ui.add(egui::DragValue::new(&mut dy).speed(0.1).suffix(suffix));
        });

        // --- B/H: einzelne Element-Größen, "—" bei gemischten Werten ---
        let sel_ids = self.selection.clone();
        let page_ref = self.doc.pages.get(self.page_index);
        let sel_els: Vec<&Element> = page_ref
            .map(|p| {
                p.elements
                    .iter()
                    .filter(|e| sel_ids.contains(&e.id))
                    .collect()
            })
            .unwrap_or_default();

        let widths: Vec<f32> = sel_els.iter().map(|e| e.w).collect();
        let heights: Vec<f32> = sel_els.iter().map(|e| e.h).collect();
        let w_uniform = widths.iter().all(|&w| (w - widths[0]).abs() < 0.01);
        let h_uniform = heights.iter().all(|&h| (h - heights[0]).abs() < 0.01);

        let mut dw = if w_uniform {
            unit.from_pt(widths[0])
        } else {
            0.0
        };
        let mut dh = if h_uniform {
            unit.from_pt(heights[0])
        } else {
            0.0
        };

        let rw = ui
            .horizontal(|ui| {
                ui.label("B:");
                let mut dv = egui::DragValue::new(&mut dw)
                    .range(0.0..=2000.0)
                    .speed(0.1)
                    .suffix(suffix);
                if !w_uniform {
                    // Wert auf 0.0 lassen und nur als "—" anzeigen.
                    // changed() wird durch den custom_formatter nicht ausgelöst.
                    dv = dv.custom_formatter(|_, _| String::from("—"));
                }
                ui.add(dv)
            })
            .inner;
        let rh = ui
            .horizontal(|ui| {
                ui.label("H:");
                let mut dv = egui::DragValue::new(&mut dh)
                    .range(0.0..=2000.0)
                    .speed(0.1)
                    .suffix(suffix);
                if !h_uniform {
                    dv = dv.custom_formatter(|_, _| String::from("—"));
                }
                ui.add(dv)
            })
            .inner;

        // Nur anwenden, wenn der Wert vom Nutzer aktiv geändert wurde
        // und nicht der "—" Indikator ist.
        if rw.changed() && w_uniform {
            let new_w = unit.to_pt(dw);
            let ids = self.selection.clone();
            if let Some(page) = self.doc.pages.get_mut(self.page_index) {
                for el in page.elements.iter_mut() {
                    if ids.contains(&el.id) {
                        el.w = new_w;
                    }
                }
            }
            self.touch();
        }
        if rh.changed() && h_uniform {
            let new_h = unit.to_pt(dh);
            let ids = self.selection.clone();
            if let Some(page) = self.doc.pages.get_mut(self.page_index) {
                for el in page.elements.iter_mut() {
                    if ids.contains(&el.id) {
                        el.h = new_h;
                    }
                }
            }
            self.touch();
        }

        // X/Y-Änderung: Delta nur aus NUTZER-Änderung berechnen.
        if (dx - before_x).abs() > 1e-6 || (dy - before_y).abs() > 1e-6 {
            let new_x_pt = unit.to_pt(dx);
            let new_y_pt = unit.to_pt(dy);
            let delta_x = new_x_pt - self.pos_x;
            let delta_y = new_y_pt - self.pos_y;
            let sel_ids = self.selection.clone();
            if let Some(page) = self.doc.pages.get_mut(self.page_index) {
                for el in page.elements.iter_mut() {
                    if sel_ids.contains(&el.id) {
                        el.x += delta_x;
                        el.y += delta_y;
                    }
                }
            }
            self.pos_x = new_x_pt;
            self.pos_y = new_y_pt;
            self.touch();
        } else {
            // Keine Nutzereingabe → Puffer an Live-Position anpassen.
            self.pos_x = anchor_x;
            self.pos_y = anchor_y;
        }
    }

    /// Export-Knopf für die aktuelle Auswahl.
    ///
    /// Doppelt zum Datei-Menü, und das mit Absicht: Eine Auswahl zu
    /// exportieren ist eine Aktion **auf der Auswahl**. Sie gehört dorthin, wo
    /// man die Auswahl gerade in der Hand hat, nicht zwei Menüebenen entfernt
    /// zwischen Öffnen und Drucken.
    ///
    /// Wird nur aufgerufen, wenn etwas ausgewählt ist — deshalb kein
    /// ausgegrauter Zustand.
    ///
    /// Beide Knöpfe gibt es auf beiden Plattformen; nur das Ziel unterscheidet
    /// sich (Datei-Dialog vs. Download), und das steckt in `io.rs`.
    fn export_section(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        let img_n = self.selected_images().len();
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            {
                let n = self.selection.len();
                let label = if n == 1 {
                    String::from("Auswahl als SVG…")
                } else {
                    format!("Auswahl als SVG… ({n})")
                };
                if ui
                    .button(label)
                    .on_hover_text(
                        "Exportiert nur die ausgewählten Objekte als Vektorgrafik — \
                         auf ihre Hüllbox beschnitten, mit durchsichtigem Hintergrund.",
                    )
                    .clicked()
                {
                    crate::io::export_svg_dialog(self, ctx, true);
                }
            }
            // Nur zeigen, wenn ein Bild dabei ist: Bei einem Textfeld gäbe es
            // nichts zu speichern, und ein dauerhaft ausgegrauter Knopf im
            // Eigenschaften-Panel ist nur Lärm.
            if img_n > 0 {
                let label = if img_n == 1 {
                    String::from("Bild speichern…")
                } else {
                    format!("Bilder speichern… ({img_n})")
                };
                let hint = if cfg!(target_arch = "wasm32") {
                    "Lädt die Bilddatei selbst herunter (PNG) — in Originalauflösung \
                     und auf den Crop beschnitten, aber ohne Drehung."
                } else {
                    "Speichert die Bilddatei selbst (PNG oder JPEG) — in \
                     Originalauflösung und auf den Crop beschnitten, aber ohne \
                     Drehung. Strg+C legt ein einzelnes Bild zusätzlich in die \
                     Zwischenablage."
                };
                if ui.button(label).on_hover_text(hint).clicked() {
                    crate::io::export_image_dialog(self);
                }
            }
        });
    }

    /// Ausrichtungs-Buttons für Mehrfachauswahl.
    fn align_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ausrichten");
        ui.label("Kanten / Mitten:");

        // Horizontal-Buttons (Links / X-Mitte / Rechts)
        ui.horizontal(|ui| {
            if ui
                .button("Links")
                .on_hover_text("Alle an der linken Kante ausrichten")
                .clicked()
            {
                self.align_objects(AlignOp::Left);
            }
            if ui
                .button("X Mitte")
                .on_hover_text("Alle horizontal mittig ausrichten")
                .clicked()
            {
                self.align_objects(AlignOp::CenterX);
            }
            if ui
                .button("Rechts")
                .on_hover_text("Alle an der rechten Kante ausrichten")
                .clicked()
            {
                self.align_objects(AlignOp::Right);
            }
        });
        ui.horizontal(|ui| {
            if ui
                .button("Oben")
                .on_hover_text("Alle an der oberen Kante ausrichten")
                .clicked()
            {
                self.align_objects(AlignOp::Top);
            }
            if ui
                .button("Y Mitte")
                .on_hover_text("Alle vertikal mittig ausrichten")
                .clicked()
            {
                self.align_objects(AlignOp::CenterY);
            }
            if ui
                .button("Unten")
                .on_hover_text("Alle an der unteren Kante ausrichten")
                .clicked()
            {
                self.align_objects(AlignOp::Bottom);
            }
        });

        ui.separator();
        ui.label("Abstände verteilen:");
        ui.horizontal(|ui| {
            if ui
                .button("Horizontal")
                .on_hover_text("Gleiche horizontale Abstände zwischen allen Objekten")
                .clicked()
            {
                self.distribute_objects(DistributeOp::Horizontal);
            }
            if ui
                .button("Vertikal")
                .on_hover_text("Gleiche vertikale Abstände zwischen allen Objekten")
                .clicked()
            {
                self.distribute_objects(DistributeOp::Vertical);
            }
        });
    }

    /// Richtet alle ausgewählten Objekte auf einer Achse aus.
    fn align_objects(&mut self, op: AlignOp) {
        self.push_history();
        let sel_ids = self.selection.clone();
        let page_idx = self.page_index;

        // Referenzwert aus dem ersten ausgewählten Element berechnen.
        let Some(page) = self.doc.pages.get(page_idx) else {
            return;
        };
        let sel_els: Vec<&Element> = page
            .elements
            .iter()
            .filter(|e| sel_ids.contains(&e.id))
            .collect();
        if sel_els.len() < 2 {
            return;
        }

        let ref_val = match op {
            AlignOp::Left => sel_els.iter().map(|e| e.x).fold(f32::INFINITY, f32::min),
            AlignOp::Right => sel_els
                .iter()
                .map(|e| e.x + e.w)
                .fold(f32::NEG_INFINITY, f32::max),
            AlignOp::CenterX => {
                let (min, max) = sel_els
                    .iter()
                    .map(|e| e.x)
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, mx), x| {
                        (mn.min(x), mx.max(x))
                    });
                (min + max) / 2.0
            }
            AlignOp::Top => sel_els.iter().map(|e| e.y).fold(f32::INFINITY, f32::min),
            AlignOp::Bottom => sel_els
                .iter()
                .map(|e| e.y + e.h)
                .fold(f32::NEG_INFINITY, f32::max),
            AlignOp::CenterY => {
                let (min, max) = sel_els
                    .iter()
                    .map(|e| e.y)
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, my), y| {
                        (mn.min(y), my.max(y))
                    });
                (min + max) / 2.0
            }
        };

        if let Some(page) = self.doc.pages.get_mut(page_idx) {
            for el in page.elements.iter_mut() {
                if !sel_ids.contains(&el.id) {
                    continue;
                }
                match op {
                    AlignOp::Left => el.x = ref_val,
                    AlignOp::Right => el.x = ref_val - el.w,
                    AlignOp::CenterX => el.x = ref_val - el.w / 2.0,
                    AlignOp::Top => el.y = ref_val,
                    AlignOp::Bottom => el.y = ref_val - el.h,
                    AlignOp::CenterY => el.y = ref_val - el.h / 2.0,
                }
            }
        }
        self.touch();
    }

    /// Verteilt alle ausgewählten Objekte mit gleichmäßigen Abständen.
    /// Horizontal: sortiert nach X, verteilt die Zwischenräume gleichmäßig.
    /// Vertikal: sortiert nach Y, entsprechend.
    fn distribute_objects(&mut self, op: DistributeOp) {
        self.push_history();
        let sel_ids = self.selection.clone();
        let page_idx = self.page_index;

        let Some(page) = self.doc.pages.get(page_idx) else {
            return;
        };
        // (id, start, size) für jede Achse.
        let mut items: Vec<(u64, f32, f32)> = page
            .elements
            .iter()
            .filter(|e| sel_ids.contains(&e.id))
            .map(|e| match op {
                DistributeOp::Horizontal => (e.id, e.x, e.w),
                DistributeOp::Vertical => (e.id, e.y, e.h),
            })
            .collect();
        if items.len() < 3 {
            return;
        }

        // Nach Startposition sortieren.
        items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Gesamt-Strecke vom Start des ersten bis zum Ende des letzten Objekts.
        let first_start = items.first().unwrap().1;
        let last_end = items.last().unwrap().1 + items.last().unwrap().2;
        let total_span = last_end - first_start;

        // Summe der Objekt-Breiten/-Höhen (ohne Abstände).
        let total_size: f32 = items.iter().map(|(_, _, s)| *s).sum();
        let count_gaps = items.len() - 1;
        let gap = if total_span > total_size {
            (total_span - total_size) / count_gaps as f32
        } else {
            0.0
        };

        // Neue Positionen: erstes bleibt, dann jeweils + size + gap.
        let mut cursor = first_start;
        let updates: Vec<(u64, f32)> = items
            .iter()
            .map(|(id, _, size)| {
                let new_pos = cursor;
                cursor += size + gap;
                (*id, new_pos)
            })
            .collect();

        if let Some(page) = self.doc.pages.get_mut(page_idx) {
            for el in page.elements.iter_mut() {
                if let Some((_, new_pos)) = updates.iter().find(|(id, _)| *id == el.id) {
                    match op {
                        DistributeOp::Horizontal => el.x = *new_pos,
                        DistributeOp::Vertical => el.y = *new_pos,
                    }
                }
            }
        }
        self.touch();
    }

    /// Text-Feld für Mehrfachauswahl.
    /// Zeigt den gemeinsamen Text an, oder leer bei unterschiedlichen Inhalten.
    fn multi_text_section(&mut self, ui: &mut egui::Ui) {
        let sel_ids = self.selection.clone();

        // Alle benötigten Daten vorab sammeln, um Borrow-Konflikte zu vermeiden.
        struct TextData {
            text: String,
            font: String,
            font_size: f32,
        }
        let data: Vec<TextData> = self
            .doc
            .pages
            .get(self.page_index)
            .map(|p| {
                p.elements
                    .iter()
                    .filter(|e| sel_ids.contains(&e.id) && e.kind == ElementKind::Text)
                    .map(|e| TextData {
                        text: e.text.clone(),
                        font: e.font.clone(),
                        font_size: e.font_size,
                    })
                    .collect()
            })
            .unwrap_or_default();

        if data.is_empty() {
            return;
        }

        ui.heading("Text");
        ui.label(format!("{} Text-Objekte", data.len()));

        // --- Text-Inhalt ---
        let text_uniform = data.iter().all(|d| d.text == data[0].text);
        let mut buf = if text_uniform {
            data[0].text.clone()
        } else {
            String::new()
        };
        let response = if text_uniform {
            ui.add(
                egui::TextEdit::multiline(&mut buf)
                    .desired_width(f32::INFINITY)
                    .desired_rows(4),
            )
        } else {
            ui.add(
                egui::TextEdit::multiline(&mut buf)
                    .hint_text("Unterschiedliche Texte — Eingabe überschreibt alle")
                    .desired_width(f32::INFINITY)
                    .desired_rows(4),
            )
        };
        if response.changed() {
            self.push_history();
            let ids = self.selection.clone();
            if let Some(page) = self.doc.pages.get_mut(self.page_index) {
                for el in page.elements.iter_mut() {
                    if ids.contains(&el.id) && el.kind == ElementKind::Text {
                        el.text = buf.clone();
                    }
                }
            }
            self.touch();
        }

        // --- Schriftart ---
        let font_uniform = data.iter().all(|d| d.font == data[0].font);
        let custom_names = self.fonts.names();
        ui.horizontal(|ui| {
            ui.label("Schrift:");
            if font_uniform {
                let mut chosen: Option<String> = None;
                for def in crate::model::FONT_CHOICES {
                    let selected = data[0].font == def.key;
                    let text =
                        egui::RichText::new(def.display).family(crate::fonts::family_for(def.key));
                    if ui.selectable_label(selected, text).clicked() {
                        chosen = Some(def.key.to_string());
                    }
                }
                // Custom-Fonts (in der .boxdoc-Datei eingebettet).
                for name in &custom_names {
                    let selected = data[0].font == name.as_str();
                    let text =
                        egui::RichText::new(name).family(crate::fonts::family_for(name));
                    if ui.selectable_label(selected, text).clicked() {
                        chosen = Some(name.clone());
                    }
                }
                if let Some(k) = chosen {
                    self.push_history();
                    let ids = self.selection.clone();
                    if let Some(page) = self.doc.pages.get_mut(self.page_index) {
                        for el in page.elements.iter_mut() {
                            if ids.contains(&el.id) && el.kind == ElementKind::Text {
                                el.font = k.clone();
                            }
                        }
                    }
                    self.touch();
                }
            } else {
                ui.label(egui::RichText::new("Unterschiedliche Schriften").weak());
            }
        });

        // --- Schriftgröße ---
        let size_uniform = data
            .iter()
            .all(|d| (d.font_size - data[0].font_size).abs() < 0.01);
        // Bei gemischten Größen: Startwert innerhalb des Range wählen, damit der
        // DragValue ihn nicht clamp't und dadurch fälschlich `changed()` auslöst.
        let initial = if size_uniform {
            data[0].font_size
        } else {
            14.0
        };
        let mut ds = initial;
        let rs = ui
            .horizontal(|ui| {
                ui.label("Schriftgröße:");
                let mut dv = egui::DragValue::new(&mut ds)
                    .range(4.0..=400.0)
                    .speed(0.5)
                    .suffix("pt");
                if !size_uniform {
                    dv = dv.custom_formatter(|_, _| String::from("—"));
                }
                ui.add(dv)
            })
            .inner;
        // Nur anwenden, wenn sich der Wert durch den Nutzer wirklich geändert hat.
        if rs.changed() && (ds - initial).abs() > 0.01 {
            self.push_history();
            let ids = self.selection.clone();
            if let Some(page) = self.doc.pages.get_mut(self.page_index) {
                for el in page.elements.iter_mut() {
                    if ids.contains(&el.id) && el.kind == ElementKind::Text {
                        el.font_size = ds;
                    }
                }
            }
            self.touch();
        }
    }

    // =======================================================================
    // Copy / Paste
    // =======================================================================

    /// Die ausgewählten Bild-Elemente der aktuellen Seite, in Seitenreihenfolge.
    ///
    /// Kopien statt Referenzen, damit die Aufrufer (Export, Zwischenablage)
    /// nebenher wieder `&mut self` benutzen dürfen.
    pub fn selected_images(&self) -> Vec<Element> {
        let Some(page) = self.doc.pages.get(self.page_index) else {
            return Vec::new();
        };
        page.elements
            .iter()
            .filter(|e| e.kind == ElementKind::Image && self.selection.contains(&e.id))
            .cloned()
            .collect()
    }

    /// Kopiert alle ausgewählten Elemente in die Zwischenablage.
    ///
    /// Ist **genau ein Bild** ausgewählt, landet es zusätzlich als Pixelbild in
    /// der System-Zwischenablage — sonst könnte man ein Bild aus BoxDoc zwar
    /// kopieren, aber in Paint oder Word nicht einfügen. Bei mehreren Bildern
    /// bleibt es bei den BoxDoc-Objekten: Welches der Bilder gemeint wäre, ist
    /// nicht zu erraten.
    pub fn copy_selection(&mut self) {
        let sel_ids = self.selection.clone();
        let Some(page) = self.doc.pages.get(self.page_index) else {
            return;
        };
        self.clipboard.clear();
        self.clip_origins.clear();
        for el in page.elements.iter().filter(|e| sel_ids.contains(&e.id)) {
            self.clipboard.push(el.clone());
            self.clip_origins.push((el.x, el.y));
        }
        if self.clipboard.is_empty() {
            return;
        }
        let mut msg = format!("{} Objekt(e) kopiert.", self.clipboard.len());
        let images = self.selected_images();
        if images.len() == 1 {
            match self.copy_image_to_system_clipboard(&images[0]) {
                Some(true) => msg.push_str(" Bild auch in der Zwischenablage."),
                Some(false) => msg.push_str(" Bild ging nicht in die Zwischenablage."),
                None => {}
            }
        }
        self.status = msg;
    }

    /// Legt das Bild eines Elements in die System-Zwischenablage.
    ///
    /// `None` = kein verwertbares Bild, `Some(false)` = Versuch gescheitert
    /// (oder Plattform ohne Unterstützung).
    fn copy_image_to_system_clipboard(&self, el: &Element) -> Option<bool> {
        let rgba = self.images.cropped_rgba(el)?;
        let png = crate::store::encode_png(&rgba)?;
        // Die Weiß-Variante nur erzeugen, wenn es wirklich Transparenz gibt —
        // bei einem Foto wäre sie Byte für Byte dasselbe Bild.
        let flat = if crate::store::has_alpha(&rgba) {
            crate::store::encode_png(&crate::store::flatten_on_white(&rgba))?
        } else {
            png.clone()
        };
        Some(crate::io::set_clipboard_image(&png, &flat))
    }

    /// Startet den Paste-Modus: Preview folgt dem Cursor bis zum Klick.
    pub fn start_paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.pasting = true;
        self.clear_selection();
        self.status = String::from("Klicke zum Platzieren · Esc bricht ab.");
    }

    /// Bestätigt das Einfügen: erstellt echte Elemente mit neuen IDs.
    /// `snapped` → Elemente landen exakt an den Originalpositionen.
    pub fn confirm_paste(&mut self, cursor_page: (f32, f32), snapped: bool) {
        if self.clipboard.is_empty() {
            self.pasting = false;
            return;
        }
        self.push_history();
        let ref_origin = self.clip_origins[0];
        let paste_ref = if snapped { ref_origin } else { cursor_page };

        // Daten vorab klonen, um Borrow-Konflikte zu vermeiden.
        let items: Vec<(Element, (f32, f32))> = self
            .clipboard
            .iter()
            .zip(self.clip_origins.iter())
            .map(|(el, origin)| (el.clone(), *origin))
            .collect();

        let mut new_ids = Vec::new();
        for (mut new_el, origin) in items {
            let old_id = new_el.id;
            let new_id = self.next_id();

            // Bei Bildern: Bilddaten kopieren.
            if new_el.kind == ElementKind::Image {
                let img_data = self.images.map.get(&old_id).map(|e| (e.png.clone(), e.dim));
                if let Some((png, dim)) = img_data {
                    self.images.insert(new_id, png, dim);
                }
            }

            new_el.id = new_id;
            new_el.x = paste_ref.0 + (origin.0 - ref_origin.0);
            new_el.y = paste_ref.1 + (origin.1 - ref_origin.1);

            if let Some(page) = self.doc.current_page_mut(self.page_index) {
                page.elements.push(new_el);
            }
            new_ids.push(new_id);
        }

        self.selection = new_ids;
        self.pasting = false;
        self.modified = true;
        self.status = String::from("Eingefügt.");
    }

    /// Achsenausgerichtete Bounding-Box aller ausgewählten Elemente (in pt).
    /// Berücksichtigt Rotation.
    fn selection_bbox(&self) -> Option<(f32, f32, f32, f32)> {
        let page = self.doc.pages.get(self.page_index)?;
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for el in page.elements.iter().filter(|e| self.is_selected(e.id)) {
            let cx = el.x + el.w / 2.0;
            let cy = el.y + el.h / 2.0;
            for corner in local_corners(el.w, el.h) {
                let w = local_to_world(egui::Pos2::new(cx, cy), el.rotation, corner);
                min_x = min_x.min(w.x);
                min_y = min_y.min(w.y);
                max_x = max_x.max(w.x);
                max_y = max_y.max(w.y);
            }
        }
        if min_x > max_x {
            return None;
        }
        Some((min_x, min_y, max_x - min_x, max_y - min_y))
    }

    fn show_status(&self, ctx: &Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let dot = if self.modified {
                    Color32::from_rgb(220, 160, 60)
                } else {
                    Color32::from_rgb(110, 200, 120)
                };
                ui.painter().circle_filled(
                    ui.min_rect().left_center() + Vec2::new(12.0, 0.0),
                    4.0,
                    dot,
                );
                ui.label(&self.status);

                // Web: Wohin gespeichert wird und was der Sync gerade tut.
                // Ohne diese Anzeige war beides unsichtbar — `web_status`
                // wurde gepflegt, aber nie gezeichnet.
                #[cfg(target_arch = "wasm32")]
                {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.web_status.is_empty() {
                            ui.label(egui::RichText::new(&self.web_status).weak());
                            ui.separator();
                        }
                        match &self.web_doc {
                            Some(w) => {
                                let label = if w.token.is_empty() {
                                    format!("Server: {}", w.slug)
                                } else {
                                    format!("Server: {} \u{1f512}", w.slug)
                                };
                                ui.label(egui::RichText::new(label).weak())
                                    .on_hover_text(w.share_url());
                            }
                            None => {
                                ui.label(
                                    egui::RichText::new("Nicht am Server gespeichert").weak(),
                                )
                                .on_hover_text(
                                    "Strg+S legt das Dokument auf dem Server an; \
                                     ab dann wird jede Änderung automatisch \
                                     gespeichert.",
                                );
                            }
                        }
                    });
                }
            });
        });
    }

    /// Rohtext-Fenster über dem aktuellen Dokument. Das `doc`-Objekt wird als
    /// pretty JSON angezeigt und kann direkt bearbeitet werden; gültige
    /// Änderungen werden live (als Undo-Schritt) angewendet.
    fn show_json_editor(&mut self, ctx: &Context) {
        if !self.show_json {
            return;
        }
        let content = |ui: &mut egui::Ui, app: &mut EditorApp| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    ui.heading("JSON");
                    ui.separator();
                    // Puffer an den aktuellen Dokumentstand anpassen, solange der
                    // Nutzer nicht gerade tippt (sichtbar für externe/KI-Änderungen
                    // und GUI-Aktionen).
                    if !app.json_focused {
                        app.json_buf = serde_json::to_string_pretty(&app.doc).unwrap_or_default();
                    }
                    let resp = ui.add(
                        egui::TextEdit::multiline(&mut app.json_buf)
                            .font(egui::TextStyle::Monospace)
                            .code_editor()
                            .desired_width(f32::INFINITY),
                    );
                    // Beim Einstieg einmalig den Undo-Snapshot sichern.
                    if resp.gained_focus() {
                        app.push_history();
                    }
                    app.json_focused = resp.has_focus();
                    // Live anwenden, solange der Nutzer tippt und der Puffer als
                    // gültiges Dokument parst.
                    if app.json_focused {
                        if let Ok(new_doc) = serde_json::from_str::<Document>(&app.json_buf) {
                            let cur = serde_json::to_string(&app.doc).unwrap_or_default();
                            let new = serde_json::to_string(&new_doc).unwrap_or_default();
                            if new != cur {
                                let known: std::collections::HashSet<u64> = new_doc
                                    .pages
                                    .iter()
                                    .flat_map(|p| p.elements.iter().map(|e| e.id))
                                    .collect();
                                let max_id = new_doc
                                    .pages
                                    .iter()
                                    .flat_map(|p| p.elements.iter().map(|e| e.id))
                                    .max()
                                    .unwrap_or(0);
                                app.doc = new_doc;
                                app.page_index =
                                    app.page_index.min(app.doc.pages.len().saturating_sub(1));
                                app.selection.retain(|id| known.contains(id));
                                app.next_id = app.next_id.max(max_id + 1);
                                app.editing = None;
                                app.interaction = Interaction::None;
                                app.touch();
                            }
                        }
                    }
                });
        };

        // Das JSON-Panel liegt gegenüber dem Eigenschaften-Panel.
        match self.settings.panel_side {
            crate::model::PanelSide::Right | crate::model::PanelSide::Bottom => {
                egui::SidePanel::left("json_editor")
                    .resizable(true)
                    .default_width(240.0)
                    .width_range(180.0..=560.0)
                    .show(ctx, |ui| content(ui, self));
            }
            crate::model::PanelSide::Left => {
                egui::SidePanel::right("json_editor")
                    .resizable(true)
                    .default_width(240.0)
                    .width_range(180.0..=560.0)
                    .show(ctx, |ui| content(ui, self));
            }
        }
    }

    /// Datei-Browser: listet Webserver-Dokumente (Web) bzw. lokale
    /// .boxdoc-Dateien (Native) auf. Klick öffnet das Dokument.
    fn show_files_panel(&mut self, ctx: &Context) {
        if !self.show_files {
            return;
        }

        // --- Refresh auslösen (alle 5 s) ---
        let now = ctx.input(|i| i.time);
        let stale = now - self.files_last_refresh > 5.0;
        if stale {
            self.files_last_refresh = now;
            #[cfg(target_arch = "wasm32")]
            {
                let base = self
                    .web_doc
                    .as_ref()
                    .map(|w| w.base.clone())
                    .unwrap_or_else(crate::web_sync::detect_base);
                crate::web_sync::spawn_file_list(&base);
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.refresh_local_files();
            }
        }

        // --- Ergebnis abholen (Web) ---
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(json) = crate::web_sync::take_file_list() {
                if let Ok(resp) = serde_json::from_str::<ListResponse>(&json) {
                    self.files_entries = resp
                        .documents
                        .into_iter()
                        .map(|d| FileEntry {
                            name: d.slug,
                            modified: d.modified,
                            size: d.size,
                            protected: d.protected,
                        })
                        .collect();
                }
            }
        }

        // --- Panel zeichnen (immer links, kleines schmales Panel) ---
        let current_slug = self.current_doc_name();

        egui::SidePanel::left("files_panel")
            .resizable(true)
            .default_width(200.0)
            .width_range(150.0..=400.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.heading("Dokumente");
                        ui.separator();

                        if self.files_entries.is_empty() {
                            ui.label(egui::RichText::new("Keine Dokumente gefunden.").weak());
                        }

                        for entry in self.files_entries.clone() {
                            let is_current = entry.name == current_slug;
                            let name_text = if entry.protected {
                                format!("{} \u{1f512}", entry.name)
                            } else {
                                entry.name.clone()
                            };
                            let rich = if is_current {
                                egui::RichText::new(&name_text).strong()
                            } else {
                                egui::RichText::new(&name_text)
                            };
                            let btn = egui::Button::new(rich)
                                .wrap_mode(egui::TextWrapMode::Truncate)
                                .min_size(egui::vec2(ui.available_width(), 0.0))
                                .fill(if is_current {
                                    ui.style().visuals.selection.bg_fill
                                } else {
                                    egui::Color32::TRANSPARENT
                                });
                            if ui.add(btn).clicked() {
                                self.open_file_entry(&entry.name);
                            }
                            ui.label(
                                egui::RichText::new(format!("{}", human_size(entry.size)))
                                    .small()
                                    .weak(),
                            );
                        }
                    });
            });
    }

    /// Name/Schlüssel des aktuell geöffneten Dokuments.
    fn current_doc_name(&self) -> String {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(w) = &self.web_doc {
                return w.slug.clone();
            }
        }
        self.file_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    }

    /// Öffnet einen Eintrag aus dem Datei-Browser.
    fn open_file_entry(&mut self, name: &str) {
        #[cfg(target_arch = "wasm32")]
        {
            crate::web_sync::navigate_to(name);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = self
                .file_path
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .or_else(|| std::env::current_dir().ok());
            if let Some(dir) = dir {
                let path = dir.join(format!("{}.boxdoc", name));
                // Über den Guard, damit ungespeicherte Arbeit nicht verloren geht.
                self.request_action(PendingAction::OpenFile(path));
            }
        }
    }

    /// Listet lokale .boxdoc-Dateien im Verzeichnis der aktuellen Datei auf.
    #[cfg(not(target_arch = "wasm32"))]
    fn refresh_local_files(&mut self) {
        let dir = self
            .file_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .or_else(|| std::env::current_dir().ok());
        let Some(dir) = dir else {
            return;
        };
        let mut entries = Vec::new();
        if let Ok(read) = std::fs::read_dir(&dir) {
            for entry in read.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("boxdoc") {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    let meta = entry.metadata();
                    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                    let modified = meta
                        .as_ref()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    entries.push(FileEntry {
                        name,
                        modified,
                        size,
                        protected: false,
                    });
                }
            }
        }
        entries.sort_by(|a, b| {
            b.modified
                .partial_cmp(&a.modified)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.files_entries = entries;
    }
}

/// Formatiert eine Byte-Größe menschenlesbar (KB / MB).
/// Menüeintrag mit rechtsbündig ausgerichtetem Tastenkürzel.
///
/// Ein Kürzel, das niemand sieht, existiert für den Nutzer nicht — deshalb
/// steht es direkt neben dem Befehl.
/// Macht aus der englischen API-Meldung einen Satz, der im Dialog weiterhilft.
///
/// Der Server spricht Englisch, weil auch KI-Agenten und curl-Aufrufe an
/// derselben API hängen. Im Dialog steht die Meldung aber direkt unter dem
/// Feld, das der Nutzer ändern soll — dort muss sie sagen, was zu tun ist.
#[cfg(target_arch = "wasm32")]
fn translate_create_error(msg: &str) -> String {
    if msg.contains("already exists") {
        "Diesen Namen gibt es schon — bitte einen anderen wählen.".to_string()
    } else if msg.contains("Invalid name") {
        "Ungültiger Name: 4 bis 32 Zeichen, nur a–z und 0–9.".to_string()
    } else if msg.contains("Rate limit") {
        "Zu viele neue Dokumente in kurzer Zeit. Bitte später erneut versuchen."
            .to_string()
    } else {
        format!("Anlegen fehlgeschlagen: {msg}")
    }
}

fn menu_entry(ui: &mut egui::Ui, label: &str, shortcut: &str) -> egui::Response {
    menu_entry_enabled(ui, true, label, shortcut)
}

/// Wie `menu_entry`, nur ausgraubar — für Einträge, die es zwar gibt, die aber
/// gerade nicht gehen (z. B. Drucken im Browser). Ausgegraut statt versteckt:
/// Wer den Eintrag sucht, soll sehen, dass es ihn gibt, und im Tooltip lesen,
/// woran es hakt.
fn menu_entry_enabled(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    shortcut: &str,
) -> egui::Response {
    ui.horizontal(|ui| {
        let resp = ui.add_enabled(enabled, egui::Button::new(label));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(egui::RichText::new(shortcut).weak().small());
        });
        resp
    })
    .inner
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Berechnet den Ursprungs-Offset (ox, oy) eines Elements aus seiner
/// horizontalen und vertikalen Ausrichtung. Bei Bildern ist der Ursprung
/// immer oben-links (0, 0).
fn origin_offset(el: &Element) -> (f32, f32) {
    use crate::model::{TextAlign, VAlign};
    if el.kind != ElementKind::Text {
        return (0.0, 0.0);
    }
    let ox = match el.align {
        TextAlign::Left => 0.0,
        TextAlign::Center => el.w / 2.0,
        TextAlign::Right => el.w,
    };
    let oy = match el.valign {
        VAlign::Top => 0.0,
        VAlign::Middle => el.h / 2.0,
        VAlign::Bottom => el.h,
    };
    (ox, oy)
}
