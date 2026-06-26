//! Die Zeichenfläche: Rendern der Seite und aller Objekte sowie die gesamte
//! Maus-Interaktion (verschieben, skalieren, drehen, zuschneiden).

use egui::{
    epaint::{Mesh, Vertex}, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2,
};

use crate::app::{CropEdge, EditorApp, Interaction};
use crate::geometry::{local_corners, local_to_world, rotate_vec, world_to_local};
use crate::model::{page_size_pt, Element, ElementKind, PageAlign, ScrollMode};
use crate::store::ImageStore;

/// Kopie der aktiven Interaktion, damit `&app` nicht während der Bearbeitung
/// gebunden bleibt.
#[derive(Clone)]
enum Active {
    Drag(Vec<(u64, f32, f32)>, Pos2),
    Resize(u64, Pos2, f32, f32),
    Rotate(u64),
    Crop(u64, CropEdge, crate::model::Crop),
    SelectionBox(Pos2),
}

pub fn show_canvas(app: &mut EditorApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // --- Eingaben ---
    let scroll = ui.input(|i| i.smooth_scroll_delta);
    let ctrl = ui.input(|i| i.modifiers.ctrl);
    let delta = ui.input(|i| i.pointer.delta());
    let middle_down = ui.input(|i| i.pointer.middle_down());
    let primary_pressed = ui.input(|i| i.pointer.primary_pressed());
    let primary_released = ui.input(|i| i.pointer.primary_released());
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let double_clicked = response.double_clicked();

    let base = rect.min;
    let (pw_pt, ph_pt) = page_size_pt(app.doc.format, app.doc.orientation);
    let page_align = app.settings.page_align;
    let rect_w = rect.width();

    // --- Alignment-Offset berechnen (vor Zoom, da Zoom ihn berücksichtigt) ---
    let compute_align_x = |zoom_val: f32| {
        let page_w_screen = pw_pt * zoom_val;
        match page_align {
            PageAlign::Left => 24.0,
            PageAlign::Center => ((rect_w - page_w_screen) / 2.0).max(24.0),
            PageAlign::Right => (rect_w - page_w_screen - 24.0).max(24.0),
        }
    };

    // --- Zoom & Verschiebung ---
    if ctrl && scroll.y.abs() > 0.0 {
        if let Some(cur) = pointer {
            if rect.contains(cur) {
                let zoom = app.view.zoom;
                let pan = app.view.pan;
                let align_x = compute_align_x(zoom);
                let page_under = Vec2::new(
                    (cur.x - base.x - align_x - pan.x) / zoom,
                    (cur.y - base.y - pan.y) / zoom,
                );
                let factor = if scroll.y > 0.0 { 1.1 } else { 0.9 };
                let new_zoom = (zoom * factor).clamp(0.1, 6.0);
                let new_align_x = compute_align_x(new_zoom);
                app.view.zoom = new_zoom;
                app.view.pan = Vec2::new(
                    cur.x - base.x - new_align_x - page_under.x * new_zoom,
                    cur.y - base.y - page_under.y * new_zoom,
                );
            }
        }
    } else if scroll != Vec2::ZERO {
        // --- Continuous-Scroll: Seitenwechsel am Ende ---
        if app.settings.scroll_mode == ScrollMode::Continuous && scroll.y.abs() > 0.1 {
            let zoom = app.view.zoom;
            let page_h_screen = ph_pt * zoom;
            let page_top = app.view.pan.y;
            let page_bottom = page_top + page_h_screen;
            if scroll.y > 0.0 && page_bottom + scroll.y < 24.0 {
                // Runterscrollen über Seitenende → nächste Seite
                if app.page_index + 1 < app.doc.pages.len() {
                    app.page_index += 1;
                    app.view.pan.y = 24.0;
                    app.clear_selection();
                }
            } else if scroll.y < 0.0 && page_top + scroll.y > 24.0 {
                // Hochscrollen über Seitenanfang → vorige Seite
                if app.page_index > 0 {
                    app.page_index -= 1;
                    let new_page_h = ph_pt * zoom;
                    app.view.pan.y = -new_page_h + 24.0;
                    app.clear_selection();
                }
            } else {
                app.view.pan += scroll;
            }
        } else {
            app.view.pan += scroll;
        }
    }
    if middle_down {
        app.view.pan += delta;
    }

    let zoom = app.view.zoom;
    let align_offset_x = compute_align_x(zoom);
    let pan = app.view.pan;
    let to_screen = |p: Pos2| {
        base + Vec2::new(align_offset_x + pan.x, pan.y) + Vec2::new(p.x, p.y) * zoom
    };
    let to_page = |s: Pos2| {
        Vec2::new(
            (s.x - base.x - align_offset_x - pan.x) / zoom,
            (s.y - base.y - pan.y) / zoom,
        )
    };

    // --- Seite zeichnen ---
    let page_rect_screen = Rect::from_min_size(to_screen(Pos2::ZERO), Vec2::new(pw_pt, ph_pt) * zoom);
    painter.rect_filled(
        page_rect_screen.translate(Vec2::new(4.0, 6.0)),
        2.0,
        Color32::from_black_alpha(35),
    );
    painter.rect_filled(page_rect_screen, 2.0, Color32::WHITE);

    // --- Elemente zeichnen ---
    let selection: Vec<u64> = app.selection.clone();
    let crop_mode = app.crop_mode;
    let page_idx = app.page_index;
    if let Some(page) = app.doc.pages.get_mut(page_idx) {
        for el in page.elements.iter_mut() {
            draw_element(el, &mut app.images, ctx, &painter, &to_screen, zoom);
            if selection.contains(&el.id) {
                if selection.len() == 1 {
                    draw_selection(el, &painter, &to_screen, zoom, crop_mode);
                } else {
                    draw_multi_selection_box(el, &painter, &to_screen, zoom);
                }
            }
        }
    }

    // --- Textbearbeitung (Overlay) ---
    if app.editing.is_some() {
        let edit_id = app.editing.as_ref().unwrap().0;
        let el_rect = app
            .doc
            .pages
            .get(page_idx)
            .and_then(|p| p.elements.iter().find(|e| e.id == edit_id))
            .map(|el| {
                Rect::from_min_size(
                    to_screen(Pos2::new(el.x, el.y)) + Vec2::new(el.indent * zoom, 0.0),
                    Vec2::new(el.w * zoom, (el.h * zoom).max(el.font_size * zoom * 1.4)),
                )
            });
        if let Some(r) = el_rect {
            let buf = &mut app.editing.as_mut().unwrap().1;
            let out = ui.put(
                r,
                egui::TextEdit::multiline(buf).desired_width(r.width()),
            );
            if app.edit_focus {
                out.request_focus();
                app.edit_focus = false;
            }
            let commit = (out.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl))
                || (primary_pressed && !r.contains(pointer.unwrap_or(Pos2::ZERO)));
            if commit {
                let (id, text) = app.editing.take().unwrap();
                if let Some(el) = app
                    .doc
                    .pages
                    .get_mut(page_idx)
                    .and_then(|p| p.elements.iter_mut().find(|e| e.id == id))
                {
                    el.text = text;
                    app.touch();
                }
            }
        }
    }

    // --- Paste: Ghost + Preview Rendering ---
    if app.pasting && !app.clipboard.is_empty() {
        let ghost_fill = Color32::from_rgba_unmultiplied(100, 160, 230, 20);
        let ghost_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(100, 160, 230, 70));

        // Ghost an Originalpositionen.
        for (i, el) in app.clipboard.iter().enumerate() {
            let (gx, gy) = app.clip_origins[i];
            let r = Rect::from_min_size(
                to_screen(Pos2::new(gx, gy)),
                Vec2::new(el.w, el.h) * zoom,
            );
            painter.rect_filled(r, 2.0, ghost_fill);
            painter.add(egui::epaint::RectShape::new(
                r, 2.0, Color32::TRANSPARENT, ghost_stroke, egui::StrokeKind::Inside,
            ));
            // Ghost-Text rendern.
            if el.kind == ElementKind::Text && !el.text.is_empty() {
                let font = FontId::new(el.font_size * zoom, crate::fonts::family_for(&el.font));
                let color = Color32::from_rgba_unmultiplied(100, 160, 230, 50);
                let galley = painter.layout(el.text.clone(), font, color, (el.w * zoom).max(1.0));
                painter.galley(to_screen(Pos2::new(gx, gy)), galley, color);
            }
        }

        // Preview am Cursor + Snap-Erkennung.
        if let Some(pt) = pointer {
            let cp = to_page(pt);
            let ref_origin = app.clip_origins[0];
            let ref_screen = to_screen(Pos2::new(ref_origin.0, ref_origin.1));
            let snapped = pt.distance(ref_screen) < 16.0;

            let paste_ref = if snapped {
                ref_origin
            } else {
                (cp.x, cp.y)
            };

            let (p_fill, p_stroke) = if snapped {
                (
                    Color32::from_rgba_unmultiplied(80, 200, 120, 50),
                    Stroke::new(1.5, Color32::from_rgb(80, 200, 120)),
                )
            } else {
                (
                    Color32::from_rgba_unmultiplied(40, 120, 220, 40),
                    Stroke::new(1.5, Color32::from_rgb(40, 120, 220)),
                )
            };

            for (i, el) in app.clipboard.iter().enumerate() {
                let rel_x = app.clip_origins[i].0 - ref_origin.0;
                let rel_y = app.clip_origins[i].1 - ref_origin.1;
                let (px, py) = (paste_ref.0 + rel_x, paste_ref.1 + rel_y);
                let r = Rect::from_min_size(
                    to_screen(Pos2::new(px, py)),
                    Vec2::new(el.w, el.h) * zoom,
                );
                painter.rect_filled(r, 2.0, p_fill);
                painter.add(egui::epaint::RectShape::new(
                    r, 2.0, Color32::TRANSPARENT, p_stroke, egui::StrokeKind::Inside,
                ));
                // Preview-Text rendern.
                if el.kind == ElementKind::Text && !el.text.is_empty() {
                    let font = FontId::new(el.font_size * zoom, crate::fonts::family_for(&el.font));
                    let color = Color32::from_rgba_unmultiplied(el.color[0], el.color[1], el.color[2], 160);
                    let galley = painter.layout(el.text.clone(), font, color, (el.w * zoom).max(1.0));
                    painter.galley(to_screen(Pos2::new(px, py)), galley, color);
                }
            }

            if snapped {
                painter.text(
                    pt + Vec2::new(12.0, -20.0),
                    egui::Align2::LEFT_BOTTOM,
                    "◉ Snap",
                    FontId::proportional(12.0),
                    Color32::from_rgb(80, 200, 120),
                );
            }
        }
    }

    // --- Interaktion beenden ---
    if primary_released {
        match app.interaction {
            Interaction::SelectionBox { start } => {
                // Auswahl-Rechteck abschließen: alle treffenden Objekte auswählen.
                if let Some(cur) = pointer {
                    let s = to_page(start);
                    let e = to_page(cur);
                    let (min_x, max_x) = (s.x.min(e.x), s.x.max(e.x));
                    let (min_y, max_y) = (s.y.min(e.y), s.y.max(e.y));
                    let shift = ui.input(|i| i.modifiers.shift);
                    if !shift {
                        app.clear_selection();
                    }
                    let hits: Vec<u64> = app.doc.pages[page_idx]
                        .elements
                        .iter()
                        .filter(|el| {
                            el.x < max_x
                                && el.x + el.w > min_x
                                && el.y < max_y
                                && el.y + el.h > min_y
                        })
                        .map(|el| el.id)
                        .collect();
                    for id in hits {
                        if !shift || !app.is_selected(id) {
                            app.selection.push(id);
                        } else if shift {
                            // Shift: bereits ausgewählte bleiben ausgewählt
                        }
                    }
                }
                app.interaction = Interaction::None;
            }
            Interaction::DragBodies { .. }
            | Interaction::Resize { .. }
            | Interaction::Rotate { .. }
            | Interaction::Crop { .. } => app.interaction = Interaction::None,
            _ => {}
        }
    }

    // --- Aktive Interaktion fortsetzen ---
    let active = match &app.interaction {
        Interaction::DragBodies { start_pointer, starts } => {
            Some(Active::Drag(starts.clone(), *start_pointer))
        }
        Interaction::Resize { id, anchor, rotation, start_aspect } => {
            Some(Active::Resize(*id, *anchor, *rotation, *start_aspect))
        }
        Interaction::Rotate { id } => Some(Active::Rotate(*id)),
        Interaction::Crop { id, edge, start_crop } => {
            Some(Active::Crop(*id, *edge, *start_crop))
        }
        Interaction::SelectionBox { start } => Some(Active::SelectionBox(*start)),
        _ => None,
    };
    if let (Some(a), Some(pointer)) = (active, pointer) {
        match a {
            Active::Drag(starts, sp) => {
                let dp = to_page(pointer) - to_page(sp);
                for (id, sx, sy) in &starts {
                    if let Some(el) = element_mut(app, page_idx, *id) {
                        el.x = sx + dp.x;
                        el.y = sy + dp.y;
                    }
                }
                app.touch();
            }
            Active::Resize(id, anchor, rotation, start_aspect) => {
                if let Some(el) = element_mut(app, page_idx, id) {
                    let shift = ui.input(|i| i.modifiers.shift);
                    resize_to_pointer(el, anchor, rotation, to_page(pointer), shift, start_aspect);
                    app.touch();
                }
            }
            Active::Rotate(id) => {
                if let Some(el) = element_mut(app, page_idx, id) {
                    let center = Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0);
                    let d = to_page(pointer) - center.to_vec2();
                    el.rotation = d.x.atan2(-d.y).to_degrees();
                    app.touch();
                }
            }
            Active::Crop(id, edge, start) => {
                if let Some(el) = element_mut(app, page_idx, id) {
                    crop_to_pointer(el, edge, start, to_page(pointer));
                    app.touch();
                }
            }
            Active::SelectionBox(start) => {
                // Auswahl-Rechteck zeichnen. `start` und `pointer` sind
                // beide Screen-Koordinaten — keine Transformation nötig.
                let r = Rect::from_two_pos(start, pointer);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(40, 120, 220, 30));
                painter.add(egui::epaint::RectShape::new(
                    r,
                    0.0,
                    egui::Color32::TRANSPARENT,
                    Stroke::new(1.0, Color32::from_rgb(40, 120, 220)),
                    egui::StrokeKind::Inside,
                ));
            }
        }
    }

    // --- Gemeinsame Eingabe-Flags ---
    let editing_active = app.editing.is_some();
    let pointer_in_canvas = pointer.map(|p| rect.contains(p)).unwrap_or(false);
    let click_on_ui = pointer
        .and_then(|p| ctx.layer_id_at(p))
        .map(|lid| lid.order != egui::Order::Background)
        .unwrap_or(false);

    // --- Paste-Modus: Klick bestätigt das Einfügen ---
    if app.pasting && primary_pressed && pointer_in_canvas && !click_on_ui {
        if let Some(pt) = pointer {
            let cp = to_page(pt);
            let ref_origin = app.clip_origins[0];
            let ref_screen = to_screen(Pos2::new(ref_origin.0, ref_origin.1));
            let snapped = pt.distance(ref_screen) < 16.0;
            app.confirm_paste((cp.x, cp.y), snapped);
        }
    }

    // --- Neue Interaktion starten (nur wenn nicht am Pasten) ---
    if !app.pasting
        && matches!(app.interaction, Interaction::None)
        && primary_pressed
        && !editing_active
        && !middle_down
        && pointer_in_canvas
        && !click_on_ui
    {
        if let Some(pointer) = pointer {
            let shift = ui.input(|i| i.modifiers.shift);
            start_interaction(app, page_idx, pointer, to_screen, to_page, zoom, shift);
        }
    }

    // --- Doppelklick → Text bearbeiten (nur wenn nicht am Pasten) ---
    if !app.pasting && double_clicked && !editing_active && pointer_in_canvas && !click_on_ui {
        if let Some(pointer) = pointer {
            // Wenn ein bestehendes Text-Objekt getroffen wird → bearbeiten.
            let mut hit_text = None;
            for el in app.doc.pages[page_idx].elements.iter().rev() {
                if el.kind == ElementKind::Text && point_in_element(el, pointer, &to_screen, zoom) {
                    hit_text = Some(el.id);
                    break;
                }
            }
            if let Some(id) = hit_text {
                let text = app.doc.pages[page_idx]
                    .elements
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.text.clone())
                    .unwrap_or_default();
                app.editing = Some((id, text));
                app.edit_focus = true;
                app.select_only(id);
            } else {
                // Leere Fläche → neues Textfeld, linke-obere Ecke an der Cursor-Position.
                let p = to_page(pointer);
                app.add_text(Some((false, p.x, p.y)));
            }
        }
    }

    // --- Tastatur ---
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Delete) && app.editing.is_none() && !app.pasting {
            app.delete_selected();
        }
        if i.key_pressed(egui::Key::Escape) {
            if app.pasting {
                app.pasting = false;
                app.status = String::from("Einfügen abgebrochen.");
            } else {
                app.crop_mode = false;
                app.editing = None;
                app.clear_selection();
                app.interaction = Interaction::None;
            }
        }
        if i.key_pressed(egui::Key::C) && i.modifiers.ctrl && app.editing.is_none() {
            app.copy_selection();
        }
        if i.key_pressed(egui::Key::V) && i.modifiers.ctrl && app.editing.is_none() {
            app.start_paste();
        }
    });

    painter.text(
        rect.left_top() + Vec2::new(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        "Strg+Scroll = Zoom · Strg+C/V = Kopieren/Einfügen · Entf = Löschen · Esc = Abbrechen",
        FontId::proportional(11.0),
        Color32::from_gray(150),
    );
}

fn element_mut<'a>(app: &'a mut EditorApp, page_idx: usize, id: u64) -> Option<&'a mut Element> {
    app.doc.pages.get_mut(page_idx)?.elements.iter_mut().find(|e| e.id == id)
}

fn draw_element(
    el: &mut Element,
    images: &mut ImageStore,
    ctx: &egui::Context,
    painter: &egui::Painter,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
) {
    match el.kind {
        ElementKind::Text => {
            let font = FontId::new(el.font_size * zoom, crate::fonts::family_for(&el.font));
            let color = Color32::from_rgba_unmultiplied(
                el.color[0], el.color[1], el.color[2], el.color[3],
            );
            let galley = painter.layout(el.text.clone(), font, color, (el.w * zoom).max(1.0));
            el.h = (galley.size().y / zoom).max(el.font_size * 1.2);
            let mut pos = to_screen(Pos2::new(el.x, el.y)) + Vec2::new(el.indent * zoom, 0.0);
            match el.align {
                crate::model::TextAlign::Left => {}
                crate::model::TextAlign::Center => pos.x += (el.w * zoom - galley.size().x) / 2.0,
                crate::model::TextAlign::Right => pos.x += el.w * zoom - galley.size().x,
            }
            match el.valign {
                crate::model::VAlign::Top => {}
                crate::model::VAlign::Middle => {
                    pos.y += (el.h * zoom - galley.size().y) / 2.0;
                }
                crate::model::VAlign::Bottom => {
                    pos.y += el.h * zoom - galley.size().y;
                }
            }
            painter.galley(pos, galley, color);
        }
        ElementKind::Image => {
            if let Some(tex) = images.texture(el.id, ctx) {
                let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
                let cl = local_corners(el.w * zoom, el.h * zoom);
                let uv = [
                    [el.crop.x, el.crop.y],
                    [el.crop.x + el.crop.w, el.crop.y],
                    [el.crop.x + el.crop.w, el.crop.y + el.crop.h],
                    [el.crop.x, el.crop.y + el.crop.h],
                ];
                let mut mesh = Mesh::default();
                mesh.texture_id = tex.id();
                for (i, lc) in cl.iter().enumerate() {
                    let pos = local_to_world(center, el.rotation, *lc);
                    mesh.vertices.push(Vertex { pos, uv: uv[i].into(), color: Color32::WHITE });
                }
                mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
                painter.add(Shape::mesh(mesh));
            } else {
                let r = Rect::from_min_size(
                    to_screen(Pos2::new(el.x, el.y)),
                    Vec2::new(el.w, el.h) * zoom,
                );
                painter.rect_filled(r, 0.0, Color32::from_rgb(120, 120, 120));
            }
        }
    }
}

/// Einfacher Rahmen für jedes Element in einer Multi-Selection (ohne Griffe).
fn draw_multi_selection_box(
    el: &Element,
    painter: &egui::Painter,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
) {
    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let cl = local_corners(el.w * zoom, el.h * zoom);
    let mut pts: Vec<Pos2> = cl
        .iter()
        .map(|lc| local_to_world(center, el.rotation, *lc))
        .collect();
    pts.push(pts[0]);
    painter.add(Shape::line(pts, Stroke::new(1.5, Color32::from_rgb(40, 120, 220))));
}

#[allow(clippy::too_many_arguments)]
fn draw_selection(
    el: &Element,
    painter: &egui::Painter,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    crop_mode: bool,
) {
    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let cl = local_corners(el.w * zoom, el.h * zoom);
    let pts: Vec<Pos2> = cl
        .iter()
        .map(|lc| local_to_world(center, el.rotation, *lc))
        .collect();

    let mut line = pts.clone();
    line.push(pts[0]);
    painter.add(Shape::line(line, Stroke::new(1.5, Color32::from_rgb(40, 120, 220))));

    if el.kind == ElementKind::Image && !crop_mode {
        let top_mid = local_to_world(center, el.rotation, Vec2::new(0.0, -el.h * zoom / 2.0));
        let grip = local_to_world(center, el.rotation, Vec2::new(0.0, -el.h * zoom / 2.0 - 24.0));
        painter.line_segment([top_mid, grip], Stroke::new(1.5, Color32::from_rgb(40, 120, 220)));
        painter.circle_filled(grip, 6.0, Color32::from_rgb(40, 120, 220));
    }

    let handle_color = if crop_mode {
        Color32::from_rgb(220, 90, 40)
    } else {
        Color32::from_rgb(40, 120, 220)
    };

    if crop_mode && el.kind == ElementKind::Image {
        for p in crop_edge_handles(el, center, zoom) {
            painter.rect_filled(
                Rect::from_center_size(p, Vec2::splat(10.0)),
                2.0,
                handle_color,
            );
        }
    } else {
        for p in &pts {
            painter.circle_filled(*p, 5.0, Color32::WHITE);
            painter.circle_stroke(*p, 5.0, Stroke::new(1.5, handle_color));
        }
    }
}

fn crop_edge_handles(el: &Element, center: Pos2, zoom: f32) -> [Pos2; 4] {
    let w = el.w * zoom;
    let h = el.h * zoom;
    let c = el.crop;
    let left = local_to_world(
        center,
        el.rotation,
        Vec2::new(-w / 2.0 + c.x * w, -h / 2.0 + (c.y + c.h / 2.0) * h),
    );
    let right = local_to_world(
        center,
        el.rotation,
        Vec2::new(-w / 2.0 + (c.x + c.w) * w, -h / 2.0 + (c.y + c.h / 2.0) * h),
    );
    let top = local_to_world(
        center,
        el.rotation,
        Vec2::new(-w / 2.0 + (c.x + c.w / 2.0) * w, -h / 2.0 + c.y * h),
    );
    let bottom = local_to_world(
        center,
        el.rotation,
        Vec2::new(-w / 2.0 + (c.x + c.w / 2.0) * w, -h / 2.0 + (c.y + c.h) * h),
    );
    [left, right, top, bottom]
}

fn point_in_element(
    el: &Element,
    pointer_screen: Pos2,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
) -> bool {
    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let local = world_to_local(center, el.rotation, pointer_screen);
    local.x.abs() <= el.w * zoom / 2.0 && local.y.abs() <= el.h * zoom / 2.0
}

fn corner_positions(el: &Element, to_screen: &impl Fn(Pos2) -> Pos2, zoom: f32) -> [Pos2; 4] {
    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let cl = local_corners(el.w * zoom, el.h * zoom);
    let mut out = [Pos2::ZERO; 4];
    for i in 0..4 {
        out[i] = local_to_world(center, el.rotation, cl[i]);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn start_interaction(
    app: &mut EditorApp,
    page_idx: usize,
    pointer: Pos2,
    to_screen: impl Fn(Pos2) -> Pos2,
    to_page: impl Fn(Pos2) -> Vec2,
    zoom: f32,
    shift: bool,
) {
    let sel = app.primary();
    let crop_mode = app.crop_mode;

    // 1) Crop-Kanten
    if crop_mode {
        if let Some(id) = sel {
            if let Some(el) = app.doc.pages[page_idx].elements.iter().find(|e| e.id == id) {
                if el.kind == ElementKind::Image {
                    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
                    for (i, hp) in crop_edge_handles(el, center, zoom).iter().enumerate() {
                        if hp.distance(pointer) < 9.0 {
                            let edge = match i {
                                0 => CropEdge::Left,
                                1 => CropEdge::Right,
                                2 => CropEdge::Top,
                                _ => CropEdge::Bottom,
                            };
                            let start_crop = el.crop;
                            app.interaction = Interaction::Crop { id, edge, start_crop };
                            return;
                        }
                    }
                }
            }
        }
    }

    // 2) Drehgriff
    if let Some(id) = sel {
        if let Some(el) = app.doc.pages[page_idx].elements.iter().find(|e| e.id == id) {
            if el.kind == ElementKind::Image && !crop_mode {
                let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
                let grip =
                    local_to_world(center, el.rotation, Vec2::new(0.0, -el.h * zoom / 2.0 - 24.0));
                if grip.distance(pointer) < 9.0 {
                    app.interaction = Interaction::Rotate { id };
                    return;
                }
            }
        }
    }

    // 3) Ecken (Größe ändern)
    if let Some(id) = sel {
        if let Some(el) = app.doc.pages[page_idx].elements.iter().find(|e| e.id == id) {
            if !(crop_mode && el.kind == ElementKind::Image) {
                let corners = corner_positions(el, &to_screen, zoom);
                for (i, cp) in corners.iter().enumerate() {
                    if cp.distance(pointer) < 9.0 {
                        let opposite = corners[(i + 2) % 4];
                        let a = to_page(opposite);
                        let anchor = Pos2::new(a.x, a.y);
                        let rotation = el.rotation;
                        let start_aspect = if el.h != 0.0 { el.w / el.h } else { 1.0 };
                        app.interaction = Interaction::Resize { id, anchor, rotation, start_aspect };
                        return;
                    }
                }
            }
        }
    }

    // 4) Körper (oberstes getroffenes Objekt)
    let hit = app.doc.pages[page_idx]
        .elements
        .iter()
        .rev()
        .find(|e| point_in_element(e, pointer, &to_screen, zoom))
        .map(|e| e.id);

    let shift_held = shift;

    match hit {
        Some(id) => {
            if app.is_selected(id) && app.selection.len() > 1 && !shift_held {
                // Bereits ausgewählt in einer Multi-Selection → alle verschieben.
                let starts: Vec<(u64, f32, f32)> = app.doc.pages[page_idx]
                    .elements
                    .iter()
                    .filter(|e| app.is_selected(e.id))
                    .map(|e| (e.id, e.x, e.y))
                    .collect();
                app.interaction = Interaction::DragBodies { start_pointer: pointer, starts };
            } else if shift_held {
                // Shift+Klick → Auswahl umschalten.
                app.toggle_selected(id);
                app.crop_mode = false;
            } else {
                // Einzelnes Objekt auswählen und verschieben.
                let xy = app.doc.pages[page_idx]
                    .elements
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| (e.x, e.y))
                    .unwrap();
                app.select_only(id);
                app.crop_mode = false;
                app.interaction = Interaction::DragBodies {
                    start_pointer: pointer,
                    starts: vec![(id, xy.0, xy.1)],
                };
            }
        }
        None => {
            // Leere Fläche → Auswahl-Rechteck starten.
            if !shift_held {
                app.clear_selection();
            }
            app.crop_mode = false;
            app.interaction = Interaction::SelectionBox { start: pointer };
        }
    }
}

fn resize_to_pointer(
    el: &mut Element,
    anchor: Pos2,
    rotation: f32,
    pointer_page: Vec2,
    shift: bool,
    start_aspect: f32,
) {
    let pointer_page = Pos2::new(pointer_page.x, pointer_page.y);
    let dv = pointer_page - anchor;
    let local = rotate_vec(dv, -rotation);
    let (mut new_w, mut new_h) = (local.x.abs().max(2.0), local.y.abs().max(2.0));
    if shift && start_aspect > 0.0 {
        // An der ursprünglichen Seiteverhältnis festhalten: die kleinere
        // Achse dominiert nicht – stattdessen diejenige nehmen, die weiter
        // vom Anker entfernt ist, und die andere ableiten.
        if new_w / start_aspect.max(0.01) > new_h {
            new_h = new_w / start_aspect.max(0.01);
        } else {
            new_w = new_h * start_aspect;
        }
    }
    let half_local = Vec2::new(
        if local.x >= 0.0 { 1.0 } else { -1.0 } * new_w / 2.0,
        if local.y >= 0.0 { 1.0 } else { -1.0 } * new_h / 2.0,
    );
    let center_offset = rotate_vec(half_local, rotation);
    let new_center = anchor + center_offset;
    el.w = new_w;
    el.h = new_h;
    el.x = new_center.x - new_w / 2.0;
    el.y = new_center.y - new_h / 2.0;
}

fn crop_to_pointer(
    el: &mut Element,
    edge: CropEdge,
    start: crate::model::Crop,
    pointer_page: Vec2,
) {
    let center = Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0);
    let local = world_to_local(center, el.rotation, Pos2::new(pointer_page.x, pointer_page.y));
    let u = ((local.x + el.w / 2.0) / el.w).clamp(0.0, 1.0);
    let v = ((local.y + el.h / 2.0) / el.h).clamp(0.0, 1.0);
    let min = 0.02;
    let mut crop = el.crop;
    match edge {
        CropEdge::Right => {
            crop.w = (u - start.x).clamp(min, start.x + start.w - start.x);
            crop.x = start.x;
        }
        CropEdge::Left => {
            let max = start.x + start.w - min;
            crop.x = u.clamp(0.0, max);
            crop.w = start.x + start.w - crop.x;
        }
        CropEdge::Bottom => {
            crop.h = (v - start.y).clamp(min, start.y + start.h - start.y);
            crop.y = start.y;
        }
        CropEdge::Top => {
            let max = start.y + start.h - min;
            crop.y = v.clamp(0.0, max);
            crop.h = start.y + start.h - crop.y;
        }
    }
    el.crop = crop.clamp();
}
