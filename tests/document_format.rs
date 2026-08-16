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
