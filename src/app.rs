//! Die Anwendung: Zustand und egui-App-Implementierung.

use std::path::PathBuf;

use egui::{Align, Color32, Context, Frame, Layout, Sense, Stroke, Vec2};

use crate::canvas::show_canvas;
use crate::geometry::{local_corners, local_to_world};
use crate::model::{
    page_size_pt, Document, Element, ElementKind, Orientation, PageAlign, PaperFormat, ScrollMode,
    Settings, TextAlign, Units,
};
use crate::store::ImageStore;

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
}

impl Default for View {
    fn default() -> Self {
        // Pan X = 0, denn die horizontale Ausrichtung wird dynamisch berechnet.
        View { zoom: 1.0, pan: Vec2::new(0.0, 24.0) }
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
    Resize { id: u64, anchor: egui::Pos2, rotation: f32, start_aspect: f32 },
    /// Drehen.
    Rotate { id: u64 },
    /// Bild zuschneiden.
    Crop { id: u64, edge: CropEdge, start_crop: crate::model::Crop },
    /// Auswahl-Rechteck ziehen.
    SelectionBox { start: egui::Pos2 },
    /// Linien-Endpunkt ziehen (id, true=start, false=end).
    LineEndpoint { id: u64, is_start: bool },
}

#[derive(Clone, Copy, PartialEq)]
pub enum CropEdge {
    Left,
    Right,
    Top,
    Bottom,
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
    /// Snap-Visual: vertikale Mittellinie aktiv (beim Drag).
    pub snap_center: bool,
    /// Linien-Zeichenmodus: None oder Some(start_point) wenn erster Punkt gesetzt.
    pub line_drawing: Option<(f32, f32)>,
    /// Theme-Fade: Quell-Thema.
    pub theme_from: crate::model::Theme,
    /// Theme-Fade: Ziel-Thema (= settings.theme).
    pub theme_target: crate::model::Theme,
    /// Theme-Fade: Fortschritt 0..1.
    pub theme_anim: f32,
    /// Undo/Redo-History.
    pub history: crate::history::History,
    /// Flag: Snapshot beim nächsten DragValue-Focus-Gain machen.
    pub prop_snapshot_pending: bool,
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

    pub fn toggle_selected(&mut self, id: u64) {
        if let Some(pos) = self.selection.iter().position(|&x| x == id) {
            self.selection.swap_remove(pos);
        } else {
            self.selection.push(id);
        }
    }
}

impl Default for EditorApp {
    fn default() -> Self {
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
            crop_mode: false,
            file_path: None,
            modified: false,
            status: String::from("Bereit. Tipp: Bild per Drag&Drop hereinziehen."),
            settings: crate::settings_io::load_or_detect(),
            multi_anchor: BBoxAnchor::default(),
            pos_x: 0.0,
            pos_y: 0.0,
            pos_last_sel: Vec::new(),
            clipboard: Vec::new(),
            clip_origins: Vec::new(),
            pasting: false,
            snap_center: false,
            line_drawing: None,
            theme_from: crate::model::Theme::default(),
            theme_target: crate::model::Theme::default(),
            theme_anim: 1.0,
            history: crate::history::History::default(),
            prop_snapshot_pending: false,
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
                let (w, h) = page_size_pt(self.doc.format, self.doc.orientation);
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
                let (w, h) = page_size_pt(self.doc.format, self.doc.orientation);
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
                let (w, h) = page_size_pt(self.doc.format, self.doc.orientation);
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
        // Modus aktiv lassen für weitere Linien (AutoCAD-Verhalten).
        self.line_drawing = Some((f32::NAN, f32::NAN));
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
            let (w, h) = page_size_pt(self.doc.format, self.doc.orientation);
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

    pub fn add_page(&mut self) {
        self.doc.pages.push(crate::model::Page::default());
        self.page_index = self.doc.pages.len() - 1;
        self.clear_selection();
        self.modified = true;
    }

    pub fn current_elements_mut(&mut self) -> Option<&mut Vec<Element>> {
        self.doc.current_page_mut(self.page_index).map(|p| &mut p.elements)
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
                self.doc = project.doc;
                self.images = images;
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
        }
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Theme-Fade animieren.
        if self.theme_anim < 1.0 {
            let dt = ctx.input(|i| i.unstable_dt).min(0.1);
            self.theme_anim = (self.theme_anim + dt / crate::themes::fade_duration()).min(1.0);
            crate::themes::tick_fade(
                &ctx,
                self.theme_from,
                self.theme_target,
                self.theme_anim,
            );
        }

        self.show_menu(&ctx);
        self.show_properties(&ctx);
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
    }
}

impl EditorApp {
    fn show_menu(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("Datei", |ui| {
                    if ui.button("Neu").clicked() {
                        self.new_document();
                    }
                    if ui.button("Öffnen…").clicked() {
                        crate::io::open_project_dialog(self);
                    }
                    if ui.button("Speichern").clicked() {
                        crate::io::save_project_dialog(self, false);
                    }
                    if ui.button("Speichern unter…").clicked() {
                        crate::io::save_project_dialog(self, true);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        ui.separator();
                        if ui.button("ODT exportieren…").clicked() {
                            crate::io::export_odt_dialog(self);
                        }
                        if ui.button("ODT öffnen…").clicked() {
                            crate::io::import_odt_dialog(self);
                        }
                        ui.separator();
                        if ui.button("PDF exportieren…").clicked() {
                            crate::printing::export_pdf_dialog(self);
                        }
                        if ui.button("Drucken…").clicked() {
                            crate::printing::print_dialog(self);
                        }
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        ui.separator();
                        ui.label(egui::RichText::new("ODT/PDF/Druck: nur Desktop-Version").weak());
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
                        }

                        ui.separator();
                        ui.label("Seitenwechsel beim Scrollen:");
                        let mut sm = self.settings.scroll_mode;
                        ui.selectable_value(&mut sm, ScrollMode::Continuous, "Fortlaufend (Scrollen wechselt Seite)");
                        ui.selectable_value(&mut sm, ScrollMode::PageByPage, "Seitenweise (über Eigenschaften)");
                        if sm != self.settings.scroll_mode {
                            self.settings.scroll_mode = sm;
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
                    ui.separator();
                    if ui.button("Rechteck").clicked() {
                        self.add_rectangle(None);
                        ui.close_menu();
                    }
                    if ui.button("Linie").clicked() {
                        self.add_line(None);
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
                    ui.label("Seitenausrichtung:");
                    let mut align = self.settings.page_align;
                    ui.selectable_value(&mut align, PageAlign::Left, "Linksbündig");
                    ui.selectable_value(&mut align, PageAlign::Center, "Mittig");
                    ui.selectable_value(&mut align, PageAlign::Right, "Rechtsbündig");
                    if align != self.settings.page_align {
                        self.settings.page_align = align;
                    }

                    ui.separator();
                    if ui.button("Ansicht zurücksetzen").clicked() {
                        self.view = View::default();
                        ui.close_menu();
                    }
                });

                ui.separator();
                ui.label("Format:");
                let mut fmt = self.doc.format;
                egui::ComboBox::from_id_salt("format")
                    .selected_text(self.doc.format.label())
                    .show_ui(ui, |ui| {
                        for f in PaperFormat::all() {
                            ui.selectable_value(&mut fmt, f, f.label());
                        }
                    });
                if fmt != self.doc.format {
                    self.doc.format = fmt;
                    self.touch();
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
                if ui.button("−").clicked() {
                    self.view.zoom = (self.view.zoom * 0.9).max(0.1);
                }
                if ui.button("＋").clicked() {
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
                            if ui.button("◀").clicked() {
                                app.page_index -= 1;
                                app.clear_selection();
                            }
                        });
                        if ui.button("＋ Seite").clicked() {
                            app.add_page();
                        }
                        ui.add_enabled_ui(app.page_index + 1 < app.doc.pages.len(), |ui| {
                            if ui.button("▶").clicked() {
                                app.page_index += 1;
                                app.clear_selection();
                            }
                        });
                    });
                    ui.separator();

                    let Some(sel) = app.primary() else {
                        ui.label("Kein Objekt ausgewählt.\nKlicke oder ziehe ein Auswahl-Rechteck.");
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
                ui.add(egui::DragValue::new(&mut w_d).range(0.01..=2000.0).speed(0.1).suffix(suffix));
                ui.label("H:");
                ui.add(egui::DragValue::new(&mut h_d).range(0.01..=2000.0).speed(0.1).suffix(suffix));
            });

            // Zurückschreiben: Element-Position = eingegebener Wert − Offset.
            el.x = unit.to_pt(x_d) - ox;
            el.y = unit.to_pt(y_d) - oy;
            el.w = unit.to_pt(w_d);
            el.h = unit.to_pt(h_d);
        }

        // --- Element-spezifische Eigenschaften ---
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
                    if let Some(k) = chosen {
                        el.font = k;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Schriftgröße:");
                    ui.add(egui::DragValue::new(&mut el.font_size).range(4.0..=400.0).speed(0.5).suffix("pt"));
                });
                ui.horizontal(|ui| {
                    if ui.selectable_label(el.bold, egui::RichText::new("B").strong()).clicked() {
                        el.bold = !el.bold;
                    }
                    if ui.selectable_label(el.italic, egui::RichText::new("I").italics()).clicked() {
                        el.italic = !el.italic;
                    }
                    if ui.selectable_label(el.underline, egui::RichText::new("U").underline()).clicked() {
                        el.underline = !el.underline;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Einzug:");
                    ui.add(egui::DragValue::new(&mut el.indent).range(0.0..=400.0).speed(0.5).suffix("pt"));
                });
                ui.horizontal(|ui| {
                    ui.label("Farbe:");
                    let mut c = Color32::from_rgba_unmultiplied(
                        el.color[0], el.color[1], el.color[2], el.color[3],
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
                    ui.add(egui::DragValue::new(&mut el.rotation).range(-360.0..=360.0).speed(0.5).suffix("°"));
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
            ElementKind::Rectangle | ElementKind::Line => {
                ui.heading(if el.kind == ElementKind::Rectangle { "Rechteck" } else { "Linie" });
                ui.horizontal(|ui| {
                    ui.label("Drehung:");
                    ui.add(egui::DragValue::new(&mut el.rotation).range(-360.0..=360.0).speed(0.5).suffix("°"));
                });
                ui.horizontal(|ui| {
                    ui.label("Rahmenfarbe:");
                    let mut c = Color32::from_rgba_unmultiplied(
                        el.stroke_color[0], el.stroke_color[1], el.stroke_color[2], el.stroke_color[3],
                    );
                    ui.color_edit_button_srgba(&mut c);
                    el.stroke_color = c.to_srgba_unmultiplied();
                });
                ui.horizontal(|ui| {
                    ui.label("Rahmenstärke:");
                    ui.add(egui::DragValue::new(&mut el.stroke_width).range(0.0..=50.0).speed(0.2).suffix("pt"));
                });
                if el.kind == ElementKind::Rectangle {
                    ui.horizontal(|ui| {
                        ui.label("Füllfarbe:");
                        let mut c = Color32::from_rgba_unmultiplied(
                            el.fill_color[0], el.fill_color[1], el.fill_color[2], el.fill_color[3],
                        );
                        ui.color_edit_button_srgba(&mut c);
                        el.fill_color = c.to_srgba_unmultiplied();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Eckradius:");
                        ui.add(egui::DragValue::new(&mut el.corner_radius).range(0.0..=100.0).speed(0.2).suffix("pt"));
                    });
                }
                if ui.button("90° drehen").clicked() {
                    el.rotation += 90.0;
                }
            }
        }

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
        let anchor_clicked = ui.horizontal(|ui| {
            let columns = [
                [BBoxAnchor::TopLeft, BBoxAnchor::MidLeft, BBoxAnchor::BotLeft],
                [BBoxAnchor::TopCenter, BBoxAnchor::Center, BBoxAnchor::BotCenter],
                [BBoxAnchor::TopRight, BBoxAnchor::MidRight, BBoxAnchor::BotRight],
            ];
            let mut changed = false;
            for col in &columns {
                ui.vertical(|ui| {
                    for anchor in col {
                        let sel = self.multi_anchor == *anchor;
                        if ui.selectable_label(sel, "●").clicked() {
                            self.multi_anchor = *anchor;
                            changed = true;
                        }
                    }
                });
            }
            changed
        }).inner;

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
            .map(|p| p.elements.iter().filter(|e| sel_ids.contains(&e.id)).collect())
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

        let rw = ui.horizontal(|ui| {
            ui.label("B:");
            let mut dv = egui::DragValue::new(&mut dw).range(0.0..=2000.0).speed(0.1).suffix(suffix);
            if !w_uniform {
                // Wert auf 0.0 lassen und nur als "—" anzeigen.
                // changed() wird durch den custom_formatter nicht ausgelöst.
                dv = dv.custom_formatter(|_, _| String::from("—"));
            }
            ui.add(dv)
        }).inner;
        let rh = ui.horizontal(|ui| {
            ui.label("H:");
            let mut dv = egui::DragValue::new(&mut dh).range(0.0..=2000.0).speed(0.1).suffix(suffix);
            if !h_uniform {
                dv = dv.custom_formatter(|_, _| String::from("—"));
            }
            ui.add(dv)
        }).inner;

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

    /// Ausrichtungs-Buttons für Mehrfachauswahl.
    fn align_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ausrichten");
        ui.label("Kanten / Mitten:");

        // Horizontal-Buttons (Links / X-Mitte / Rechts)
        ui.horizontal(|ui| {
            if ui.button("⟨ Links").on_hover_text("Alle an der linken Kante ausrichten").clicked() {
                self.align_objects(AlignOp::Left);
            }
            if ui.button("X Mitte").on_hover_text("Alle horizontal mittig ausrichten").clicked() {
                self.align_objects(AlignOp::CenterX);
            }
            if ui.button("Rechts ⟩").on_hover_text("Alle an der rechten Kante ausrichten").clicked() {
                self.align_objects(AlignOp::Right);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("⟨ Oben").on_hover_text("Alle an der oberen Kante ausrichten").clicked() {
                self.align_objects(AlignOp::Top);
            }
            if ui.button("Y Mitte").on_hover_text("Alle vertikal mittig ausrichten").clicked() {
                self.align_objects(AlignOp::CenterY);
            }
            if ui.button("Unten ⟩").on_hover_text("Alle an der unteren Kante ausrichten").clicked() {
                self.align_objects(AlignOp::Bottom);
            }
        });

        ui.separator();
        ui.label("Abstände verteilen:");
        ui.horizontal(|ui| {
            if ui.button("⇿ Horizontal").on_hover_text("Gleiche horizontale Abstände zwischen allen Objekten").clicked() {
                self.distribute_objects(DistributeOp::Horizontal);
            }
            if ui.button("⇕ Vertikal").on_hover_text("Gleiche vertikale Abstände zwischen allen Objekten").clicked() {
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
        let Some(page) = self.doc.pages.get(page_idx) else { return };
        let sel_els: Vec<&Element> = page.elements.iter().filter(|e| sel_ids.contains(&e.id)).collect();
        if sel_els.len() < 2 {
            return;
        }

        let ref_val = match op {
            AlignOp::Left => sel_els.iter().map(|e| e.x).fold(f32::INFINITY, f32::min),
            AlignOp::Right => sel_els.iter().map(|e| e.x + e.w).fold(f32::NEG_INFINITY, f32::max),
            AlignOp::CenterX => {
                let (min, max) = sel_els
                    .iter()
                    .map(|e| e.x)
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, mx), x| (mn.min(x), mx.max(x)));
                (min + max) / 2.0
            }
            AlignOp::Top => sel_els.iter().map(|e| e.y).fold(f32::INFINITY, f32::min),
            AlignOp::Bottom => sel_els.iter().map(|e| e.y + e.h).fold(f32::NEG_INFINITY, f32::max),
            AlignOp::CenterY => {
                let (min, max) = sel_els
                    .iter()
                    .map(|e| e.y)
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, my), y| (mn.min(y), my.max(y)));
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

        let Some(page) = self.doc.pages.get(page_idx) else { return };
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
        ui.horizontal(|ui| {
            ui.label("Schrift:");
            if font_uniform {
                let mut chosen: Option<String> = None;
                for def in crate::model::FONT_CHOICES {
                    let selected = data[0].font == def.key;
                    let text = egui::RichText::new(def.display)
                        .family(crate::fonts::family_for(def.key));
                    if ui.selectable_label(selected, text).clicked() {
                        chosen = Some(def.key.to_string());
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
        let size_uniform = data.iter().all(|d| (d.font_size - data[0].font_size).abs() < 0.01);
        let mut ds = if size_uniform {
            data[0].font_size
        } else {
            0.0
        };
        let rs = ui.horizontal(|ui| {
            ui.label("Schriftgröße:");
            let mut dv = egui::DragValue::new(&mut ds)
                .range(4.0..=400.0)
                .speed(0.5)
                .suffix("pt");
            if !size_uniform {
                dv = dv.custom_formatter(|_, _| String::from("—"));
            }
            ui.add(dv)
        }).inner;
        if rs.changed() {
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

    /// Kopiert alle ausgewählten Elemente in die Zwischenablage.
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
        if !self.clipboard.is_empty() {
            self.status = format!("{} Objekt(e) kopiert.", self.clipboard.len());
        }
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
        let paste_ref = if snapped {
            ref_origin
        } else {
            cursor_page
        };

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
                ui.painter()
                    .circle_filled(ui.min_rect().left_center() + Vec2::new(12.0, 0.0), 4.0, dot);
                ui.label(&self.status);
            });
        });
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
