//! Die Zeichenfläche: Rendern der Seite und aller Objekte sowie die gesamte
//! Maus-Interaktion (verschieben, skalieren, drehen, zuschneiden).

use egui::{
    epaint::{Mesh, Vertex},
    Color32, FontFamily, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2,
};

use crate::app::{CropEdge, EditorApp, Interaction};
use crate::geometry::{local_corners, local_to_world, rotate_vec, snap_angle_45, world_to_local};
use crate::model::{page_size_pt, Element, ElementKind, PageAlign, ScrollMode};
use crate::store::ImageStore;

/// Kopie der aktiven Interaktion, damit `&app` nicht während der Bearbeitung
/// gebunden bleibt.
#[derive(Clone)]
enum Active {
    Drag(Vec<(u64, f32, f32)>, Pos2),
    Resize(u64, Pos2, f32, f32),
    ResizeEdge(u64, CropEdge, f32, Pos2),
    Rotate(u64),
    Crop(u64, CropEdge, crate::model::Crop),
    SelectionBox(Pos2),
    /// Linien-Endpunkt ziehen (id, is_start).
    LineEndpoint(u64, bool),
    /// Pfad-Stützpunkt ziehen (id, Knotenindex).
    PathNode(u64, usize),
    /// Pfad-Kurvengriff ziehen (id, Knotenindex, Ausgangsgriff?).
    PathHandle(u64, usize, bool),
}

/// Farbe der Pfad-Werkzeuge und der Knotenbearbeitung.
const PATH_ACCENT: Color32 = Color32::from_rgb(230, 120, 40);
/// Anfassradius für Knoten und Griffe, in Bildschirmpixeln.
const NODE_GRAB: f32 = 9.0;

/// Trefferradius eines Pfads beim **Klick**, in Bildschirmpixeln.
///
/// Gleich großzügig wie bei der Linie: Eine 0,5-pt-Kontur ist sonst ein
/// Ziel von zwei Pixeln Breite.
const PATH_CLICK_TOL: f32 = 8.0;
/// Trefferradius beim **Doppelklick**, in Bildschirmpixeln.
///
/// Deutlich größer als beim Klick. Ein Doppelklick verlangt zwei Treffer
/// derselben Stelle in kurzer Folge — wer dabei die Kontur um ein paar Pixel
/// verfehlt, will trotzdem den Pfad und nicht ein Textfeld darüber.
const PATH_DBLCLICK_TOL: f32 = 16.0;
/// Trefferradius beim Doppelklick auf einen **bereits ausgewählten** Pfad.
///
/// Wer den Pfad angeklickt hat und dann doppelklickt, meint diesen Pfad. Die
/// Absicht ist eindeutig, also darf der Radius grob sein.
const PATH_DBLCLICK_TOL_SELECTED: f32 = 40.0;

pub fn show_canvas(app: &mut EditorApp, ctx: &egui::Context, ui: &mut egui::Ui) {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    // --- Eingaben ---
    let scroll = ui.input(|i| i.smooth_scroll_delta);
    let zoom_delta = ui.input(|i| i.zoom_delta());
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

    // --- Zoom (Ctrl+Scroll UND Pinch-to-Zoom) ---
    // egui's zoom_delta() liefert den Faktor für beide Gesten.
    if zoom_delta != 1.0 {
        if let Some(cur) = pointer {
            let zoom = app.view.zoom;
            let pan = app.view.pan;
            let align_x = compute_align_x(zoom);
            let page_under = Vec2::new(
                (cur.x - base.x - align_x - pan.x) / zoom,
                (cur.y - base.y - pan.y) / zoom,
            );
            let new_zoom = (zoom * zoom_delta).clamp(0.1, 6.0);
            let new_align_x = compute_align_x(new_zoom);
            app.view.zoom = new_zoom;
            app.view.pan = Vec2::new(
                cur.x - base.x - new_align_x - page_under.x * new_zoom,
                cur.y - base.y - page_under.y * new_zoom,
            );
        }
    }

    // --- Scroll / Pan (ohne Zoom) ---
    if zoom_delta == 1.0 && scroll != Vec2::ZERO {
        // --- Continuous-Scroll: Seitenwechsel am Ende ---
        //
        // `scroll.y` ist positiv, wenn der Inhalt nach unten wandert (der
        // Nutzer also nach oben scrollt) — die Vorzeichen hier müssen dazu
        // passen, sonst schluckt der Zweig die Scroll-Eingabe und die Ansicht
        // klemmt in eine Richtung fest.
        if app.settings.scroll_mode == ScrollMode::Continuous {
            app.view.pan.x += scroll.x;
            let page_h_screen = ph_pt * app.view.zoom;
            // Grenzen: oben steht der Seitenanfang bei 24 px, unten hört es
            // auf, wenn das Seitenende am unteren Rand angekommen ist. Passt
            // die Seite ganz auf den Bildschirm, fallen beide Grenzen zusammen.
            let min_pan_y = (rect.height() - 24.0 - page_h_screen).min(24.0);
            let target = app.view.pan.y + scroll.y;
            let now = ui.input(|i| i.time);
            // Eine Mausrad-Raste liefert ihr Delta über mehrere Frames verteilt.
            // Ohne diese Sperre würde ein einziges Rasten mehrere Seiten weit
            // springen, sobald die Seite komplett auf den Bildschirm passt.
            let may_flip = now - app.view.last_page_flip > 0.25;
            if target < min_pan_y {
                // Nach unten über das Seitenende hinaus.
                if may_flip && app.page_index + 1 < app.doc.pages.len() {
                    app.page_index += 1;
                    app.view.pan.y = 24.0;
                    app.view.last_page_flip = now;
                    app.clear_selection();
                } else {
                    app.view.pan.y = min_pan_y;
                }
            } else if target > 24.0 {
                // Nach oben über den Seitenanfang hinaus.
                if may_flip && app.page_index > 0 {
                    app.page_index -= 1;
                    app.view.pan.y = min_pan_y;
                    app.view.last_page_flip = now;
                    app.clear_selection();
                } else {
                    app.view.pan.y = 24.0;
                }
            } else {
                app.view.pan.y = target;
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
    let to_screen =
        |p: Pos2| base + Vec2::new(align_offset_x + pan.x, pan.y) + Vec2::new(p.x, p.y) * zoom;
    let to_page = |s: Pos2| {
        Vec2::new(
            (s.x - base.x - align_offset_x - pan.x) / zoom,
            (s.y - base.y - pan.y) / zoom,
        )
    };

    // --- Seite zeichnen ---
    let page_rect_screen =
        Rect::from_min_size(to_screen(Pos2::ZERO), Vec2::new(pw_pt, ph_pt) * zoom);
    painter.rect_filled(
        page_rect_screen.translate(Vec2::new(4.0, 6.0)),
        2.0,
        Color32::from_black_alpha(35),
    );
    painter.rect_filled(page_rect_screen, 2.0, Color32::WHITE);

    // --- Reflow: Auto-Höhe der Textboxen ---
    // Textboxen wachsen mit ihrem Inhalt. Das ist abgeleiteter Zustand, kein
    // Nutzer-Edit — deshalb weder `touch()` noch History.
    //
    // Bewusst mit scale = 1.0 und VOR dem Zeichnen: Früher passierte das
    // mitten im Zeichnen mit dem Zoomfaktor, wodurch die gespeicherte Höhe
    // vom Zoomstand abhing.
    reflow_text_heights(app, ctx);

    // --- Elemente zeichnen ---
    let selection: Vec<u64> = app.selection.clone();
    let crop_mode = app.crop_mode;
    let page_idx = app.page_index;
    // In der Knotenbearbeitung tritt der Auswahlrahmen zurück: Er läge sonst
    // über den Knoten, die gerade angefasst werden sollen.
    let node_edit = app.path_edit.filter(|e| selection.contains(&e.id));
    if let Some(page) = app.doc.pages.get_mut(page_idx) {
        for el in page.elements.iter_mut() {
            draw_element(el, &mut app.images, ctx, &painter, &to_screen, zoom);
            let editing_nodes = node_edit.map(|e| e.id) == Some(el.id);
            if selection.contains(&el.id) && !editing_nodes {
                if selection.len() == 1 {
                    draw_selection(el, &painter, &to_screen, zoom, crop_mode);
                } else {
                    draw_multi_selection_box(el, &painter, &to_screen, zoom);
                }
            }
            if editing_nodes {
                draw_path_nodes(
                    &painter,
                    el,
                    node_edit.and_then(|e| e.node),
                    pointer.map(|p| to_page(p).to_pos2()),
                    zoom,
                    &to_screen,
                );
            }
        }
    }
    // Ein Pfad, der nicht mehr ausgewählt ist, wird auch nicht mehr auf
    // Knotenebene bearbeitet.
    if app.path_edit.is_some() && node_edit.is_none() {
        app.path_edit = None;
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
            let out = ui.put(r, egui::TextEdit::multiline(buf).desired_width(r.width()));
            if app.edit_focus {
                out.request_focus();
                app.edit_focus = false;
            }
            let commit = (out.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl))
                || (primary_pressed && !r.contains(pointer.unwrap_or(Pos2::ZERO)));
            if commit {
                let (id, text) = app.editing.take().unwrap();
                // Erst prüfen, ob sich überhaupt etwas geändert hat, dann den
                // Snapshot des ALTEN Standes nehmen — sonst wäre der getippte
                // Text nicht mit Strg+Z rückgängig zu machen.
                let changed = app
                    .doc
                    .pages
                    .get(page_idx)
                    .and_then(|p| p.elements.iter().find(|e| e.id == id))
                    .map(|el| el.text != text)
                    .unwrap_or(false);
                if changed {
                    app.push_history();
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
    }

    // --- Snap-Führungslinien (Außenkanten + Mittelpunkt zur Seite) ---
    if app.snap_lines.vertical.is_some() || app.snap_lines.horizontal.is_some() {
        let color = Color32::from_rgb(80, 200, 120);
        let stroke = Stroke::new(1.5_f32, color);
        if let Some(vx) = app.snap_lines.vertical {
            let top = to_screen(Pos2::new(vx, 0.0));
            let bot = to_screen(Pos2::new(vx, ph_pt));
            painter.line_segment([top, bot], stroke);
            painter.circle_filled(top, 4.0, color);
            painter.circle_filled(bot, 4.0, color);
        }
        if let Some(hy) = app.snap_lines.horizontal {
            let left = to_screen(Pos2::new(0.0, hy));
            let right = to_screen(Pos2::new(pw_pt, hy));
            painter.line_segment([left, right], stroke);
            painter.circle_filled(left, 4.0, color);
            painter.circle_filled(right, 4.0, color);
        }
    }

    // --- Paste: Ghost + Preview Rendering ---
    if app.pasting && !app.clipboard.is_empty() {
        let ghost_fill = Color32::from_rgba_unmultiplied(100, 160, 230, 20);
        let ghost_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(100, 160, 230, 70));

        // Ghost an Originalpositionen.
        for (i, el) in app.clipboard.iter().enumerate() {
            let (gx, gy) = app.clip_origins[i];
            let r = Rect::from_min_size(to_screen(Pos2::new(gx, gy)), Vec2::new(el.w, el.h) * zoom);
            painter.rect_filled(r, 2.0, ghost_fill);
            painter.add(egui::epaint::RectShape::new(
                r,
                2.0,
                Color32::TRANSPARENT,
                ghost_stroke,
                egui::StrokeKind::Inside,
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

            let paste_ref = if snapped { ref_origin } else { (cp.x, cp.y) };

            let (p_fill, p_stroke) = if snapped {
                (
                    Color32::from_rgba_unmultiplied(80, 200, 120, 50),
                    Stroke::new(1.5_f32, Color32::from_rgb(80, 200, 120)),
                )
            } else {
                (
                    Color32::from_rgba_unmultiplied(40, 120, 220, 40),
                    Stroke::new(1.5_f32, Color32::from_rgb(40, 120, 220)),
                )
            };

            for (i, el) in app.clipboard.iter().enumerate() {
                let rel_x = app.clip_origins[i].0 - ref_origin.0;
                let rel_y = app.clip_origins[i].1 - ref_origin.1;
                let (px, py) = (paste_ref.0 + rel_x, paste_ref.1 + rel_y);
                let r =
                    Rect::from_min_size(to_screen(Pos2::new(px, py)), Vec2::new(el.w, el.h) * zoom);
                painter.rect_filled(r, 2.0, p_fill);
                painter.add(egui::epaint::RectShape::new(
                    r,
                    2.0,
                    Color32::TRANSPARENT,
                    p_stroke,
                    egui::StrokeKind::Inside,
                ));
                // Preview-Text rendern.
                if el.kind == ElementKind::Text && !el.text.is_empty() {
                    let font = FontId::new(el.font_size * zoom, crate::fonts::family_for(&el.font));
                    let color =
                        Color32::from_rgba_unmultiplied(el.color[0], el.color[1], el.color[2], 160);
                    let galley =
                        painter.layout(el.text.clone(), font, color, (el.w * zoom).max(1.0));
                    painter.galley(to_screen(Pos2::new(px, py)), galley, color);
                }
            }

            if snapped {
                let snap_color = Color32::from_rgb(80, 200, 120);
                painter.circle_filled(pt + Vec2::new(4.0, -14.0), 4.0, snap_color);
                painter.text(
                    pt + Vec2::new(12.0, -20.0),
                    egui::Align2::LEFT_BOTTOM,
                    "Snap",
                    FontId::proportional(12.0),
                    snap_color,
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
                    let additive = ui.input(|i| i.modifiers.shift || i.modifiers.ctrl);
                    if !additive {
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
                        if additive && app.is_selected(id) {
                            // Bereits ausgewählt → entfernen (Toggle).
                            if let Some(pos) = app.selection.iter().position(|&x| x == id) {
                                app.selection.swap_remove(pos);
                            }
                        } else if !app.is_selected(id) {
                            app.selection.push(id);
                        }
                    }
                }
                app.interaction = Interaction::None;
            }
            Interaction::DragBodies { .. }
            | Interaction::Resize { .. }
            | Interaction::ResizeEdge { .. }
            | Interaction::Rotate { .. }
            | Interaction::Crop { .. }
            | Interaction::LineEndpoint { .. }
            | Interaction::PathNode { .. }
            | Interaction::PathHandle { .. } => {
                app.interaction = Interaction::None;
                app.snap_lines = crate::app::SnapLines::default();
            }
            _ => {}
        }
    }

    // --- Aktive Interaktion fortsetzen ---
    let active = match &app.interaction {
        Interaction::DragBodies {
            start_pointer,
            starts,
        } => Some(Active::Drag(starts.clone(), *start_pointer)),
        Interaction::Resize {
            id,
            anchor,
            rotation,
            start_aspect,
        } => Some(Active::Resize(*id, *anchor, *rotation, *start_aspect)),
        Interaction::ResizeEdge {
            id,
            edge,
            rotation,
            anchor,
        } => Some(Active::ResizeEdge(*id, *edge, *rotation, *anchor)),
        Interaction::Rotate { id } => Some(Active::Rotate(*id)),
        Interaction::Crop {
            id,
            edge,
            start_crop,
        } => Some(Active::Crop(*id, *edge, *start_crop)),
        Interaction::SelectionBox { start } => Some(Active::SelectionBox(*start)),
        Interaction::LineEndpoint { id, is_start } => Some(Active::LineEndpoint(*id, *is_start)),
        Interaction::PathNode { id, index } => Some(Active::PathNode(*id, *index)),
        Interaction::PathHandle {
            id,
            index,
            outgoing,
        } => Some(Active::PathHandle(*id, *index, *outgoing)),
        _ => None,
    };
    if let (Some(a), Some(pointer)) = (active, pointer) {
        match a {
            Active::Drag(starts, sp) => {
                // Klick vs. Drag unterscheiden: erst ab deutlicher Cursor-
                // Bewegung (egui-Schwelle, ~6 px) wird verschoben. So wird
                // ein reiner Auswähl-Klick nicht als winziger Drag interpretiert
                // (Maus-Sensor-Jitter, Trackpad-Drift) und das Objekt bleibt
                // exakt an seiner Position.
                let dragging = ui
                    .input(|i| i.pointer.is_decidedly_dragging())
                    || (pointer.distance(sp) >= 6.0);
                let dp = if dragging {
                    to_page(pointer) - to_page(sp)
                } else {
                    Vec2::ZERO
                };
                for (id, sx, sy) in &starts {
                    if let Some(el) = element_mut(app, page_idx, *id) {
                        el.x = sx + dp.x;
                        el.y = sy + dp.y;
                    }
                }
                if dragging {
                    // --- Snapping: Außenkanten + Mittelpunkt zur Seite und
                    // zu allen anderen Objekten. Pro Achse (X/Y) wird der
                    // jeweils nächste Andockpunkt gesucht und angezogen:
                    //   • linke/obere Außenkante, Zentrum, rechte/untere Außenkante
                    //     des gezogenen Objekts →
                    //   • Seitenränder, Seitenmitte oder Außenkante/Mittelpunkt
                    //     eines anderen Objekts.
                    let snap_px = 8.0;
                    let (pw_pt_snap, ph_pt_snap) =
                        page_size_pt(app.doc.format, app.doc.orientation);
                    let ids: Vec<u64> = starts.iter().map(|(id, _, _)| *id).collect();
                    let (x_targets, y_targets) = collect_snap_targets(
                        &app.doc.pages[page_idx].elements,
                        &ids,
                        pw_pt_snap,
                        ph_pt_snap,
                    );
                    let mut best_x: Option<(f32, f32, f32)> = None; // (dist, offset, line)
                    let mut best_y: Option<(f32, f32, f32)> = None;
                    for (id, _, _) in &starts {
                        let Some(el) = app.doc.pages[page_idx]
                            .elements
                            .iter()
                            .find(|e| e.id == *id)
                        else {
                            continue;
                        };
                        // X-Quellen: linke Außenkante, Zentrum, rechte Außenkante.
                        for obj_val in [el.x, el.x + el.w / 2.0, el.x + el.w] {
                            if let Some(target) =
                                pick_snap(obj_val, &x_targets, snap_px, app.view.zoom)
                            {
                                let dist = (obj_val - target).abs() / app.view.zoom;
                                if best_x.map_or(true, |(d, _, _)| dist < d) {
                                    best_x = Some((dist, target - obj_val, target));
                                }
                            }
                        }
                        // Y-Quellen.
                        for obj_val in [el.y, el.y + el.h / 2.0, el.y + el.h] {
                            if let Some(target) =
                                pick_snap(obj_val, &y_targets, snap_px, app.view.zoom)
                            {
                                let dist = (obj_val - target).abs() / app.view.zoom;
                                if best_y.map_or(true, |(d, _, _)| dist < d) {
                                    best_y = Some((dist, target - obj_val, target));
                                }
                            }
                        }
                    }
                    let mut snap_v = None;
                    let mut snap_h = None;
                    if let Some((_, off, line)) = best_x {
                        if let Some(page) = app.doc.pages.get_mut(page_idx) {
                            for el in page.elements.iter_mut() {
                                if ids.contains(&el.id) {
                                    el.x += off;
                                }
                            }
                        }
                        snap_v = Some(line);
                    }
                    if let Some((_, off, line)) = best_y {
                        if let Some(page) = app.doc.pages.get_mut(page_idx) {
                            for el in page.elements.iter_mut() {
                                if ids.contains(&el.id) {
                                    el.y += off;
                                }
                            }
                        }
                        snap_h = Some(line);
                    }
                    // Snap-Visual im Canvas-Status speichern.
                    app.snap_lines.vertical = snap_v;
                    app.snap_lines.horizontal = snap_h;
                    app.touch();
                }
            }
            Active::Resize(id, anchor, rotation, start_aspect) => {
                // Snap-Parameter vor der Element-Borrow holen.
                let (pw_pt_snap, ph_pt_snap) =
                    page_size_pt(app.doc.format, app.doc.orientation);
                let zoom_snap = app.view.zoom;
                let (x_targets, y_targets) = collect_snap_targets(
                    &app.doc.pages[page_idx].elements,
                    &[id],
                    pw_pt_snap,
                    ph_pt_snap,
                );
                let mut snap_v = None;
                let mut snap_h = None;
                if let Some(el) = element_mut(app, page_idx, id) {
                    let shift = ui.input(|i| i.modifiers.shift);
                    resize_to_pointer(el, anchor, rotation, to_page(pointer), shift, start_aspect);
                    (snap_v, snap_h) =
                        snap_resize_edges(el, anchor, &x_targets, &y_targets, 8.0, zoom_snap);
                    app.touch();
                }
                app.snap_lines.vertical = snap_v;
                app.snap_lines.horizontal = snap_h;
            }
            Active::ResizeEdge(id, edge, rotation, anchor) => {
                let (pw_pt_snap, ph_pt_snap) =
                    page_size_pt(app.doc.format, app.doc.orientation);
                let zoom_snap = app.view.zoom;
                let (x_targets, y_targets) = collect_snap_targets(
                    &app.doc.pages[page_idx].elements,
                    &[id],
                    pw_pt_snap,
                    ph_pt_snap,
                );
                let mut snap_v = None;
                let mut snap_h = None;
                if let Some(el) = element_mut(app, page_idx, id) {
                    // Wer die Ober- oder Unterkante einer Textbox zieht, legt
                    // die Höhe bewusst selbst fest — ab jetzt gilt `valign`
                    // statt Auto-Höhe.
                    if el.kind == ElementKind::Text
                        && matches!(edge, CropEdge::Top | CropEdge::Bottom)
                    {
                        el.auto_height = false;
                    }
                    resize_edge_to_pointer(el, edge, anchor, rotation, to_page(pointer));
                    (snap_v, snap_h) =
                        snap_resize_edges(el, anchor, &x_targets, &y_targets, 8.0, zoom_snap);
                    app.touch();
                }
                app.snap_lines.vertical = snap_v;
                app.snap_lines.horizontal = snap_h;
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
                    Stroke::new(1.0_f32, Color32::from_rgb(40, 120, 220)),
                    egui::StrokeKind::Inside,
                ));
            }
            Active::LineEndpoint(id, is_start) => {
                if let Some(el) = element_mut(app, page_idx, id) {
                    // Aktuelle Endpunkte in Seitenkoordinaten berechnen.
                    let center = Pos2::new(el.x + el.w / 2.0, el.y);
                    let start = local_to_world(center, el.rotation, Vec2::new(-el.w / 2.0, 0.0));
                    let end = local_to_world(center, el.rotation, Vec2::new(el.w / 2.0, 0.0));
                    // Der nicht-gezogene Endpunkt bleibt fixiert.
                    let fixed = if is_start { end } else { start };
                    let ptr_page = to_page(pointer);
                    let mut target = Pos2::new(ptr_page.x, ptr_page.y);
                    // Shift → auf 45°-Raster schnappen (H/V + Diagonalen).
                    let shift = ui.input(|i| i.modifiers.shift);
                    if shift {
                        target = snap_angle_45(fixed, target);
                    }
                    // Neue Geometrie aus (start, end) herleiten.
                    let (s, e) = if is_start {
                        (target, fixed)
                    } else {
                        (fixed, target)
                    };
                    let dx = e.x - s.x;
                    let dy = e.y - s.y;
                    let len = dx.hypot(dy).max(1.0);
                    el.w = len;
                    el.rotation = dy.atan2(dx).to_degrees();
                    el.x = (s.x + e.x) / 2.0 - len / 2.0;
                    el.y = (s.y + e.y) / 2.0;
                    app.touch();
                }
            }
            Active::PathNode(id, index) => {
                let mut target = to_page(pointer).to_pos2();
                if ui.input(|i| i.modifiers.shift) {
                    // Shift: waagrecht/senkrecht/diagonal zum Vorgängerknoten.
                    // Beim ersten Knoten eines geschlossenen Pfads ist das der
                    // letzte — sonst wirkte Shift ausgerechnet dort nicht.
                    if let Some(prev) = element(app, page_idx, id).and_then(|el| {
                        let nodes = crate::geometry::path_nodes(el);
                        let prev_idx = match index.checked_sub(1) {
                            Some(i) => Some(i),
                            None if el.path_closed => nodes.len().checked_sub(1),
                            None => None,
                        };
                        prev_idx.and_then(|i| nodes.get(i).map(|n| n.anchor))
                    }) {
                        target = snap_angle_45(prev, target);
                    }
                }
                if let Some(el) = element_mut(app, page_idx, id) {
                    crate::geometry::move_node(el, index, target);
                    app.touch();
                }
            }
            Active::PathHandle(id, index, outgoing) => {
                // Alt bricht die Symmetrie auf — für Spitzen und Knicke.
                let mirror = !ui.input(|i| i.modifiers.alt);
                let target = to_page(pointer).to_pos2();
                if let Some(el) = element_mut(app, page_idx, id) {
                    crate::geometry::move_handle(el, index, outgoing, target, mirror);
                    app.touch();
                }
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

    // --- Linien-Werkzeug ---
    let drawing = app.tool != crate::app::Tool::Select;
    if app.tool == crate::app::Tool::Line {
        let shift = ui.input(|i| i.modifiers.shift);
        // Vorschau rendern wenn Startpunkt gesetzt.
        let has_start = app.line_drawing.is_some();
        if has_start {
            if let (Some(start), Some(pt)) = (app.line_drawing, pointer) {
                let start_pos = Pos2::new(start.0, start.1);
                let end_page = if shift {
                    snap_angle_45(start_pos, to_page(pt).to_pos2())
                } else {
                    to_page(pt).to_pos2()
                };
                let sp = to_screen(start_pos);
                let ep = to_screen(end_page);
                let color = if shift {
                    Color32::from_rgb(80, 200, 120)
                } else {
                    Color32::from_rgb(40, 120, 220)
                };
                painter.line_segment([sp, ep], Stroke::new(2.0_f32, color));
                painter.circle_filled(sp, 5.0, color);
                painter.circle_stroke(ep, 5.0, Stroke::new(1.5_f32, color));
                if shift {
                    painter.text(
                        ep + Vec2::new(10.0, -6.0),
                        egui::Align2::LEFT_BOTTOM,
                        "45°",
                        FontId::proportional(11.0),
                        color,
                    );
                }
            }
        } else if let Some(pt) = pointer {
            painter.circle_stroke(pt, 5.0, Stroke::new(1.5_f32, Color32::from_rgb(40, 120, 220)));
        }
        // Klick-Handling.
        if primary_pressed && pointer_in_canvas && !click_on_ui {
            if let Some(pt) = pointer {
                let p = to_page(pt);
                match app.line_drawing {
                    None => {
                        // Erster Klick → Startpunkt setzen.
                        app.line_drawing = Some((p.x, p.y));
                        app.status =
                            String::from("Linie: Klicke den Endpunkt (Shift = 45°-Raster).");
                    }
                    Some(current) => {
                        // Zweiter Klick → Linie erstellen (mit Snap wenn Shift).
                        let start_pos = Pos2::new(current.0, current.1);
                        let end_pos = if shift {
                            snap_angle_45(start_pos, p.to_pos2())
                        } else {
                            p.to_pos2()
                        };
                        app.add_line_between((start_pos.x, start_pos.y), (end_pos.x, end_pos.y));
                        app.status = String::from(
                            "Linie: Klicke den nächsten Startpunkt (Esc zum Beenden).",
                        );
                    }
                }
            }
        }
    }

    // --- Pfad- und Freihand-Werkzeug ---
    if matches!(
        app.tool,
        crate::app::Tool::Pen | crate::app::Tool::Freehand
    ) {
        path_tool(
            app,
            ui,
            &painter,
            pointer,
            &to_screen,
            &to_page,
            primary_pressed,
            primary_released,
            double_clicked,
            pointer_in_canvas && !click_on_ui,
        );
    }

    // --- Neue Interaktion starten (nur wenn nicht am Pasten/Zeichnen) ---
    if !app.pasting
        && !drawing
        && matches!(app.interaction, Interaction::None)
        && primary_pressed
        && !editing_active
        && !middle_down
        && pointer_in_canvas
        && !click_on_ui
    {
        if let Some(pointer) = pointer {
            let additive = ui.input(|i| i.modifiers.shift || i.modifiers.ctrl);
            start_interaction(
                app,
                page_idx,
                pointer,
                to_screen,
                to_page,
                zoom,
                additive,
                ctx,
            );
        }
    }

    // --- Hover-Cursor: zeigen, was ein Klick hier täte ---
    //
    // Ohne Rückmeldung ist jedes Objekt ein Ratespiel — eine dünne Kontur
    // sowieso, aber auch ein ungefüllter Rahmen oder ein Text mit viel Luft
    // in der Box. Der Cursor sagt vorher, ob man trifft, und was passiert.
    if !app.pasting && pointer_in_canvas && !click_on_ui {
        if let Some(pt) = pointer {
            if app.tool == crate::app::Tool::Select {
                if let Some(icon) = hover_cursor(app, page_idx, pt, &to_screen, zoom, ctx) {
                    ctx.set_cursor_icon(icon);
                }
            } else {
                // Zeichenwerkzeug aktiv: Das Fadenkreuz sagt, dass hier gesetzt
                // und nicht ausgewählt wird.
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
    }

    // --- Doppelklick in der Knotenbearbeitung → Knoten einfügen ---
    //
    // Muss vor der Textbearbeitung stehen: Sonst legte ein Doppelklick neben
    // die Linie ein Textfeld an, statt den Pfad zu ergänzen.
    let mut node_inserted = false;
    if !app.pasting && !drawing && double_clicked && pointer_in_canvas && !click_on_ui {
        if let (Some(edit), Some(pt)) = (app.path_edit, pointer) {
            let p = to_page(pt).to_pos2();
            // Toleranz in Seitenkoordinaten: am Bildschirm konstant, egal
            // wie weit hinein- oder herausgezoomt ist. Großzügig sein kostet
            // nichts — eingefügt wird ohnehin auf der Kontur, nicht dort, wo
            // der Zeiger stand.
            let tol = PATH_DBLCLICK_TOL / zoom;
            let hit = element(app, page_idx, edit.id)
                .and_then(|el| crate::geometry::path_nearest(el, p))
                .filter(|h| h.dist <= tol);
            if let Some(hit) = hit {
                app.push_history();
                let inserted = element_mut(app, page_idx, edit.id)
                    .and_then(|el| crate::geometry::insert_node(el, &hit));
                if let Some(index) = inserted {
                    app.path_edit = Some(crate::app::PathEdit {
                        id: edit.id,
                        node: Some(index),
                    });
                    app.status = String::from("Knoten eingefügt.");
                    app.touch();
                    node_inserted = true;
                }
            }
        }
    }

    // --- Doppelklick → Text bearbeiten (nur wenn nicht am Pasten) ---
    if !app.pasting
        && !drawing
        && !node_inserted
        && double_clicked
        && !editing_active
        && pointer_in_canvas
        && !click_on_ui
    {
        if let Some(pointer) = pointer {
            // Wenn ein bestehendes Text-Objekt getroffen wird → bearbeiten.
            // Nur Klicks auf den tatsächlichen Text-Glyphen zählen (starker
            // Treffer), damit ein Doppelklick in eine leere Ecke der Text-Box
            // ein neues Textfeld erzeugt statt ein fernliegendes zu öffnen.
            let mut hit_text = None;
            for el in app.doc.pages[page_idx].elements.iter().rev() {
                if el.kind != ElementKind::Text {
                    continue;
                }
                match element_hit_strength(el, pointer, &to_screen, zoom, ctx) {
                    Some(s) if s > 0 => {
                        hit_text = Some(el.id);
                        break;
                    }
                    _ => {}
                }
            }
            // Doppelklick auf einen Pfad → Knoten bearbeiten.
            //
            // Das ist die Geste, die man an einer Kurve zuerst probiert. Sie
            // muss großzügig treffen: Danebengehen kostet hier nicht nichts,
            // sondern legt ein Textfeld über den Pfad — genau das, was man am
            // wenigsten wollte. Deshalb ein spürbar größerer Radius als beim
            // einfachen Klick, und für den bereits ausgewählten Pfad ein noch
            // größerer: Wer ihn angeklickt hat und dann doppelklickt, meint
            // ihn.
            let hit_path = if hit_text.is_none() {
                let selected = app.selected_path();
                let near_selected = selected.filter(|&id| {
                    element(app, page_idx, id).is_some_and(|el| {
                        path_hit_within(
                            el,
                            pointer,
                            &to_screen,
                            zoom,
                            PATH_DBLCLICK_TOL_SELECTED,
                        )
                    })
                });
                near_selected.or_else(|| {
                    app.doc.pages[page_idx]
                        .elements
                        .iter()
                        .rev()
                        .find(|el| {
                            path_hit_within(el, pointer, &to_screen, zoom, PATH_DBLCLICK_TOL)
                        })
                        .map(|el| el.id)
                })
            } else {
                None
            };

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
            } else if let Some(id) = hit_path {
                // Wird dieser Pfad schon bearbeitet, war der Doppelklick nur
                // knapp neben einem Segment. Dann nichts anfassen — sonst
                // verlöre man die Knotenauswahl für eine Geste, die gar keine
                // Wirkung haben sollte.
                let already = app.path_edit.is_some_and(|e| e.id == id);
                if !already {
                    app.select_only(id);
                    app.crop_mode = false;
                    app.interaction = Interaction::None;
                    app.path_edit = Some(crate::app::PathEdit { id, node: None });
                    app.status = String::from(
                        "Knoten bearbeiten: ziehen zum Verschieben · Doppelklick auf ein Segment \
                         fügt einen Knoten ein · Knoten anklicken und Entf löscht ihn · \
                         Alt+Klick schaltet Ecke/Kurve um · N oder Esc beendet.",
                    );
                }
            } else {
                // Leere Fläche → neues Textfeld, linke-obere Ecke an der Cursor-Position.
                let p = to_page(pointer);
                app.add_text(Some((false, p.x, p.y)));
            }
        }
    }

    // --- Tastatur: Copy/Paste ---
    // Auf Native: egui wandelt Ctrl+C/V in Event::Copy/Event::Paste um.
    // Auf Web: oft nur als Event::Key sichtbar. Wir prüfen BEIDE Wege.
    //
    // `typing` deckt JEDES fokussierte Texteingabefeld ab — nicht nur die
    // Canvas-Bearbeitung (`app.editing`). Ohne diese Prüfung würde Strg+C im
    // Eigenschaften-Panel Objekte statt Text kopieren und Strg+V im
    // JSON-Editor gar nicht ankommen, weil das Event hier weggefiltert wird.
    let typing = ctx.wants_keyboard_input();
    let mut do_copy = false;
    let mut do_paste = false;
    ctx.input_mut(|i| {
        i.events.retain(|e| {
            match e {
                egui::Event::Copy => {
                    if !typing {
                        do_copy = true;
                        false
                    } else {
                        true
                    }
                }
                egui::Event::Paste(_) => {
                    if !typing {
                        do_paste = true;
                        false
                    } else {
                        true
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if !typing {
                        match *key {
                            egui::Key::C if modifiers.ctrl => {
                                do_copy = true;
                                false // konsumieren
                            }
                            egui::Key::V if modifiers.ctrl => {
                                do_paste = true;
                                false
                            }
                            _ => true,
                        }
                    } else {
                        true
                    }
                }
                _ => true,
            }
        });
    });
    if do_copy && !app.selection.is_empty() {
        app.copy_selection();
    }
    if do_paste {
        // Priorität 1: Bild aus der Zwischenablage (Native).
        if let Some(img_bytes) = crate::io::poll_clipboard_image() {
            app.add_image_from_bytes(img_bytes, None);
        }
        // Priorität 2: BoxDoc-Objekte aus der eigenen Zwischenablage.
        else if !app.clipboard.is_empty() {
            app.start_paste();
        }
    }

    // Undo/Redo über rohe Key-Events (egui konsumiert Ctrl+Z sonst).
    let (do_undo, do_redo) = ctx.input(|i| {
        let mut u = false;
        let mut r = false;
        for event in &i.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                match *key {
                    egui::Key::Z if modifiers.ctrl && !modifiers.shift => u = true,
                    egui::Key::Z if modifiers.ctrl && modifiers.shift => r = true,
                    egui::Key::Y if modifiers.ctrl => r = true,
                    _ => {}
                }
            }
        }
        (u, r)
    });
    if (do_undo || do_redo) && app.editing.is_none() {
        if do_undo {
            app.undo();
        }
        if do_redo {
            app.redo();
        }
    }

    // Globale Tasten wirken nur, wenn KEIN Textfeld den Fokus hat. Sonst würde
    // z. B. Entf im JSON-Editor das ausgewählte Objekt löschen.
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Delete) && !typing && !app.pasting {
            // In der Knotenbearbeitung löscht Entf den Knoten, nicht den
            // ganzen Pfad — sonst wäre ein Vertipper der Verlust der Arbeit.
            let node_deleted = match app.path_edit {
                Some(crate::app::PathEdit {
                    id,
                    node: Some(index),
                }) => {
                    // Erst prüfen, dann den Schnappschuss nehmen: Ein
                    // Undo-Schritt, der nichts rückgängig macht, ist nur ein
                    // Strg+Z, das scheinbar nicht wirkt.
                    let removable = element(app, page_idx, id)
                        .map(|el| {
                            let min = if el.path_closed { 3 } else { 2 };
                            el.points.len() > min && index < el.points.len()
                        })
                        .unwrap_or(false);
                    if removable {
                        app.push_history();
                        if let Some(el) = element_mut(app, page_idx, id) {
                            crate::geometry::remove_node(el, index);
                        }
                        app.path_edit = Some(crate::app::PathEdit { id, node: None });
                        app.status = String::from("Knoten gelöscht.");
                        app.touch();
                    } else {
                        app.status =
                            String::from("Der letzte Knoten eines Pfads lässt sich nicht löschen.");
                    }
                    true
                }
                _ => false,
            };
            if !node_deleted {
                app.delete_selected();
            }
        }
        if i.key_pressed(egui::Key::Escape) {
            // Von innen nach außen abbrechen: erst der laufende Vorgang,
            // dann das Werkzeug, dann die Auswahl.
            if app.pasting {
                app.pasting = false;
                app.status = String::from("Einfügen abgebrochen.");
            } else if app.path_draft.is_some() {
                app.path_draft = None;
                app.status = String::from("Pfad abgebrochen.");
            } else if app.line_drawing.is_some() {
                app.line_drawing = None;
                app.status = String::from("Linie abgebrochen.");
            } else if app.tool != crate::app::Tool::Select {
                app.set_tool(crate::app::Tool::Select);
            } else if app.path_edit.is_some() {
                app.path_edit = None;
                app.status = String::from("Knotenbearbeitung beendet.");
            } else {
                app.crop_mode = false;
                app.editing = None;
                app.clear_selection();
                app.interaction = Interaction::None;
            }
        }
        // Enter beendet einen offenen Pfad, ohne ihn zu schließen.
        if i.key_pressed(egui::Key::Enter) && !typing && app.path_draft.is_some() {
            app.finish_path(false);
        }
        // Rücktaste nimmt den zuletzt gesetzten Knoten zurück.
        if i.key_pressed(egui::Key::Backspace) && !typing {
            if let Some(draft) = app.path_draft.as_mut() {
                draft.nodes.pop();
                if draft.nodes.is_empty() {
                    app.path_draft = None;
                    app.status = String::from("Pfad abgebrochen.");
                }
            }
        }

        // Werkzeugwahl — nur ohne Strg/Cmd. Sonst würde Strg+P neben dem
        // Drucken auch das Pfad-Werkzeug aktivieren, Strg+N neben dem neuen
        // Dokument die Knotenbearbeitung, Strg+V neben dem Einfügen das
        // Auswahl-Werkzeug.
        if !typing && !app.pasting && !i.modifiers.command && !i.modifiers.alt {
            if i.key_pressed(egui::Key::L) {
                app.set_tool(crate::app::Tool::Line);
            }
            if i.key_pressed(egui::Key::P) {
                app.set_tool(crate::app::Tool::Pen);
            }
            if i.key_pressed(egui::Key::F) {
                app.set_tool(crate::app::Tool::Freehand);
            }
            if i.key_pressed(egui::Key::V) {
                app.set_tool(crate::app::Tool::Select);
            }
            // N schaltet die Knotenbearbeitung des ausgewählten Pfads um.
            if i.key_pressed(egui::Key::N) {
                app.toggle_path_edit();
            }
        }

        // Pfeiltasten: Auswahl verschieben (1 pt pro Druck, 10 pt mit Shift).
        if !typing && !app.selection.is_empty() {
            let step = if i.modifiers.shift { 10.0 } else { 1.0 };
            let mut dx = 0.0;
            let mut dy = 0.0;
            if i.key_pressed(egui::Key::ArrowLeft) {
                dx = -step;
            }
            if i.key_pressed(egui::Key::ArrowRight) {
                dx = step;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                dy = -step;
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                dy = step;
            }
            if dx != 0.0 || dy != 0.0 {
                // Undo-Snapshot, aber zusammengefasst: eine zusammenhängende
                // Serie von Pfeiltasten-Drücken ist EIN Undo-Schritt. Sonst
                // müsste man 40x Strg+Z drücken, um ein Verschieben um 40 pt
                // rückgängig zu machen.
                const NUDGE_COALESCE_S: f64 = 0.6;
                if i.time - app.last_nudge_time > NUDGE_COALESCE_S {
                    app.push_history();
                }
                app.last_nudge_time = i.time;

                let ids = app.selection.clone();
                if let Some(page) = app.doc.pages.get_mut(app.page_index) {
                    for el in page.elements.iter_mut() {
                        if ids.contains(&el.id) {
                            el.x += dx;
                            el.y += dy;
                        }
                    }
                }
                app.touch();
            }
        }
    });

    painter.text(
        rect.left_top() + Vec2::new(8.0, 6.0),
        egui::Align2::LEFT_TOP,
        "Strg+Z = Rückgängig · Strg+Y = Wiederherstellen · Strg+C/V = Kopieren/Einfügen · Entf = Löschen · Esc = Abbrechen",
        FontId::proportional(11.0),
        Color32::from_gray(150),
    );
}

fn element_mut<'a>(app: &'a mut EditorApp, page_idx: usize, id: u64) -> Option<&'a mut Element> {
    app.doc
        .pages
        .get_mut(page_idx)?
        .elements
        .iter_mut()
        .find(|e| e.id == id)
}

/// Kehrt `to_screen` um: Bildschirm- zurück in Seitenkoordinaten.
///
/// `element_hit_strength` bekommt nur die Hinrichtung übergeben. Die Abbildung
/// ist eine Verschiebung mit gleichmäßiger Skalierung — ihr Bild des Ursprungs
/// genügt daher, um sie exakt zu invertieren.
fn screen_to_page(to_screen: &impl Fn(Pos2) -> Pos2, zoom: f32, p: Pos2) -> Pos2 {
    let origin = to_screen(Pos2::ZERO);
    Pos2::new((p.x - origin.x) / zoom, (p.y - origin.y) / zoom)
}

fn element<'a>(app: &'a EditorApp, page_idx: usize, id: u64) -> Option<&'a Element> {
    app.doc
        .pages
        .get(page_idx)?
        .elements
        .iter()
        .find(|e| e.id == id)
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
            let color =
                Color32::from_rgba_unmultiplied(el.color[0], el.color[1], el.color[2], el.color[3]);
            let font = crate::text_layout::font_id_for(el, zoom);

            // Layout kommt aus dem gemeinsamen Modul — exakt dasselbe, das der
            // PDF-Export benutzt. Dadurch ist WYSIWYG keine Absichtserklärung,
            // sondern eine Eigenschaft der Architektur.
            let layout = ctx.fonts_mut(|f| crate::text_layout::layout(f, el, zoom));

            let origin = to_screen(Pos2::new(el.x, el.y));
            for laid in &layout.lines {
                if laid.text.is_empty() {
                    continue;
                }
                // Jede Zeile einzeln setzen: Position und Umbruch stehen bereits
                // fest, egui muss nur noch die Glyphen malen.
                let galley =
                    painter.layout_no_wrap(laid.text.clone(), font.clone(), color);
                // `painter.galley` erwartet die linke OBERE Ecke, das Layout
                // liefert die Grundlinie — Differenz ist der Ascent.
                let ascent = galley
                    .rows
                    .first()
                    .and_then(|r| r.row.glyphs.first().map(|g| g.font_ascent))
                    .unwrap_or(el.font_size * zoom * 0.8);
                let pos = origin
                    + Vec2::new(laid.x * zoom, laid.baseline_y * zoom - ascent);

                if el.underline {
                    let underline_y = origin.y + laid.baseline_y * zoom + 2.0;
                    painter.line_segment(
                        [
                            Pos2::new(origin.x + laid.x * zoom, underline_y),
                            Pos2::new(origin.x + (laid.x + laid.width) * zoom, underline_y),
                        ],
                        Stroke::new((el.font_size * zoom * 0.05).max(1.0), color),
                    );
                }
                painter.galley(pos, galley, color);
            }
        }
        ElementKind::Image => {
            if let Some(tex) = images.texture(el.id, ctx) {
                let uv = [
                    [el.crop.x, el.crop.y],
                    [el.crop.x + el.crop.w, el.crop.y],
                    [el.crop.x + el.crop.w, el.crop.y + el.crop.h],
                    [el.crop.x, el.crop.y + el.crop.h],
                ];
                let mut mesh = Mesh::default();
                mesh.texture_id = tex.id();
                for (i, corner) in crate::geometry::quad_corners(el).iter().enumerate() {
                    mesh.vertices.push(Vertex {
                        pos: to_screen(*corner),
                        uv: uv[i].into(),
                        color: Color32::WHITE,
                    });
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
        ElementKind::Rectangle => {
            let pts = screen_pts(&crate::geometry::rect_outline(el), to_screen);
            fill_and_stroke(painter, el, zoom, pts);
        }
        ElementKind::Ellipse => {
            let pts = screen_pts(&crate::geometry::ellipse_outline(el), to_screen);
            fill_and_stroke(painter, el, zoom, pts);
        }
        ElementKind::Line => {
            let (a, b) = crate::geometry::line_endpoints(el);
            let stroke = Stroke::new(el.stroke_width * zoom, stroke_color(el));
            painter.line_segment([to_screen(a), to_screen(b)], stroke);
        }
        ElementKind::Path => {
            let pts = screen_pts(&crate::geometry::path_outline(el), to_screen);
            if el.path_closed {
                fill_and_stroke(painter, el, zoom, pts);
            } else if pts.len() >= 2 {
                // Offener Zug: keine Fläche, und die Kontur darf nicht vom
                // Endpunkt zum Startpunkt zurücklaufen.
                let stroke = Stroke::new(el.stroke_width * zoom, stroke_color(el));
                if stroke.width > 0.0 && stroke.color.a() > 0 {
                    painter.add(Shape::line(pts, stroke));
                }
            }
        }
    }
}

/// Bildet Seitenkoordinaten auf Bildschirmkoordinaten ab.
fn screen_pts(pts: &[Pos2], to_screen: &impl Fn(Pos2) -> Pos2) -> Vec<Pos2> {
    pts.iter().map(|p| to_screen(*p)).collect()
}

fn fill_color(el: &Element) -> Color32 {
    Color32::from_rgba_unmultiplied(
        el.fill_color[0],
        el.fill_color[1],
        el.fill_color[2],
        el.fill_color[3],
    )
}

fn stroke_color(el: &Element) -> Color32 {
    Color32::from_rgba_unmultiplied(
        el.stroke_color[0],
        el.stroke_color[1],
        el.stroke_color[2],
        el.stroke_color[3],
    )
}

/// Füllt und umrandet einen geschlossenen Umriss.
///
/// Die Füllung wird trianguliert (`geometry::triangulate`), damit auch
/// konkave Umrisse stimmen — importierte Pfade sind es regelmäßig.
fn fill_and_stroke(painter: &egui::Painter, el: &Element, zoom: f32, pts: Vec<Pos2>) {
    if pts.len() < 3 {
        return;
    }
    let fill = fill_color(el);
    if fill.a() > 0 {
        let mut mesh = Mesh::default();
        for p in &pts {
            mesh.vertices.push(Vertex {
                pos: *p,
                uv: [0.0, 0.0].into(),
                color: fill,
            });
        }
        // Triangulieren statt Dreiecksfächer: Der Fächer füllt jede konkave
        // Einbuchtung mit zu — bei Rechteck und Ellipse fiel das nie auf, bei
        // importierten Pfaden sofort.
        for [a, b, c] in crate::geometry::triangulate(&pts) {
            mesh.indices.extend_from_slice(&[a, b, c]);
        }
        painter.add(Shape::mesh(mesh));
    }
    let stroke = Stroke::new(el.stroke_width * zoom, stroke_color(el));
    if stroke.width > 0.0 && stroke.color.a() > 0 {
        // Geschlossene Linie statt offenem Linienzug: nur so werden die Ecken
        // sauber verbunden und es entsteht keine Lücke am Start-/Endpunkt.
        painter.add(Shape::closed_line(pts, stroke));
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
    let pts: Vec<Pos2> = cl
        .iter()
        .map(|lc| local_to_world(center, el.rotation, *lc))
        .collect();
    painter.add(Shape::closed_line(
        pts,
        Stroke::new(1.5_f32, Color32::from_rgb(40, 120, 220)),
    ));
}

#[allow(clippy::too_many_arguments)]
/// Passt die Höhe aller Textelemente an ihren umgebrochenen Inhalt an.
///
/// Läuft mit `scale = 1.0`, damit die gespeicherte Höhe unabhängig vom Zoom
/// ist — sonst würde ein Dokument je nach Zoomstand mit anderen Werten
/// gespeichert.
fn reflow_text_heights(app: &mut EditorApp, ctx: &egui::Context) {
    let page_idx = app.page_index;
    let Some(page) = app.doc.pages.get_mut(page_idx) else {
        return;
    };
    ctx.fonts_mut(|fonts| {
        for el in page.elements.iter_mut() {
            // Nur Textboxen, und nur solche, deren Höhe der Nutzer nicht
            // selbst festgelegt hat.
            if el.kind != ElementKind::Text || !el.auto_height {
                continue;
            }
            let layout = crate::text_layout::layout(fonts, el, 1.0);
            let min_h = el.font_size * 1.2;
            el.h = layout.height.max(min_h);
        }
    });
}

fn draw_selection(
    el: &Element,
    painter: &egui::Painter,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    crop_mode: bool,
) {
    // Linien: nur Endpunkt-Griffe, keine Box.
    if el.kind == ElementKind::Line {
        let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y));
        let start = local_to_world(center, el.rotation, Vec2::new(-el.w * zoom / 2.0, 0.0));
        let end = local_to_world(center, el.rotation, Vec2::new(el.w * zoom / 2.0, 0.0));
        let handle_color = Color32::from_rgb(40, 120, 220);
        for p in [start, end] {
            painter.circle_filled(p, 6.0, Color32::WHITE);
            painter.circle_stroke(p, 6.0, Stroke::new(2.0_f32, handle_color));
        }
        return;
    }

    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let cl = local_corners(el.w * zoom, el.h * zoom);
    let pts: Vec<Pos2> = cl
        .iter()
        .map(|lc| local_to_world(center, el.rotation, *lc))
        .collect();

    painter.add(Shape::closed_line(
        pts.clone(),
        Stroke::new(1.5_f32, Color32::from_rgb(40, 120, 220)),
    ));

    if el.kind == ElementKind::Image && !crop_mode {
        let top_mid = local_to_world(center, el.rotation, Vec2::new(0.0, -el.h * zoom / 2.0));
        let grip = local_to_world(
            center,
            el.rotation,
            Vec2::new(0.0, -el.h * zoom / 2.0 - 24.0),
        );
        painter.line_segment(
            [top_mid, grip],
            Stroke::new(1.5_f32, Color32::from_rgb(40, 120, 220)),
        );
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
            painter.circle_stroke(*p, 5.0, Stroke::new(1.5_f32, handle_color));
        }
        // Kanten-Griffe (Mitten) für alle Flächenformen, nicht für Linien.
        if matches!(
            el.kind,
            ElementKind::Rectangle | ElementKind::Ellipse | ElementKind::Path
        ) {
            for p in edge_mid_positions(el, &to_screen, zoom) {
                painter.rect_filled(
                    Rect::from_center_size(p, Vec2::splat(8.0)),
                    1.5,
                    Color32::WHITE,
                );
                painter.rect_stroke(
                    Rect::from_center_size(p, Vec2::splat(8.0)),
                    1.5,
                    Stroke::new(1.5_f32, handle_color),
                    egui::StrokeKind::Inside,
                );
            }
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

/// Treffer-Stärke für die Auswahl-Priorisierung.
///
/// `None`  – Pointer liegt außerhalb der Bounding-Box (kein Treffer).
/// `Some(0)` – Schwacher Treffer: nur Bounding-Box, am Punkt selbst ist
///             kein sichtbarer Inhalt (z. B. Inneres eines Rahmens ohne
///             Füllung oder leerer Bereich eines Text-Elements).
/// `Some(1)` – Stark: Pointer liegt auf sichtbarem Inhalt (Text-Glyphen,
///             Füllung, Rahmenlinie, Bild, Linie).
///
/// Bei überlappenden Objekten gewinnt das oberste Element mit der höchsten
/// Stärke — so lässt sich z. B. Text durch Klick auf die Glyphen auswählen,
/// selbst wenn ein ungefüllter Rahmen darüber liegt.
/// Liegt der Zeiger höchstens `tol_screen` Pixel von der Pfadkontur entfernt?
///
/// Bei einem geschlossenen Pfad mit Füllung zählt auch das Innere.
///
/// `tol_screen` ist ein Bildschirmmaß und kommt **zusätzlich** zur halben
/// Strichstärke — die Toleranz bleibt beim Zoomen also gefühlt gleich, eine
/// dicke Kontur ist aber trotzdem großzügiger als eine dünne.
fn path_hit_within(
    el: &Element,
    pointer_screen: Pos2,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    tol_screen: f32,
) -> bool {
    if el.kind != ElementKind::Path {
        return false;
    }
    let outline = crate::geometry::path_outline(el);
    if outline.len() < 2 {
        return false;
    }
    let page = screen_to_page(to_screen, zoom, pointer_screen);
    if el.path_closed && el.fill_color[3] > 0 && crate::geometry::point_in_polygon(&outline, page) {
        return true;
    }
    let tol = (el.stroke_width * zoom / 2.0).max(0.0) + tol_screen;
    crate::geometry::path_nearest(el, page).is_some_and(|h| h.dist * zoom <= tol)
}

/// Liegt der Zeiger in der um `tol_screen` aufgeweiteten Hüllbox des Pfads?
///
/// Grobfilter für den schwachen Treffer: nah genug, dass der Pfad überhaupt
/// gemeint sein könnte, aber nicht auf der Kontur.
fn path_near_box(
    el: &Element,
    pointer_screen: Pos2,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    tol_screen: f32,
) -> bool {
    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let local = world_to_local(center, el.rotation, pointer_screen);
    let margin = (el.stroke_width * zoom / 2.0).max(0.0) + tol_screen;
    local.x.abs() <= el.w * zoom / 2.0 + margin && local.y.abs() <= el.h * zoom / 2.0 + margin
}

fn element_hit_strength(
    el: &Element,
    pointer_screen: Pos2,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    ctx: &egui::Context,
) -> Option<u8> {
    // Linien: Distanz zur Linie prüfen (mit Toleranz abhängig von Strichstärke).
    if el.kind == ElementKind::Line {
        let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y));
        let start = local_to_world(center, el.rotation, Vec2::new(-el.w * zoom / 2.0, 0.0));
        let end = local_to_world(center, el.rotation, Vec2::new(el.w * zoom / 2.0, 0.0));
        let ab = end - start;
        let ap = pointer_screen - start;
        let t = (ap.dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
        let closest = start + ab * t;
        let stroke_half = (el.stroke_width * zoom / 2.0).max(0.0);
        let tol = 8.0_f32.max(stroke_half + 3.0);
        return if pointer_screen.distance(closest) < tol {
            Some(1)
        } else {
            None
        };
    }

    // Pfad: gegen die Kontur prüfen, nicht gegen die Box.
    //
    // Muss **vor** der Hüllbox-Prüfung weiter unten stehen. Die verwirft
    // randlos alles außerhalb von w×h — und die Box eines flachen Pfads ist
    // nur Bruchteile eines Punktes hoch (`PATH_MIN_EXTENT`). Die Toleranz
    // hier kam damit früher nie an: Ein waagerechter Zug war auf ein
    // Viertelpixel genau zu treffen.
    if el.kind == ElementKind::Path {
        let hit = path_hit_within(el, pointer_screen, to_screen, zoom, PATH_CLICK_TOL);
        // Nahe an der Form, aber daneben: schwacher Treffer, damit ein
        // darunter liegendes Objekt gewinnen kann.
        if hit {
            return Some(1);
        }
        return if path_near_box(el, pointer_screen, to_screen, zoom, PATH_CLICK_TOL) {
            Some(0)
        } else {
            None
        };
    }

    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let local = world_to_local(center, el.rotation, pointer_screen);

    // Ellipse: (dx/rx)^2 + (dy/ry)^2 <= 1
    if el.kind == ElementKind::Ellipse {
        let rx = (el.w * zoom).max(1.0) / 2.0 + 4.0;
        let ry = (el.h * zoom).max(1.0) / 2.0 + 4.0;
        let dx = local.x / rx;
        let dy = local.y / ry;
        if dx * dx + dy * dy > 1.0 {
            return None;
        }
        return Some(1);
    }

    let hw = el.w * zoom / 2.0;
    let hh = el.h * zoom / 2.0;
    if local.x.abs() > hw || local.y.abs() > hh {
        return None;
    }

    match el.kind {
        ElementKind::Image => Some(1),
        ElementKind::Rectangle => {
            let has_fill = el.fill_color[3] > 0;
            if has_fill {
                return Some(1);
            }
            // Rahmen ohne Füllung: nur die Kontur ist ein starker Treffer,
            // das Innere ist schwach (damit darunter liegende Objekte, z. B.
            // Text, durch Klick ausgewählt werden können).
            let stroke_half = (el.stroke_width * zoom / 2.0).max(0.0);
            let tol = stroke_half + 4.0;
            let dist_to_edge = (hw - local.x.abs()).min(hh - local.y.abs());
            if dist_to_edge <= tol {
                Some(1)
            } else {
                Some(0)
            }
        }
        ElementKind::Text => {
            // Enge Bounding-Box um die tatsächlichen Glyphen berechnen.
            // Nur da ist das Text-Element "stark" treffbar; außerhalb
            // (aber innerhalb w×h) ist es nur ein schwacher Treffer.
            if el.text.trim().is_empty() {
                return Some(0);
            }
            let tight = text_tight_rect(el, to_screen, zoom, ctx);
            if tight.contains(pointer_screen) {
                Some(1)
            } else {
                Some(0)
            }
        }
        _ => Some(1),
    }
}

/// Berechnet das enge Rechteck (Bildschirmkoordinaten) des tatsächlichen
/// Text-Inhalts — also der Fläche, die von den Glyphen bedeckt ist, nicht
/// der vollen `w × h`-Box des Elements.
fn text_tight_rect(
    el: &Element,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    ctx: &egui::Context,
) -> Rect {
    let mut font = FontId::new(el.font_size * zoom, crate::fonts::family_for(&el.font));
    if el.font == "default" || el.font.is_empty() {
        if el.bold && el.italic {
            font = FontId::new(el.font_size * zoom, FontFamily::Name("Bold Italic".into()));
        } else if el.bold {
            font = FontId::new(el.font_size * zoom, FontFamily::Name("Bold".into()));
        } else if el.italic {
            font = FontId::new(el.font_size * zoom, FontFamily::Name("Italics".into()));
        }
    }
    let galley = ctx.fonts_mut(|f| {
        f.layout(el.text.clone(), font, Color32::TRANSPARENT, (el.w * zoom).max(1.0))
    });
    let galley_size = galley.size();

    let mut pos = to_screen(Pos2::new(el.x, el.y)) + Vec2::new(el.indent * zoom, 0.0);
    match el.align {
        crate::model::TextAlign::Left => {}
        crate::model::TextAlign::Center => pos.x += (el.w * zoom - galley_size.x) / 2.0,
        crate::model::TextAlign::Right => pos.x += el.w * zoom - galley_size.x,
    }
    match el.valign {
        crate::model::VAlign::Top => {}
        crate::model::VAlign::Middle => pos.y += (el.h * zoom - galley_size.y) / 2.0,
        crate::model::VAlign::Bottom => pos.y += el.h * zoom - galley_size.y,
    }
    Rect::from_min_size(pos, galley_size)
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

/// Mittelpunkte der vier Kanten in Bildschirmkoordinaten.
/// Reihenfolge: [Left, Right, Top, Bottom].
fn edge_mid_positions(el: &Element, to_screen: &impl Fn(Pos2) -> Pos2, zoom: f32) -> [Pos2; 4] {
    let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
    let hw = el.w * zoom / 2.0;
    let hh = el.h * zoom / 2.0;
    let local = [
        Vec2::new(-hw, 0.0),
        Vec2::new(hw, 0.0),
        Vec2::new(0.0, -hh),
        Vec2::new(0.0, hh),
    ];
    let mut out = [Pos2::ZERO; 4];
    for i in 0..4 {
        out[i] = local_to_world(center, el.rotation, local[i]);
    }
    out
}

/// Passendes Resize-Symbol für einen Griff, der vom Mittelpunkt aus in
/// Richtung `dir` (Bildschirmkoordinaten, y nach unten) liegt.
///
/// Über den Winkel statt über die Griff-Nummer, damit gedrehte Objekte
/// stimmen: Die untere Kante eines um 90° gedrehten Bildes zieht waagerecht.
fn resize_icon(dir: Vec2) -> egui::CursorIcon {
    let a = dir.y.atan2(dir.x).to_degrees().rem_euclid(180.0);
    if !(22.5..157.5).contains(&a) {
        egui::CursorIcon::ResizeHorizontal
    } else if a < 67.5 {
        egui::CursorIcon::ResizeNwSe
    } else if a < 112.5 {
        egui::CursorIcon::ResizeVertical
    } else {
        egui::CursorIcon::ResizeNeSw
    }
}

/// Welches Cursor-Symbol gehört an diese Stelle?
///
/// Spiegelt die Reihenfolge von [`start_interaction`]: erst Knoten, dann
/// Griffe, zuletzt der Körper. Was der Cursor zeigt, ist damit genau das, was
/// ein Klick hier auslöst.
///
/// `None` heißt „nichts Besonderes" — der Standardzeiger bleibt stehen.
fn hover_cursor(
    app: &EditorApp,
    page_idx: usize,
    pointer: Pos2,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    ctx: &egui::Context,
) -> Option<egui::CursorIcon> {
    let sel = app.primary();

    // 0) Knoten und Kurvengriffe der Knotenbearbeitung.
    if let Some(edit) = app.path_edit {
        if let Some(el) = element(app, page_idx, edit.id) {
            if path_node_at(el, pointer, to_screen).is_some() {
                return Some(egui::CursorIcon::Grab);
            }
        }
    }

    if let Some(id) = sel {
        if let Some(el) = element(app, page_idx, id) {
            let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));

            // 1) Crop-Kanten
            if app.crop_mode && el.kind == ElementKind::Image {
                for hp in crop_edge_handles(el, center, zoom).iter() {
                    if hp.distance(pointer) < 9.0 {
                        return Some(resize_icon(*hp - center));
                    }
                }
            }

            // 2) Linien-Endpunkte
            if el.kind == ElementKind::Line {
                let lc = to_screen(Pos2::new(el.x + el.w / 2.0, el.y));
                for s in [-1.0_f32, 1.0] {
                    let end = local_to_world(lc, el.rotation, Vec2::new(s * el.w * zoom / 2.0, 0.0));
                    if end.distance(pointer) < 9.0 {
                        return Some(egui::CursorIcon::Grab);
                    }
                }
            }

            // 3) Drehgriff
            if el.kind == ElementKind::Image && !app.crop_mode {
                let grip = local_to_world(
                    center,
                    el.rotation,
                    Vec2::new(0.0, -el.h * zoom / 2.0 - 24.0),
                );
                if grip.distance(pointer) < 9.0 {
                    return Some(egui::CursorIcon::Grab);
                }
            }

            // 4+5) Kanten- und Eckgriffe
            let boxed = el.kind != ElementKind::Line
                && !(app.crop_mode && el.kind == ElementKind::Image);
            if boxed {
                for gp in edge_mid_positions(el, to_screen, zoom)
                    .iter()
                    .chain(corner_positions(el, to_screen, zoom).iter())
                {
                    if gp.distance(pointer) < 9.0 {
                        return Some(resize_icon(*gp - center));
                    }
                }
            }
        }
    }

    // 6) Körper. Ausgewählt heißt: Ziehen verschiebt. Noch nicht ausgewählt
    //    heißt: Klicken wählt aus.
    let id = topmost_at(app, page_idx, pointer, to_screen, zoom, ctx)?;
    Some(if app.is_selected(id) {
        egui::CursorIcon::Move
    } else {
        egui::CursorIcon::PointingHand
    })
}

/// Oberstes Objekt unter dem Zeiger — dasjenige, das ein Klick auswählen würde.
///
/// Stärke: 1 = sichtbarer Inhalt (Glyphen, Füllung, Rahmen, Bild, Linie),
///         0 = nur Bounding-Box (z. B. Inneres eines ungefüllten Rahmens).
/// Bei Überlappung gewinnt das oberste Element mit der höchsten Stärke — so
/// lässt sich z. B. Text unter einem ungefüllten Rahmen durch Klick auf die
/// Glyphen auswählen, der Rahmen selbst durch Klick auf seine Kontur.
///
/// Klick **und** Hover-Cursor gehen durch diese eine Funktion. Getrennt
/// gerechnet würde der Cursor irgendwann etwas anderes versprechen, als der
/// Klick dann tut — und ein Cursor, der lügt, ist schlimmer als keiner.
fn topmost_at(
    app: &EditorApp,
    page_idx: usize,
    pointer: Pos2,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    ctx: &egui::Context,
) -> Option<u64> {
    let mut best_strength: i32 = -1;
    let mut best_id: Option<u64> = None;
    for el in app.doc.pages.get(page_idx)?.elements.iter().rev() {
        if let Some(s) = element_hit_strength(el, pointer, to_screen, zoom, ctx) {
            let s = s as i32;
            if s > best_strength {
                best_strength = s;
                best_id = Some(el.id);
            }
            // Sobald wir einen starken Treffer (s == 1) gefunden haben,
            // kann kein späteres (tiefer liegendes) Element mehr gewinnen.
            if best_strength >= 1 {
                break;
            }
        }
    }
    best_id
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
    ctx: &egui::Context,
) {
    let sel = app.primary();
    let crop_mode = app.crop_mode;

    // 0) Knotenbearbeitung — vor allem anderen.
    //
    // Die Knoten liegen auf der Kontur des Pfads und damit oft unter dessen
    // Auswahlrahmen. Käme der Rahmen zuerst dran, würde ein Klick auf einen
    // Knoten das Objekt verschieben statt den Knoten.
    if let Some(edit) = app.path_edit {
        let hit = element(app, page_idx, edit.id)
            .and_then(|el| path_node_at(el, pointer, &to_screen));
        if let Some((index, handle)) = hit {
            app.path_edit = Some(crate::app::PathEdit {
                id: edit.id,
                node: Some(index),
            });
            // Alt+Klick schaltet zwischen Ecke und glattem Übergang um,
            // statt zu ziehen.
            if ctx.input(|i| i.modifiers.alt) && handle.is_none() {
                app.push_history();
                if let Some(el) = element_mut(app, page_idx, edit.id) {
                    crate::geometry::toggle_node_smooth(el, index);
                }
                app.touch();
                return;
            }
            app.push_history();
            app.interaction = match handle {
                Some(outgoing) => Interaction::PathHandle {
                    id: edit.id,
                    index,
                    outgoing,
                },
                None => Interaction::PathNode {
                    id: edit.id,
                    index,
                },
            };
            return;
        }
    }

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
                            app.push_history();
                            app.interaction = Interaction::Crop {
                                id,
                                edge,
                                start_crop,
                            };
                            return;
                        }
                    }
                }
            }
        }
    }

    // 2) Linien-Endpunkte
    if let Some(id) = sel {
        if let Some(el) = app.doc.pages[page_idx].elements.iter().find(|e| e.id == id) {
            if el.kind == ElementKind::Line {
                let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y));
                let start = local_to_world(center, el.rotation, Vec2::new(-el.w * zoom / 2.0, 0.0));
                let end = local_to_world(center, el.rotation, Vec2::new(el.w * zoom / 2.0, 0.0));
                if start.distance(pointer) < 9.0 {
                    app.push_history();
                    app.interaction = Interaction::LineEndpoint { id, is_start: true };
                    return;
                }
                if end.distance(pointer) < 9.0 {
                    app.push_history();
                    app.interaction = Interaction::LineEndpoint {
                        id,
                        is_start: false,
                    };
                    return;
                }
            }
        }
    }

    // 3) Drehgriff
    if let Some(id) = sel {
        if let Some(el) = app.doc.pages[page_idx].elements.iter().find(|e| e.id == id) {
            if el.kind == ElementKind::Image && !crop_mode {
                let center = to_screen(Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0));
                let grip = local_to_world(
                    center,
                    el.rotation,
                    Vec2::new(0.0, -el.h * zoom / 2.0 - 24.0),
                );
                if grip.distance(pointer) < 9.0 {
                    app.push_history();
                    app.interaction = Interaction::Rotate { id };
                    return;
                }
            }
        }
    }

    // 4) Kanten (Größe in einer Richtung ändern — nicht für Linien)
    if let Some(id) = sel {
        if let Some(el) = app.doc.pages[page_idx].elements.iter().find(|e| e.id == id) {
            if el.kind != ElementKind::Line && !(crop_mode && el.kind == ElementKind::Image) {
                let edges = edge_mid_positions(el, &to_screen, zoom);
                // [Left, Right, Top, Bottom]
                for (i, ep) in edges.iter().enumerate() {
                    if ep.distance(pointer) < 9.0 {
                        let (edge, anchor_idx) = match i {
                            0 => (CropEdge::Left, 1),
                            1 => (CropEdge::Right, 0),
                            2 => (CropEdge::Top, 3),
                            _ => (CropEdge::Bottom, 2),
                        };
                        let a = to_page(edges[anchor_idx]);
                        let anchor = Pos2::new(a.x, a.y);
                        let rotation = el.rotation;
                        app.push_history();
                        app.interaction = Interaction::ResizeEdge {
                            id,
                            edge,
                            rotation,
                            anchor,
                        };
                        return;
                    }
                }
            }
        }
    }

    // 5) Ecken (Größe ändern — nicht für Linien)
    if let Some(id) = sel {
        if let Some(el) = app.doc.pages[page_idx].elements.iter().find(|e| e.id == id) {
            if el.kind != ElementKind::Line && !(crop_mode && el.kind == ElementKind::Image) {
                let corners = corner_positions(el, &to_screen, zoom);
                for (i, cp) in corners.iter().enumerate() {
                    if cp.distance(pointer) < 9.0 {
                        let opposite = corners[(i + 2) % 4];
                        let a = to_page(opposite);
                        let anchor = Pos2::new(a.x, a.y);
                        let rotation = el.rotation;
                        let start_aspect = if el.h != 0.0 { el.w / el.h } else { 1.0 };
                        app.push_history();
                        app.interaction = Interaction::Resize {
                            id,
                            anchor,
                            rotation,
                            start_aspect,
                        };
                        return;
                    }
                }
            }
        }
    }

    // 4) Körper (oberstes getroffenes Objekt mit der höchsten Treffer-Stärke).
    // Stärke: 1 = sichtbarer Inhalt (Glyphen, Füllung, Rahmen, Bild, Linie),
    //         0 = nur Bounding-Box (z. B. Inneres eines ungefüllten Rahmens).
    // Bei Überlappung gewinnt das oberste Element mit der höchsten Stärke —
    // so lässt sich z. B. Text unter einem ungefüllten Rahmen durch Klick
    // auf die Glyphen auswählen, der Rahmen selbst durch Klick auf seine
    // Kontur.
    let hit = topmost_at(app, page_idx, pointer, &to_screen, zoom, ctx);

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
                app.push_history();
                app.interaction = Interaction::DragBodies {
                    start_pointer: pointer,
                    starts,
                };
            } else if shift_held {
                // Shift/Ctrl+Klick → Auswahl umschalten (hinzufügen/entfernen).
                app.toggle_selected(id);
                app.crop_mode = false;
            } else {
                // Einzelnes Objekt auswählen und verschieben.
                let el = app.doc.pages[page_idx]
                    .elements
                    .iter()
                    .find(|e| e.id == id)
                    .unwrap();
                let xy = (el.x, el.y);
                let is_path = el.kind == ElementKind::Path;
                app.select_only(id);
                app.crop_mode = false;
                // Ein ausgewählter Pfad sieht aus wie jede andere Form. Ohne
                // diesen Hinweis findet niemand, dass an ihm die einzelnen
                // Punkte veränderbar sind.
                if is_path && app.path_edit.is_none() {
                    app.status = String::from(
                        "Pfad ausgewählt — Doppelklick oder N, um die Punkte zu bearbeiten.",
                    );
                }
                app.push_history();
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

/// Dockt die beweglichen Außenkanten eines Elements an die nächste Snap-
/// Zielposition (Seitenränder, Seitenmitten, Außenkanten/Mitten anderer
/// Objekte), wenn sie innerhalb der Snap-Schwelle (8 px Bildschirm) liegt.
/// `anchor` = Fixpunkt des Resizes (gegenüberliegende Ecke bzw. Kantenmitte);
/// eine Kante gilt als beweglich, wenn der Anker auf der gegenüberliegenden
/// Seite des Element-Zentrums liegt. Liefert die Positionen der Snap-Linien
/// (vertikal, horizontal) in Seitenkoordinaten — `None`, wenn nicht andockiert.
fn snap_resize_edges(
    el: &mut Element,
    anchor: Pos2,
    x_targets: &[f32],
    y_targets: &[f32],
    snap_px: f32,
    zoom: f32,
) -> (Option<f32>, Option<f32>) {
    let el_cx = el.x + el.w / 2.0;
    let el_cy = el.y + el.h / 2.0;
    let mut snap_v = None;
    let mut snap_h = None;

    // X-Achse: linke oder rechte Außenkante ist beweglich (je nach Anker-Seite).
    if anchor.x > el_cx {
        // Anker liegt rechts → linke Außenkante bewegt sich.
        if let Some(target) = pick_snap(el.x, x_targets, snap_px, zoom) {
            let delta = target - el.x;
            el.x += delta;
            el.w -= delta;
            snap_v = Some(target);
        }
    } else if anchor.x < el_cx {
        // Anker liegt links → rechte Außenkante bewegt sich.
        let right = el.x + el.w;
        if let Some(target) = pick_snap(right, x_targets, snap_px, zoom) {
            el.w += target - right;
            snap_v = Some(target);
        }
    }

    // Y-Achse.
    if anchor.y > el_cy {
        if let Some(target) = pick_snap(el.y, y_targets, snap_px, zoom) {
            let delta = target - el.y;
            el.y += delta;
            el.h -= delta;
            snap_h = Some(target);
        }
    } else if anchor.y < el_cy {
        let bottom = el.y + el.h;
        if let Some(target) = pick_snap(bottom, y_targets, snap_px, zoom) {
            el.h += target - bottom;
            snap_h = Some(target);
        }
    }

    (snap_v, snap_h)
}

/// Wählt das am nächsten gelegene Ziel innerhalb der Snap-Schwelle. Maß:
/// Bildschirm-Pixel = |obj_val - target| / zoom.
fn pick_snap(obj_val: f32, targets: &[f32], snap_px: f32, zoom: f32) -> Option<f32> {
    let mut best: Option<(f32, f32)> = None;
    for &target in targets {
        let dist = (obj_val - target).abs() / zoom;
        if dist < snap_px && best.map_or(true, |(d, _)| dist < d) {
            best = Some((dist, target));
        }
    }
    best.map(|(_, t)| t)
}

/// Sammelt alle Snap-Ziele einer Seite: Seitenränder und -mitten sowie
/// die Außenkanten aller Objekte, deren ID nicht in `exclude` steht
/// (typischerweise die gerade gezogenen / skalierten IDs).
/// Objekt-Mitten werden bewusst NICHT aufgenommen — sie sind keine
/// sichtbaren Kanten und würden Andockpunkte erzeugen, die nicht dem
/// realen Objekt-Rahmen entsprechen.
/// Liefert (X-Targets, Y-Targets) in Seitenkoordinaten.
fn collect_snap_targets(
    elements: &[Element],
    exclude: &[u64],
    pw_pt: f32,
    ph_pt: f32,
) -> (Vec<f32>, Vec<f32>) {
    let mut xs: Vec<f32> = vec![0.0, pw_pt / 2.0, pw_pt];
    let mut ys: Vec<f32> = vec![0.0, ph_pt / 2.0, ph_pt];
    for el in elements {
        if exclude.contains(&el.id) {
            continue;
        }
        // Nur echte Außenkanten der Objekte.
        xs.push(el.x);
        xs.push(el.x + el.w);
        ys.push(el.y);
        ys.push(el.y + el.h);
    }
    (xs, ys)
}

/// Ändert nur eine Dimension (Breite bei Left/Right, Höhe bei Top/Bottom).
/// Die gegenüberliegende Kante bleibt fixiert (anchor = ihr Mittelpunkt).
fn resize_edge_to_pointer(
    el: &mut Element,
    edge: CropEdge,
    anchor: Pos2,
    rotation: f32,
    pointer_page: Vec2,
) {
    let pointer_page = Pos2::new(pointer_page.x, pointer_page.y);
    let dv = pointer_page - anchor;
    let local = rotate_vec(dv, -rotation);
    match edge {
        CropEdge::Left | CropEdge::Right => {
            // Breite = |lokale x-Distanz|, Höhe bleibt unverändert.
            let new_w = local.x.abs().max(2.0);
            let sign_x = if local.x >= 0.0 { 1.0 } else { -1.0 };
            let half_local = Vec2::new(sign_x * new_w / 2.0, 0.0);
            let center_offset = rotate_vec(half_local, rotation);
            let new_center = anchor + center_offset;
            el.w = new_w;
            el.x = new_center.x - new_w / 2.0;
            el.y = new_center.y - el.h / 2.0;
        }
        CropEdge::Top | CropEdge::Bottom => {
            let new_h = local.y.abs().max(2.0);
            let sign_y = if local.y >= 0.0 { 1.0 } else { -1.0 };
            let half_local = Vec2::new(0.0, sign_y * new_h / 2.0);
            let center_offset = rotate_vec(half_local, rotation);
            let new_center = anchor + center_offset;
            el.h = new_h;
            el.y = new_center.y - new_h / 2.0;
            el.x = new_center.x - el.w / 2.0;
        }
    }
}

fn crop_to_pointer(
    el: &mut Element,
    edge: CropEdge,
    start: crate::model::Crop,
    pointer_page: Vec2,
) {
    let center = Pos2::new(el.x + el.w / 2.0, el.y + el.h / 2.0);
    let local = world_to_local(
        center,
        el.rotation,
        Pos2::new(pointer_page.x, pointer_page.y),
    );
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

// ===========================================================================
// Pfad-Werkzeuge: zeichnen (Pen, Freihand) und Knoten bearbeiten
// ===========================================================================

/// Zeichenwerkzeuge für freie Pfade.
///
/// Beide Werkzeuge füllen denselben Entwurf (`app.path_draft`) und übergeben
/// ihn am Ende an `EditorApp::finish_path`. Der Unterschied liegt nur darin,
/// **wie** die Knoten entstehen: beim Pen einzeln durch Klicks, beim Freihand
/// aus einer aufgezeichneten Spur, die erst beim Loslassen ausgedünnt wird.
#[allow(clippy::too_many_arguments)]
fn path_tool(
    app: &mut EditorApp,
    ui: &egui::Ui,
    painter: &egui::Painter,
    pointer: Option<Pos2>,
    to_screen: &impl Fn(Pos2) -> Pos2,
    to_page: &impl Fn(Pos2) -> Vec2,
    primary_pressed: bool,
    primary_released: bool,
    double_clicked: bool,
    in_canvas: bool,
) {
    let zoom = app.view.zoom;
    let shift = ui.input(|i| i.modifiers.shift);
    let primary_down = ui.input(|i| i.pointer.primary_down());
    let page_pos = pointer.map(|p| to_page(p).to_pos2());

    match app.tool {
        crate::app::Tool::Pen => pen_tool(
            app,
            painter,
            pointer,
            page_pos,
            to_screen,
            zoom,
            shift,
            primary_pressed,
            primary_released,
            double_clicked,
            primary_down,
            in_canvas,
        ),
        crate::app::Tool::Freehand => freehand_tool(
            app,
            painter,
            page_pos,
            to_screen,
            primary_pressed,
            primary_released,
            primary_down,
            in_canvas,
        ),
        _ => {}
    }
}

/// Das Pen-Werkzeug: Klicken setzt eine Ecke, Ziehen zieht die Kurvengriffe
/// heraus.
///
/// Die Griffe werden **symmetrisch** gesetzt: Der Ausgangsgriff folgt dem
/// Cursor, der Eingangsgriff spiegelt ihn. Damit läuft die Kurve ohne Knick
/// durch den Knoten — das ist die Erwartung an ein Pen-Werkzeug, und wer eine
/// Spitze braucht, bricht sie hinterher in der Knotenbearbeitung auf.
#[allow(clippy::too_many_arguments)]
fn pen_tool(
    app: &mut EditorApp,
    painter: &egui::Painter,
    pointer: Option<Pos2>,
    page_pos: Option<Pos2>,
    to_screen: &impl Fn(Pos2) -> Pos2,
    zoom: f32,
    shift: bool,
    primary_pressed: bool,
    primary_released: bool,
    double_clicked: bool,
    primary_down: bool,
    in_canvas: bool,
) {
    use crate::geometry::PathNode;

    // --- Doppelklick beendet den offenen Pfad ---
    //
    // Der zweite Klick hat schon einen Knoten gesetzt, bevor der Doppelklick
    // gemeldet wird — der wird hier wieder zurückgenommen. Ohne das säße am
    // Ende jedes so beendeten Pfads ein doppelter Knoten.
    if double_clicked {
        if let Some(draft) = app.path_draft.as_mut() {
            if draft.nodes.len() > 2 {
                draft.nodes.pop();
                app.finish_path(false);
                return;
            }
        }
    }

    // --- Klick: Knoten setzen oder Pfad schließen ---
    if primary_pressed && in_canvas {
        if let Some(mut p) = page_pos {
            let close_tol = 10.0 / zoom;
            let closing = app
                .path_draft
                .as_ref()
                .filter(|d| d.nodes.len() >= 2)
                .map(|d| (p - d.nodes[0].anchor).length() <= close_tol)
                .unwrap_or(false);
            if closing {
                app.finish_path(true);
                return;
            }
            // Shift: den neuen Knoten auf ein 45°-Raster zum letzten legen.
            if shift {
                if let Some(last) = app.path_draft.as_ref().and_then(|d| d.nodes.last()) {
                    p = snap_angle_45(last.anchor, p);
                }
            }
            let draft = app.path_draft.get_or_insert_with(Default::default);
            draft.nodes.push(PathNode::corner(p));
            draft.dragging = true;
        }
    }

    // --- Ziehen: Griffe aus dem zuletzt gesetzten Knoten herausziehen ---
    if primary_down {
        if let (Some(p), Some(draft)) = (page_pos, app.path_draft.as_mut()) {
            if draft.dragging {
                if let Some(last) = draft.nodes.last_mut() {
                    let anchor = last.anchor;
                    // Erst ab einer spürbaren Bewegung; sonst erzeugte jeder
                    // Klick mit ruhiger Hand einen winzigen Griff.
                    if (p - anchor).length() * zoom >= 3.0 {
                        last.out_h = p;
                        last.in_h = anchor - (p - anchor);
                    }
                }
            }
        }
    }
    if primary_released {
        if let Some(draft) = app.path_draft.as_mut() {
            draft.dragging = false;
        }
    }

    // --- Vorschau ---
    let Some(draft) = app.path_draft.as_ref() else {
        // Noch kein Knoten gesetzt: nur ein Punkt am Cursor.
        if let Some(pt) = pointer {
            painter.circle_stroke(pt, 5.0, Stroke::new(1.5_f32, PATH_ACCENT));
        }
        return;
    };

    let mut preview: Vec<PathNode> = draft.nodes.clone();
    // Gummiband zum Cursor — als eigener Eckknoten, damit die Vorschau exakt
    // dem entspricht, was ein Klick an dieser Stelle ergäbe.
    if !draft.dragging {
        if let Some(p) = page_pos {
            let p = if shift {
                draft
                    .nodes
                    .last()
                    .map(|l| snap_angle_45(l.anchor, p))
                    .unwrap_or(p)
            } else {
                p
            };
            preview.push(PathNode::corner(p));
        }
    }
    draw_node_preview(painter, &preview, draft.nodes.len(), to_screen);

    // Der Startknoten wird hervorgehoben, sobald ein Klick den Pfad schließen
    // würde — sonst rät man, wie nah „nah genug" ist.
    if draft.nodes.len() >= 2 {
        if let Some(p) = page_pos {
            let start = draft.nodes[0].anchor;
            if (p - start).length() <= 10.0 / zoom {
                painter.circle_stroke(
                    to_screen(start),
                    8.0,
                    Stroke::new(2.0_f32, PATH_ACCENT),
                );
            }
        }
    }
}

/// Das Freihand-Werkzeug: Spur aufzeichnen, beim Loslassen ausdünnen und
/// glätten.
#[allow(clippy::too_many_arguments)]
fn freehand_tool(
    app: &mut EditorApp,
    painter: &egui::Painter,
    page_pos: Option<Pos2>,
    to_screen: &impl Fn(Pos2) -> Pos2,
    primary_pressed: bool,
    primary_released: bool,
    primary_down: bool,
    in_canvas: bool,
) {
    /// Mindestabstand zweier aufgezeichneter Punkte (pt). Alles darunter ist
    /// Zittern und blähte die Spur nur auf.
    const MIN_STEP: f32 = 1.0;
    /// Toleranz beim Ausdünnen (pt). Größer = weniger Knoten, kantiger.
    const SIMPLIFY_EPS: f32 = 1.5;
    /// Abstand, unter dem ein Zug als geschlossen gilt (pt).
    const CLOSE_TOL: f32 = 12.0;

    if primary_pressed && in_canvas {
        if let Some(p) = page_pos {
            let draft = app.path_draft.get_or_insert_with(Default::default);
            draft.trace.clear();
            draft.trace.push(p);
        }
    }

    if primary_down {
        if let (Some(p), Some(draft)) = (page_pos, app.path_draft.as_mut()) {
            if draft
                .trace
                .last()
                .is_none_or(|last| (p - *last).length() >= MIN_STEP)
            {
                draft.trace.push(p);
            }
        }
    }

    if primary_released {
        if let Some(draft) = app.path_draft.as_mut() {
            let trace = std::mem::take(&mut draft.trace);
            if trace.len() < 2 {
                app.path_draft = None;
                return;
            }
            let simple = crate::geometry::simplify_polyline(&trace, SIMPLIFY_EPS);
            // Ein Zug, der ungefähr dort endet, wo er begann, war als Umriss
            // gemeint — dann wird er geschlossen und der doppelte Endpunkt
            // fällt weg.
            let closed =
                simple.len() > 3 && (simple[simple.len() - 1] - simple[0]).length() <= CLOSE_TOL;
            let pts = if closed {
                &simple[..simple.len() - 1]
            } else {
                &simple[..]
            };
            let nodes = crate::geometry::nodes_from_polyline(pts, closed);
            app.path_draft = Some(crate::app::PathDraft {
                nodes,
                dragging: false,
                trace: Vec::new(),
            });
            app.finish_path(closed);
        }
    }

    // Vorschau: die Rohspur, damit der Strich dem Stift folgt.
    if let Some(draft) = app.path_draft.as_ref() {
        if draft.trace.len() >= 2 {
            let pts: Vec<Pos2> = draft.trace.iter().map(|p| to_screen(*p)).collect();
            painter.add(Shape::line(pts, Stroke::new(2.0_f32, PATH_ACCENT)));
        }
    }
}

/// Zeichnet die Vorschau eines Pfad-Entwurfs: Kurve, Stützpunkte und Griffe.
///
/// `committed` ist die Zahl der wirklich gesetzten Knoten — alles darüber ist
/// das Gummiband zum Cursor und wird blasser gezeichnet.
fn draw_node_preview(
    painter: &egui::Painter,
    nodes: &[crate::geometry::PathNode],
    committed: usize,
    to_screen: &impl Fn(Pos2) -> Pos2,
) {
    if nodes.is_empty() {
        return;
    }
    // Die Kurve selbst — über ein Wegwerf-Element, damit exakt dieselbe
    // Auflösung greift wie beim fertigen Pfad.
    if nodes.len() >= 2 {
        let el = crate::geometry::path_from_nodes(0, nodes, false);
        let pts: Vec<Pos2> = crate::geometry::path_outline(&el)
            .iter()
            .map(|p| to_screen(*p))
            .collect();
        if pts.len() >= 2 {
            painter.add(Shape::line(pts, Stroke::new(2.0_f32, PATH_ACCENT)));
        }
    }

    for (i, n) in nodes.iter().enumerate() {
        let s = to_screen(n.anchor);
        let color = if i >= committed {
            PATH_ACCENT.gamma_multiply(0.5)
        } else {
            PATH_ACCENT
        };
        // Griffe zeigen, solange sie nicht auf dem Stützpunkt liegen.
        if !n.is_corner() {
            for h in [n.in_h, n.out_h] {
                let hs = to_screen(h);
                painter.line_segment([s, hs], Stroke::new(1.0_f32, color));
                painter.circle_filled(hs, 3.5, color);
            }
        }
        let r = if i == 0 { 5.0 } else { 4.0 };
        painter.circle_filled(s, r, Color32::WHITE);
        painter.circle_stroke(s, r, Stroke::new(1.5_f32, color));
    }
}

/// Zeichnet die Knotenbearbeitung eines Pfads: Stützpunkte, Griffe, den
/// gerade ausgewählten Knoten und die Einfügestelle unter dem Cursor.
///
/// Die Knotenform sagt, was der Knoten mit der Kurve macht:
///
/// * **Quadrat** — Ecke, beide Griffe liegen auf dem Stützpunkt.
/// * **Kreis** — glatter Übergang, die Kurve läuft ohne Knick hindurch.
/// * **Raute** — Spitze: Der Knoten hat Griffe, aber sie liegen nicht auf
///   einer Geraden, die Kurve knickt also.
///
/// Ohne diesen Unterschied müsste man jeden Knoten anfassen, um zu sehen,
/// woher ein unerwarteter Knick kommt.
fn draw_path_nodes(
    painter: &egui::Painter,
    el: &Element,
    selected: Option<usize>,
    pointer_page: Option<Pos2>,
    zoom: f32,
    to_screen: &impl Fn(Pos2) -> Pos2,
) {
    // Einfügestelle: Wo würde ein Doppelklick einen Knoten setzen? Ohne diese
    // Vorschau ist die Geste unsichtbar.
    if let Some(p) = pointer_page {
        if let Some(hit) = crate::geometry::path_nearest(el, p) {
            let near_node = crate::geometry::path_nodes(el)
                .iter()
                .any(|n| (n.anchor - hit.pos).length() * zoom <= NODE_GRAB);
            if hit.dist * zoom <= NODE_GRAB && !near_node {
                let s = to_screen(hit.pos);
                painter.circle_stroke(s, 4.0, Stroke::new(1.5_f32, PATH_ACCENT));
                painter.line_segment(
                    [s - Vec2::new(3.0, 0.0), s + Vec2::new(3.0, 0.0)],
                    Stroke::new(1.5_f32, PATH_ACCENT),
                );
                painter.line_segment(
                    [s - Vec2::new(0.0, 3.0), s + Vec2::new(0.0, 3.0)],
                    Stroke::new(1.5_f32, PATH_ACCENT),
                );
            }
        }
    }

    let nodes = crate::geometry::path_nodes(el);
    for (i, n) in nodes.iter().enumerate() {
        let s = to_screen(n.anchor);
        if !n.is_corner() {
            for h in [n.in_h, n.out_h] {
                let hs = to_screen(h);
                painter.line_segment([s, hs], Stroke::new(1.0_f32, PATH_ACCENT));
                painter.circle_filled(hs, 3.5, Color32::WHITE);
                painter.circle_stroke(hs, 3.5, Stroke::new(1.5_f32, PATH_ACCENT));
            }
        }
        let active = selected == Some(i);
        let fill = if active { PATH_ACCENT } else { Color32::WHITE };
        let stroke = Stroke::new(1.5_f32, PATH_ACCENT);
        if n.is_corner() {
            let r = Rect::from_center_size(s, Vec2::splat(9.0));
            painter.rect_filled(r, 1.0, fill);
            painter.rect_stroke(r, 1.0, stroke, egui::StrokeKind::Inside);
        } else if n.is_smooth() {
            painter.circle_filled(s, 4.5, fill);
            painter.circle_stroke(s, 4.5, stroke);
        } else {
            // Raute = Spitze.
            let d = 6.0;
            let pts = vec![
                s + Vec2::new(0.0, -d),
                s + Vec2::new(d, 0.0),
                s + Vec2::new(0.0, d),
                s + Vec2::new(-d, 0.0),
            ];
            painter.add(Shape::convex_polygon(pts, fill, stroke));
        }
    }
}

/// Sucht Knoten oder Griff unter dem Cursor.
///
/// Rückgabe: `(Index, Griff)` — `None` als Griff heißt „der Stützpunkt selbst".
/// Griffe gewinnen bei Gleichstand, weil sie in der Regel auf dem Stützpunkt
/// aufsitzen und sonst nie erreichbar wären.
fn path_node_at(
    el: &Element,
    pointer: Pos2,
    to_screen: &impl Fn(Pos2) -> Pos2,
) -> Option<(usize, Option<bool>)> {
    let nodes = crate::geometry::path_nodes(el);
    let mut best: Option<(f32, usize, Option<bool>)> = None;
    for (i, n) in nodes.iter().enumerate() {
        if !n.is_corner() {
            for (outgoing, h) in [(false, n.in_h), (true, n.out_h)] {
                let d = to_screen(h).distance(pointer);
                if d <= NODE_GRAB && best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
                    best = Some((d, i, Some(outgoing)));
                }
            }
        }
        let d = to_screen(n.anchor).distance(pointer);
        // Der Stützpunkt braucht einen echten Vorsprung, um einen Griff zu
        // verdrängen — sonst ließe sich ein kurzer Griff nie fassen.
        if d <= NODE_GRAB && best.as_ref().is_none_or(|(bd, _, _)| d < *bd - 2.0) {
            best = Some((d, i, None));
        }
    }
    best.map(|(_, i, h)| (i, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::PathNode;

    /// Zoom 1 und Ursprung bei (0,0): Bildschirm- und Seitenkoordinaten sind
    /// identisch, jede Toleranz in Pixeln ist damit direkt ablesbar.
    fn ident(p: Pos2) -> Pos2 {
        p
    }

    /// Waagerechter Zug — der Fall, der die Hüllbox auf `PATH_MIN_EXTENT`
    /// zusammenfallen lässt.
    fn flacher_pfad() -> Element {
        crate::geometry::path_from_nodes(
            1,
            &[
                PathNode::corner(Pos2::new(100.0, 200.0)),
                PathNode::corner(Pos2::new(300.0, 200.0)),
            ],
            false,
        )
    }

    #[test]
    fn flacher_pfad_ist_auch_neben_der_kontur_treffbar() {
        let el = flacher_pfad();
        // Die Box ist nur PATH_MIN_EXTENT hoch. Früher verwarf die randlose
        // Hüllbox-Prüfung diesen Klick, bevor die Toleranz überhaupt zum
        // Zug kam — der Pfad war auf ein Viertelpixel genau zu treffen.
        assert!(el.h <= crate::geometry::PATH_MIN_EXTENT);
        let ctx = egui::Context::default();
        assert_eq!(
            element_hit_strength(&el, Pos2::new(200.0, 195.0), &ident, 1.0, &ctx),
            Some(1),
            "5 px neben einer waagerechten Kontur muss ein Treffer sein"
        );
    }

    #[test]
    fn weit_entfernter_klick_trifft_den_pfad_nicht() {
        let el = flacher_pfad();
        let ctx = egui::Context::default();
        assert_eq!(
            element_hit_strength(&el, Pos2::new(200.0, 400.0), &ident, 1.0, &ctx),
            None
        );
    }

    #[test]
    fn doppelklick_verzeiht_mehr_als_der_klick() {
        let el = flacher_pfad();
        let daneben = Pos2::new(200.0, 188.0); // 12 px über der Kontur
        assert!(
            !path_hit_within(&el, daneben, &ident, 1.0, PATH_CLICK_TOL),
            "für den einfachen Klick ist das zu weit"
        );
        assert!(
            path_hit_within(&el, daneben, &ident, 1.0, PATH_DBLCLICK_TOL),
            "der Doppelklick muss hier greifen, sonst entsteht ein Textfeld \
             über dem Pfad"
        );
    }

    #[test]
    fn ausgewaehlter_pfad_faengt_auch_groebe_doppelklicks() {
        let el = flacher_pfad();
        let weit = Pos2::new(200.0, 170.0); // 30 px über der Kontur
        assert!(
            !path_hit_within(&el, weit, &ident, 1.0, PATH_DBLCLICK_TOL),
            "ohne Auswahl bleibt das zu weit"
        );
        assert!(
            path_hit_within(&el, weit, &ident, 1.0, PATH_DBLCLICK_TOL_SELECTED),
            "wer den Pfad ausgewählt hat, meint beim Doppelklick ihn"
        );
    }

    #[test]
    fn toleranz_bleibt_am_bildschirm_konstant_beim_zoomen() {
        let el = flacher_pfad();
        // Bei Zoom 2 liegt die Kontur auf Bildschirm-y 400; 5 px darüber
        // muss weiterhin treffen, obwohl das in Seitenkoordinaten nur noch
        // 2,5 pt sind.
        let ctx = egui::Context::default();
        let to_screen = |p: Pos2| Pos2::new(p.x * 2.0, p.y * 2.0);
        assert_eq!(
            element_hit_strength(&el, Pos2::new(400.0, 395.0), &to_screen, 2.0, &ctx),
            Some(1)
        );
    }

    #[test]
    fn resize_symbol_folgt_der_griffrichtung() {
        use egui::CursorIcon::*;
        // Bildschirmkoordinaten: y zeigt nach unten.
        assert_eq!(resize_icon(Vec2::new(1.0, 0.0)), ResizeHorizontal);
        assert_eq!(resize_icon(Vec2::new(-1.0, 0.0)), ResizeHorizontal);
        assert_eq!(resize_icon(Vec2::new(0.0, 1.0)), ResizeVertical);
        assert_eq!(resize_icon(Vec2::new(1.0, 1.0)), ResizeNwSe);
        assert_eq!(resize_icon(Vec2::new(1.0, -1.0)), ResizeNeSw);
    }

    #[test]
    fn gedrehtes_objekt_bekommt_gedrehtes_resize_symbol() {
        // Die rechte Kante eines um 90° gedrehten Objekts liegt am Bildschirm
        // unten — der Cursor muss senkrecht zeigen, nicht waagerecht.
        let mut el = rechteck(1);
        el.rotation = 90.0; // Grad, nicht Radiant

        let center = Pos2::new(200.0, 150.0);
        let rechts = edge_mid_positions(&el, &ident, 1.0)[1];
        assert_eq!(resize_icon(rechts - center), egui::CursorIcon::ResizeVertical);
    }

    /// Minimale App mit einer Seite und den übergebenen Objekten.
    fn app_mit(elements: Vec<Element>) -> EditorApp {
        let mut app = EditorApp::default();
        app.doc.pages[0].elements = elements;
        app
    }

    /// Gefülltes Rechteck bei (100,100), 200×100 — Mittelpunkt (200,150).
    fn rechteck(id: u64) -> Element {
        let mut el = Element::new_rectangle(id, 100.0, 100.0);
        el.w = 200.0;
        el.h = 100.0;
        el.fill_color = crate::model::default_fill_color();
        el
    }

    #[test]
    fn nicht_ausgewaehltes_objekt_zeigt_die_hand() {
        let app = app_mit(vec![rechteck(7)]);
        let ctx = egui::Context::default();
        assert_eq!(
            hover_cursor(&app, 0, Pos2::new(200.0, 150.0), &ident, 1.0, &ctx),
            Some(egui::CursorIcon::PointingHand)
        );
    }

    #[test]
    fn ausgewaehltes_objekt_zeigt_das_verschiebe_symbol() {
        let mut app = app_mit(vec![rechteck(7)]);
        app.select_only(7);
        let ctx = egui::Context::default();
        assert_eq!(
            hover_cursor(&app, 0, Pos2::new(200.0, 150.0), &ident, 1.0, &ctx),
            Some(egui::CursorIcon::Move)
        );
    }

    #[test]
    fn leere_flaeche_laesst_den_cursor_in_ruhe() {
        let app = app_mit(vec![rechteck(7)]);
        let ctx = egui::Context::default();
        assert_eq!(
            hover_cursor(&app, 0, Pos2::new(600.0, 600.0), &ident, 1.0, &ctx),
            None
        );
    }

    #[test]
    fn eckgriff_schlaegt_den_koerper() {
        let mut app = app_mit(vec![rechteck(7)]);
        app.select_only(7);
        let ecke = corner_positions(&app.doc.pages[0].elements[0], &ident, 1.0)[0];
        let ctx = egui::Context::default();
        let icon = hover_cursor(&app, 0, ecke, &ident, 1.0, &ctx).expect("Griff muss antworten");
        assert_ne!(
            icon,
            egui::CursorIcon::Move,
            "auf dem Eckgriff wird skaliert, nicht verschoben"
        );
    }

    #[test]
    fn cursor_und_klick_treffen_dasselbe_objekt() {
        // Zwei überlappende Rechtecke: Der Cursor darf nicht das eine
        // versprechen, während der Klick das andere auswählt.
        let mut unten = rechteck(1);
        let mut oben = rechteck(2);
        unten.x = 100.0;
        oben.x = 200.0;
        let app = app_mit(vec![unten, oben]);
        let ctx = egui::Context::default();
        for p in [
            Pos2::new(150.0, 150.0),
            Pos2::new(250.0, 150.0),
            Pos2::new(350.0, 150.0),
        ] {
            let klick = topmost_at(&app, 0, p, &ident, 1.0, &ctx);
            let cursor = hover_cursor(&app, 0, p, &ident, 1.0, &ctx);
            assert_eq!(
                klick.is_some(),
                cursor.is_some(),
                "bei {p:?} widersprechen sich Cursor und Klick"
            );
        }
    }

    #[test]
    fn gefuellte_flaeche_zaehlt_als_treffer() {
        let el = crate::geometry::path_from_nodes(
            2,
            &[
                PathNode::corner(Pos2::new(100.0, 100.0)),
                PathNode::corner(Pos2::new(300.0, 100.0)),
                PathNode::corner(Pos2::new(300.0, 300.0)),
                PathNode::corner(Pos2::new(100.0, 300.0)),
            ],
            true,
        );
        let mut el = el;
        el.fill_color = crate::model::default_fill_color();
        assert!(el.fill_color[3] > 0, "Test braucht eine sichtbare Füllung");
        assert!(path_hit_within(
            &el,
            Pos2::new(200.0, 200.0),
            &ident,
            1.0,
            PATH_CLICK_TOL
        ));
    }
}
