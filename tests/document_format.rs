//! Tests für das `.boxdoc`-Dateiformat.
//!
//! Das Dateiformat ist zugleich die AI-Schnittstelle und das Speicherformat.
//! Ein Feld, das beim Speichern-Laden-Zyklus verloren geht, ist stiller
//! Datenverlust — genau die Fehlerklasse, die bisher niemand bemerkt hätte,
//! weil es keine Tests gab.

use boxdoc::model::{
    Crop, Document, Element, ElementKind, Orientation, PaperFormat, Page, TextAlign, VAlign,
};

/// Baut ein Element, in dem **jedes** Feld von seinem Default abweicht.
/// Wenn der Roundtrip das übersteht, überlebt jedes reale Dokument.
fn fully_populated_element() -> Element {
    let mut el = Element::new_text(42, 10.0, 20.0);
    el.kind = ElementKind::Rectangle;
    el.x = 12.5;
    el.y = 34.25;
    el.w = 111.75;
    el.h = 222.5;
    el.rotation = 37.5;
    el.text = "Zeile 1\nZeile 2 mit Ümläüten und emoji 🎉".to_string();
    el.font_size = 23.5;
    el.font = "pacifico".to_string();
    el.color = [1, 2, 3, 4];
    el.bold = true;
    el.italic = true;
    el.underline = true;
    el.align = TextAlign::Right;
    el.valign = VAlign::Bottom;
    el.indent = 7.25;
    el.auto_height = false;
    el.crop = Crop {
        x: 0.1,
        y: 0.2,
        w: 0.3,
        h: 0.4,
    };
    el.image_w = 640;
    el.image_h = 480;
    el.fill_color = [9, 8, 7, 6];
    el.stroke_width = 3.75;
    el.stroke_color = [11, 22, 33, 44];
    el.corner_radius = 5.5;
    el.points = vec![[0.0, 0.0], [1.0, 0.25], [0.5, 1.0]];
    el.handles = vec![
        [0.0, 0.0, 0.1, 0.05],
        [0.8, 0.2, 1.0, 0.35],
        [0.6, 0.9, 0.5, 1.0],
    ];
    el.path_closed = true;
    el
}

fn doc_with(el: Element) -> Document {
    Document {
        format: PaperFormat::Legal,
        orientation: Orientation::Landscape,
        pages: vec![Page { elements: vec![el] }],
    }
}

#[test]
fn roundtrip_erhaelt_alle_felder() {
    let original = doc_with(fully_populated_element());
    let json = serde_json::to_string_pretty(&original).expect("serialisieren");
    let back: Document = serde_json::from_str(&json).expect("deserialisieren");

    assert_eq!(back.format, original.format);
    assert_eq!(back.orientation, original.orientation);
    assert_eq!(back.pages.len(), 1);

    let a = &original.pages[0].elements[0];
    let b = &back.pages[0].elements[0];

    // Genau der Vergleich, den auch der Merge benutzt.
    assert!(
        boxdoc::merge::elements_equal(a, b),
        "Roundtrip hat Felder verändert:\nvorher: {a:#?}\nnachher: {b:#?}"
    );
}

#[test]
fn roundtrip_ist_stabil_ueber_mehrere_durchlaeufe() {
    // Ein zweiter Durchlauf darf nichts mehr verändern — sonst driftet ein
    // Dokument bei jedem Speichern.
    let original = doc_with(fully_populated_element());
    let json1 = serde_json::to_string(&original).unwrap();
    let back1: Document = serde_json::from_str(&json1).unwrap();
    let json2 = serde_json::to_string(&back1).unwrap();
    assert_eq!(json1, json2, "Serialisierung ist nicht idempotent");
}

#[test]
fn alte_dateien_ohne_auto_height_laden_weiterhin() {
    // Vorwärtskompatibilität: Dateien, die vor der Einführung von
    // `auto_height` geschrieben wurden, dürfen nicht kaputtgehen.
    let json = r#"{
        "format": "A4",
        "orientation": "Portrait",
        "pages": [{"elements": [{
            "id": 1, "kind": "Text",
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 20.0, "rotation": 0.0,
            "text": "alt", "font_size": 12.0, "color": [0,0,0,255],
            "align": "Left", "indent": 0.0,
            "crop": {"x":0.0,"y":0.0,"w":1.0,"h":1.0},
            "image_w": 0, "image_h": 0
        }]}]
    }"#;
    let doc: Document = serde_json::from_str(json).expect("altes Format muss laden");
    let el = &doc.pages[0].elements[0];
    assert!(el.auto_height, "auto_height muss auf true defaulten");
    assert_eq!(el.font, "default");
    assert!(!el.bold);
    // Auch die später hinzugekommenen Pfad-Felder müssen fehlen dürfen.
    assert!(el.points.is_empty());
    assert!(!el.path_closed);
}

#[test]
fn pfad_element_ueberlebt_den_roundtrip() {
    // Ein Pfad ist der einzige Elementtyp mit einer Liste im JSON — genau da
    // fällt ein vergessenes Feld beim Speichern am ehesten unter den Tisch.
    let el = Element::new_path(
        7,
        &[(10.0, 10.0), (110.0, 10.0), (110.0, 60.0), (60.0, 35.0)],
        true,
    );
    let doc = doc_with(el.clone());
    let json = serde_json::to_string(&doc).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    let b = &back.pages[0].elements[0];

    assert_eq!(b.kind, ElementKind::Path);
    assert!(boxdoc::merge::elements_equal(&el, b), "Pfad verändert: {b:#?}");
    assert_eq!(b.points.len(), 4);
    assert!(b.path_closed);
}

#[test]
fn gerader_pfad_schreibt_kein_handles_feld() {
    // Das Dateiformat ist die AI-Schnittstelle. Ein Streckenzug soll in der
    // Datei genauso kurz bleiben wie vor der Kurven-Erweiterung — sonst steht
    // in jedem Dokument ein Vektor voller Wiederholungen, den niemand liest.
    let el = Element::new_path(7, &[(10.0, 10.0), (110.0, 60.0)], false);
    let json = serde_json::to_string(&doc_with(el)).unwrap();
    assert!(
        !json.contains("handles"),
        "leeres handles-Feld darf nicht geschrieben werden: {json}"
    );
}

#[test]
fn kurvenpfad_ueberlebt_den_roundtrip() {
    use boxdoc::geometry::{self, PathNode};
    use egui::Pos2;

    let nodes = vec![
        PathNode::corner(Pos2::new(50.0, 200.0)),
        PathNode {
            anchor: Pos2::new(150.0, 120.0),
            in_h: Pos2::new(100.0, 120.0),
            out_h: Pos2::new(200.0, 120.0),
        },
        PathNode::corner(Pos2::new(250.0, 200.0)),
    ];
    let el = geometry::path_from_nodes(7, &nodes, false);
    assert!(el.path_is_curved(), "Testaufbau: Pfad sollte Kurven haben");

    let json = serde_json::to_string(&doc_with(el.clone())).unwrap();
    let back: Document = serde_json::from_str(&json).unwrap();
    let b = &back.pages[0].elements[0];

    assert!(boxdoc::merge::elements_equal(&el, b), "Pfad verändert: {b:#?}");
    assert!(b.path_is_curved());
    // Und die Knoten liegen nach dem Rundlauf an derselben Stelle.
    for (got, want) in geometry::path_nodes(b).iter().zip(nodes.iter()) {
        assert!((got.anchor - want.anchor).length() < 0.01);
        assert!((got.in_h - want.in_h).length() < 0.01);
        assert!((got.out_h - want.out_h).length() < 0.01);
    }
}

#[test]
fn das_pfad_beispiel_aus_agents_md_stimmt() {
    // `AGENTS.md` **ist** die KI-Schnittstelle. Ein Beispiel darin, das nicht
    // das beschriebene Ergebnis liefert, ist schlimmer als gar keines — die
    // KI baut darauf auf und merkt den Fehler nie. Deshalb steht hier das
    // Dreieck aus der Doku, Zeichen für Zeichen, samt seiner Behauptungen.
    use boxdoc::geometry;
    use egui::Pos2;

    let json = r#"{
        "format": "A4", "orientation": "Portrait",
        "pages": [{"elements": [{
            "id": 8, "kind": "Path",
            "x": 50.0, "y": 50.0, "w": 100.0, "h": 100.0, "rotation": 0.0,
            "points":  [[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]],
            "handles": [[0.0, 1.0, 0.0, 1.0],
                        [1.0, 1.0, 1.4, 0.5],
                        [0.5, 0.0, 0.5, 0.0]],
            "path_closed": true,
            "fill_color": [80, 140, 220, 120],
            "stroke_width": 1.0, "stroke_color": [30, 60, 120, 255],
            "text": "", "font_size": 14.0, "font": "default",
            "color": [0,0,0,255], "align": "Left", "valign": "Top", "indent": 0.0,
            "crop": {"x":0.0,"y":0.0,"w":1.0,"h":1.0}, "image_w": 0, "image_h": 0
        }]}]
    }"#;
    let doc: Document = serde_json::from_str(json).expect("Beispiel muss laden");
    let el = &doc.pages[0].elements[0];

    // Die Stützpunkte liegen dort, wo die Umrechnung in der Doku es sagt.
    let nodes = geometry::path_nodes(el);
    let erwartet = [
        Pos2::new(50.0, 150.0),
        Pos2::new(150.0, 150.0),
        Pos2::new(100.0, 50.0),
    ];
    for (n, p) in nodes.iter().zip(erwartet.iter()) {
        assert!((n.anchor - *p).length() < 0.01, "{:?} statt {p:?}", n.anchor);
    }

    // „Unterkante und linke Seite sind Geraden, die rechte Seite wölbt sich."
    let (_, segs) = geometry::path_segments(el).expect("drei Segmente");
    assert_eq!(segs.len(), 3, "geschlossen = drei Segmente");
    assert!(matches!(segs[0], geometry::PathSeg::Line(_)), "Unterkante");
    assert!(matches!(segs[1], geometry::PathSeg::Cubic(..)), "rechte Seite");
    assert!(matches!(segs[2], geometry::PathSeg::Line(_)), "linke Seite");

    // Und sie wölbt sich wirklich nach außen, also über x = 150 hinaus.
    let max_x = geometry::path_outline(el)
        .iter()
        .fold(f32::MIN, |m, p| m.max(p.x));
    assert!(max_x > 155.0, "Wölbung reicht nur bis x = {max_x}");
}

#[test]
fn pfad_mit_kaputten_griffen_laedt_trotzdem() {
    // Eine KI oder ein Editor kann `handles` in der falschen Länge schreiben.
    // Das darf den Pfad nicht halb gerade machen, sondern muss beim Laden
    // repariert werden.
    let json = r#"{
        "format": "A4", "orientation": "Portrait",
        "pages": [{"elements": [{
            "id": 1, "kind": "Path",
            "x": 0.0, "y": 0.0, "w": 100.0, "h": 100.0, "rotation": 0.0,
            "text": "", "font_size": 14.0, "font": "default",
            "color": [0,0,0,255], "align": "Left", "valign": "Top", "indent": 0.0,
            "crop": {"x":0.0,"y":0.0,"w":1.0,"h":1.0}, "image_w": 0, "image_h": 0,
            "points": [[0.0,0.0],[0.5,1.0],[1.0,0.0]],
            "handles": [[0.0,0.0,0.2,0.4]],
            "path_closed": false
        }]}]
    }"#;
    let doc: Document = serde_json::from_str(json).expect("muss laden");
    let el = &doc.pages[0].elements[0];
    assert!(el.path_handles_valid(), "Griffe wurden nicht repariert");
    assert_eq!(el.handles.len(), el.points.len());
}

#[test]
fn unbekannte_felder_brechen_das_laden_nicht() {
    // Eine KI, die ein Feld ergänzt, darf das Dokument nicht unlesbar machen.
    let json = r#"{
        "format": "A4", "orientation": "Portrait",
        "pages": [{"elements": []}],
        "irgendwas_neues": 123
    }"#;
    let doc: Document = serde_json::from_str(json).expect("muss tolerant sein");
    assert_eq!(doc.pages.len(), 1);
}

#[test]
fn alle_papierformate_und_ausrichtungen_roundtrippen() {
    for format in PaperFormat::all() {
        for orientation in [Orientation::Portrait, Orientation::Landscape] {
            let doc = Document {
                format,
                orientation,
                pages: vec![Page::default()],
            };
            let json = serde_json::to_string(&doc).unwrap();
            let back: Document = serde_json::from_str(&json).unwrap();
            assert_eq!(back.format, format);
            assert_eq!(back.orientation, orientation);
        }
    }
}

#[test]
fn alle_element_arten_roundtrippen() {
    let kinds = [
        Element::new_text(1, 0.0, 0.0),
        Element::new_rectangle(2, 0.0, 0.0),
        Element::new_line(3, 0.0, 0.0),
        Element::new_ellipse(4, 0.0, 0.0),
        Element::new_image(5, 0, 0, 100, 50),
    ];
    for el in kinds {
        let expected_kind = el.kind;
        let json = serde_json::to_string(&el).unwrap();
        let back: Element = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, expected_kind);
        assert!(boxdoc::merge::elements_equal(&back, &{
            let mut e = back.clone();
            e.id = back.id;
            e
        }));
    }
}

#[test]
fn seitengroessen_stimmen() {
    // A4 Hochformat = 595 x 842 pt. Wenn diese Umrechnung kippt, stimmt das
    // gesamte Layout inklusive PDF nicht mehr.
    let (w, h) = boxdoc::model::page_size_pt(PaperFormat::A4, Orientation::Portrait);
    assert!((w - 595.0).abs() < 1.0, "Breite war {w}");
    assert!((h - 842.0).abs() < 1.0, "Höhe war {h}");

    let (lw, lh) = boxdoc::model::page_size_pt(PaperFormat::A4, Orientation::Landscape);
    assert!((lw - h).abs() < 0.01, "Querformat muss Höhe/Breite tauschen");
    assert!((lh - w).abs() < 0.01);
}

#[test]
fn einheiten_umrechnung_ist_verlustfrei() {
    use boxdoc::model::Units;
    for unit in Units::all() {
        for pt in [0.0_f32, 1.0, 72.0, 595.0, 1234.5] {
            let back = unit.to_pt(unit.from_pt(pt));
            assert!(
                (back - pt).abs() < 0.01,
                "{:?}: {pt} pt -> {} -> {back}",
                unit,
                unit.from_pt(pt)
            );
        }
    }
}
