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
            settings: Settings::default(),
            multi_anchor: BBoxAnchor::default(),
            pos_x: 0.0,
            pos_y: 0.0,
            pos_last_sel: Vec::new(),
        }
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

    pub fn add_image_from_bytes(&mut self, bytes: Vec<u8>, at: Option<(f32, f32)>) {
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
        let ids: Vec<u64> = self.selection.clone();
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.retain(|e| !ids.contains(&e.id));
        }
        for &id in &ids {
            self.images.remove(id);
        }
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
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.show_menu(&ctx);
        self.show_properties(&ctx);
        self.show_status(&ctx);

        egui::CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(0.0))
            .show(&ctx, |ui| {
                show_canvas(self, &ctx, ui);
            });

        // Dateien, die per Drag&Drop herein gezogen wurden.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for path in &dropped {
            if let Ok(bytes) = std::fs::read(path) {
                if bytes.starts_with(&[0x89, b'P', b'N', b'G'])
                    || bytes.starts_with(&[0xFF, 0xD8, 0xFF])
                {
                    self.add_image_from_bytes(bytes, None);
                }
            }
        }
        if !dropped.is_empty() {
            ctx.input_mut(|i| i.raw.dropped_files.clear());
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
                    if ui.button("Seite").clicked() {
                        self.add_page();
                        ui.close_menu();
                    }
                });

                ui.menu_button("Ansicht", |ui| {
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
        egui::SidePanel::right("properties")
            .resizable(true)
            .default_width(240.0)
            .width_range(180.0..=360.0)
            .show(ctx, |ui| {
                ui.heading("Eigenschaften");
                ui.separator();

                // Seitennavigation
                ui.label(format!(
                    "Seite {} / {}",
                    self.page_index + 1,
                    self.doc.pages.len()
                ));
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(self.page_index > 0, |ui| {
                        if ui.button("◀").clicked() {
                            self.page_index -= 1;
                            self.clear_selection();
                        }
                    });
                    if ui.button("＋ Seite").clicked() {
                        self.add_page();
                    }
                    ui.add_enabled_ui(self.page_index + 1 < self.doc.pages.len(), |ui| {
                        if ui.button("▶").clicked() {
                            self.page_index += 1;
                            self.clear_selection();
                        }
                    });
                });
                ui.separator();

                let Some(sel) = self.primary() else {
                    ui.label("Kein Objekt ausgewählt.\nKlicke oder ziehe ein Auswahl-Rechteck.");
                    return;
                };
                if self.selection.len() == 1 {
                    self.properties_for(ui, sel);
                } else {
                    self.position_section(ui);
                    ui.separator();
                    self.align_section(ui);
                    ui.separator();
                    if ui.button("Alle löschen").clicked() {
                        self.delete_selected();
                    }
                }
            });
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
                    ui.label("Einzug:");
                    ui.add(egui::DragValue::new(&mut el.indent).range(0.0..=400.0).speed(0.5).suffix("pt"));
                });
                ui.horizontal(|ui| {
                    ui.label("Farbe:");
                    let mut c = Color32::from_rgba_unmultiplied(
                        el.color[0], el.color[1], el.color[2], el.color[3],
                    );
                    ui.color_edit_button_srgba(&mut c);
                    el.color = [c.r(), c.g(), c.b(), c.a()];
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
        let mut dw = unit.from_pt(bw);
        let mut dh = unit.from_pt(bh);

        let before_x = dx;
        let before_y = dy;

        ui.horizontal(|ui| {
            ui.label("X:");
            ui.add(egui::DragValue::new(&mut dx).speed(0.1).suffix(suffix));
            ui.label("Y:");
            ui.add(egui::DragValue::new(&mut dy).speed(0.1).suffix(suffix));
        });
        ui.horizontal(|ui| {
            ui.label("B:");
            ui.add(egui::DragValue::new(&mut dw).range(0.01..=2000.0).speed(0.1).suffix(suffix));
            ui.label("H:");
            ui.add(egui::DragValue::new(&mut dh).range(0.01..=2000.0).speed(0.1).suffix(suffix));
        });

        // Delta nur aus NUTZER-Änderung berechnen (Vergleich Vorher/Nachher).
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
            // Keine Nutzereingabe → Puffer an Live-Position anpassen
            // (z.B. nach Drag im Canvas).
            self.pos_x = anchor_x;
            self.pos_y = anchor_y;
        }
    }

    /// Ausrichtungs-Buttons für Mehrfachauswahl.
    fn align_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("Ausrichten");
        ui.label("Kanten / Mitten:");

        // Jede Zeile: Horizontal-Buttons (Links / X-Mitte / Rechts)
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
    }

    /// Richtet alle ausgewählten Objekte auf einer Achse aus.
    fn align_objects(&mut self, op: AlignOp) {
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
