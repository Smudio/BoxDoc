# AGENTS.md

BoxDoc ist ein nativer Dokumenten-Editor (Rust + egui). Dokumente sind
`.boxdoc`-Dateien: Pretty-JSON, UTF-8.

**Das ist die komplette KI-Schnittstelle.** Es gibt kein zusätzliches Protokoll,
keine API, keine Sockets. Du (die KI) liest und bearbeitest die Datei direkt
mit deinen normalen Datei-Werkzeugen – genau wie eine Quellcode-Datei.

Wenn BoxDoc läuft und die Datei geöffnet hat, übernimmt es jede externe Änderung
automatisch (≤ 300 ms) als Undo-Schritt. Der Nutzer sieht sie live und kann sie
mit Strg+Z zurückrollen.

---

## Dateiformat

```json
{
  "doc": {
    "format": "A4" | "A3" | "A5" | "Letter" | "Legal",
    "orientation": "Portrait" | "Landscape",
    "pages": [ { "elements": [ <Element>, ... ] } ]
  },
  "images": [ { "id": <u64>, "png_base64": "<base64-PNG-Bytes>" } ]
}
```

### Element

Jedes Element hat zwingend einen `"kind"` und eine `"id"` (`u64`). Je nach
`kind` sind verschiedene Felder relevant.

```json
{
  "id": <u64>,
  "kind": "Text" | "Image" | "Rectangle" | "Line",

  "x": <f32 pt>,     // linke obere Ecke (unrotiert)
  "y": <f32 pt>,
  "w": <f32 pt>,     // Breite
  "h": <f32 pt>,     // Höhe (bei "Line" = 0)
  "rotation": <Grad>,

  "text": "<Inhalt>",            // Text, kann \n enthalten
  "font_size": <pt>,
  "font": "default" | "inter" | "roboto" | "lora" | "jetbrains" | "pacifico",
  "color": [r, g, b, a],         // 0..255; a=255 deckend
  "bold": <bool>,
  "italic": <bool>,
  "underline": <bool>,
  "align": "Left" | "Center" | "Right",
  "valign": "Top" | "Middle" | "Bottom",
  "indent": <pt>,

  "crop": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },  // Image; normalisiert 0..1
  "image_w": <px>,                                      // Image
  "image_h": <px>,                                      // Image

  "fill_color": [r, g, b, a],     // Shape; Alpha 0 = transparent
  "stroke_width": <pt>,           // Shape; 0 = kein Rahmen
  "stroke_color": [r, g, b, a],   // Shape
  "corner_radius": <pt>           // Rectangle; 0 = scharfe Ecken
}
```

### Koordinatensystem

- **Maßeinheit:** Punkt (1 pt = 1/72 Zoll; 1 Zoll = 25,4 mm).
- **Ursprung:** oben-links, y zeigt nach **unten**.
- **A4 Hochformat:** 595 × 842 pt.
- **A4 Querformat:** 842 × 595 pt.
- **Seitenrand/Typografie:** 1 cm ≈ 28,3 pt; 1 mm ≈ 2,83 pt.

### Z-Order

Elemente weiter hinten im `elements`-Array liegen **oben** (werden zuletzt
gezeichnet). Das letzte Element verdeckt frühere bei Überlappung.

---

## Element-Typen im Detail

| Kind        | Relevante Felder                                                |
|-------------|----------------------------------------------------------------|
| `Text`      | text, font_size, font, color, bold, italic, underline, align, valign |
| `Rectangle` | fill_color, stroke_width, stroke_color, corner_radius          |
| `Line`      | stroke_width, stroke_color (Linie = horizontale Box mit h=0 + rotation) |
| `Image`     | id (verweist auf `images[].id`), crop, image_w, image_h        |

**Linien zeichnen:** Eine Linie ist ein `Rectangle` mit `h: 0`, `fill_color`
transparent, `stroke_color` = Linienfarbe, `stroke_width` = Dicke. Position
über `x,y` + `w` + `rotation` (Winkel gegen Uhrzeigersinn).

---

## Regeln für die KI

1. **Lesen:** Datei als JSON parsen, Dokument verstehen.
2. **Bearbeiten:** Felder direkt ändern (Text, Position, Größe, Farben, Schrift,
   Format …) mit deinen normalen Edit-Tools.
3. **IDs sind `u64` und stabil.** Referenziere beim Aktualisieren nur
   existierende IDs. Für neue Elemente: höchste vorhandene ID + 1.
4. **Bilder nicht anfassen:** Lass `"images"`, `"png_base64"`, `"image_w"`,
   `"image_h"` unverändert. Bearbeite nur Text, Layout und Styling.
5. **Ungültiges JSON wird still ignoriert** – BoxDoc reloadet nur sauber
   parsebare Dateien. Teilgeschriebene Dateien sind unkritisch.
6. **Nach jedem Speichern** übernimmt BoxDoc die Änderung automatisch (≤ 300 ms)
   und legt sie als Undo-Schritt ab. Der Nutzer kann mit Strg+Z zurückrollen;
   BoxDoc schreibt dann den zurückgesetzten Stand in die Datei zurück.

---

## Typische Aufgaben

### Text ändern
```json
{ "id": 5, "text": "Neuer Titel" }
```

### Position/Größe ändern
```json
{ "id": 5, "x": 120.0, "y": 80.0, "w": 300.0 }
```

### Farbe/Stil ändern
```json
{ "id": 5, "color": [30, 80, 160, 255], "bold": true, "font_size": 28.0 }
```

### Neues Text-Element hinzufügen
An `elements` anhängen (nächste freie ID):
```json
{
  "id": 42, "kind": "Text",
  "x": 100.0, "y": 200.0, "w": 400.0, "h": 40.0, "rotation": 0.0,
  "text": "Neuer Absatz", "font_size": 14.0, "font": "default",
  "color": [20, 20, 20, 255], "bold": false, "italic": false, "underline": false,
  "align": "Left", "valign": "Top", "indent": 0.0,
  "crop": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },
  "image_w": 0, "image_h": 0,
  "fill_color": [80, 140, 220, 60], "stroke_width": 2.0,
  "stroke_color": [40, 100, 180, 255], "corner_radius": 0.0
}
```

### Element löschen
Aus dem `elements`-Array entfernen.

### Rechteck (z. B. farbiger Balken) hinzufügen
```json
{
  "id": 43, "kind": "Rectangle",
  "x": 0.0, "y": 0.0, "w": 595.0, "h": 8.0, "rotation": 0.0,
  "text": "", "font_size": 14.0, "font": "default",
  "color": [20, 20, 20, 255], "bold": false, "italic": false, "underline": false,
  "align": "Left", "valign": "Top", "indent": 0.0,
  "crop": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },
  "image_w": 0, "image_h": 0,
  "fill_color": [79, 195, 197, 255], "stroke_width": 0.0,
  "stroke_color": [79, 195, 197, 255], "corner_radius": 0.0
}
```

### Linie hinzufügen
```json
{
  "id": 44, "kind": "Line",
  "x": 100.0, "y": 300.0, "w": 400.0, "h": 0.0, "rotation": 0.0,
  "text": "", "font_size": 14.0, "font": "default",
  "color": [20, 20, 20, 255], "bold": false, "italic": false, "underline": false,
  "align": "Left", "valign": "Top", "indent": 0.0,
  "crop": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },
  "image_w": 0, "image_h": 0,
  "fill_color": [0, 0, 0, 0], "stroke_width": 2.0,
  "stroke_color": [40, 40, 40, 255], "corner_radius": 0.0
}
```

### Neue Seite hinzufügen
An `pages` ein weiteres `{ "elements": [] }` anhängen.

### Seitenformat ändern
`"format"` oder `"orientation"` im `doc`-Objekt anpassen.

---

## Design-Leitfaden (für schöne Ergebnisse)

- **Typografische Hierarchie:** Titel 28–36pt bold, Überschrift 18–22pt bold,
  Fließtext 10–12pt, Caption/Footer 8–9pt.
- **Zeilenabstand:** Lass zwischen Text-Elementen ca. 1,3× die Schriftgröße.
- **Seitenränder:** Mindestens ~50 pt (≈ 1,8 cm) zum Rand.
- **Akzentfarben:** Eine Hauptfarbe + eine Akzentfarbe für ein ruhiges Bild.
- **Aufzählungen:** Mit `"• "` prefixen, eine Zeile pro Punkt.
- **Z-Order:** Dekorationen (Hintergrundbalken) **vorne** im Array (Index 0),
  Text **hinten** (wird oben gezeichnet).
- **Linien als Trenner:** `h: 0`, `fill_color: [0,0,0,0]`, nur `stroke_*`.

---

## Beispiel-Workflow

1. `boxdoc dokument.boxdoc` starten (oder der Nutzer öffnet die Datei manuell).
2. Du öffnest `dokument.boxdoc`, liest die Struktur.
3. Du änderst Felder oder fügst Elemente hinzu.
4. BoxDoc zeigt jeden Schritt live (≤ 300 ms nach dem Speichern).
5. Der Nutzer kann jederzeit Strg+Z drücken, um zurückzurollen.
6. Fertig – der Nutzer exportiert als PDF/ODT.

---

## Siehe auch

- [`ai-schnittstelle.txt`](ai-schnittstelle.txt) – kuratierte Kurz-Spec, geeignet
  als System-Prompt oder Kontext-Datei.
- [`src/model.rs`](src/model.rs) – die kanonische Rust-Definition des
  Datenmodells (Source of Truth, falls diese Doku driftet).
