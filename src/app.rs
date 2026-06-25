//! Die Anwendung: Zustand und egui-App-Implementierung.

use std::path::PathBuf;

use egui::{Align, Color32, Context, Frame, Layout, Sense, Stroke, Vec2};

use crate::canvas::show_canvas;
use crate::model::{
    page_size_pt, Document, Element, ElementKind, Orientation, PaperFormat, TextAlign,
};
use crate::store::ImageStore;

/// Ansicht (Zoom & Verschiebung) der Zeichenfläche.
#[derive(Clone)]
pub struct View {
    pub zoom: f32,
    pub pan: Vec2,
}

impl Default for View {
    fn default() -> Self {
        View { zoom: 1.0, pan: Vec2::new(48.0, 24.0) }
    }
}

/// Welche Aktion gerade mit der Maus ausgeführt wird.
pub enum Interaction {
    None,
    /// Körper verschieben.
    DragBody { id: u64, start_pointer: egui::Pos2, start_xy: (f32, f32) },
    /// Größe ändern; gegenüberliegende Ecke bleibt fix.
    Resize { id: u64, anchor: egui::Pos2, rotation: f32, start_aspect: f32 },
    /// Drehen.
    Rotate { id: u64 },
    /// Bild zuschneiden.
    Crop { id: u64, edge: CropEdge, start_crop: crate::model::Crop },
}

#[derive(Clone, Copy, PartialEq)]
pub enum CropEdge {
    Left,
    Right,
    Top,
    Bottom,
}

pub struct EditorApp {
    pub doc: Document,
    pub page_index: usize,
    pub next_id: u64,
    pub selected: Option<u64>,
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
}

impl Default for EditorApp {
    fn default() -> Self {
        EditorApp {
            doc: Document::default(),
            page_index: 0,
            next_id: 1,
            selected: None,
            editing: None,
            edit_focus: false,
            interaction: Interaction::None,
            view: View::default(),
            images: ImageStore::default(),
            crop_mode: false,
            file_path: None,
            modified: false,
            status: String::from("Bereit. Tipp: Bild per Drag&Drop hereinziehen."),
        }
    }
}

impl EditorApp {
    pub fn new_document(&mut self) {
        self.doc = Document::default();
        self.page_index = 0;
        self.next_id = 1;
        self.selected = None;
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

    pub fn add_text(&mut self, at_center: Option<(f32, f32)>) {
        let id = self.next_id();
        let center = at_center.unwrap_or_else(|| {
            let (w, h) = page_size_pt(self.doc.format, self.doc.orientation);
            (w / 2.0, h / 2.0)
        });
        let mut el = Element::new_text(id, 0.0, 0.0);
        el.x = center.0 - el.w / 2.0;
        el.y = center.1 - el.h / 2.0;
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.push(el);
        }
        self.selected = Some(id);
        self.crop_mode = false;
        self.modified = true;
        self.status = String::from("Text hinzugefügt. Doppelklick zum Bearbeiten.");
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
        self.selected = Some(id);
        self.crop_mode = false;
        self.modified = true;
        self.status = format!("Bild hinzugefügt ({}×{}).", dims.0, dims.1);
    }

    pub fn delete_selected(&mut self) {
        let Some(id) = self.selected else { return };
        if let Some(page) = self.doc.current_page_mut(self.page_index) {
            page.elements.retain(|e| e.id != id);
        }
        self.images.remove(id);
        self.selected = None;
        self.editing = None;
        self.crop_mode = false;
        self.interaction = Interaction::None;
        self.modified = true;
        self.status = String::from("Objekt gelöscht.");
    }

    pub fn add_page(&mut self) {
        self.doc.pages.push(crate::model::Page::default());
        self.page_index = self.doc.pages.len() - 1;
        self.selected = None;
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
                if ui.button("⟲").on_hover_text("Ansicht zurücksetzen").clicked() {
                    self.view = View::default();
                }
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
                            self.selected = None;
                        }
                    });
                    if ui.button("＋ Seite").clicked() {
                        self.add_page();
                    }
                    ui.add_enabled_ui(self.page_index + 1 < self.doc.pages.len(), |ui| {
                        if ui.button("▶").clicked() {
                            self.page_index += 1;
                            self.selected = None;
                        }
                    });
                });
                ui.separator();

                let Some(sel) = self.selected else {
                    ui.label("Kein Objekt ausgewählt.\nKlicke ein Objekt an.");
                    return;
                };
                self.properties_for(ui, sel);
            });
    }

    fn properties_for(&mut self, ui: &mut egui::Ui, sel: u64) {
        // Werte holen, bearbeiten, zurück schreiben.
        let page_idx = self.page_index;
        let Some(el_idx) = self.doc.pages[page_idx]
            .elements
            .iter()
            .position(|e| e.id == sel)
        else {
            return;
        };

        let el = &mut self.doc.pages[page_idx].elements[el_idx];

        ui.horizontal(|ui| {
            ui.label("X:");
            ui.add(egui::DragValue::new(&mut el.x).speed(1.0));
            ui.label("Y:");
            ui.add(egui::DragValue::new(&mut el.y).speed(1.0));
        });
        ui.horizontal(|ui| {
            ui.label("B:");
            ui.add(egui::DragValue::new(&mut el.w).range(1.0..=4000.0).speed(1.0));
            ui.label("H:");
            ui.add(egui::DragValue::new(&mut el.h).range(1.0..=4000.0).speed(1.0));
        });

        match el.kind {
            ElementKind::Text => {
                ui.label("Text:");
                ui.add(
                    egui::TextEdit::multiline(&mut el.text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(4),
                );
                ui.horizontal(|ui| {
                    ui.label("Schriftgröße:");
                    ui.add(egui::DragValue::new(&mut el.font_size).range(4.0..=400.0).speed(0.5));
                });
                ui.horizontal(|ui| {
                    ui.label("Einzug:");
                    ui.add(egui::DragValue::new(&mut el.indent).range(0.0..=400.0).speed(0.5));
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
                    ui.label("Ausrichtung:");
                    ui.selectable_value(&mut el.align, TextAlign::Left, "Links");
                    ui.selectable_value(&mut el.align, TextAlign::Center, "Mitte");
                    ui.selectable_value(&mut el.align, TextAlign::Right, "Rechts");
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
        let _ = Layout::top_down(Align::TOP);
        let _ = (Sense::hover(), Stroke::default());
        if ui.button("Objekt löschen").clicked() {
            self.delete_selected();
        }
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
