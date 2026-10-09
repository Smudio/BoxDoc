//! SVG-Import: Rundreise durch BoxDocs eigenen SVG-Export.
//!
//! Der härteste und zugleich wichtigste Fall: Wer eine Seite als SVG
//! exportiert und wieder öffnet, muss dieselben Objekte zurückbekommen —
//! dieselbe Art, dieselbe Lage, dieselben Farben. Weicht hier etwas ab, ist
//! „SVG bearbeiten" mit BoxDoc eine Einbahnstraße.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;

use boxdoc::model::{Crop, Document, Element, ElementKind, Page, PaperFormat, TextAlign};
use boxdoc::store::ImageStore;
use boxdoc::svg::{svg_string, Scope};
use boxdoc::svg_import;

fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    boxdoc::fonts::install(&ctx);
    let _ = ctx.run(Default::default(), |_| {});
    ctx
}

fn no_files(_: &str) -> Option<Vec<u8>> {
    None
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

fn close(a: f32, b: f32, tol: f32, what: &str) {
    assert!((a - b).abs() <= tol, "{what}: {a} statt {b}");
}

fn same_box(a: &Element, b: &Element, tol: f32) {
    close(a.x, b.x, tol, "x");
    close(a.y, b.y, tol, "y");
    close(a.w, b.w, tol, "w");
    close(a.h, b.h, tol, "h");
    let dr = (a.rotation - b.rotation).rem_euclid(360.0);
    assert!(dr < 0.05 || dr > 359.95, "Drehung {} statt {}", a.rotation, b.rotation);
}

/// Exportiert Seite 1 und liest sie wieder ein — so wie „Seite als SVG
/// exportieren" und danach „SVG öffnen".
fn roundtrip(doc: &Document, images: &ImageStore, ctx: &egui::Context) -> (Document, Vec<svg_import::ImportedImage>) {
    let layouts = boxdoc::printing::collect_layouts(ctx, doc);
    let svg = svg_string(doc, images, &layouts, &Scope::Page(0)).expect("Export");
    let imp = svg_import::parse(&svg, 1, &no_files).expect("Import");
    let fixes = imp.text_fixes.clone();
    let (mut back, imgs) = imp.into_document("rundreise");
    svg_import::fit_texts(ctx, &mut back.pages[0].elements, &fixes);
    (back, imgs)
}

#[test]
fn formen_kommen_unveraendert_zurueck() {
    let mut rect = Element::new_rectangle(1, 60.0, 80.0);
    rect.rotation = 25.0;
    rect.corner_radius = 8.0;
    rect.fill_color = [10, 120, 200, 90];
    rect.stroke_width = 3.0;
    rect.stroke_color = [0, 0, 0, 255];

    let mut ell = Element::new_ellipse(2, 300.0, 100.0);
    ell.rotation = -40.0;

    let mut line = Element::new_line(3, 100.0, 400.0);
    line.w = 250.0;
    line.rotation = 15.0;
    line.stroke_width = 4.0;

    // Pfad mit einer echten Kurve.
    let nodes = vec![
        boxdoc::geometry::PathNode::corner(egui::pos2(100.0, 600.0)),
        boxdoc::geometry::PathNode {
            anchor: egui::pos2(200.0, 520.0),
            in_h: egui::pos2(150.0, 520.0),
            out_h: egui::pos2(250.0, 520.0),
        },
        boxdoc::geometry::PathNode::corner(egui::pos2(300.0, 600.0)),
    ];
    let mut path = boxdoc::geometry::path_from_nodes(4, &nodes, true);
    path.fill_color = [255, 200, 0, 255];

    let doc = Document {
        background: Some([250, 245, 230, 255]),
        pages: vec![Page {
            elements: vec![rect.clone(), ell.clone(), line.clone(), path.clone()],
        }],
        ..Document::default()
    };
    let (back, _) = roundtrip(&doc, &ImageStore::default(), &ctx());

    assert_eq!(back.format, PaperFormat::A4, "Seitenformat wiedererkannt");
    assert_eq!(back.background, Some([250, 245, 230, 255]), "Hintergrund wieder Hintergrund");
    let els = &back.pages[0].elements;
    assert_eq!(els.len(), 4, "{els:#?}");

    assert_eq!(els[0].kind, ElementKind::Rectangle);
    same_box(&els[0], &rect, 0.01);
    close(els[0].corner_radius, 8.0, 0.01, "Eckradius");
    // 90/255 → 0.353 → 90: Die Deckkraft übersteht die Rundung auf 3 Stellen.
    assert_eq!(els[0].fill_color, rect.fill_color);
    assert_eq!(els[0].stroke_color, rect.stroke_color);
    close(els[0].stroke_width, 3.0, 0.01, "Rahmen");

    assert_eq!(els[1].kind, ElementKind::Ellipse);
    same_box(&els[1], &ell, 0.01);
    assert_eq!(els[1].fill_color, ell.fill_color);

    assert_eq!(els[2].kind, ElementKind::Line);
    same_box(&els[2], &line, 0.01);
    assert_eq!(els[2].stroke_color, line.stroke_color);

    assert_eq!(els[3].kind, ElementKind::Path);
    assert!(els[3].path_closed && els[3].path_is_curved());
    same_box(&els[3], &path, 0.05);
    assert_eq!(els[3].points.len(), 3);
    for (a, b) in els[3].points.iter().zip(path.points.iter()) {
        close(a[0], b[0], 1e-3, "Punkt x");
        close(a[1], b[1], 1e-3, "Punkt y");
    }
    for (a, b) in els[3].handles.iter().zip(path.handles.iter()) {
        for k in 0..4 {
            close(a[k], b[k], 1e-3, "Griff");
        }
    }
}

#[test]
fn transparente_seite_bleibt_transparent() {
    let doc = Document {
        background: None,
        pages: vec![Page {
            elements: vec![Element::new_rectangle(1, 10.0, 10.0)],
        }],
        ..Document::default()
    };
    let (back, _) = roundtrip(&doc, &ImageStore::default(), &ctx());
    assert_eq!(back.background, None);
    assert_eq!(back.pages[0].elements.len(), 1);
}

#[test]
fn beschnittenes_gedrehtes_bild_behaelt_ausschnitt() {
    let mut img = Element::new_image(1, 0, 0, 40, 20);
    img.x = 100.0;
    img.y = 150.0;
    img.w = 120.0;
    img.h = 80.0;
    img.rotation = 30.0;
    img.crop = Crop { x: 0.25, y: 0.1, w: 0.5, h: 0.8 };
    let mut images = ImageStore::default();
    images.insert(1, png(40, 20), (40, 20));

    let doc = Document {
        pages: vec![Page { elements: vec![img.clone()] }],
        ..Document::default()
    };
    let (back, imgs) = roundtrip(&doc, &images, &ctx());
    let el = &back.pages[0].elements[0];
    assert_eq!(el.kind, ElementKind::Image);
    same_box(el, &img, 0.02);
    close(el.crop.x, 0.25, 1e-3, "crop.x");
    close(el.crop.y, 0.1, 1e-3, "crop.y");
    close(el.crop.w, 0.5, 1e-3, "crop.w");
    close(el.crop.h, 0.8, 1e-3, "crop.h");
    assert_eq!(imgs.len(), 1);
    assert_eq!(imgs[0].id, el.id, "Bild hängt an der Element-ID");
    assert_eq!(imgs[0].dim, (40, 20));
}

#[test]
fn text_landet_auf_seiner_grundlinie() {
    let mut t = Element::new_text(1, 80.0, 120.0);
    t.text = String::from("Hallo SVG-Welt");
    t.font_size = 18.0;
    t.font = String::from("roboto");
    t.bold = true;
    t.underline = true;
    t.color = [30, 60, 90, 255];
    t.w = 400.0;

    let mut c = Element::new_text(2, 100.0, 300.0);
    c.text = String::from("Zentriert\nZweite Zeile");
    c.align = TextAlign::Center;
    c.w = 300.0;

    let ctx = ctx();
    let doc = Document {
        pages: vec![Page { elements: vec![t.clone(), c.clone()] }],
        ..Document::default()
    };
    let layouts = boxdoc::printing::collect_layouts(&ctx, &doc);
    let (back, _) = roundtrip(&doc, &ImageStore::default(), &ctx);
    let els = &back.pages[0].elements;
    assert_eq!(els.len(), 2);

    let a = &els[0];
    assert_eq!(a.kind, ElementKind::Text);
    assert_eq!(a.text, "Hallo SVG-Welt");
    assert_eq!(a.font, "roboto");
    assert!(a.bold && a.underline && !a.italic);
    assert_eq!(a.color, t.color);
    close(a.font_size, 18.0, 0.01, "Schriftgröße");
    // Grundlinie der ersten Zeile: genau da, wo sie vorher war.
    let before = &layouts[&1].lines[0];
    let after = boxdoc::printing::collect_layouts(&ctx, &back);
    let after_line = &after[&a.id].lines[0];
    close(a.x + after_line.x, t.x + before.x, 0.05, "Textanfang");
    close(a.y + after_line.baseline_y, t.y + before.baseline_y, 0.05, "Grundlinie");

    let b = &els[1];
    assert_eq!(b.text, "Zentriert\nZweite Zeile");
    assert_eq!(b.align, TextAlign::Center);
    let before = &layouts[&2].lines[0];
    let after_line = &after[&b.id].lines[0];
    close(
        b.x + after_line.x + after_line.width / 2.0,
        c.x + before.x + before.width / 2.0,
        0.05,
        "Mitte der ersten Zeile",
    );
    close(a.y + after[&a.id].lines[0].baseline_y, t.y + layouts[&1].lines[0].baseline_y, 0.05, "Grundlinie");
}

#[test]
fn absatz_bricht_nach_der_rundreise_weiter_weich_um() {
    // Ein Absatz, den BoxDoc selbst umbricht, plus ein harter Umbruch — und
    // ein einzeiliger, zentrierter Titel, dessen Ausrichtung man an den
    // Zeilen allein nicht ablesen kann.
    let mut para = Element::new_text(1, 60.0, 100.0);
    para.text = String::from(
        "Dieser Absatz ist lang genug, um in einer schmalen Box mehrfach \
         umzubrechen.\nNach dem harten Umbruch geht es weiter.",
    );
    para.w = 160.0;
    para.font_size = 11.0;

    let mut title = Element::new_text(2, 300.0, 100.0);
    title.text = String::from("CH");
    title.align = TextAlign::Center;
    title.w = 72.0;
    title.font_size = 28.0;

    let ctx = ctx();
    let doc = Document {
        pages: vec![Page { elements: vec![para.clone(), title.clone()] }],
        ..Document::default()
    };
    let layouts = boxdoc::printing::collect_layouts(&ctx, &doc);
    assert!(layouts[&1].lines.len() > 3, "Testaufbau: der Absatz muss umbrechen");

    let (back, _) = roundtrip(&doc, &ImageStore::default(), &ctx);
    let els = &back.pages[0].elements;
    for (orig, got) in [(&para, &els[0]), (&title, &els[1])] {
        assert_eq!(got.text, orig.text, "Originaltext mit nur dem harten Umbruch");
        assert_eq!(got.align, orig.align);
        close(got.x, orig.x, 0.05, "x");
        close(got.y, orig.y, 0.05, "y");
        close(got.w, orig.w, 0.01, "Boxbreite");
    }
}

#[test]
fn in_anderem_programm_geaenderter_text_gewinnt() {
    // Veraltete data-boxdoc-Angaben dürfen eine spätere Änderung des
    // sichtbaren Texts nicht überschreiben.
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200pt" height="100pt" viewBox="0 0 200 100">
      <text font-size="12" data-boxdoc-w="50" data-boxdoc-align="center" data-boxdoc-text="Alter Text">
        <tspan x="10" y="20">Neuer Text</tspan>
      </text>
    </svg>"#;
    let imp = svg_import::parse(svg, 1, &no_files).unwrap();
    assert_eq!(imp.elements[0].text, "Neuer Text");
    assert_eq!(imp.elements[0].align, TextAlign::Left);
    assert!(!imp.text_fixes[0].keep_layout);
}

#[test]
fn fremdes_svg_mit_ueblichen_eigenheiten() {
    // So ähnlich schreiben Inkscape und Illustrator: px-Einheiten, viewBox
    // mit Versatz, Gruppen-Transformation, <style>-Klassen, Namensräume.
    let svg = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
     width="400" height="200" viewBox="100 50 400 200">
  <sodipodi:namedview pagecolor="#ffffff"/>
  <defs>
    <style>.st0{fill:#E30613;} .st1{fill:none;stroke:#1D1D1B;stroke-width:4;}</style>
    <linearGradient id="g"><stop offset="0" stop-color="#000"/><stop offset="1" stop-color="#fff"/></linearGradient>
  </defs>
  <g transform="translate(100 50)">
    <rect class="st0" x="10" y="10" width="80" height="40"/>
    <polyline class="st1" points="10,100 50,60 90,100"/>
    <circle cx="200" cy="100" r="30" fill="url(#g)"/>
    <text x="250" y="40" style="font-size:20px;font-family:'Arial'">Logo</text>
  </g>
</svg>"##;
    let imp = svg_import::parse(svg, 1, &no_files).expect("Import");
    close(imp.width, 300.0, 0.01, "Breite (400 px = 300 pt)");
    close(imp.height, 150.0, 0.01, "Höhe");
    let els = &imp.elements;
    assert_eq!(els.len(), 4, "{els:#?}");

    // viewBox-Versatz (100, 50) hebt die Gruppen-Verschiebung auf.
    assert_eq!(els[0].kind, ElementKind::Rectangle);
    close(els[0].x, 7.5, 0.01, "rect x");
    close(els[0].w, 60.0, 0.01, "rect w");
    assert_eq!(els[0].fill_color, [0xe3, 0x06, 0x13, 255]);

    assert_eq!(els[1].kind, ElementKind::Path);
    assert!(!els[1].path_closed);
    assert_eq!(els[1].fill_color[3], 0);
    close(els[1].stroke_width, 3.0, 0.01, "Linie 4 px = 3 pt");

    assert_eq!(els[2].kind, ElementKind::Ellipse);
    assert_eq!(&els[2].fill_color[..3], &[128, 128, 128], "Verlauf → Mischfarbe");

    assert_eq!(els[3].kind, ElementKind::Text);
    assert_eq!(els[3].font, "arial");
    close(els[3].font_size, 15.0, 0.01, "20 px = 15 pt");
}
