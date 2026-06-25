//! Die Zeichenfläche: Rendern der Seite und aller Objekte sowie die gesamte
//! Maus-Interaktion (verschieben, skalieren, drehen, zuschneiden).

use egui::{
    epaint::{Mesh, Vertex}, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2,
};

use crate::app::{CropEdge, EditorApp, Interaction};
use crate::geometry::{local_corners, local_to_world, rotate_vec, world_to_local};
use crate::model::{page_size_pt, Element, ElementKind};
use crate::store::ImageStore;

/// Kopie der aktiven Interaktion, damit `&app` nicht während der Bearbeitung
/// gebunden bleibt.
#[derive(Clone, Copy)]
enum Active {
    Drag(u64, Pos2, (f32, f32)),
    Resize(u64, Pos2, f32, f32),
    Rotate(u64),
    Crop(u64, CropEdge, crate::model::Crop),
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

    // --- Zoom & Verschiebung ---
    if ctrl && scroll.y.abs() > 0.0 {
        if let Some(cur) = pointer {
            if rect.contains(cur) {
                let zoom = app.view.zoom;
                let base = rect.min;
                let pan = app.view.pan;
                let page_under =
                    Vec2::new((cur.x - base.x - pan.x) / zoom, (cur.y - base.y - pan.y) / zoom);
                let factor = if scroll.y > 0.0 { 1.1 } else { 0.9 };
                let new_zoom = (zoom * factor).clamp(0.1, 6.0);
                app.view.zoom = new_zoom;
                app.view.pan = Vec2::new(
                    cur.x - base.x - page_under.x * new_zoom,
                    cur.y - base.y - page_under.y * new_zoom,
                );
            }
        }
    } else if scroll != Vec2::ZERO {
        app.view.pan += scroll;
    }
    if middle_down {
        app.view.pan += delta;
    }

    let base = rect.min;
    let zoom = app.view.zoom;
    let pan = app.view.pan;
    let to_screen = |p: Pos2| base + pan + Vec2::new(p.x, p.y) * zoom;
    let to_page = |s: Pos2| Vec2::new((s.x - base.x - pan.x) / zoom, (s.y - base.y - pan.y) / zoom);

    // --- Seite zeichnen ---
    let (pw, ph) = page_size_pt(app.doc.format, app.doc.orientation);
    let page_rect_screen = Rect::from_min_size(to_screen(Pos2::ZERO), Vec2::new(pw, ph) * zoom);
    painter.rect_filled(
        page_rect_screen.translate(Vec2::new(4.0, 6.0)),
        2.0,
        Color32::from_black_alpha(35),
    );
    painter.rect_filled(page_rect_screen, 2.0, Color32::WHITE);

    // --- Elemente zeichnen ---
    let selected = app.selected;
    let crop_mode = app.crop_mode;
    let page_idx = app.page_index;
    if let Some(page) = app.doc.pages.get_mut(page_idx) {
        for el in page.elements.iter_mut() {
            draw_element(el, &mut app.images, ctx, &painter, &to_screen, zoom);
            if Some(el.id) == selected {
                draw_selection(el, &painter, &to_screen, zoom, crop_mode);
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

    // --- Interaktion beenden ---
    if primary_released {
        match &app.interaction {
            Interaction::DragBody { .. }
            | Interaction::Resize { .. }
            | Interaction::Rotate { .. }
            | Interaction::Crop { .. } => app.interaction = Interaction::None,
            _ => {}
        }
    }

    // --- Aktive Interaktion fortsetzen (Daten kopieren, dann app frei nutzen) ---
    let active = match &app.interaction {
        Interaction::DragBody { id, start_pointer, start_xy } => {
            Some(Active::Drag(*id, *start_pointer, *start_xy))
        }
        Interaction::Resize { id, anchor, rotation, start_aspect } => Some(Active::Resize(*id, *anchor, *rotation, *start_aspect)),
        Interaction::Rotate { id } => Some(Active::Rotate(*id)),
        Interaction::Crop { id, edge, start_crop } => Some(Active::Crop(*id, *edge, *start_crop)),
        _ => None,
    };
    if let (Some(a), Some(pointer)) = (active, pointer) {
        match a {
            Active::Drag(id, sp, xy) => {
                let dp = to_page(pointer) - to_page(sp);
                if let Some(el) = element_mut(app, page_idx, id) {
                    el.x = xy.0 + dp.x;
                    el.y = xy.1 + dp.y;
                    app.touch();
                }
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
        }
    }

    // --- Neue Interaktion starten ---
    let editing_active = app.editing.is_some();
    let pointer_in_canvas = pointer.map(|p| rect.contains(p)).unwrap_or(false);
    if matches!(app.interaction, Interaction::None)
        && primary_pressed
        && !editing_active
        && !middle_down
        && pointer_in_canvas
    {
        if let Some(pointer) = pointer {
            start_interaction(app, page_idx, pointer, to_screen, to_page, zoom);
        }
    }

    // --- Doppelklick → Text bearbeiten ---
    if double_clicked && !editing_active && pointer_in_canvas {
        if let Some(pointer) = pointer {
            for el in app.doc.pages[page_idx].elements.iter().rev() {
                if el.kind == ElementKind::Text && point_in_element(el, pointer, &to_screen, zoom) {
                    app.editing = Some((el.id, el.text.clone()));
                    app.edit_focus = true;
                    app.selected = Some(el.id);
                    break;
                }
            }
        }
    }

    // --- Tastatur ---
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Delete) && app.editing.is_none() {
            app.delete_selected();
        }
        if i.key_pressed(egui::Key::Escape) {
            app.crop_mode = false;
            app.editing = None;
            app.selected = None;
            app.interaction = Interaction::None;
        }
    });

    painter.text(
        rect.left_top() + Vec2::new(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        "Strg+Scroll = Zoom · Mittel-Taste = Verschieben · Entf = Löschen · Esc = Abbrechen",
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
            let font = FontId::proportional(el.font_size * zoom);
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
) {
    let sel = app.selected;
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

    // 4) Körper
    let hit = app.doc.pages[page_idx]
        .elements
        .iter()
        .rev()
        .find(|e| point_in_element(e, pointer, &to_screen, zoom))
        .map(|e| e.id);

    match hit {
        Some(id) => {
            let xy = app.doc.pages[page_idx]
                .elements
                .iter()
                .find(|e| e.id == id)
                .map(|e| (e.x, e.y))
                .unwrap();
            app.selected = Some(id);
            app.crop_mode = false;
            app.interaction = Interaction::DragBody { id, start_pointer: pointer, start_xy: xy };
        }
        None => {
            app.selected = None;
            app.crop_mode = false;
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
