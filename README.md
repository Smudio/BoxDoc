# BoxDoc

Ein simples, leistungsstarkes und zuverlässiges Werkzeug zum Erstellen, Bearbeiten und Speichern von Dokumenten.

BoxDoc verbindet die Einfachheit eines Objekt-Canvas mit der Vertrautheit klassischer Textverarbeitung — nativ, schnell und plattformübergreifend.

---

## Funktionen

**Objekte**
- Frei verschiebbare Text- und Bild-Objekte
- Text mit Inline-Bearbeitung (Doppelklick), Einzug, Farbe, Schriftart und -größe
- Bilder per Drag & Drop — skalieren, frei drehen, zuschneiden (Crop)
- Horizontale und vertikale Textausrichtung

**Auswahl & Bearbeitung**
- Auswahl-Rechteck: mehrere Objekte gleichzeitig auswählen
- Mehrere Objekte gleichzeitig verschieben
- Ausrichtungs-Buttons: links / mittig / rechts, oben / mittig / unten
- Copy & Paste (Strg+C / Strg+V) mit Ghost-Vorschau und Snap an die Originalposition

**Seiten & Layout**
- Papierformate: A3, A4, A5, Letter, Legal
- Hoch- und Querformat
- Mehrere Seiten pro Dokument
- Papier mittig, links- oder rechtsbündig ausrichtbar

**Positionierung**
- Koordinaten relativ zum Text-Ursprungspunkt (abhängig von Ausrichtung)
- Referenzpunkt-Raster (3×3) für Mehrfachauswahl
- Einheiten wählbar: cm, mm, pt, Zoll

**Dateiformate**
- Natives `.boxdoc`-Format (Speichern/Öffnen)
- ODT-Import und -Export (OpenDocument, LibreOffice-kompatibel)
- PDF-Export und Drucken

---

## Bedienung

| Aktion | Tasten |
|---|---|
| Zoomen | Strg + Scroll |
| Ansicht verschieben | Mittlere Maustaste |
| Text erstellen | Doppelklick auf leere Fläche |
| Mehrere Objekte auswählen | Ziehen auf leerer Fläche |
| Zur Auswahl hinzufügen | Shift + Klick |
| Kopieren | Strg + C |
| Einfügen | Strg + V (Klick zum Platzieren) |
| Löschen | Entf |
| Abbrechen | Esc |

---

## Tech-Stack

**Rust** + **egui** — nativ kompiliert, ohne Laufzeit-Abhängigkeiten.

Die Architektur ist sauber in Module getrennt:

```
src/
├── main.rs       Einstiegspunkt
├── app.rs        Anwendungszustand & UI-Logik
├── canvas.rs     Zeichenfläche & Interaktion
├── model.rs      Datenmodell (Dokument, Seite, Element)
├── geometry.rs   Geometrie-Helfer (Rotation)
├── fonts.rs      Schriftverwaltung
├── store.rs      Bildspeicher
├── io.rs         Datei-Dialoge & Projektformat
├── odt.rs        OpenDocument-Import/Export
└── printing.rs   PDF-Export & Drucken
```

Die Wahl von Rust + egui ermöglicht später eine Portierung auf Web (WASM) und mobile Geräte.

---

## Build

```sh
cargo build --release
```

Die fertige Binary liegt unter `target/release/boxdoc`.

---

## Lizenz

MIT oder Apache-2.0.
