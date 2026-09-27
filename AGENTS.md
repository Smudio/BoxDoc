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
    "format": "A4" | "A3" | "A5" | "Letter" | "Legal"
            | { "Custom": { "name": "<Name>", "w_mm": <mm>, "h_mm": <mm> } },
    "orientation": "Portrait" | "Landscape",
    "custom_formats": [ { "name": "<Name>", "w_mm": <mm>, "h_mm": <mm> }, ... ],
    "pages": [ { "elements": [ <Element>, ... ] } ]
  },
  "fonts": [ { "name": "<key>", "ttf_base64": "<base64-TTF-Bytes>" } ],
  "images": [ { "id": <u64>, "png_base64": "<base64-PNG-Bytes>" } ]
}
```

Reihenfolge in der Datei: `doc` → `_ai_hint` → `fonts` → `images`.
`fonts[]` und `images[]` enthalten nur base64-Blöcke und stehen am Ende —
du kannst sie ignorieren, wenn du nur Layout/Text änderst.

### Element

Jedes Element hat zwingend einen `"kind"` und eine `"id"` (`u64`). Je nach
`kind` sind verschiedene Felder relevant.

```json
{
  "id": <u64>,
  "kind": "Text" | "Image" | "Rectangle" | "Line" | "Ellipse" | "Path",

  "x": <f32 pt>,     // linke obere Ecke (unrotiert)
  "y": <f32 pt>,
  "w": <f32 pt>,     // Breite
  "h": <f32 pt>,     // Höhe (bei "Line" = 0)
  "rotation": <Grad>,

  "text": "<Inhalt>",            // Text, kann \n enthalten
  "font_size": <pt>,
  "font": "default" | "inter" | "roboto" | "lora" | "jetbrains" | "pacifico" | <custom-font-name>,
  "color": [r, g, b, a],         // 0..255; a=255 deckend
  "bold": <bool>,
  "italic": <bool>,
  "underline": <bool>,
  "strikethrough": <bool>,
  "align": "Left" | "Center" | "Right",
  "valign": "Top" | "Middle" | "Bottom",
  "indent": <pt>,

  "crop": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },  // Image; normalisiert 0..1
  "image_w": <px>,                                      // Image
  "image_h": <px>,                                      // Image

  "fill_color": [r, g, b, a],     // Shape; Alpha 0 = transparent
  "stroke_width": <pt>,           // Shape; 0 = kein Rahmen
  "stroke_color": [r, g, b, a],   // Shape
  "corner_radius": <pt>,          // Rectangle; 0 = scharfe Ecken

  "points":  [[nx, ny], ...],           // Path; auf die Box normalisiert 0..1
  "handles": [[ix, iy, ox, oy], ...],   // Path; Kurvengriffe, leer = Strecken
  "path_closed": <bool>                 // Path; geschlossen = Fläche
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
| `Text`      | text, font_size, font, color, bold, italic, underline, strikethrough, align, valign |
| `Rectangle` | fill_color, stroke_width, stroke_color, corner_radius          |
| `Line`      | stroke_width, stroke_color (Linie = horizontale Box mit h=0 + rotation) |
| `Ellipse`   | fill_color, stroke_width, stroke_color (Kreis = Spezialfall mit w == h; `corner_radius` wird ignoriert) |
| `Path`      | points, handles, path_closed, fill_color, stroke_width, stroke_color |
| `Image`     | id (verweist auf `images[].id`), crop, image_w, image_h        |

**Linien zeichnen:** Eine Linie ist ein `Rectangle` mit `h: 0`, `fill_color`
transparent, `stroke_color` = Linienfarbe, `stroke_width` = Dicke. Position
über `x,y` + `w` + `rotation` (Winkel gegen Uhrzeigersinn).

---

### Textauszeichnung

Vier Schalter, alle frei kombinierbar und alle im PDF- **und** SVG-Export
enthalten:

| Feld              | Wirkung          |
|-------------------|------------------|
| `bold`            | fett             |
| `italic`          | kursiv           |
| `underline`       | unterstrichen    |
| `strikethrough`   | durchgestrichen  |

Sie gelten immer für das **ganze Element**. Eine Auszeichnung einzelner Wörter
innerhalb eines Textblocks gibt es nicht — soll ein Wort hervorstechen, mach
ein eigenes Text-Element daraus und setze es daneben.

`bold` und `italic` wählen einen echten Schriftschnitt, wo die Schrift einen
mitbringt (die System-Schriften auf dem Desktop: Arial, Calibri, Georgia …).
Wo nicht — bei den eingebetteten Schriften und damit überall im Browser —
werden sie nachgeahmt und sehen etwas anders aus als ein gesetzter Schnitt.
Der Zeilenumbruch stimmt in beiden Fällen, weil er mit derselben Schrift
gemessen wird, mit der auch gezeichnet wird.

`underline` und `strikethrough` sind gezeichnete Linien, keine Schnitte; sie
funktionieren deshalb bei jeder Schrift gleich.

---

## Pfade und Kurven (`kind: "Path"`)

Ein Pfad ist ein Zug aus Stützpunkten, wahlweise mit kubischen Bézier-Kurven
dazwischen. Er kann offen (Linienzug) oder geschlossen (Fläche) sein.

```json
{
  "id": 7, "kind": "Path",
  "x": 100.0, "y": 200.0, "w": 200.0, "h": 80.0, "rotation": 0.0,
  "points":  [[0.0, 1.0], [0.5, 0.0], [1.0, 1.0]],
  "handles": [[0.0, 1.0, 0.2, 0.4],
              [0.3, 0.0, 0.7, 0.0],
              [0.8, 0.4, 1.0, 1.0]],
  "path_closed": false,
  "stroke_width": 2.0, "stroke_color": [40, 40, 40, 255],
  "fill_color": [0, 0, 0, 0]
}
```

### Koordinaten sind **normalisiert auf die Box**

`points[i] = [nx, ny]` mit `0,0` = linke obere und `1,1` = rechte untere Ecke
der Box. Der Punkt in Seitenkoordinaten ist also:

```
x_seite = x + nx * w
y_seite = y + ny * h        (danach um die Boxmitte um `rotation` gedreht)
```

**Warum so:** Verschieben, Skalieren und Drehen laufen damit über dieselben
Felder wie bei jeder anderen Form (`x`,`y`,`w`,`h`,`rotation`), und die
Stützpunkte bleiben unangetastet. Willst du einen Pfad nur verschieben, ändere
`x`/`y` — **nicht** die Punkte.

### `handles` — die Kurvengriffe

`handles[i] = [in_x, in_y, out_x, out_y]` sind die beiden Kontrollpunkte des
Knotens `i`, **im selben normalisierten Raum wie `points`** — also absolute
Positionen, keine Abstände zum Stützpunkt.

* `in` gilt für das Segment **vor** dem Knoten, `out` für das **danach**.
* Ein **Eckknoten** hat beide Griffe auf seinem Stützpunkt:
  `[nx, ny, nx, ny]`. Beide Nachbarsegmente werden dann Geraden.
* Griffe dürfen außerhalb von `[0,1]` liegen. Wenn **BoxDoc** einen Pfad
  bearbeitet, legt es die Box anschließend eng um die gezeichnete Kurve (nicht
  um die Griffe). Schreibst du die Box selbst, sollte sie die Kurve enthalten —
  Klick-Erkennung, Auswahl-Rechteck und Ausrichtung lesen nur die Box.
* Für einen **glatten** Übergang müssen `in`, Stützpunkt und `out` auf einer
  Geraden liegen — der übliche Weg ist `in = p - t` und `out = p + t`.

**Zwei Regeln, an die du dich halten musst:**

1. `handles` ist entweder **leer/weggelassen** (dann ist der Pfad ein reiner
   Streckenzug) oder **genau so lang wie `points`**. Andere Längen repariert
   BoxDoc beim Laden, indem es die fehlenden Knoten zu Ecken macht.
2. Lässt du `handles` weg, ist das kein Fehler, sondern der Normalfall für
   Vielecke. Schreibe es nur, wenn du wirklich Kurven willst.

### Segmente

Zwischen `points[i]` und `points[i+1]` liegt eine **Gerade**, wenn
`handles[i].out` auf `points[i]` und `handles[i+1].in` auf `points[i+1]`
liegt — sonst eine kubische Kurve mit genau diesen beiden Kontrollpunkten.
Bei `"path_closed": true` gibt es zusätzlich das Segment vom letzten zurück
zum ersten Knoten.

Ein offener Pfad wird **nie gefüllt**, egal was in `fill_color` steht.

### Beispiel: Dreieck mit einer runden Seite

```json
{
  "id": 8, "kind": "Path",
  "x": 50.0, "y": 50.0, "w": 100.0, "h": 100.0, "rotation": 0.0,
  "points":  [[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]],
  "handles": [[0.0, 1.0, 0.0, 1.0],
              [1.0, 1.0, 1.4, 0.5],
              [0.5, 0.0, 0.5, 0.0]],
  "path_closed": true,
  "fill_color": [80, 140, 220, 120],
  "stroke_width": 1.0, "stroke_color": [30, 60, 120, 255]
}
```

Die Unterkante und die linke Seite sind Geraden (Eckknoten), die rechte Seite
wölbt sich nach außen (der `out`-Griff des zweiten Knotens liegt bei `x = 1.4`,
also rechts von der Box).

---

## Regeln für die KI

1. **Lesen:** Datei als JSON parsen, Dokument verstehen.
2. **Bearbeiten:** Felder direkt ändern (Text, Position, Größe, Farben, Schrift,
   Format …) mit deinen normalen Edit-Tools.
3. **IDs sind `u64` und stabil.** Referenziere beim Aktualisieren nur
   existierende IDs. Für neue Elemente: höchste vorhandene ID + 1.
4. **Bilder und Custom-Fonts nicht anfassen:** Lass `"images"`, `"png_base64"`,
   `"image_w"`, `"image_h"`, `"fonts"` und `"ttf_base64"` unverändert.
   Bearbeite nur Text, Layout und Styling. Ein Text-Element kann auf einen
   Custom-Font verweisen, der unter `"fonts[].name"` definiert ist.
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
  "color": [20, 20, 20, 255], "bold": false, "italic": false, "underline": false, "strikethrough": false,
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
  "color": [20, 20, 20, 255], "bold": false, "italic": false, "underline": false, "strikethrough": false,
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
  "color": [20, 20, 20, 255], "bold": false, "italic": false, "underline": false, "strikethrough": false,
  "align": "Left", "valign": "Top", "indent": 0.0,
  "crop": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },
  "image_w": 0, "image_h": 0,
  "fill_color": [0, 0, 0, 0], "stroke_width": 2.0,
  "stroke_color": [40, 40, 40, 255], "corner_radius": 0.0
}
```

### Ellipse (oder Kreis) hinzufügen
Kreis = Spezialfall mit `w == h`. `corner_radius` wird bei Ellipse ignoriert.
```json
{
  "id": 45, "kind": "Ellipse",
  "x": 200.0, "y": 150.0, "w": 200.0, "h": 120.0, "rotation": 0.0,
  "text": "", "font_size": 14.0, "font": "default",
  "color": [20, 20, 20, 255], "bold": false, "italic": false, "underline": false, "strikethrough": false,
  "align": "Left", "valign": "Top", "indent": 0.0,
  "crop": { "x": 0.0, "y": 0.0, "w": 1.0, "h": 1.0 },
  "image_w": 0, "image_h": 0,
  "fill_color": [79, 195, 197, 120], "stroke_width": 2.0,
  "stroke_color": [40, 100, 180, 255], "corner_radius": 0.0
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
