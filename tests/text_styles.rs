//! Textauszeichnung: Schnitte (fett/kursiv) und Auszeichnungslinien
//! (unterstrichen/durchgestrichen).
//!
//! Zwei Fehlerklassen sind hier abgesichert:
//!
//! 1. **Der Schnitt fiel beim Layout unter den Tisch.** `font_id_for` wertete
//!    fett und kursiv nur für die Standardschrift aus. Wer Arial fett wählte,
//!    sah auf dem Bildschirm magere Schrift, bekam im PDF aber fette — und weil
//!    fette Glyphen breiter sind, brachen beide an verschiedenen Stellen um.
//!
//! 2. **Die Auszeichnungslinien wurden dreimal getrennt berechnet.** Canvas,
//!    PDF und SVG hatten je eigene Zahlen für Lage und Stärke. Jetzt kommen
//!    sie aus `text_layout::decoration_metrics`.

#![cfg(not(target_arch = "wasm32"))]

use boxdoc::model::{Document, Element, FontStyle, Page, FONT_CHOICES};
use boxdoc::store::ImageStore;
use boxdoc::text_layout;

/// egui-Kontext wie ihn die Anwendung aufsetzt — mit registrierten Schriften.
fn ctx() -> egui::Context {
    let ctx = egui::Context::default();
    boxdoc::fonts::install(&ctx);
    let _ = ctx.run(Default::default(), |_| {});
    ctx
}

fn text_el(font: &str) -> Element {
    let mut el = Element::new_text(1, 40.0, 40.0);
    el.text = "Handgloves Wechseljahr 12345".to_string();
    el.font = font.to_string();
    el.font_size = 14.0;
    el.w = 400.0;
    el.h = 100.0;
    el.auto_height = false;
    el
}

/// Eine Schrift, die auf **diesem** System einen echten Fett-Schnitt hat.
/// `None` → die Tests, die einen brauchen, überspringen sich selbst.
fn font_mit_echtem_fettschnitt() -> Option<&'static str> {
    FONT_CHOICES
        .iter()
        .map(|d| d.key)
        .find(|k| *k != "default" && boxdoc::fonts::has_style(k, FontStyle::Bold))
}

// ---------------------------------------------------------------------------
// FontStyle
// ---------------------------------------------------------------------------

#[test]
fn fontstyle_bildet_die_vier_kombinationen_umkehrbar_ab() {
    for bold in [false, true] {
        for italic in [false, true] {
            let s = FontStyle::new(bold, italic);
            assert_eq!(s.bold(), bold, "bold ging verloren bei {s:?}");
            assert_eq!(s.italic(), italic, "italic ging verloren bei {s:?}");
        }
    }
}

#[test]
fn jeder_schnitt_hat_ein_eigenes_familien_suffix() {
    // Kollidierten zwei Suffixe, landete ein Schnitt in der Familie eines
    // anderen — der Fehler wäre erst am fertigen Dokument sichtbar.
    let mut seen: Vec<&str> = Vec::new();
    for style in FontStyle::all() {
        assert!(
            !seen.contains(&style.suffix()),
            "Suffix {:?} doppelt vergeben",
            style.suffix()
        );
        seen.push(style.suffix());
    }
    assert_eq!(
        FontStyle::Regular.suffix(),
        "",
        "Regular muss den nackten Schlüssel behalten, sonst bricht family_for"
    );
}

#[test]
fn fontstyle_of_liest_das_element() {
    let mut el = text_el("default");
    el.bold = true;
    el.italic = true;
    assert_eq!(FontStyle::of(&el), FontStyle::BoldItalic);
    el.italic = false;
    assert_eq!(FontStyle::of(&el), FontStyle::Bold);
}

#[test]
fn regular_ist_immer_verfuegbar() {
    for def in FONT_CHOICES {
        assert!(
            boxdoc::fonts::has_style(def.key, FontStyle::Regular),
            "{} meldet keinen Regular-Schnitt",
            def.key
        );
    }
}

#[test]
fn die_standardschrift_meldet_keinen_echten_fettschnitt() {
    // Sie hat nachweislich keinen: Fett und mager messen exakt gleich breit.
    // Meldete `has_style` hier `true`, ahmte niemand die Fettung nach — und
    // fett gesetzter Text bliebe auf dem Bildschirm unverändert mager.
    let ctx = ctx();
    assert!(!boxdoc::fonts::has_style("default", FontStyle::Bold));

    let regular = text_el("default");
    let mut bold = regular.clone();
    bold.bold = true;
    let w_regular = ctx.fonts_mut(|f| text_layout::natural_width(f, &regular));
    let w_bold = ctx.fonts_mut(|f| text_layout::natural_width(f, &bold));
    assert_eq!(
        w_bold, w_regular,
        "die Standardschrift hat wider Erwarten doch einen Fett-Schnitt — \
         dann darf has_style ihn auch melden"
    );
}

#[test]
fn eingebettete_schriften_melden_keinen_echten_fettschnitt() {
    // Sie liegen nur als Regular in der Binary. Meldeten sie fälschlich einen
    // Schnitt, würde niemand ihn nachahmen — und fett bliebe wirkungslos.
    let _ctx = ctx();
    for def in FONT_CHOICES.iter().filter(|d| d.bundled) {
        assert!(
            !boxdoc::fonts::has_style(def.key, FontStyle::Bold),
            "{} behauptet einen Fett-Schnitt zu haben",
            def.key
        );
    }
}

// ---------------------------------------------------------------------------
// Layout: der Schnitt muss beim Messen ankommen
// ---------------------------------------------------------------------------

#[test]
fn fetter_text_wird_breiter_gemessen_als_magerer() {
    // Der eigentliche Beweis für Fehler 1: Ohne den Schnitt im Layout wären
    // beide Breiten exakt gleich.
    let ctx = ctx();
    let Some(key) = font_mit_echtem_fettschnitt() else {
        eprintln!("kein echter Fett-Schnitt installiert, Test übersprungen");
        return;
    };

    let regular = text_el(key);
    let mut bold = regular.clone();
    bold.bold = true;

    let w_regular = ctx.fonts_mut(|f| text_layout::natural_width(f, &regular));
    let w_bold = ctx.fonts_mut(|f| text_layout::natural_width(f, &bold));

    assert!(
        w_bold > w_regular,
        "fett ({key}) muss breiter messen als mager: {w_bold} vs {w_regular}"
    );
}

#[test]
fn der_schnitt_waehlt_eine_andere_familie() {
    let _ctx = ctx();
    let Some(key) = font_mit_echtem_fettschnitt() else {
        eprintln!("kein echter Fett-Schnitt installiert, Test übersprungen");
        return;
    };
    let regular = text_el(key);
    let mut bold = regular.clone();
    bold.bold = true;

    assert_ne!(
        text_layout::font_id_for(&regular, 1.0).family,
        text_layout::font_id_for(&bold, 1.0).family,
        "fett und mager landen in derselben Familie"
    );
}

#[test]
fn fehlender_schnitt_faellt_auf_dieselbe_schrift_zurueck() {
    // Eine eingebettete Schrift hat keinen Fett-Schnitt. Sie darf deshalb
    // nicht in einer fremden Schrift landen — lieber dieselbe Schrift ohne
    // Fettung (die dann nachgeahmt wird).
    let _ctx = ctx();
    let mut el = text_el("pacifico");
    el.bold = true;
    assert_eq!(
        text_layout::font_id_for(&el, 1.0).family,
        boxdoc::fonts::family_for("pacifico"),
        "Rückfall führt in eine fremde Schrift"
    );
}

#[test]
fn der_schnitt_aendert_den_umbruch_nicht_zwischen_canvas_und_export() {
    // WYSIWYG gilt auch für ausgezeichneten Text: Canvas layoutet mit Zoom,
    // der Export ohne — die Umbruchstellen müssen dieselben bleiben.
    let ctx = ctx();
    let mut el = text_el("default");
    el.text = "Ein bewusst langer Absatz, der in einer schmalen Box zwingend \
               über mehrere Zeilen umbrechen muss."
        .to_string();
    el.w = 150.0;
    el.bold = true;
    el.italic = true;

    let export = ctx.fonts_mut(|f| text_layout::layout(f, &el, 1.0));
    assert!(export.lines.len() > 1, "der Absatz muss umbrechen");

    for scale in [0.5_f32, 2.0, 3.0] {
        let canvas = ctx.fonts_mut(|f| text_layout::layout(f, &el, scale));
        let a: Vec<&str> = export.lines.iter().map(|l| l.text.as_str()).collect();
        let b: Vec<&str> = canvas.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(a, b, "Umbruch weicht bei scale={scale} ab");
    }
}

// ---------------------------------------------------------------------------
// Auszeichnungslinien
// ---------------------------------------------------------------------------

#[test]
fn unterstrich_liegt_unter_und_durchstreichung_ueber_der_grundlinie() {
    // Beide Werte sind als *Abstand* definiert, also positiv; die Richtung
    // steckt im Namen. Ein negativer Wert hier hieße, dass der Unterstrich im
    // PDF über dem Text landet.
    let m = text_layout::decoration_metrics(14.0);
    assert!(m.underline_dy > 0.0, "underline_dy: {}", m.underline_dy);
    assert!(m.strike_dy > 0.0, "strike_dy: {}", m.strike_dy);
    assert!(m.thickness > 0.0, "thickness: {}", m.thickness);
}

#[test]
fn die_durchstreichung_liegt_innerhalb_der_zeile() {
    // Sie soll durch die Kleinbuchstaben laufen, nicht darüber. Grob: unter
    // der Oberlänge (~0,7 em über der Grundlinie).
    for size in [8.0_f32, 14.0, 48.0, 200.0] {
        let m = text_layout::decoration_metrics(size);
        assert!(
            m.strike_dy < size * 0.7,
            "Durchstreichung bei {size}pt zu hoch: {}",
            m.strike_dy
        );
    }
}

#[test]
fn die_auszeichnungsmasse_skalieren_mit_der_schriftgroesse() {
    let klein = text_layout::decoration_metrics(10.0);
    let gross = text_layout::decoration_metrics(100.0);
    assert!(gross.underline_dy > klein.underline_dy);
    assert!(gross.strike_dy > klein.strike_dy);
    assert!(gross.thickness > klein.thickness);
}

#[test]
fn winzige_schrift_bekommt_trotzdem_einen_sichtbaren_strich() {
    // Ohne Untergrenze wäre die Linie bei 4 pt rechnerisch 0,24 pt stark und
    // damit in vielen Betrachtern schlicht unsichtbar.
    let m = text_layout::decoration_metrics(4.0);
    assert!(m.thickness >= 0.4, "zu dünn: {}", m.thickness);
}

// ---------------------------------------------------------------------------
// Durch die Export-Pipelines
// ---------------------------------------------------------------------------

fn doc_mit(el: Element) -> Document {
    Document {
        pages: vec![Page { elements: vec![el] }],
        ..Document::default()
    }
}

#[test]
fn ausgezeichneter_text_kommt_vollstaendig_ins_pdf() {
    // Alle vier Auszeichnungen gleichzeitig — inklusive des Falls, der früher
    // brach: Ab der zweiten Zeile überschrieb der Unterstrich die Strichstärke
    // des nachgeahmten Fettdrucks.
    let ctx = ctx();
    let mut el = text_el("default");
    el.text = "Erste Zeile mit allem\nZweite Zeile mit allem\nDritte Zeile".to_string();
    el.bold = true;
    el.italic = true;
    el.underline = true;
    el.strikethrough = true;
    let doc = doc_mit(el);

    let layouts = boxdoc::printing::collect_layouts(&ctx, &doc);
    assert_eq!(layouts[&1].lines.len(), 3, "drei harte Zeilen erwartet");

    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("textstile.pdf");
    boxdoc::printing::export_pdf(&path, &doc, &ImageStore::default(), &layouts)
        .expect("PDF-Export mit Auszeichnungen");

    let bytes = std::fs::read(&path).expect("PDF lesen");
    assert!(bytes.starts_with(b"%PDF"), "keine gültige PDF-Datei");
}

#[test]
fn eine_nachgeahmte_schrift_exportiert_ebenfalls() {
    // Der andere Zweig von `resolve_text_font`: kein echter Schnitt vorhanden,
    // also Umriss und Scherung. Darf nicht scheitern und nicht panicken.
    let ctx = ctx();
    let mut el = text_el("pacifico");
    el.bold = true;
    el.italic = true;
    el.underline = true;
    let doc = doc_mit(el);

    let layouts = boxdoc::printing::collect_layouts(&ctx, &doc);
    let dir = std::env::temp_dir().join("boxdoc_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("textstile_synth.pdf");
    boxdoc::printing::export_pdf(&path, &doc, &ImageStore::default(), &layouts)
        .expect("PDF-Export mit nachgeahmtem Schnitt");
    assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
}

#[test]
fn die_durchstreichung_ueberlebt_den_merge() {
    let base = doc_mit(text_el("default"));
    let mut remote = base.clone();
    remote.pages[0].elements[0].strikethrough = true;

    // Nur die Gegenseite hat gestrichen — das muss ankommen.
    let (merged, _) = boxdoc::merge::merge_documents(&base, &base, &remote);
    assert!(
        merged.pages[0].elements[0].strikethrough,
        "Durchstreichung ging beim Merge verloren"
    );
}

#[test]
fn die_durchstreichung_zaehlt_als_aenderung() {
    // Fehlte das Feld im Vergleich, hielte der Sync zwei verschiedene
    // Dokumente für gleich und verwürfe die Änderung stillschweigend.
    let a = text_el("default");
    let mut b = a.clone();
    b.strikethrough = true;
    assert!(!boxdoc::merge::elements_equal(&a, &b));
}

