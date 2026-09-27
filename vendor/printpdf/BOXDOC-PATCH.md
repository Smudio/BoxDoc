# printpdf 0.7.0 — BoxDoc-Fork

Wörtliche Kopie von `printpdf 0.7.0` aus der crates.io-Registry, eingebunden
über `[patch.crates-io]` in der Wurzel-`Cargo.toml`.

## Warum überhaupt ein Fork

BoxDoc soll im Browser dasselbe PDF erzeugen wie als EXE. printpdf 0.7.0 lässt
sich aber für `wasm32-unknown-unknown` **gar nicht** bauen — nicht wegen einer
nativen Abhängigkeit, sondern weil sein eigener Datums-Polyfill (`src/date.rs`)
unvollständig ist:

```
error[E0599]: no method named `offset` found for reference `&js_sys_date::OffsetDateTime`
   --> src/document_info.rs:104:23
error[E0277]: the trait bound `u8: From<u32>` is not satisfied
   --> src/document_info.rs:109:9
```

`document_info.rs::to_pdf_time_stamp_metadata` ruft `date.offset()` und
`u8::from(date.month())`. Der wasm-Polyfill hat kein `offset()`, und sein
`month()` gibt `u32` zurück — `u8: From<u32>` gibt es nicht. Beide Shims
(`js_sys_date` **und** `unix_epoch_stub_date`) sind betroffen, das Abschalten
des `js-sys`-Features hilft also nicht.

## Was geändert wurde

Zwei Stellen: `src/date.rs` (wasm-Build, siehe oben) und `src/xobject.rs`
(Transparenz, siehe unten).

### 1. `src/date.rs` — nur innerhalb von `cfg(target_arch = "wasm32")`-Blöcken:

| Änderung | Shim |
| --- | --- |
| `offset() -> time::UtcOffset` ergänzt (aus `Date::get_timezone_offset()`, Vorzeichen invertiert) | `js_sys_date` |
| `month()` gibt `u8` statt `u32` zurück | `js_sys_date` |
| `offset() -> time::UtcOffset` ergänzt (konstant `UTC`) | `unix_epoch_stub_date` |
| `month()` gibt `u8` statt `u32` zurück | `unix_epoch_stub_date` |

Hier ist der native Codepfad byte-identisch zum Original. Native benutzt
`time::OffsetDateTime` direkt (`date.rs` Zeile 11) und sieht die Shims nie.

### 2. `src/xobject.rs` — Soft-Masks (`/SMask`)

`XObjectList::into_with_document` behandelt Bilder mit Soft-Mask jetzt gesondert.
Das Original ist an dieser Stelle schlicht kaputt (`impl From<ImageXObject> for
lopdf::Stream`, Zeile ~263):

* Die Maske landet als **Inline-Stream** im Bild-Dictionary. Ein Stream darf im
  PDF aber nur ein indirektes Objekt sein — Reader ignorieren die Maske oder
  stolpern über die Datei.
* Die Maskenhöhe wird aus der **Breite** des Bildes abgeleitet
  (`height: img.width`), was bei allem außer Quadraten falsch ist.

Der Patch legt die Maske als eigenes Graustufen-XObject an, hängt sie per
`doc.add_object` in das Dokument und trägt in `/SMask` die Referenz ein. Der
Zweig greift nur bei `smask: Some(..)`; ohne Maske ist der Codepfad unverändert.

Ohne diesen Patch kann `printing.rs` keine Transparenz ausgeben — PNGs mit
Alphakanal müssten gegen Weiß gerechnet werden und bekämen einen weißen Kasten.
Abgesichert durch `tests/pdf_transparency.rs`, die Geometrie zusätzlich durch
`tests/pdf_geometry.rs` und `tests/pdf_wysiwyg.rs`.

## Was entfernt wurde

Nur ungenutztes Beiwerk, um das Repository klein zu halten (1,1 MB statt 4,0 MB):

- `assets/ISOcoated_v2_eci.icc` (1,8 MB) — nie referenziert
- `assets/Color Profile Bundling License_10.15.08.pdf` (286 KB)
- `assets/fonts/`, `assets/img/`, `assets/svg/` — nur Beispiel-Eingaben
- `Cargo.lock`, `Cargo.toml.orig`, `.travis.yml`, `appveyor.yml`, `README.tpl`

`assets/CoatedFOGRA39.icc` **muss** bleiben: `src/lib.rs` bindet es über
`include_bytes!` als `ICC_PROFILE_ECI_V2` ein, also bei jedem Build.
Ebenso die `.txt`-Dateien (`include_str!` in `font.rs` und `xmp_metadata.rs`).

## Bei einem Upgrade

printpdf 0.9 ist die aktuelle Reihe, hat wasm-Support von Haus aus — und eine
komplett andere API. Ein Upgrade heißt `src/printing.rs` (~900 Zeilen) neu
schreiben. Wer das angeht: erst `tests/pdf_geometry.rs` und
`tests/pdf_wysiwyg.rs` grün halten, dann diesen Ordner löschen und den
`[patch.crates-io]`-Block aus der Wurzel-`Cargo.toml` entfernen.

Lizenz unverändert MIT (siehe `LICENSE`).
