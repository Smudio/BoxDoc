//! Bild-Export: Crop, verlustfreies Durchreichen und Alpha-Behandlung.
//!
//! Geprüft werden die tatsächlich erzeugten Bytes, nicht Zwischenwerte — was
//! hier zugesichert wird, landet genau so in der Datei bzw. in der
//! Zwischenablage.

use boxdoc::model::{Crop, Element};
use boxdoc::store::{encode_png, flatten_on_white, has_alpha, ImageStore};
use image::{Rgba, RgbaImage};

/// 4×4-Bild aus vier einfarbigen 2×2-Quadranten.
fn quadranten() -> RgbaImage {
    RgbaImage::from_fn(4, 4, |x, y| match (x < 2, y < 2) {
        (true, true) => Rgba([255, 0, 0, 255]),    // links oben: rot
        (false, true) => Rgba([0, 255, 0, 255]),   // rechts oben: grün
        (true, false) => Rgba([0, 0, 255, 255]),   // links unten: blau
        (false, false) => Rgba([255, 255, 0, 255]), // rechts unten: gelb
    })
}

fn store_mit(img: &RgbaImage) -> (ImageStore, Vec<u8>) {
    let png = encode_png(img).expect("PNG kodierbar");
    let mut store = ImageStore::default();
    store.insert(7, png.clone(), (img.width(), img.height()));
    (store, png)
}

fn bild_element() -> Element {
    Element::new_image(7, 0, 0, 4, 4)
}

#[test]
fn crop_schneidet_den_sichtbaren_ausschnitt() {
    let (store, _) = store_mit(&quadranten());
    let mut el = bild_element();
    el.crop = Crop { x: 0.5, y: 0.0, w: 0.5, h: 0.5 };

    let out = store.cropped_rgba(&el).expect("Bild vorhanden");
    assert_eq!((out.width(), out.height()), (2, 2));
    for p in out.pixels() {
        assert_eq!(p.0, [0, 255, 0, 255], "nur der grüne Quadrant darf übrig sein");
    }
}

#[test]
fn ohne_crop_bleibt_das_ganze_bild() {
    let (store, _) = store_mit(&quadranten());
    let out = store.cropped_rgba(&bild_element()).unwrap();
    assert_eq!((out.width(), out.height()), (4, 4));
}

/// Ein unbeschnittenes PNG wird byteweise durchgereicht: Kein erneutes
/// Kodieren, keine verlorenen Metadaten.
#[test]
fn unbeschnittenes_png_wird_durchgereicht() {
    let (store, original) = store_mit(&quadranten());
    let out = store.element_png(&bild_element()).unwrap();
    assert_eq!(out, original);
}

#[test]
fn beschnittenes_png_wird_neu_kodiert() {
    let (store, original) = store_mit(&quadranten());
    let mut el = bild_element();
    el.crop = Crop { x: 0.0, y: 0.5, w: 0.5, h: 0.5 };

    let out = store.element_png(&el).unwrap();
    assert_ne!(out, original);
    let dekodiert = image::load_from_memory(&out).unwrap().to_rgba8();
    assert_eq!((dekodiert.width(), dekodiert.height()), (2, 2));
    assert_eq!(dekodiert.get_pixel(0, 0).0, [0, 0, 255, 255], "linker unterer Quadrant");
}

/// JPEG kennt kein Alpha — der Export muss auf Weiß legen, sonst wird
/// Transparenz beim Öffnen schwarz.
#[test]
fn jpeg_export_legt_transparenz_auf_weiss() {
    let img = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));
    let (store, _) = store_mit(&img);
    let mut el = bild_element();
    el.image_w = 8;
    el.image_h = 8;

    let jpg = store.element_jpeg(&el, 92).expect("JPEG kodierbar");
    let dekodiert = image::load_from_memory(&jpg).unwrap().to_rgb8();
    assert_eq!((dekodiert.width(), dekodiert.height()), (8, 8));
    for p in dekodiert.pixels() {
        // JPEG ist verlustbehaftet, deshalb Toleranz statt exaktem Weiß.
        assert!(p.0.iter().all(|&c| c > 240), "erwartet nahezu weiß, war {:?}", p.0);
    }
}

#[test]
fn alpha_erkennung_und_weiss_hinterlegung() {
    let mut img = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
    assert!(!has_alpha(&img));

    img.put_pixel(1, 0, Rgba([0, 0, 0, 0]));
    assert!(has_alpha(&img));

    let flach = flatten_on_white(&img);
    assert!(!has_alpha(&flach));
    assert_eq!(flach.get_pixel(0, 0).0, [0, 0, 0, 255], "deckendes Schwarz bleibt");
    assert_eq!(flach.get_pixel(1, 0).0, [255, 255, 255, 255], "durchsichtig wird weiß");
}

#[test]
fn halbtransparent_wird_zur_haelfte_aufgehellt() {
    let img = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 128]));
    let flach = flatten_on_white(&img);
    let p = flach.get_pixel(0, 0).0;
    assert!((p[0] as i32 - 127).abs() <= 2, "erwartet ~127, war {}", p[0]);
    assert_eq!(p[3], 255);
}

/// Ohne passenden Eintrag im Store gibt es kein Bild — und keinen Panic.
#[test]
fn fehlendes_bild_liefert_none() {
    let store = ImageStore::default();
    assert!(store.cropped_rgba(&bild_element()).is_none());
    assert!(store.element_png(&bild_element()).is_none());
}
