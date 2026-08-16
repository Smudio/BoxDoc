//! Prüft, dass ein PDF **so** ankommt, wie es aussieht.
//!
//! Die Tests bauen ihre PDFs von Hand aus Content-Stream-Operatoren, statt sie
//! über BoxDocs eigenen Export zu erzeugen. Das ist Absicht: Ein Import, der
//! nur die eigenen Dateien versteht, ist wertlos. Hier stehen genau die
//! Konstruktionen, an denen der Import reihenweise gescheitert ist —
//! senkrechte Linien, gedrehter Text, verschachtelte Form-XObjects, gedrehte
//! Bilder, freie Kurven.

#![cfg(not(target_arch = "wasm32"))]

use boxdoc::geometry;
use boxdoc::model::{Element, ElementKind};
use egui::Pos2;

const PAGE_H: f32 = 842.0; // A4 Hochformat in pt

/// Baut ein minimales PDF aus fertigen Objekt-Bodies (1-basiert nummeriert).
fn build_pdf(objs: &[String]) -> Vec<u8> {
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

fn stream(dict: &str, content: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Ein einseitiges PDF mit dem gegebenen Content-Stream. `extra_objects`
/// hängt weitere Objekte an (ab Nummer 5), `resources` verweist darauf.
fn pdf_with(content: &str, resources: &str, extra_objects: &[String]) -> Vec<u8> {
    let mut objs = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R \
             /Resources << {resources} >> >>"
        ),
        stream("", content),
    ];
    objs.extend_from_slice(extra_objects);
    build_pdf(&objs)
}

/// pdfium hält globalen Zustand und ist nicht threadsicher; `cargo test` lässt
/// die Tests aber parallel laufen. In der Anwendung passiert der Import immer
/// auf dem UI-Thread — hier muss er von Hand serialisiert werden.
static PDFIUM: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Schreibt das PDF und importiert es. `None`, wenn pdfium nicht verfügbar ist
/// — dann überspringt der Test sich selbst, statt falsch zu scheitern.
fn import(name: &str, bytes: Vec<u8>) -> Option<Vec<Element>> {
    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let _guard = PDFIUM.lock().unwrap_or_else(|e| e.into_inner());
    match boxdoc::pdf_import::import_pdf(&path) {
        Ok((doc, _, _)) => Some(doc.pages[0].elements.clone()),
        Err(_) => {
            eprintln!("pdfium nicht verfügbar, Test übersprungen");
            None
        }
    }
}

/// PDF-Punkt → BoxDoc-Seitenkoordinate (y-Achse gespiegelt).
fn p(x: f32, y_pdf: f32) -> Pos2 {
    Pos2::new(x, PAGE_H - y_pdf)
}

fn only<'a>(els: &'a [Element], kind: ElementKind) -> &'a Element {
    let matching: Vec<_> = els.iter().filter(|e| e.kind == kind).collect();
    assert_eq!(
        matching.len(),
        1,
        "genau ein Element vom Typ {kind:?} erwartet, gefunden: {}",
        matching.len()
    );
    matching[0]
}

/// Standard-Helvetica als Ressource — braucht keine eingebettete Schriftdatei.
const FONT_RES: &str = "/Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >>";

// --- Text -------------------------------------------------------------------

#[test]
fn gedrehter_text_behaelt_winkel_lage_und_groesse() {
    // `0 1 -1 0` dreht die Schreibrichtung um 90 Grad gegen den Uhrzeigersinn
    // (in PDF-Koordinaten) — auf dem Bildschirm also nach oben.
    let content = "BT /F1 12 Tf 1 0 0 1 60 700 Tm (Waagrecht) Tj ET\n\
                   BT /F1 12 Tf 0 1 -1 0 300 400 Tm (Senkrecht) Tj ET\n";
    let Some(els) = import("text_rot.pdf", pdf_with(content, FONT_RES, &[])) else {
        return;
    };

    let texts: Vec<_> = els.iter().filter(|e| e.kind == ElementKind::Text).collect();
    assert_eq!(texts.len(), 2, "beide Textstücke erwartet");

    let waagrecht = texts.iter().find(|e| e.text == "Waagrecht").unwrap();
    assert_eq!(waagrecht.rotation, 0.0);
    assert!((waagrecht.font_size - 12.0).abs() < 0.1);

    let senkrecht = texts.iter().find(|e| e.text == "Senkrecht").unwrap();
    assert!(
        (senkrecht.rotation + 90.0).abs() < 0.5,
        "Drehung war {}",
        senkrecht.rotation
    );
    // Die Schriftgröße kam früher als 4 an: pdfium skaliert sie mit dem
    // d-Glied der Matrix, und das ist bei 90 Grad null.
    assert!(
        (senkrecht.font_size - 12.0).abs() < 0.1,
        "Schriftgröße war {}",
        senkrecht.font_size
    );
    // Die Box muss so lang sein wie der Text — nicht so schmal wie eine Zeile.
    assert!(
        senkrecht.w > 40.0 && senkrecht.w < 70.0,
        "Textbreite war {}",
        senkrecht.w
    );
    assert!(
        senkrecht.h < 20.0,
        "Textzeile darf nicht hoch sein, war {}",
        senkrecht.h
    );
    // Und sie muss dort liegen, wo der Text steht: Mitte der Hüllbox.
    let center = geometry::element_center(senkrecht);
    assert!(
        (center.x - 295.7).abs() < 2.0 && (center.y - (PAGE_H - 427.2)).abs() < 2.0,
        "Mitte lag bei {center:?}"
    );
}

#[test]
fn unsichtbarer_text_wird_ausgelassen() {
    // Render-Modus 3 = unsichtbar. Gescannte PDFs legen so ihre
    // OCR-Erkennung über das Seitenbild; sichtbar importiert stünde alles
    // doppelt da.
    let content = "BT /F1 12 Tf 1 0 0 1 60 700 Tm (Sichtbar) Tj ET\n\
                   BT /F1 12 Tf 3 Tr 1 0 0 1 60 650 Tm (Unsichtbar) Tj ET\n";
    let Some(els) = import("text_invisible.pdf", pdf_with(content, FONT_RES, &[])) else {
        return;
    };

    let texts: Vec<_> = els.iter().filter(|e| e.kind == ElementKind::Text).collect();
    assert_eq!(texts.len(), 1, "nur der sichtbare Text erwartet: {texts:#?}");
    assert_eq!(texts[0].text, "Sichtbar");
}

// --- Verschachtelung --------------------------------------------------------

#[test]
fn inhalt_aus_form_xobjects_kommt_an_der_richtigen_stelle_an() {
    // Ein Form-XObject ist ein eingebettetes Miniatur-Dokument. Sein Inhalt
    // liegt in seinem eigenen Koordinatensystem — früher wurde er komplett
    // ignoriert, die halbe Seite fehlte also.
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 100 100]",
        "0 0 1 rg\n10 10 50 20 re f\n",
    );
    let content = "q 1 0 0 1 200 500 cm /Fm1 Do Q\n";
    let Some(els) = import(
        "form_xobject.pdf",
        pdf_with(content, "/XObject << /Fm1 5 0 R >>", &[form]),
    ) else {
        return;
    };

    let rect = only(&els, ElementKind::Rectangle);
    // 10,10 im Formular + Verschiebung 200,500 → 210,510 auf der Seite.
    assert!(
        (rect.x - 210.0).abs() < 0.1 && (rect.y - (PAGE_H - 530.0)).abs() < 0.1,
        "Rechteck lag bei ({},{})",
        rect.x,
        rect.y
    );
    assert!((rect.w - 50.0).abs() < 0.1 && (rect.h - 20.0).abs() < 0.1);
    assert_eq!(rect.fill_color, [0, 0, 255, 255], "Blau aus dem Formular");
}

// --- Bilder -----------------------------------------------------------------

/// 4×2-Pixel-Bild, obere Zeile rot, untere blau — asymmetrisch, damit eine
/// verlorene Drehung auffiele.
fn image_object() -> String {
    stream(
        "/Type /XObject /Subtype /Image /Width 4 /Height 2 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Filter /ASCIIHexDecode",
        "ff0000 ff0000 ff0000 ff0000 0000ff 0000ff 0000ff 0000ff>",
    )
}

#[test]
fn gedrehtes_bild_behaelt_drehung_und_seitenverhaeltnis() {
    // `0 100 -200 0` stellt das Bild hochkant: Die Bildbreite läuft 100 pt
    // nach oben, die Bildhöhe 200 pt nach links.
    let content = "q 0 100 -200 0 400 100 cm /Im1 Do Q\n";
    let Some(els) = import(
        "bild_rot.pdf",
        pdf_with(content, "/XObject << /Im1 5 0 R >>", &[image_object()]),
    ) else {
        return;
    };

    let img = only(&els, ElementKind::Image);
    assert!(
        (img.rotation + 90.0).abs() < 0.5,
        "Drehung war {}",
        img.rotation
    );
    // Unrotiert ist das Bild 100 breit und 200 hoch — nicht 200 × 100 wie
    // seine Hüllbox.
    assert!(
        (img.w - 100.0).abs() < 0.5 && (img.h - 200.0).abs() < 0.5,
        "Größe war {}×{}",
        img.w,
        img.h
    );
    assert_eq!((img.image_w, img.image_h), (4, 2), "Pixelmaße des Bilds");

    // Die vier Ecken müssen auf der Fläche liegen, die das PDF beschreibt.
    let corners = geometry::quad_corners(img);
    for c in corners {
        assert!(
            c.x >= 199.0 && c.x <= 401.0 && c.y >= PAGE_H - 201.0 && c.y <= PAGE_H - 99.0,
            "Ecke {c:?} liegt außerhalb der Platzierung"
        );
    }
}

#[test]
fn ungedrehtes_bild_liegt_wie_gehabt() {
    let content = "q 200 0 0 100 100 600 cm /Im1 Do Q\n";
    let Some(els) = import(
        "bild_gerade.pdf",
        pdf_with(content, "/XObject << /Im1 5 0 R >>", &[image_object()]),
    ) else {
        return;
    };

    let img = only(&els, ElementKind::Image);
    assert_eq!(img.rotation, 0.0);
    assert!((img.x - 100.0).abs() < 0.1, "x war {}", img.x);
    assert!(
        (img.y - (PAGE_H - 700.0)).abs() < 0.1,
        "y war {} (erwartet {})",
        img.y,
        PAGE_H - 700.0
    );
    assert!((img.w - 200.0).abs() < 0.1 && (img.h - 100.0).abs() < 0.1);
}

// --- Vektorgrafik -----------------------------------------------------------

#[test]
fn linien_behalten_ihre_richtung() {
    // Der ursprüngliche Fehler: Alles kam waagrecht an, weil die Hüllbox
    // ausgewertet wurde — und die kennt keine Richtung.
    let content = "1 w 0 G\n\
        100 700 m 100 500 l S\n\
        100 500 m 300 400 l S\n\
        200 200 m 300 250 l 400 200 l S\n";
    let Some(els) = import("linien.pdf", pdf_with(content, "", &[])) else {
        return;
    };

    let erwartet = [
        (p(100.0, 700.0), p(100.0, 500.0)), // senkrecht
        (p(100.0, 500.0), p(300.0, 400.0)), // diagonal
        (p(200.0, 200.0), p(300.0, 250.0)), // Polygonzug, 1. Strecke
        (p(300.0, 250.0), p(400.0, 200.0)), // Polygonzug, 2. Strecke
    ];
    let linien: Vec<_> = els.iter().filter(|e| e.kind == ElementKind::Line).collect();
    assert_eq!(linien.len(), erwartet.len(), "Linienzahl");

    for (el, (ea, eb)) in linien.iter().zip(erwartet.iter()) {
        let (a, b) = geometry::line_endpoints(el);
        assert!(
            (a - *ea).length() < 0.1 && (b - *eb).length() < 0.1,
            "Linie {a:?}→{b:?} statt {ea:?}→{eb:?}"
        );
    }
}

#[test]
fn rechtecke_behalten_drehung_und_fuellung() {
    // Zwei Fälle: ein nur gestrichenes achsparalleles Rechteck (der Innenraum
    // muss durchsichtig bleiben) und ein um 45 Grad gedrehtes Quadrat.
    let content = "1 w 0 G\n\
        50 300 200 100 re S\n\
        0 0 1 rg 100 600 m 150 650 l 200 600 l 150 550 l h f\n";
    let Some(els) = import("rechtecke.pdf", pdf_with(content, "", &[])) else {
        return;
    };

    let rects: Vec<_> = els
        .iter()
        .filter(|e| e.kind == ElementKind::Rectangle)
        .collect();
    assert_eq!(rects.len(), 2, "zwei Rechtecke erwartet: {rects:#?}");

    let umriss = rects[0];
    assert_eq!(umriss.rotation, 0.0);
    assert!((umriss.w - 200.0).abs() < 0.1 && (umriss.h - 100.0).abs() < 0.1);
    assert_eq!(
        umriss.fill_color[3], 0,
        "nur gestrichen — darf nicht füllen"
    );

    let gedreht = rects[1];
    assert!(
        (gedreht.rotation.abs() - 45.0).abs() < 0.5,
        "Drehung war {}",
        gedreht.rotation
    );
    assert!(
        (gedreht.w - 70.71).abs() < 0.2 && (gedreht.h - 70.71).abs() < 0.2,
        "Kantenlänge war {}×{}",
        gedreht.w,
        gedreht.h
    );
    assert_eq!(gedreht.fill_color, [0, 0, 255, 255]);
    // Die Ecken müssen die Raute des Originals sein.
    let corners = geometry::quad_corners(gedreht);
    for erwartet in [
        p(100.0, 600.0),
        p(150.0, 650.0),
        p(200.0, 600.0),
        p(150.0, 550.0),
    ] {
        assert!(
            corners.iter().any(|c| (*c - erwartet).length() < 0.2),
            "Ecke {erwartet:?} fehlt in {corners:?}"
        );
    }
}

#[test]
fn kreis_wird_als_ellipse_erkannt() {
    // Die übliche Vier-Bogen-Näherung eines Kreises (Kappa ≈ 0,5523).
    let content = "0 g 250 700 m \
        250 727.6 227.6 750 200 750 c \
        172.4 750 150 727.6 150 700 c \
        150 672.4 172.4 650 200 650 c \
        227.6 650 250 672.4 250 700 c h f\n";
    let Some(els) = import("kreis.pdf", pdf_with(content, "", &[])) else {
        return;
    };

    let e = only(&els, ElementKind::Ellipse);
    assert!(
        (e.w - 100.0).abs() < 1.0 && (e.h - 100.0).abs() < 1.0,
        "Größe war {}×{}",
        e.w,
        e.h
    );
    assert!((e.x - 150.0).abs() < 1.0 && (e.y - (PAGE_H - 750.0)).abs() < 1.0);
}

#[test]
fn abgerundetes_rechteck_wird_keine_ellipse() {
    // Vier Bögen plus vier Geraden. Früher entschied allein die Zahl der
    // Bögen — damit wurde daraus eine Ellipse, also eine sichtbar andere Form.
    // Jetzt wird die Form selbst geprüft; als Pfad bleibt sie exakt erhalten.
    let content = "0.5 g 60 200 m 140 200 l \
        150 200 150 210 150 210 c 150 260 l \
        150 270 140 270 140 270 c 70 270 l \
        60 270 60 260 60 260 c 60 210 l \
        60 200 60 200 60 200 c h f\n";
    let Some(els) = import("rundrechteck.pdf", pdf_with(content, "", &[])) else {
        return;
    };

    assert!(
        !els.iter().any(|e| e.kind == ElementKind::Ellipse),
        "darf keine Ellipse werden: {els:#?}"
    );
    let path = only(&els, ElementKind::Path);
    assert!(path.path_closed);
    assert!(path.points.len() > 8, "Rundungen müssen erhalten bleiben");
    assert!(
        (path.x - 60.0).abs() < 0.1
            && (path.y - (PAGE_H - 270.0)).abs() < 0.1
            && (path.w - 90.0).abs() < 0.1
            && (path.h - 70.0).abs() < 0.1,
        "Lage/Größe: ({},{}) {}×{}",
        path.x,
        path.y,
        path.w,
        path.h
    );
}

#[test]
fn offene_kurve_bleibt_als_pfad_erhalten() {
    // Früher wurde sie ersatzlos verworfen — eine gezeichnete Linie fehlte
    // dann einfach.
    let content = "1 0 0 RG 2 w 300 600 m 340 660 380 660 420 600 c S\n";
    let Some(els) = import("kurve.pdf", pdf_with(content, "", &[])) else {
        return;
    };

    let path = only(&els, ElementKind::Path);
    assert!(!path.path_closed, "offene Kurve darf nicht geschlossen sein");
    assert_eq!(path.fill_color[3], 0, "offener Pfad wird nie gefüllt");
    assert!(path.stroke_width > 0.0);
    assert_eq!(path.stroke_color, [255, 0, 0, 255]);

    // Der Umriss muss auf der Kurve liegen: Start- und Endpunkt exakt, der
    // Scheitel dazwischen.
    let outline = geometry::path_outline(path);
    assert!((outline[0] - p(300.0, 600.0)).length() < 0.5);
    assert!((*outline.last().unwrap() - p(420.0, 600.0)).length() < 0.5);
    let hoechster = outline.iter().fold(f32::MAX, |acc, q| acc.min(q.y));
    assert!(
        (hoechster - (PAGE_H - 645.0)).abs() < 1.0,
        "Scheitel der Kurve lag bei {hoechster}"
    );
}

#[test]
fn gefuellter_pfad_ohne_h_verschwindet_nicht() {
    // `f` schließt den Pfad implizit — ein `h` davor ist nicht nötig. Wer das
    // ignoriert, verliert einen Großteil aller gefüllten Formen.
    let content = "0 0 1 rg 100 300 m 200 300 l 150 380 l f\n";
    let Some(els) = import("implizit.pdf", pdf_with(content, "", &[])) else {
        return;
    };

    let path = only(&els, ElementKind::Path);
    assert!(path.path_closed);
    assert_eq!(path.fill_color, [0, 0, 255, 255]);
    assert_eq!(path.points.len(), 3, "Dreieck mit drei Ecken");
}
