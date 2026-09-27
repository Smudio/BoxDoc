//! SVG-Export: Geometrie, Auswahl-Beschnitt und Wohlgeformtheit.
//!
//! Das Modul erzeugt reinen Text, deshalb prüft dieser Test das **tatsächliche
//! Ergebnis** und keine Zwischenwerte: Was hier zugesichert wird, steht so in
//! der Datei.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;

use boxdoc::model::{Crop, Document, Element, ElementKind, Page};
use boxdoc::store::ImageStore;
use boxdoc::svg::{svg_string, Scope, SELECTION_MARGIN};
use boxdoc::text_layout::TextLayout;

/// egui-Kontext wie ihn die Anwendung aufsetzt.
///
/// `fonts::install` ist nötig, nicht Beiwerk: Ohne die registrierten
/// Bold/Italic-Familien panickt epaint, sobald ein fetter Text layoutet wird.
fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    boxdoc::fonts::install(&ctx);
    let _ = ctx.run(Default::default(), |_| {});
    ctx
}

fn leer() -> (ImageStore, HashMap<u64, TextLayout>) {
    (ImageStore::default(), HashMap::new())
}

fn doc_mit(elements: Vec<Element>) -> Document {
    Document {
        pages: vec![Page { elements }],
        ..Document::default()
    }
}

fn rechteck(id: u64, x: f32, y: f32) -> Element {
    let mut el = Element::new_rectangle(id, x, y);
    el.w = 100.0;
    el.h = 50.0;
    el
}

fn seite(doc: &Document) -> String {
    let (images, layouts) = leer();
    svg_string(doc, &images, &layouts, &Scope::Page(0)).expect("Export")
}

// ---------------------------------------------------------------------------
// Grundgerüst
// ---------------------------------------------------------------------------

#[test]
fn leinwand_einer_seite_ist_das_seitenformat() {
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0)]);
    let svg = seite(&doc);
    let (w, h) = doc.page_size_pt();
    assert!(
        svg.contains(&format!("viewBox=\"0 0 {} {}\"", fmt(w), fmt(h))),
        "viewBox fehlt oder stimmt nicht:\n{svg}"
    );
    assert!(svg.contains("fill=\"#ffffff\""), "Seite braucht weißen Grund");
}

#[test]
fn hintergrund_keines_ergibt_transparentes_svg() {
    let mut doc = doc_mit(vec![rechteck(1, 50.0, 50.0)]);
    doc.background = None;
    let svg = seite(&doc);
    // Genau ein Rechteck: das Objekt selbst, kein Hintergrund.
    assert_eq!(
        svg.matches("<rect").count(),
        1,
        "ohne Hintergrund darf kein Hintergrund-Rechteck stehen:\n{svg}"
    );
}

#[test]
fn hintergrund_farbe_und_deckkraft_landen_im_svg() {
    let mut doc = doc_mit(vec![]);
    doc.background = Some([80, 140, 220, 128]);
    let svg = seite(&doc);
    assert!(
        svg.contains("fill=\"#508cdc\""),
        "Farbe fehlt im Hintergrund:\n{svg}"
    );
    assert!(
        svg.contains(&format!("fill-opacity=\"{}\"", fmt(128.0 / 255.0))),
        "Deckkraft fehlt im Hintergrund:\n{svg}"
    );
}

/// Dieselbe Zahlenformatierung wie im Modul (drei Stellen, ohne Nullen).
fn fmt(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[test]
fn jedes_element_wird_zu_seinem_eigenen_svg_tag() {
    let mut ellipse = rechteck(2, 200.0, 50.0);
    ellipse.kind = ElementKind::Ellipse;
    let mut linie = rechteck(3, 50.0, 200.0);
    linie.kind = ElementKind::Line;
    linie.h = 0.0;
    let pfad = Element::new_path(4, &[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)], false);

    let svg = seite(&doc_mit(vec![rechteck(1, 50.0, 50.0), ellipse, linie, pfad]));
    assert!(svg.contains("<rect"), "Rechteck bleibt ein <rect>");
    assert!(svg.contains("<ellipse"), "Ellipse bleibt eine <ellipse>");
    assert!(svg.contains("<line"), "Linie bleibt eine <line>");
    assert!(svg.contains("<path"), "Pfad bleibt ein <path>");
}

#[test]
fn tags_sind_alle_geschlossen() {
    let mut text = Element::new_text(9, 40.0, 40.0);
    text.text = String::from("Hallo & <Welt>");
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0), text]);
    let layouts = boxdoc::printing::collect_layouts(&ctx(), &doc);
    let svg = svg_string(&doc, &ImageStore::default(), &layouts, &Scope::Page(0)).expect("Export");

    assert_eq!(svg.matches("<svg").count(), 1);
    assert_eq!(svg.matches("</svg>").count(), 1);
    assert_eq!(
        svg.matches("<text").count(),
        svg.matches("</text>").count(),
        "offenes <text>:\n{svg}"
    );
    assert_eq!(
        svg.matches("<tspan").count(),
        svg.matches("</tspan>").count()
    );
}

// ---------------------------------------------------------------------------
// Auswahl
// ---------------------------------------------------------------------------

#[test]
fn auswahl_enthaelt_nur_die_gewaehlten_objekte() {
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0), rechteck(2, 300.0, 400.0)]);
    let (images, layouts) = leer();
    let svg = svg_string(
        &doc,
        &images,
        &layouts,
        &Scope::Selection {
            page: 0,
            ids: vec![2],
        },
    )
    .expect("Export");

    assert_eq!(svg.matches("<rect").count(), 1, "nur ein Rechteck:\n{svg}");
    assert!(svg.contains("x=\"300\""), "und zwar das ausgewählte:\n{svg}");
}

#[test]
fn auswahl_wird_auf_ihre_huellbox_plus_rand_beschnitten() {
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0), rechteck(2, 300.0, 400.0)]);
    let (images, layouts) = leer();
    let svg = svg_string(
        &doc,
        &images,
        &layouts,
        &Scope::Selection {
            page: 0,
            ids: vec![2],
        },
    )
    .expect("Export");

    // Element: 300,400 bis 400,450. Plus Rand.
    let m = SELECTION_MARGIN;
    let erwartet = format!(
        "viewBox=\"{} {} {} {}\"",
        fmt(300.0 - m),
        fmt(400.0 - m),
        fmt(100.0 + 2.0 * m),
        fmt(50.0 + 2.0 * m)
    );
    assert!(svg.contains(&erwartet), "erwartet {erwartet} in:\n{svg}");
}

#[test]
fn auswahl_bleibt_durchsichtig() {
    // Eine exportierte Auswahl soll sich in ein anderes Dokument legen lassen,
    // ohne einen weißen Kasten mitzubringen.
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0)]);
    let (images, layouts) = leer();
    let svg = svg_string(
        &doc,
        &images,
        &layouts,
        &Scope::Selection {
            page: 0,
            ids: vec![1],
        },
    )
    .expect("Export");
    assert!(
        !svg.contains("fill=\"#ffffff\""),
        "kein weißer Hintergrund:\n{svg}"
    );
}

#[test]
fn gedrehtes_objekt_passt_vollstaendig_auf_die_leinwand() {
    // Ein um 45° gedrehtes Rechteck ragt über x/y/w/h hinaus. Wer die Leinwand
    // aus der Box statt aus der Kontur aufspannt, schneidet die Ecken ab.
    let mut el = rechteck(1, 100.0, 100.0);
    el.rotation = 45.0;
    let doc = doc_mit(vec![el]);
    let (images, layouts) = leer();
    let svg = svg_string(
        &doc,
        &images,
        &layouts,
        &Scope::Selection {
            page: 0,
            ids: vec![1],
        },
    )
    .expect("Export");

    let ecken = boxdoc::geometry::quad_corners(&doc.pages[0].elements[0]);
    let view = view_box(&svg);
    for p in ecken {
        assert!(
            p.x >= view.0 && p.x <= view.0 + view.2 && p.y >= view.1 && p.y <= view.1 + view.3,
            "Ecke {p:?} liegt außerhalb der Leinwand {view:?}"
        );
    }
}

/// Liest `viewBox` als (x, y, w, h).
fn view_box(svg: &str) -> (f32, f32, f32, f32) {
    let start = svg.find("viewBox=\"").expect("viewBox") + 9;
    let rest = &svg[start..];
    let end = rest.find('"').expect("Ende der viewBox");
    let v: Vec<f32> = rest[..end]
        .split_whitespace()
        .map(|s| s.parse().expect("Zahl"))
        .collect();
    (v[0], v[1], v[2], v[3])
}

#[test]
fn leere_auswahl_meldet_einen_fehler() {
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0)]);
    let (images, layouts) = leer();
    let r = svg_string(
        &doc,
        &images,
        &layouts,
        &Scope::Selection {
            page: 0,
            ids: vec![],
        },
    );
    assert!(r.is_err(), "leere Auswahl darf keine leere Datei erzeugen");
}

#[test]
fn seite_ausserhalb_des_dokuments_meldet_einen_fehler() {
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0)]);
    let (images, layouts) = leer();
    assert!(svg_string(&doc, &images, &layouts, &Scope::Page(7)).is_err());
}

#[test]
fn auswahl_behaelt_die_stapelreihenfolge_der_seite() {
    // Nicht die Klickreihenfolge: Sonst läge das Objekt oben, das zufällig
    // zuletzt angeklickt wurde.
    let doc = doc_mit(vec![rechteck(1, 50.0, 50.0), rechteck(2, 60.0, 60.0)]);
    let (images, layouts) = leer();
    let svg = svg_string(
        &doc,
        &images,
        &layouts,
        &Scope::Selection {
            page: 0,
            ids: vec![2, 1], // umgekehrt ausgewählt
        },
    )
    .expect("Export");

    let erst = svg.find("x=\"50\"").expect("Element 1");
    let dann = svg.find("x=\"60\"").expect("Element 2");
    assert!(erst < dann, "Seitenreihenfolge muss gewinnen:\n{svg}");
}

// ---------------------------------------------------------------------------
// Treue zur Vorlage
// ---------------------------------------------------------------------------

#[test]
fn kurven_bleiben_kurven() {
    // Der Kern des Ganzen: Ein Pfad mit Griffen darf nicht als Vieleck mit
    // zweihundert Ecken herauskommen, sondern als kubische Bézier-Segmente.
    let mut el = Element::new_path(1, &[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)], false);
    el.handles = vec![
        [0.0, 1.0, 0.0, 1.0],
        [0.25, 0.0, 0.75, 0.0],
        [1.0, 1.0, 1.0, 1.0],
    ];
    assert!(el.path_is_curved(), "Testvorlage muss gekrümmt sein");

    let svg = seite(&doc_mit(vec![el]));
    let d = attribut(&svg, "d=\"");
    assert!(d.contains(" C "), "kubische Segmente erwartet, bekam: {d}");
    assert!(
        d.matches(" C ").count() <= 4,
        "eine flachgeklopfte Kurve hätte viel mehr Segmente: {d}"
    );
}

#[test]
fn offener_pfad_wird_nicht_gefuellt_und_nicht_geschlossen() {
    let el = Element::new_path(1, &[(0.0, 0.0), (1.0, 1.0)], false);
    let svg = seite(&doc_mit(vec![el]));
    let d = attribut(&svg, "d=\"");
    assert!(!d.contains('Z'), "offener Pfad darf nicht geschlossen sein");
    assert!(
        svg.contains("fill=\"none\""),
        "offener Pfad darf nie gefüllt werden:\n{svg}"
    );
}

#[test]
fn geschlossener_pfad_wird_geschlossen_und_gefuellt() {
    let mut el = Element::new_path(1, &[(0.0, 0.0), (1.0, 0.0), (0.5, 1.0)], true);
    el.fill_color = [200, 30, 40, 255];
    let svg = seite(&doc_mit(vec![el]));
    let d = attribut(&svg, "d=\"");
    assert!(d.trim_end().ends_with('Z'), "erwarte Z am Ende: {d}");
    assert!(svg.contains("fill=\"#c81e28\""), "{svg}");
}

#[test]
fn halbdurchsichtige_fuellung_bleibt_halbdurchsichtig() {
    // Der PDF-Export muss über Weiß mischen (printpdf kann kein ExtGState).
    // SVG kann es richtig — und tut es hier.
    let mut el = rechteck(1, 50.0, 50.0);
    el.fill_color = [255, 0, 0, 51]; // 20 %
    let svg = seite(&doc_mit(vec![el]));
    assert!(svg.contains("fill=\"#ff0000\""), "{svg}");
    assert!(svg.contains("fill-opacity=\"0.2\""), "{svg}");
}

#[test]
fn drehung_wird_als_transform_geschrieben() {
    let mut el = rechteck(1, 100.0, 100.0);
    el.rotation = 30.0;
    let svg = seite(&doc_mit(vec![el]));
    // Mittelpunkt von 100,100 100×50 ist 150,125.
    assert!(
        svg.contains("transform=\"rotate(30 150 125)\""),
        "Drehung um den Mittelpunkt erwartet:\n{svg}"
    );
}

#[test]
fn gedrehte_linie_bekommt_keine_zweite_drehung() {
    // line_endpoints liefert bereits gedrehte Punkte. Käme zusätzlich ein
    // transform dazu, stünde die Linie doppelt verdreht auf der Seite.
    let mut el = rechteck(1, 100.0, 100.0);
    el.kind = ElementKind::Line;
    el.h = 0.0;
    el.rotation = 90.0;
    let svg = seite(&doc_mit(vec![el]));
    let linie = svg
        .lines()
        .find(|l| l.contains("<line"))
        .expect("Linie")
        .to_string();
    assert!(
        !linie.contains("rotate"),
        "Endpunkte sind schon gedreht: {linie}"
    );
    // 100,100 mit w=100 → Mittelpunkt 150,100; um 90° gedreht liegen die
    // Enden senkrecht darüber und darunter.
    assert!(linie.contains("x1=\"150\""), "{linie}");
    assert!(linie.contains("x2=\"150\""), "{linie}");
}

#[test]
fn text_wird_als_text_und_nicht_als_pfad_exportiert() {
    let mut el = Element::new_text(1, 40.0, 40.0);
    el.text = String::from("Hallo Welt");
    el.bold = true;
    el.italic = true;
    el.underline = true;
    let doc = doc_mit(vec![el]);
    let layouts = boxdoc::printing::collect_layouts(&ctx(), &doc);
    let svg = svg_string(&doc, &ImageStore::default(), &layouts, &Scope::Page(0)).expect("Export");

    assert!(svg.contains("<text"), "{svg}");
    assert!(svg.contains(">Hallo Welt<"), "Text bleibt lesbar:\n{svg}");
    assert!(svg.contains("font-weight=\"bold\""), "{svg}");
    assert!(svg.contains("font-style=\"italic\""), "{svg}");
    assert!(svg.contains("text-decoration=\"underline\""), "{svg}");
}

#[test]
fn unterstrich_und_durchstreichung_stehen_in_einem_attribut() {
    // SVG kennt nur ein `text-decoration` je Element. Zweimal notiert gewönne
    // das letzte, und die Unterstreichung fiele stillschweigend weg.
    let mut el = Element::new_text(1, 40.0, 40.0);
    el.text = String::from("Hallo Welt");
    el.underline = true;
    el.strikethrough = true;
    let doc = doc_mit(vec![el]);
    let layouts = boxdoc::printing::collect_layouts(&ctx(), &doc);
    let svg = svg_string(&doc, &ImageStore::default(), &layouts, &Scope::Page(0)).expect("Export");

    assert_eq!(
        svg.matches("text-decoration=").count(),
        1,
        "genau ein text-decoration erwartet:\n{svg}"
    );
    assert!(
        svg.contains("text-decoration=\"underline line-through\""),
        "{svg}"
    );
}

#[test]
fn nur_durchgestrichener_text_wird_nicht_unterstrichen() {
    let mut el = Element::new_text(1, 40.0, 40.0);
    el.text = String::from("Hallo Welt");
    el.strikethrough = true;
    let doc = doc_mit(vec![el]);
    let layouts = boxdoc::printing::collect_layouts(&ctx(), &doc);
    let svg = svg_string(&doc, &ImageStore::default(), &layouts, &Scope::Page(0)).expect("Export");

    assert!(svg.contains("text-decoration=\"line-through\""), "{svg}");
    assert!(!svg.contains("underline"), "{svg}");
}

#[test]
fn text_ohne_layout_wird_uebersprungen_statt_falsch_gesetzt() {
    // Dieselbe Regel wie im PDF-Export: Lieber nichts als geratener Umbruch.
    let mut el = Element::new_text(1, 40.0, 40.0);
    el.text = String::from("Hallo");
    let svg = seite(&doc_mit(vec![el])); // leere layouts
    assert!(!svg.contains("<text"), "{svg}");
}

#[test]
fn sonderzeichen_im_text_zerstoeren_die_datei_nicht() {
    let mut el = Element::new_text(1, 40.0, 40.0);
    el.text = String::from("a < b & c > d");
    let doc = doc_mit(vec![el]);
    let layouts = boxdoc::printing::collect_layouts(&ctx(), &doc);
    let svg = svg_string(&doc, &ImageStore::default(), &layouts, &Scope::Page(0)).expect("Export");

    assert!(svg.contains("&lt;") && svg.contains("&amp;"), "{svg}");
    // Nach dem Maskieren darf zwischen <tspan> und </tspan> kein rohes < mehr
    // stehen — sonst hielte ein Parser das für ein Tag.
    let start = svg.find("<tspan").expect("tspan");
    let inhalt = &svg[start..];
    let ende = inhalt.find("</tspan>").expect("Ende");
    let roh = &inhalt[inhalt.find('>').unwrap() + 1..ende];
    assert!(!roh.contains('<'), "rohes < im Textinhalt: {roh}");
}

// ---------------------------------------------------------------------------
// Bilder
// ---------------------------------------------------------------------------

/// Ein winziges, gültiges PNG.
fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .expect("PNG");
    buf.into_inner()
}

fn store_mit_bild(id: u64) -> ImageStore {
    let mut s = ImageStore::default();
    s.insert(id, png(), (4, 4));
    s
}

#[test]
fn bild_wird_eingebettet_statt_verlinkt() {
    // Eine verlinkte Datei wäre beim Weitergeben sofort kaputt.
    let mut el = rechteck(1, 50.0, 50.0);
    el.kind = ElementKind::Image;
    let doc = doc_mit(vec![el]);
    let svg = svg_string(
        &doc,
        &store_mit_bild(1),
        &HashMap::new(),
        &Scope::Page(0),
    )
    .expect("Export");

    assert!(svg.contains("data:image/png;base64,"), "{svg}");
    assert!(!svg.contains("file://"), "kein Verweis nach draußen");
}

#[test]
fn zugeschnittenes_bild_bekommt_einen_clip_pfad() {
    let mut el = rechteck(1, 50.0, 50.0);
    el.kind = ElementKind::Image;
    el.crop = Crop {
        x: 0.25,
        y: 0.0,
        w: 0.5,
        h: 1.0,
    };
    let doc = doc_mit(vec![el]);
    let svg = svg_string(
        &doc,
        &store_mit_bild(1),
        &HashMap::new(),
        &Scope::Page(0),
    )
    .expect("Export");

    assert!(svg.contains("<clipPath"), "{svg}");
    // Halbe Breite sichtbar → das ganze Bild ist doppelt so breit wie die Box
    // und um eine halbe Boxbreite nach links versetzt.
    assert!(svg.contains("width=\"200\""), "volles Bild 2× so breit:\n{svg}");
    assert!(svg.contains("x=\"0\""), "50 − 0.5·100 = 0:\n{svg}");
}

#[test]
fn unzugeschnittenes_bild_kommt_ohne_clip_aus() {
    let mut el = rechteck(1, 50.0, 50.0);
    el.kind = ElementKind::Image;
    let doc = doc_mit(vec![el]);
    let svg = svg_string(
        &doc,
        &store_mit_bild(1),
        &HashMap::new(),
        &Scope::Page(0),
    )
    .expect("Export");
    assert!(!svg.contains("<clipPath"), "unnötiger Clip:\n{svg}");
}

#[test]
fn fehlendes_bild_reisst_den_export_nicht_ab() {
    let mut el = rechteck(1, 50.0, 50.0);
    el.kind = ElementKind::Image;
    let doc = doc_mit(vec![el, rechteck(2, 200.0, 200.0)]);
    let svg = seite(&doc); // leerer ImageStore
    assert!(svg.contains("</svg>"), "Datei bleibt vollständig");
    assert!(svg.contains("<rect"), "das andere Objekt fehlt nicht");
}

// ---------------------------------------------------------------------------
// Wohlgeformtheit — mit einem echten Parser, nicht mit gezählten Tags
// ---------------------------------------------------------------------------

/// Liest das SVG mit einem XML-Parser durch. Fehler → Panic mit Fundstelle.
///
/// Selbst gezählte Tags übersehen genau das, was in der Praxis bricht:
/// Anführungszeichen in Attributwerten, unmaskierte Zeichen im Textinhalt,
/// ein base64-Block mit einem `<` darin.
fn parse(svg: &str, label: &str) {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(svg);
    r.config_mut().check_end_names = true;
    let mut buf = Vec::new();
    let mut tiefe = 0i32;
    loop {
        match r.read_event_into(&mut buf) {
            Ok(Event::Start(_)) => tiefe += 1,
            Ok(Event::End(_)) => tiefe -= 1,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => panic!("{label}: kein gültiges XML bei Byte {}: {e}", r.buffer_position()),
        }
        buf.clear();
    }
    assert_eq!(tiefe, 0, "{label}: nicht alle Tags geschlossen");
}

#[test]
fn der_xml_pruefer_schlaegt_bei_kaputtem_svg_auch_wirklich_an() {
    // Ohne diese Gegenprobe wäre `parse` nur ein Test, der immer durchgeht.
    let kaputt = "<svg><text>a < b</text>";
    assert!(
        std::panic::catch_unwind(|| parse(kaputt, "absichtlich kaputt")).is_err(),
        "der Prüfer hätte anschlagen müssen"
    );
}

#[test]
fn jede_variante_ist_gueltiges_xml() {
    let mut bild = rechteck(1, 40.0, 40.0);
    bild.kind = ElementKind::Image;
    bild.crop = Crop {
        x: 0.1,
        y: 0.2,
        w: 0.6,
        h: 0.7,
    };
    let mut text = Element::new_text(2, 200.0, 200.0);
    text.text = String::from("Anführung \" & Klammer <tag> — Umlaute: äöüß");
    text.italic = true;
    let mut pfad = Element::new_path(3, &[(0.0, 0.0), (1.0, 0.5), (0.4, 1.0)], true);
    pfad.x = 60.0;
    pfad.y = 400.0;
    pfad.rotation = 33.0;
    let mut linie = rechteck(4, 300.0, 300.0);
    linie.kind = ElementKind::Line;
    linie.h = 0.0;
    let mut ell = rechteck(5, 350.0, 500.0);
    ell.kind = ElementKind::Ellipse;
    ell.rotation = -17.0;

    let doc = doc_mit(vec![bild, text, pfad, linie, ell]);
    let layouts = boxdoc::printing::collect_layouts(&ctx(), &doc);
    let images = store_mit_bild(1);

    let seite = svg_string(&doc, &images, &layouts, &Scope::Page(0)).expect("Seite");
    parse(&seite, "ganze Seite");

    let auswahl = svg_string(
        &doc,
        &images,
        &layouts,
        &Scope::Selection {
            page: 0,
            ids: vec![1, 2, 3],
        },
    )
    .expect("Auswahl");
    parse(&auswahl, "Auswahl");
}

/// Holt den Wert des ersten Attributs, das mit `key` beginnt (z. B. `d="`).
fn attribut(svg: &str, key: &str) -> String {
    let start = svg.find(key).unwrap_or_else(|| panic!("{key} fehlt in:\n{svg}")) + key.len();
    let rest = &svg[start..];
    let end = rest.find('"').expect("Attributende");
    rest[..end].to_string()
}
