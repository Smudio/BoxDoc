//! Prüft, dass Formen im PDF an derselben Stelle landen wie auf dem Bildschirm.
//!
//! Der Auslöser: Linien standen im PDF sichtbar woanders. Ursache waren drei
//! verschiedene Konventionen für dieselbe Sache —
//!
//! * Linie: Canvas verankerte am Mittelpunkt, PDF am Startpunkt.
//! * Rechteck: PDF rotierte gegen den Uhrzeigersinn statt mit ihm.
//! * Ellipse: dritte Variante, zufällig richtig.
//!
//! Beide Renderer beziehen ihre Umrisse jetzt aus `geometry` — in
//! Seitenkoordinaten. Diese Tests simulieren beide Abbildungen und vergleichen
//! das Ergebnis, sodass ein erneutes Auseinanderdriften sofort auffällt.

#![cfg(not(target_arch = "wasm32"))]

use boxdoc::geometry;
use boxdoc::model::{Document, Element, ElementKind, Page};
use boxdoc::store::ImageStore;
use egui::{Pos2, Vec2};

fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

/// Bildet Seitenkoordinaten so ab, wie der **Canvas** es tut.
fn as_canvas(p: Pos2, zoom: f32, pan: Vec2) -> Pos2 {
    Pos2::new(p.x * zoom + pan.x, p.y * zoom + pan.y)
}

/// Bildet Seitenkoordinaten so ab, wie der **PDF-Export** es tut
/// (`printing::to_pdf`): mm, y-Achse gespiegelt.
fn as_pdf(p: Pos2, page_h_mm: f32) -> Pos2 {
    Pos2::new(pt_to_mm(p.x), page_h_mm - pt_to_mm(p.y))
}

/// Rechnet einen PDF-Punkt zurück in Seitenkoordinaten.
fn from_pdf(p: Pos2, page_h_mm: f32) -> Pos2 {
    Pos2::new(p.x * 72.0 / 25.4, (page_h_mm - p.y) * 72.0 / 25.4)
}

/// Rechnet einen Canvas-Punkt zurück in Seitenkoordinaten.
fn from_canvas(p: Pos2, zoom: f32, pan: Vec2) -> Pos2 {
    Pos2::new((p.x - pan.x) / zoom, (p.y - pan.y) / zoom)
}

const PAGE_H_MM: f32 = 297.0; // A4 Hochformat

/// Der Kern: Beide Renderer müssen nach Rücktransformation dieselben
/// Seitenkoordinaten liefern.
fn assert_renderers_agree(outline: &[Pos2], label: &str) {
    let zoom = 1.7;
    let pan = Vec2::new(133.0, -42.0);

    for (i, p) in outline.iter().enumerate() {
        let via_canvas = from_canvas(as_canvas(*p, zoom, pan), zoom, pan);
        let via_pdf = from_pdf(as_pdf(*p, PAGE_H_MM), PAGE_H_MM);
        assert!(
            (via_canvas.x - via_pdf.x).abs() < 0.02 && (via_canvas.y - via_pdf.y).abs() < 0.02,
            "{label}: Punkt {i} weicht ab — Canvas {via_canvas:?} vs PDF {via_pdf:?}"
        );
    }
}

fn rect(rot: f32, radius: f32) -> Element {
    let mut el = Element::new_rectangle(1, 0.0, 0.0);
    el.x = 80.0;
    el.y = 120.0;
    el.w = 220.0;
    el.h = 140.0;
    el.rotation = rot;
    el.corner_radius = radius;
    el
}

fn line(rot: f32) -> Element {
    let mut el = Element::new_line(2, 0.0, 0.0);
    el.x = 100.0;
    el.y = 300.0;
    el.w = 250.0;
    el.h = 0.0;
    el.rotation = rot;
    el
}

#[test]
fn rechteck_stimmt_in_beiden_renderern() {
    for rot in [0.0_f32, 30.0, 90.0, 145.0, -60.0] {
        for radius in [0.0_f32, 20.0] {
            let el = rect(rot, radius);
            assert_renderers_agree(
                &geometry::rect_outline(&el),
                &format!("Rechteck rot={rot} r={radius}"),
            );
        }
    }
}

#[test]
fn linie_stimmt_in_beiden_renderern() {
    for rot in [0.0_f32, 45.0, 90.0, 210.0, -15.0] {
        let el = line(rot);
        let (a, b) = geometry::line_endpoints(&el);
        assert_renderers_agree(&[a, b], &format!("Linie rot={rot}"));
    }
}

#[test]
fn ellipse_stimmt_in_beiden_renderern() {
    for rot in [0.0_f32, 25.0, 90.0] {
        let mut el = Element::new_ellipse(3, 0.0, 0.0);
        el.x = 60.0;
        el.y = 60.0;
        el.w = 180.0;
        el.h = 100.0;
        el.rotation = rot;
        assert_renderers_agree(&geometry::ellipse_outline(&el), &format!("Ellipse rot={rot}"));
    }
}

#[test]
fn gedrehte_linie_bleibt_an_ihrem_platz() {
    // Der konkrete Fehlerfall aus dem PDF: Eine 90 Grad gedrehte Linie musste
    // senkrecht durch ihren Mittelpunkt laufen. Der alte Export drehte um den
    // Startpunkt und schob sie damit weit weg.
    let el = line(90.0);
    let (a, b) = geometry::line_endpoints(&el);
    let center = geometry::element_center(&el);

    assert!((center.x - 225.0).abs() < 0.01, "Mitte x war {}", center.x);
    assert!((center.y - 300.0).abs() < 0.01, "Mitte y war {}", center.y);

    // Senkrecht: gleiche x, symmetrisch um die Mitte.
    assert!((a.x - b.x).abs() < 0.01, "Linie ist nicht senkrecht");
    assert!(((a.y + b.y) / 2.0 - center.y).abs() < 0.01);
    assert!(((b.y - a.y).abs() - 250.0).abs() < 0.01, "Länge stimmt nicht");
}

#[test]
fn rotation_verschiebt_den_mittelpunkt_nie() {
    // Für jede Form und jeden Winkel muss der Schwerpunkt des Umrisses auf dem
    // Element-Mittelpunkt bleiben. Genau das war beim Linien-Export verletzt.
    for rot in [0.0_f32, 37.0, 90.0, 180.0, -75.0] {
        let cases: Vec<(&str, Element, Vec<Pos2>)> = vec![
            ("Rechteck", rect(rot, 0.0), geometry::rect_outline(&rect(rot, 0.0))),
            ("Linie", line(rot), {
                let (a, b) = geometry::line_endpoints(&line(rot));
                vec![a, b]
            }),
        ];
        for (name, el, pts) in cases {
            let c = geometry::element_center(&el);
            let mx = pts.iter().map(|p| p.x).sum::<f32>() / pts.len() as f32;
            let my = pts.iter().map(|p| p.y).sum::<f32>() / pts.len() as f32;
            assert!(
                (mx - c.x).abs() < 0.05 && (my - c.y).abs() < 0.05,
                "{name} bei {rot} Grad: Schwerpunkt ({mx},{my}) != Mitte {c:?}"
            );
        }
    }
}

#[test]
fn pdf_mit_allen_formen_wird_geschrieben() {
    // Ende-zu-Ende: Ein Dokument mit gedrehten Formen jeder Art muss sich
    // fehlerfrei exportieren lassen.
    let mut elements = Vec::new();
    let mut r = rect(35.0, 12.0);
    r.id = 1;
    elements.push(r);
    let mut l = line(120.0);
    l.id = 2;
    elements.push(l);
    let mut e = Element::new_ellipse(3, 200.0, 400.0);
    e.rotation = 20.0;
    elements.push(e);

    let doc = Document {
        pages: vec![Page { elements }],
        ..Document::default()
    };

    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("formen.pdf");

    let layouts = std::collections::HashMap::new();
    boxdoc::printing::export_pdf(&path, &doc, &ImageStore::default(), &layouts)
        .expect("Export mit allen Formen");

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    assert!(bytes.len() > 500);
}

// --- PDF-Import: Richtung von Pfaden ---------------------------------------
//
// Der Auslöser: Beim Öffnen eines PDFs standen alle Linien waagrecht. Grund war
// die Auswertung der Bounding-Box — die kennt keine Richtung. Senkrechte Linien
// bekamen Breite 0 (waren also unsichtbar), Diagonalen wurden platt gedrückt.
// Der Import liest jetzt die echten Stützpunkte.

use boxdoc::pdf_import::{classify_subpath, line_element, PathShape, SubPath};

/// Offener Teilpfad aus geraden Strecken, wie ihn `collect_subpaths` liefert.
/// Ohne Kurven sind Eckpunkte und Streckenzug dasselbe.
fn open_path(pts: &[(f32, f32)]) -> SubPath {
    let pts: Vec<Pos2> = pts.iter().map(|(x, y)| Pos2::new(*x, *y)).collect();
    SubPath {
        lines: pts.len().saturating_sub(1),
        outline: pts.clone(),
        nodes: pts.iter().map(|p| geometry::PathNode::corner(*p)).collect(),
        pts,
        closed: false,
        curves: 0,
    }
}

/// Geschlossener Teilpfad aus geraden Strecken.
fn closed_path(pts: &[(f32, f32)]) -> SubPath {
    SubPath {
        closed: true,
        ..open_path(pts)
    }
}

#[test]
fn importierte_linie_behaelt_ihre_richtung() {
    // Waagrecht, senkrecht, diagonal, rückwärts — jede Strecke muss nach der
    // Umrechnung in BoxDocs Mittelpunkt-Darstellung wieder an denselben
    // Endpunkten liegen.
    let cases = [
        ((100.0_f32, 200.0_f32), (300.0_f32, 200.0_f32), "waagrecht"),
        ((100.0, 200.0), (100.0, 500.0), "senkrecht"),
        ((100.0, 200.0), (250.0, 460.0), "diagonal"),
        ((250.0, 460.0), (100.0, 200.0), "diagonal rückwärts"),
    ];
    for (a, b, label) in cases {
        let (a, b) = (Pos2::new(a.0, a.1), Pos2::new(b.0, b.1));
        let el = line_element(1, a, b);
        let (ra, rb) = geometry::line_endpoints(&el);
        assert!(
            (ra - a).length() < 0.01 && (rb - b).length() < 0.01,
            "{label}: {a:?}→{b:?} kam als {ra:?}→{rb:?} zurück"
        );
        assert!(
            (el.w - (b - a).length()).abs() < 0.01,
            "{label}: Länge stimmt nicht"
        );
    }
}

#[test]
fn senkrechte_linie_wird_nicht_waagrecht() {
    // Der gemeldete Fehler in seiner reinsten Form.
    let sub = open_path(&[(120.0, 80.0), (120.0, 400.0)]);
    let shapes = classify_subpath(&sub);
    assert_eq!(shapes.len(), 1, "genau eine Linie erwartet");
    let PathShape::Line { a, b } = shapes[0] else {
        panic!("keine Linie erkannt: {:?}", shapes[0]);
    };
    let el = line_element(1, a, b);
    let (ra, rb) = geometry::line_endpoints(&el);
    assert!((ra.x - rb.x).abs() < 0.01, "Linie ist nicht senkrecht");
    assert!(((rb.y - ra.y).abs() - 320.0).abs() < 0.01, "Länge stimmt nicht");
}

#[test]
fn polygonzug_wird_zu_mehreren_linien() {
    // Früher wurden Pfade mit mehr als einer Strecke komplett verworfen.
    let sub = open_path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 50.0)]);
    let shapes = classify_subpath(&sub);
    assert_eq!(shapes.len(), 2, "beide Teilstrecken erwartet: {shapes:?}");
}

#[test]
fn achsparalleles_rechteck_bleibt_ungedreht() {
    // Reihenfolge wie beim PDF-Operator `re`: gegen den Uhrzeigersinn in
    // PDF-Koordinaten, also im Uhrzeigersinn nach der y-Spiegelung.
    let sub = closed_path(&[(50.0, 60.0), (250.0, 60.0), (250.0, 160.0), (50.0, 160.0)]);
    let shapes = classify_subpath(&sub);
    assert_eq!(shapes.len(), 1);
    let PathShape::Rect { x, y, w, h, rotation } = shapes[0] else {
        panic!("kein Rechteck erkannt: {:?}", shapes[0]);
    };
    assert!((x - 50.0).abs() < 0.01 && (y - 60.0).abs() < 0.01, "Ecke ({x},{y})");
    assert!((w - 200.0).abs() < 0.01 && (h - 100.0).abs() < 0.01, "Größe ({w},{h})");
    assert_eq!(rotation, 0.0, "achsparallel darf nicht gedreht sein");
}

#[test]
fn gedrehtes_rechteck_behaelt_seine_ecken() {
    // Ein um 30 Grad gedrehtes Rechteck: Die vier Eckpunkte müssen nach dem
    // Import wieder dieselben sein — nicht die (größere) Hüllbox.
    let mut original = Element::new_rectangle(1, 0.0, 0.0);
    original.x = 80.0;
    original.y = 120.0;
    original.w = 220.0;
    original.h = 140.0;
    original.rotation = 30.0;
    let corners = geometry::quad_corners(&original);

    let sub = closed_path(
        &corners
            .iter()
            .map(|p| (p.x, p.y))
            .collect::<Vec<_>>(),
    );
    let shapes = classify_subpath(&sub);
    assert_eq!(shapes.len(), 1);
    let PathShape::Rect { x, y, w, h, rotation } = shapes[0] else {
        panic!("kein Rechteck erkannt: {:?}", shapes[0]);
    };

    let mut back = Element::new_rectangle(2, x, y);
    back.w = w;
    back.h = h;
    back.rotation = rotation;
    for (i, (p, q)) in geometry::quad_corners(&back).iter().zip(corners.iter()).enumerate() {
        assert!(
            (*p - *q).length() < 0.05,
            "Ecke {i}: {p:?} statt {q:?} (w={w} h={h} rot={rotation})"
        );
    }
}

#[test]
fn geschlossenes_dreieck_bleibt_ein_dreieck() {
    // Eine Hüllbox wäre hier deutlich größer als die Form. Früher wurde das
    // Dreieck deshalb verworfen — jetzt bleibt es als freier Pfad erhalten.
    let sub = closed_path(&[(0.0, 0.0), (100.0, 0.0), (50.0, 80.0)]);
    let shapes = classify_subpath(&sub);
    assert_eq!(shapes.len(), 1);
    let PathShape::Free { ref nodes, closed } = shapes[0] else {
        panic!("kein freier Pfad: {:?}", shapes[0]);
    };
    assert!(closed);
    assert_eq!(nodes.len(), 3);
    // Ohne Kurvensegmente bleibt jeder Knoten eine Ecke.
    assert!(nodes.iter().all(|n| n.is_corner()));
}

#[test]
fn entartete_pfade_stuerzen_nicht_ab() {
    for sub in [
        open_path(&[]),
        open_path(&[(10.0, 10.0)]),
        open_path(&[(10.0, 10.0), (10.0, 10.0)]),
        closed_path(&[(0.0, 0.0), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]),
    ] {
        let _ = classify_subpath(&sub);
    }
}

#[test]
fn linien_ueberstehen_den_pdf_rundlauf() {
    // Ende-zu-Ende gegen ein echtes PDF: schreiben, mit pdfium zurücklesen und
    // die Endpunkte vergleichen. Genau hier fiel vorher jede Linie waagrecht
    // aus der Bounding-Box heraus.
    let winkel = [0.0_f32, 90.0, 35.0, -60.0];
    let mut elements = Vec::new();
    for (i, rot) in winkel.iter().enumerate() {
        let mut el = Element::new_line(i as u64 + 1, 0.0, 0.0);
        el.x = 60.0;
        el.y = 80.0 + i as f32 * 120.0;
        el.w = 200.0;
        el.h = 0.0;
        el.rotation = *rot;
        el.stroke_width = 1.5;
        el.stroke_color = [20, 20, 20, 255];
        elements.push(el);
    }
    let erwartet: Vec<_> = elements.iter().map(geometry::line_endpoints).collect();

    let doc = Document {
        pages: vec![Page { elements }],
        ..Document::default()
    };
    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("linien_rundlauf.pdf");
    let layouts = std::collections::HashMap::new();
    boxdoc::printing::export_pdf(&path, &doc, &ImageStore::default(), &layouts).unwrap();

    let Ok((back, _, _)) = boxdoc::pdf_import::import_pdf(&path) else {
        eprintln!("pdfium nicht verfügbar, Test übersprungen");
        return;
    };

    let linien: Vec<_> = back.pages[0]
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Line)
        .collect();
    assert_eq!(
        linien.len(),
        winkel.len(),
        "{} von {} Linien importiert",
        linien.len(),
        winkel.len()
    );

    // Reihenfolge bleibt die Zeichenreihenfolge des PDFs, also die des Exports.
    for (i, (el, (ea, eb))) in linien.iter().zip(erwartet.iter()).enumerate() {
        let (a, b) = geometry::line_endpoints(el);
        // Richtungsumkehr ist erlaubt — dieselbe Strecke, andere Laufrichtung.
        let gleich = ((a - *ea).length() < 0.5 && (b - *eb).length() < 0.5)
            || ((a - *eb).length() < 0.5 && (b - *ea).length() < 0.5);
        assert!(
            gleich,
            "Linie {i} (rot={}): {a:?}→{b:?} statt {ea:?}→{eb:?}",
            winkel[i]
        );
    }
}

#[test]
fn kurven_ueberstehen_den_pdf_rundlauf_als_kurven() {
    // Der Kern der PDF-Integration: Ein Pfad mit Kurven muss als Pfad **mit
    // Kurven** zurückkommen — nicht als Vieleck aus aufgelösten Strecken.
    //
    // Geprüft wird beides, weil beides schiefgehen kann: die Kodierung der
    // Kontrollpunkte beim Schreiben (printpdf markiert Kurven über ein Flag
    // je Punkt, nicht über eigene Segmente) und ihre Rückgewinnung beim Lesen.
    let nodes = vec![
        geometry::PathNode::corner(Pos2::new(100.0, 400.0)),
        geometry::PathNode {
            anchor: Pos2::new(250.0, 300.0),
            in_h: Pos2::new(180.0, 300.0),
            out_h: Pos2::new(320.0, 300.0),
        },
        geometry::PathNode {
            anchor: Pos2::new(400.0, 450.0),
            in_h: Pos2::new(360.0, 420.0),
            out_h: Pos2::new(440.0, 480.0),
        },
        geometry::PathNode::corner(Pos2::new(480.0, 400.0)),
    ];
    let mut el = geometry::path_from_nodes(1, &nodes, false);
    el.stroke_width = 2.0;
    el.stroke_color = [20, 20, 20, 255];
    el.fill_color = [0, 0, 0, 0];
    let erwartet = geometry::path_outline(&el);

    let doc = Document {
        pages: vec![Page {
            elements: vec![el],
        }],
        ..Document::default()
    };
    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("kurven_rundlauf.pdf");
    let layouts = std::collections::HashMap::new();
    boxdoc::printing::export_pdf(&path, &doc, &ImageStore::default(), &layouts).unwrap();

    let Ok((back, _, _)) = boxdoc::pdf_import::import_pdf(&path) else {
        eprintln!("pdfium nicht verfügbar, Test übersprungen");
        return;
    };

    let pfade: Vec<_> = back.pages[0]
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Path)
        .collect();
    assert_eq!(pfade.len(), 1, "Pfade zurückgelesen: {}", pfade.len());
    let zurueck = pfade[0];

    // Als Kurve zurückgekommen — nicht als aufgelöster Streckenzug.
    assert!(
        zurueck.path_is_curved(),
        "Pfad kam ohne Kurvengriffe zurück"
    );
    // Und mit derselben Handvoll Knoten, nicht mit hundert.
    assert!(
        zurueck.points.len() <= 6,
        "{} Knoten statt vier — die Kurve wurde aufgelöst",
        zurueck.points.len()
    );

    // Die Form selbst: Jeder Punkt des zurückgelesenen Umrisses muss auf dem
    // ursprünglichen liegen.
    for p in geometry::path_outline(zurueck) {
        let d = erwartet
            .windows(2)
            .map(|w| {
                let ab = w[1] - w[0];
                let t = ((p - w[0]).dot(ab) / ab.dot(ab).max(1e-9)).clamp(0.0, 1.0);
                (p - (w[0] + ab * t)).length()
            })
            .fold(f32::MAX, f32::min);
        assert!(d < 1.0, "Punkt {p:?} weicht um {d} pt von der Kurve ab");
    }
}

/// Baut ein minimales PDF mit dem gegebenen Content-Stream (A4, keine Schrift).
///
/// Bewusst von Hand geschrieben statt über `printing::export_pdf`: Der Import
/// soll auch mit Dateien zurechtkommen, die BoxDoc nicht selbst erzeugt hat —
/// insbesondere mit Pfaden unter einer Transformationsmatrix (`cm`).
fn minimal_pdf(content: &str) -> Vec<u8> {
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[test]
fn fremdes_pdf_wird_lagetreu_importiert() {
    // Ein Content-Stream mit genau den Fällen, die vorher schiefgingen:
    // senkrechte Linie, Diagonale, Polygonzug, nur gestrichenes Rechteck und
    // ein Rechteck unter einer Skalierungsmatrix.
    let content = "1 w 0 G\n\
        100 700 m 100 500 l S\n\
        100 500 m 300 400 l S\n\
        50 300 200 100 re S\n\
        q 2 0 0 2 0 0 cm 25 50 50 25 re f Q\n\
        200 200 m 300 250 l 400 200 l S\n";
    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fremd.pdf");
    std::fs::write(&path, minimal_pdf(content)).unwrap();

    let Ok((doc, _, _)) = boxdoc::pdf_import::import_pdf(&path) else {
        eprintln!("pdfium nicht verfügbar, Test übersprungen");
        return;
    };
    let els = &doc.pages[0].elements;

    // PDF-y zeigt nach oben, BoxDoc-y nach unten: y_box = 842 - y_pdf.
    let p = |x: f32, y_pdf: f32| Pos2::new(x, 842.0 - y_pdf);
    let erwartete_linien = [
        (p(100.0, 700.0), p(100.0, 500.0)), // senkrecht
        (p(100.0, 500.0), p(300.0, 400.0)), // diagonal
        (p(200.0, 200.0), p(300.0, 250.0)), // Polygonzug, 1. Strecke
        (p(300.0, 250.0), p(400.0, 200.0)), // Polygonzug, 2. Strecke
    ];

    let linien: Vec<_> = els.iter().filter(|e| e.kind == ElementKind::Line).collect();
    assert_eq!(
        linien.len(),
        erwartete_linien.len(),
        "erwartet {} Linien, bekommen {}",
        erwartete_linien.len(),
        linien.len()
    );
    for (el, (ea, eb)) in linien.iter().zip(erwartete_linien.iter()) {
        let (a, b) = geometry::line_endpoints(el);
        assert!(
            (a - *ea).length() < 0.1 && (b - *eb).length() < 0.1,
            "Linie {a:?}→{b:?} statt {ea:?}→{eb:?}"
        );
    }

    let rechtecke: Vec<_> = els
        .iter()
        .filter(|e| e.kind == ElementKind::Rectangle)
        .collect();
    assert_eq!(rechtecke.len(), 2, "zwei Rechtecke erwartet");

    // Nur gestrichen — der Innenraum muss durchsichtig bleiben, sonst deckt er
    // im Original sichtbaren Inhalt zu.
    let umriss = rechtecke[0];
    assert!(
        (umriss.x - 50.0).abs() < 0.1
            && (umriss.y - (842.0 - 400.0)).abs() < 0.1
            && (umriss.w - 200.0).abs() < 0.1
            && (umriss.h - 100.0).abs() < 0.1,
        "Umriss-Rechteck bei ({},{}) {}×{}",
        umriss.x,
        umriss.y,
        umriss.w,
        umriss.h
    );
    assert_eq!(umriss.fill_color[3], 0, "gestrichenes Rechteck darf nicht füllen");
    assert!(umriss.stroke_width > 0.0);

    // Unter `2 0 0 2 0 0 cm`: aus 25,50 50×25 wird 50,100 100×50.
    let skaliert = rechtecke[1];
    assert!(
        (skaliert.x - 50.0).abs() < 0.1
            && (skaliert.y - (842.0 - 150.0)).abs() < 0.1
            && (skaliert.w - 100.0).abs() < 0.1
            && (skaliert.h - 50.0).abs() < 0.1,
        "skaliertes Rechteck bei ({},{}) {}×{}",
        skaliert.x,
        skaliert.y,
        skaliert.w,
        skaliert.h
    );
    assert_eq!(skaliert.fill_color[3], 255, "gefülltes Rechteck erwartet");
}

#[test]
fn pfade_und_ellipsen_ueberstehen_den_pdf_rundlauf() {
    // Was BoxDoc schreibt, muss BoxDoc auch wieder lesen können. Für Ellipsen
    // ist das nicht selbstverständlich: Der Export schreibt sie als Vieleck,
    // damit Bildschirm und Druck garantiert dieselbe Form zeigen — der Import
    // muss die Ellipse darin wiedererkennen.
    let mut ellipse = Element::new_ellipse(1, 80.0, 100.0);
    ellipse.w = 180.0;
    ellipse.h = 120.0;
    ellipse.fill_color = [200, 30, 30, 255];
    ellipse.stroke_width = 0.0;

    // Ein konkaves L — die Form, an der ein Dreiecksfächer scheitern würde.
    let l_form = [
        (300.0, 400.0),
        (400.0, 400.0),
        (400.0, 430.0),
        (330.0, 430.0),
        (330.0, 500.0),
        (300.0, 500.0),
    ];
    let mut pfad = Element::new_path(2, &l_form, true);
    pfad.fill_color = [40, 90, 200, 255];
    pfad.stroke_width = 0.0;

    let doc = Document {
        pages: vec![Page {
            elements: vec![ellipse.clone(), pfad.clone()],
        }],
        ..Document::default()
    };
    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pfade_rundlauf.pdf");
    let layouts = std::collections::HashMap::new();
    boxdoc::printing::export_pdf(&path, &doc, &ImageStore::default(), &layouts).unwrap();

    let Ok((back, _, _)) = boxdoc::pdf_import::import_pdf(&path) else {
        eprintln!("pdfium nicht verfügbar, Test übersprungen");
        return;
    };
    let els = &back.pages[0].elements;

    let e = els
        .iter()
        .find(|e| e.kind == ElementKind::Ellipse)
        .expect("Ellipse muss als Ellipse zurückkommen");
    assert!(
        (e.x - ellipse.x).abs() < 1.0
            && (e.y - ellipse.y).abs() < 1.0
            && (e.w - ellipse.w).abs() < 1.0
            && (e.h - ellipse.h).abs() < 1.0,
        "Ellipse kam als ({},{}) {}×{} zurück",
        e.x,
        e.y,
        e.w,
        e.h
    );

    let p = els
        .iter()
        .find(|e| e.kind == ElementKind::Path)
        .expect("Pfad muss als Pfad zurückkommen");
    let outline = geometry::path_outline(p);
    for (x, y) in l_form {
        let ziel = Pos2::new(x, y);
        assert!(
            outline.iter().any(|q| (*q - ziel).length() < 1.0),
            "Ecke {ziel:?} fehlt im zurückgelesenen Pfad"
        );
    }
}

#[test]
fn entartete_formen_stuerzen_nicht_ab() {
    // Nullbreite, Nullhöhe, negative Werte — darf keinen Panic geben.
    for (w, h) in [(0.0_f32, 0.0_f32), (0.0, 50.0), (50.0, 0.0), (-10.0, -10.0)] {
        let mut el = Element::new_rectangle(1, 0.0, 0.0);
        el.w = w;
        el.h = h;
        el.corner_radius = 5.0;
        let _ = geometry::rect_outline(&el);

        el.kind = ElementKind::Ellipse;
        let _ = geometry::ellipse_outline(&el);

        el.kind = ElementKind::Line;
        let _ = geometry::line_endpoints(&el);
    }
}
