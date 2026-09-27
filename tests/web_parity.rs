//! Prüft, dass die Web-Version dieselben Bytes erzeugt wie die EXE.
//!
//! Der Hintergrund: PDF-, SVG- und ODT-Export liefen früher nur auf Native, weil
//! sie direkt in eine Datei schrieben. Für den Browser wurde jeder Export in
//! zwei Teile geschnitten — einen, der die Bytes baut (`pdf_bytes`,
//! `svg_string`, `export_to_bytes`), und einen dünnen Datei-Wrapper darüber.
//!
//! Genau an dieser Naht kann die Angleichung wieder auseinanderfallen: Wenn
//! jemand später eine Änderung nur in den Datei-Zweig einbaut, exportiert die
//! Web-Version stillschweigend etwas anderes als die EXE. Diese Tests machen
//! daraus einen Fehlschlag statt eines unbemerkten Unterschieds.
//!
//! Die Tests laufen nur nativ — es gibt keinen Browser, in dem sie laufen
//! könnten. Sie prüfen aber genau die Funktionen, die die Web-Version aufruft.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;

use boxdoc::model::{Document, Element, Page};
use boxdoc::store::ImageStore;

/// Ein Dokument mit je einem Element jeder Art, damit alle Zeichenpfade
/// tatsächlich durchlaufen werden.
fn doc_mit_allen_formen() -> Document {
    let mut elements = Vec::new();

    let mut text = Element::new_text(1, 40.0, 60.0);
    text.text = String::from("Hallo Welt\nzweite Zeile mit Ümlaut");
    text.w = 300.0;
    text.h = 60.0;
    elements.push(text);

    let mut rect = Element::new_rectangle(2, 50.0, 200.0);
    rect.rotation = 17.0;
    rect.corner_radius = 8.0;
    elements.push(rect);

    let mut ellipse = Element::new_ellipse(3, 200.0, 400.0);
    ellipse.rotation = 20.0;
    elements.push(ellipse);

    let mut line = Element::new_line(4, 60.0, 600.0);
    line.rotation = 33.0;
    elements.push(line);

    Document {
        pages: vec![Page { elements }],
        ..Document::default()
    }
}

fn tmp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("boxdoc_tests_web_parity");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn pdf_download_und_pdf_datei_haben_denselben_inhalt() {
    let doc = doc_mit_allen_formen();
    let images = ImageStore::default();
    let layouts: HashMap<u64, boxdoc::text_layout::TextLayout> = HashMap::new();

    // Der Weg der Web-Version.
    let aus_bytes = boxdoc::printing::pdf_bytes(&doc, &images, &layouts).expect("pdf_bytes");

    // Der Weg der EXE.
    let path = tmp_dir().join("parity.pdf");
    boxdoc::printing::export_pdf(&path, &doc, &images, &layouts).expect("export_pdf");
    let aus_datei = std::fs::read(&path).unwrap();

    assert!(aus_bytes.starts_with(b"%PDF"), "kein PDF-Header");
    assert!(aus_bytes.len() > 500, "PDF verdächtig klein");

    // Wichtig: NICHT auf Byte-Gleichheit prüfen. Ein PDF enthält einen
    // Zeitstempel, und zwischen den beiden Aufrufen kann eine Sekunde
    // vergehen. Verglichen wird die Größe — zwei Exporte desselben Dokuments
    // dürfen sich nur im Zeitstempel unterscheiden, und der ist fix lang.
    let diff = aus_bytes.len().abs_diff(aus_datei.len());
    assert!(
        diff <= 8,
        "Download-PDF ({} B) und Datei-PDF ({} B) unterscheiden sich um {diff} B — \
         mehr als ein Zeitstempel erklärt",
        aus_bytes.len(),
        aus_datei.len()
    );
}

#[test]
fn svg_download_und_svg_datei_haben_denselben_inhalt() {
    let doc = doc_mit_allen_formen();
    let images = ImageStore::default();
    let layouts: HashMap<u64, boxdoc::text_layout::TextLayout> = HashMap::new();
    let scope = boxdoc::svg::Scope::Page(0);

    // Der Weg der Web-Version.
    let aus_string = boxdoc::svg::svg_string(&doc, &images, &layouts, &scope).expect("svg_string");

    // Der Weg der EXE.
    let path = tmp_dir().join("parity.svg");
    boxdoc::svg::export_svg(&path, &doc, &images, &layouts, &scope).expect("export_svg");
    let aus_datei = std::fs::read_to_string(&path).unwrap();

    // SVG hat keinen Zeitstempel — hier muss es exakt gleich sein.
    assert_eq!(
        aus_string, aus_datei,
        "Download-SVG und Datei-SVG unterscheiden sich"
    );
    assert!(aus_string.contains("<svg"), "kein SVG-Wurzelelement");
}

#[test]
fn odt_download_und_odt_datei_haben_denselben_inhalt() {
    let doc = doc_mit_allen_formen();
    let images = ImageStore::default();

    // Der Weg der Web-Version.
    let aus_bytes = boxdoc::odt::export_to_bytes(&doc, &images).expect("export_to_bytes");

    // Der Weg der EXE.
    let path = tmp_dir().join("parity.odt");
    boxdoc::odt::export(&path, &doc, &images).expect("export");
    let aus_datei = std::fs::read(&path).unwrap();

    assert_eq!(
        aus_bytes, aus_datei,
        "Download-ODT und Datei-ODT unterscheiden sich"
    );
    // ODT ist ein ZIP, das mit dem unkomprimierten mimetype-Eintrag beginnt.
    assert!(aus_bytes.starts_with(b"PK"), "kein ZIP-Container");
}

#[test]
fn odt_ueberlebt_die_runde_durch_den_speicher() {
    // Der Import der Web-Version bekommt Bytes, keinen Pfad. Er muss dasselbe
    // lesen können, was der Export geschrieben hat.
    let doc = doc_mit_allen_formen();
    let images = ImageStore::default();

    let bytes = boxdoc::odt::export_to_bytes(&doc, &images).expect("export_to_bytes");
    let (zurueck, _images, next_id) =
        boxdoc::odt::import_from_bytes(&bytes).expect("import_from_bytes");

    assert_eq!(zurueck.pages.len(), 1, "Seitenzahl verändert");
    // Der ODT-Import ist Best-Effort: Formen ohne Text und ohne Bild sind in
    // ODT reine Zeichenobjekte, die BoxDoc nicht zurückliest. Der Text muss
    // aber ankommen — sonst ist die Runde wertlos.
    let texte: Vec<&String> = zurueck.pages[0].elements.iter().map(|e| &e.text).collect();
    assert!(
        texte.iter().any(|t| t.contains("Hallo Welt")),
        "Text nach der Runde durch den Speicher verloren: {texte:?}"
    );
    assert!(next_id > 1, "next_id nicht hochgezählt");
}

#[test]
fn odt_import_von_pfad_und_von_bytes_liefern_dasselbe() {
    let doc = doc_mit_allen_formen();
    let images = ImageStore::default();

    let path = tmp_dir().join("import_parity.odt");
    boxdoc::odt::export(&path, &doc, &images).expect("export");

    let (von_pfad, _i1, id1) = boxdoc::odt::import(&path).expect("import von Pfad");
    let bytes = std::fs::read(&path).unwrap();
    let (von_bytes, _i2, id2) = boxdoc::odt::import_from_bytes(&bytes).expect("import von Bytes");

    assert_eq!(id1, id2, "next_id weicht ab");
    assert_eq!(
        von_pfad.pages[0].elements.len(),
        von_bytes.pages[0].elements.len(),
        "Elementzahl weicht ab"
    );
    assert_eq!(
        serde_json::to_string(&von_pfad).unwrap(),
        serde_json::to_string(&von_bytes).unwrap(),
        "Dokument aus Pfad und aus Bytes unterscheiden sich"
    );
}
