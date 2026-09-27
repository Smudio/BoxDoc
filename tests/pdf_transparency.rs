//! Transparente PNGs müssen im PDF transparent bleiben.
//!
//! Der Auslöser: Der Export hat den Alphakanal gegen Weiß verrechnet und die
//! Farbkanäle deckend ins PDF geschrieben. Ein freigestelltes Logo bekam so
//! einen weißen Kasten — sichtbar, sobald etwas darunter liegt oder das Papier
//! nicht weiß ist.
//!
//! Geprüft werden die erzeugten PDF-Bytes: Das Bild-XObject muss eine
//! Soft-Mask (`/SMask`) als eigenes indirektes Graustufen-Objekt tragen, deren
//! Pixel exakt der Alphakanal sind — und die Farbkanäle müssen unverfälscht
//! durchkommen.

#![cfg(not(target_arch = "wasm32"))]

use boxdoc::model::{Document, Element, Page};
use boxdoc::store::{encode_png, ImageStore};
use image::{Rgba, RgbaImage};
use printpdf::lopdf::{self, Document as PdfDoc, Object};

/// 4×4: linke Hälfte deckend rot, rechte Hälfte vollständig transparent.
fn halb_transparent() -> RgbaImage {
    RgbaImage::from_fn(4, 4, |x, _| {
        if x < 2 {
            Rgba([220, 30, 40, 255])
        } else {
            Rgba([220, 30, 40, 0])
        }
    })
}

fn pdf_mit_bild(img: &RgbaImage) -> PdfDoc {
    let mut store = ImageStore::default();
    store.insert(7, encode_png(img).unwrap(), (img.width(), img.height()));

    let mut doc = Document::default();
    doc.pages = vec![Page {
        elements: vec![Element::new_image(7, 100, 100, 80, 80)],
        ..Page::default()
    }];

    let bytes = boxdoc::printing::pdf_bytes(&doc, &store, &Default::default()).unwrap();
    PdfDoc::load_mem(&bytes).expect("erzeugtes PDF ist lesbar")
}

/// Streaminhalt, ggf. entpackt. lopdf weigert sich, Streams mit
/// `/Subtype /Image` zu entpacken — deshalb der abgezogene Schluessel.
fn entpackt(s: &lopdf::Stream) -> Vec<u8> {
    let mut s = s.clone();
    s.dict.remove(b"Subtype");
    s.decompress();
    s.content
}

/// Das Bild-XObject im PDF: (Dictionary, entpackte Bilddaten).
fn bild_xobject(pdf: &PdfDoc) -> (lopdf::Dictionary, Vec<u8>) {
    for (_, obj) in pdf.objects.iter() {
        if let Object::Stream(s) = obj {
            let ist_bild = s.dict.get(b"Subtype").and_then(Object::as_name).map(|n| n == b"Image").unwrap_or(false);
            let ist_rgb = s.dict.get(b"ColorSpace").and_then(Object::as_name).map(|n| n == b"DeviceRGB").unwrap_or(false);
            if ist_bild && ist_rgb {
                return (s.dict.clone(), entpackt(s));
            }
        }
    }
    panic!("kein RGB-Bild-XObject im PDF gefunden");
}

#[test]
fn transparentes_png_bekommt_eine_soft_mask() {
    let img = halb_transparent();
    let pdf = pdf_mit_bild(&img);
    let (dict, rgb) = bild_xobject(&pdf);

    // Die Maske muss eine *Referenz* sein — ein Inline-Stream im Dictionary
    // wäre syntaktisch kaputtes PDF und würde von Readern ignoriert.
    let smask_ref = match dict.get(b"SMask") {
        Ok(Object::Reference(r)) => *r,
        andere => panic!("/SMask fehlt oder ist keine Referenz: {andere:?}"),
    };

    let maske = match pdf.get_object(smask_ref).unwrap() {
        Object::Stream(s) => s.clone(),
        andere => panic!("/SMask zeigt nicht auf einen Stream: {andere:?}"),
    };
    let masken_pixel = entpackt(&maske);

    assert_eq!(maske.dict.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceGray");
    assert_eq!(maske.dict.get(b"Width").unwrap().as_i64().unwrap(), 4);
    assert_eq!(maske.dict.get(b"Height").unwrap().as_i64().unwrap(), 4);

    // Genau der Alphakanal, Pixel für Pixel.
    let erwartet: Vec<u8> = img.pixels().map(|p| p.0[3]).collect();
    assert_eq!(masken_pixel, erwartet, "Soft-Mask muss dem Alphakanal entsprechen");

    // Und die Farbe bleibt Farbe: kein Weiß-Einrechnen mehr.
    assert_eq!(rgb.len(), 4 * 4 * 3);
    for (i, chunk) in rgb.chunks(3).enumerate() {
        assert_eq!(chunk, [220, 30, 40], "Pixel {i} wurde verfälscht (Weiß eingerechnet?)");
    }
}

/// Ohne Transparenz bleibt alles wie bisher — keine überflüssige Maske.
#[test]
fn deckendes_png_bekommt_keine_soft_mask() {
    let img = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
    let pdf = pdf_mit_bild(&img);
    let (dict, _) = bild_xobject(&pdf);

    match dict.get(b"SMask") {
        Err(_) | Ok(Object::Null) => {}
        andere => panic!("deckendes Bild sollte keine Maske tragen: {andere:?}"),
    }
}
