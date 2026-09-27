# BoxDoc Architecture

> *Technische Architektur-Dokumentation — Letztes Update: 14. August 2026*
> *Verbindliche Planung: [`ROADMAP.md`](ROADMAP.md)*

## Überblick

BoxDoc ist ein **plattformübergreifender Dokumenten-Editor** in **Rust + egui**,
von Grund auf für **AI-native Workflows** gebaut. Die primäre AI-Schnittstelle
ist die `.boxdoc`-Datei selbst (Pretty-JSON, UTF-8).

```
┌─────────────────────────────────────────────────────────────┐
│                      BoxDoc Anwendung                         │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐       │
│  │   Model     │    │    UI       │    │  File I/O   │       │
│  │ (JSON/DOC)  │◄──►│ (egui)      │◄──►│ (WASM/FS)   │       │
│  └─────────────┘    └─────────────┘    └─────────────┘       │
│           ▲                  ▲                  ▲              │
│           │                  │                  │              │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                 File Watcher (AI, nur nativ)            │    │
│  │  ┌──────────┐    ┌──────────┐    ┌──────────┐       │    │
│  │  │ Detects  │    │ Triggers │    │ Reloads  │       │    │
│  │  │ Changes │───►│ Undo     │───►│ State    │       │    │
│  │  └──────────┘    └──────────┘    └──────────┘       │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

## Komponenten

### 1. Datenmodell (`src/model.rs`)

```rust
pub struct Document {
    pub format: PaperFormat,        // A4, A3, Letter, ...
    pub orientation: Orientation,   // Portrait, Landscape
    pub pages: Vec<Page>,
}

pub struct Page {
    pub elements: Vec<Element>,
}

pub enum ElementKind {
    Text,
    Image,
    Rectangle,
    Line,
    Ellipse,
    Path,       // freier Pfad (points[] + optionale handles[], auf die Box normalisiert)
}

pub struct Element {
    pub id: u64,
    pub kind: ElementKind,
    pub x: f32, pub y: f32,
    pub w: f32, pub h: f32,
    pub rotation: f32,
    // ... kind-spezifische Felder
}
```

**Dateiformat:** `.boxdoc` (Pretty-JSON, UTF-8).
Spezifikation siehe [`AGENTS.md`](AGENTS.md).

### 2. Anwendungs-Zustand (`src/app.rs`)

> **Hinweis:** Aktuell eine ~2050 Zeilen starke Monolith-Datei. Aufteilung
> in Module (`app/state.rs`, `app/actions.rs`, ...) ist in `ROADMAP.md`
> Phase 7d vorgesehen.

- **Single source of truth:** `EditorApp`-Struct.
- **Snapshot-Undo:** max. 200 Einträge (`src/history.rs`).
- **Selektion:** Multi-Select via Vec von `id`.
- **View:** Zoom (0.1–6.0), Pan, Seiten-Ausrichtung (links/mitte/rechts).

#### Interaktions-Zustände

```rust
pub enum Interaction {
    None,
    DragBodies { start_pointer, starts },
    Resize { id, anchor, rotation },
    ResizeEdge { id, edge, rotation },
    Rotate { id },
    Crop { id, edge, start_crop },
    SelectionBox { start },
    LineEndpoint { id, is_start },
    PathNode { id, index },                  // Pfad-Stützpunkt ziehen
    PathHandle { id, index, outgoing },      // Bézier-Griff ziehen
}
```

#### Werkzeuge

```rust
pub enum Tool { Select, Line, Pen, Freehand }
```

Genau **ein** Feld (`EditorApp::tool`) hält den Werkzeugzustand. Zwei
Zeichenvorgänge können damit nicht gleichzeitig laufen. Was ein Werkzeug an
halbfertiger Arbeit hält, steckt daneben in `path_draft` (Pfad-Entwurf, noch
ohne ID und noch nicht im Dokument) bzw. `line_drawing` (gesetzter
Linien-Startpunkt). `path_edit` sagt, welcher **fertige** Pfad gerade auf
Knotenebene bearbeitet wird.

### 3. Rendering (`src/canvas.rs`)

> **Hinweis:** Aktuell eine ~1430 Zeilen starke Monolith-Datei. Aufteilung
> in `canvas/rendering.rs`, `canvas/interaction.rs` etc. ist in Phase 7e
> vorgesehen.

#### Rendering-Pipeline

1. Seitenhintergrund (weiß + Schatten)
2. Alle sichtbaren Elemente (z-Order = Array-Reihenfolge)
3. Selektions-Overlay (Handles)
4. Interaktions-Overlay (Drag-Linien, Resize-Boxen)
5. Paste-Ghost-Vorschau
6. Linien-Vorschau beim Zeichnen

#### Koordinatensystem

- **Einheit:** Punkte (1 pt = 1/72 Zoll; 1 Zoll = 25,4 mm).
- **Ursprung:** oben-links, y wächst nach unten.
- **A4 Portrait:** 595 × 842 pt · **Landscape:** 842 × 595 pt.

### 4. File I/O (`src/io.rs`)

| Format | Extension | Lesen | Schreiben | Bemerkung |
|--------|-----------|------|----------|-----------|
| BoxDoc | `.boxdoc` | ✅ | ✅ pretty JSON | Natives Format |
| ODT | `.odt` | ✅ nativ | ✅ nativ | OpenDocument |
| PDF | `.pdf` | ✅ pdfium, Kurven bleiben Kurven | ✅ Text (umgebrochen), Bild, alle Shapes, echte Bézier-Kurven | printpdf |

Projektstruktur:

```json
{
  "doc": { "format": "A4", "orientation": "Portrait", "custom_formats": [...], "pages": [...] },
  "images": [{ "id": 1, "png_base64": "..." }]
}
```

Plattform-spezifische Implementierungen:

- `mod native` (`io.rs:149`) — `std::fs`, `rfd`-Dialoge.
- `mod web_impl` (`io.rs:361`) — Browser File-Input, Blob-Download.

#### Pfad-Validierung (Security)

- `is_safe_path(path, base)` (`io.rs`) — Defensiv-Check für Automatisierung
  (zukünftige Server-Variante). Prüft via `canonicalize` + `starts_with`.
  Aktuell nicht gegen `current_dir` erzwungen, da `rfd::FileDialog`-Pfade
  überall liegen dürfen.
- `ensure_canonicalizable(path)` (`io.rs`) — Defensiv-Check, der in
  `load_project`, `save_project` und `import_odt_dialog` aufgerufen wird.
  Blockiert kaputte und symlink-basierte Pfade, ohne legitime File-Dialog-Pfade
  einzuschränken. Siehe `SECURITY.md` SW-002.

### 5. History (`src/history.rs`)

```rust
pub struct History {
    snapshots: Vec<Snapshot>,  // max. 200
    cursor: usize,
}

pub struct Snapshot {
    doc: Document,
    selection: Vec<u64>,
    page_index: usize,
}
```

Strg+Z = Undo, Strg+Y / Strg+Shift+Z = Redo. Bilder werden nicht
gesnapshottet (nur IDs), um Speicher zu sparen.

### 6. AI-Integration (`src/file_watch.rs`)

- **Nativ:** `notify` + `notify-debouncer-mini`, 300 ms Debounce.
  Externe Änderungen werden als **Undo-Schritt** geladen
  (`app.rs:642` → `reload_from_disk`).
- **WASM:** Fehler-Stub, gibt "nicht verfügbar" zurück. Eine
  Polling-Variante ist nicht implementiert (frühere Doku-Behauptung war
  falsch). In Phase 4 wird Remote-Polling einer URL ergänzt.

#### AI-Workflow

1. AI liest `.boxdoc`-Datei direkt (mit ihren Datei-Tools).
2. AI modifiziert JSON (Text, Position, Styling, neue Elemente …).
3. BoxDoc erkennt die Änderung (≤ 300 ms).
4. BoxDoc lädt den neuen Stand als Undo-Schritt.
5. Nutzer kann mit Strg+Z zurückrollen; BoxDoc schreibt den alten Stand
   in die Datei zurück (`write_back_to_disk`, `app.rs:691`).

### 7. PDF-Export (`src/printing.rs`)

- **Nativ:** `printpdf` 0.7.
- **Status:** Text (inkl. fett, kursiv, unterstrichen, durchgestrichen),
  Bilder, Rechteck, Ellipse, Linie, freier Pfad.
- **Eingebettet wird die Schrift, mit der auch gezeichnet wurde.** Für die
  Standardschrift kommen die Bytes aus `egui::FontDefinitions::default()`,
  für die mitgelieferten aus `fonts::bundled_bytes`, für System-Schriften von
  der Platte — je Schnitt. Vorher stand für „default" Helvetica im PDF und die
  eingebetteten Schriften fielen sogar ganz auf Arial zurück (der Export suchte
  sie über `paths`, das bei ihnen leer ist). Beides sind andere Schriften mit
  anderen Zeichenbreiten als die, mit der `text_layout` den Umbruch gerechnet
  hat: Der Text lief über seine gemessene Breite hinaus, und die
  Unterstreichung endete sichtbar vor dem letzten Buchstaben.
- **Grenze:** Schriften, die als Custom-Font in der `.boxdoc`-Datei stecken,
  werden noch nicht eingebettet — sie landen in der Fallback-Schrift. Dafür
  müsste `printing` den `FontStore` mitbekommen.
- **Textlayout kommt aus `src/text_layout.rs`** — demselben Modul, das der
  Canvas benutzt. `printing.rs` rechnet bewusst nichts mehr selbst aus:
  Jede eigene Schätzung wäre eine neue Quelle für Abweichungen zwischen
  Bildschirm und PDF. Vorher brach der Export nur an `\n` um und schätzte
  Textbreiten mit `Zeichen × 0,5 × Größe`; ein umgebrochener Absatz lief
  im PDF aus der Seite heraus.
- Der Export braucht deshalb den `egui::Context` (Schriftsystem). Die
  Dialoge liegen in `io.rs`, der reine Export in `printing.rs` — dadurch
  ist er ohne GUI testbar (`tests/pdf_wysiwyg.rs`).
- **Shell-Aufruf (Windows):** `cmd /C start <pdf>` mit separaten `.arg()`-
  Aufrufen (SW-001 gefixt, siehe `SECURITY.md`).
- **WASM: läuft**, mit denselben Bytes wie nativ. `pdf_bytes()` baut das PDF,
  `export_pdf()` ist nur ein Datei-Wrapper darüber; im Browser geht dasselbe
  Ergebnis als Download raus. Zwei Unterschiede:
  - Als Fallback-Schrift nimmt WASM `default_font()` (die mitgelieferte
    egui-Schrift) statt einer Systemschrift von der Platte — es gibt dort keine.
    Das ist sogar die genauere Wahl, denn genau die sieht der Nutzer am Schirm.
  - Nicht eingebettete Schriften aus `FONT_CHOICES` liegen als Datei auf der
    Platte und fallen im Browser auf den Fallback zurück.
- **printpdf 0.7 braucht dafür einen Patch:** sein wasm-Datums-Polyfill ist
  unvollständig und lässt sich gar nicht bauen. `vendor/printpdf` ist eine Kopie
  mit zwei Korrekturen, ausschließlich in `cfg(target_arch = "wasm32")`-Blöcken —
  der native Codepfad ist unverändert. Details und Upgrade-Pfad:
  `vendor/printpdf/BOXDOC-PATCH.md`.
- **Drucken bleibt nativ.** Der Browser-Druckdialog druckt die HTML-Seite, also
  den Canvas als Pixelbild. Der Menüeintrag ist auf Web ausgegraut statt
  versteckt, mit dem Hinweis „PDF exportieren und das PDF drucken".

### 7b. SVG-Export (`src/svg.rs`)

- **Ohne Fremdbibliothek** — das Modul baut eine Zeichenkette. Dadurch hat es
  keine UI- und keine Plattform-Abhängigkeit und ist vollständig testbar
  (`tests/svg_export.rs`, 25 Tests, darunter eine XML-Prüfung mit echtem
  Parser).
- **Zwei Bereiche** (`Scope`): eine ganze Seite (Seitenformat als Leinwand,
  weißer Grund) oder eine **Auswahl** (Leinwand = gemeinsame Hüllbox der
  gewählten Objekte plus 8 pt Rand, Grund durchsichtig). Eine exportierte
  Auswahl soll sich in ein anderes Dokument legen lassen, ohne einen weißen
  Kasten mitzubringen.
- **Jedes Objekt bleibt sein eigenes Primitiv:** `<rect>`, `<ellipse>`,
  `<line>`, `<path>` mit kubischen Bézier-Segmenten, `<text>` mit einem
  `<tspan>` je Zeile. Nichts wird zu einem Vieleck aufgelöst — eine
  importierte Rundung taucht in Illustrator oder Inkscape mit ihren vier
  Griffen wieder auf.
- **Transparenz ist echt.** Der PDF-Export muss halbdurchsichtige Farben über
  Weiß mischen (printpdf 0.7 gibt keinen Zugriff auf `ExtGState`, siehe
  `blend_over_white`). SVG kennt `fill-opacity`; überlappende Formen scheinen
  hier durcheinander durch, genau wie auf dem Bildschirm.
- **Bilder** werden als data-URI eingebettet (nie verlinkt — eine verlinkte
  Datei wäre beim Weitergeben sofort kaputt). Der Crop entsteht über
  `<clipPath>` plus Versatz statt durch Neuberechnung der Pixel: Die
  Bilddaten bleiben unangetastet und der Ausschnitt im SVG verschiebbar.
- **Textlayout** kommt wie beim PDF aus `text_layout.rs`. Fehlt der Eintrag zu
  einem Textelement, wird es übersprungen statt mit geratenem Umbruch falsch
  gesetzt.
- **Leinwand aus der Kontur, nicht aus der Box:** `geometry::element_bounds`
  rechnet über die gedrehten Ecken bzw. die abgetastete Kurve. Eine um 45°
  gedrehte Form ragt über `x/y/w/h` hinaus; eine Linie hat `h == 0`.
- **Nur die aktuelle Seite.** SVG hat kein Seitenkonzept; alle Seiten in eine
  Datei zu legen hieße, sie übereinanderzustapeln.

### 7a. Textlayout (`src/text_layout.rs`)

Die **einzige** Quelle der Wahrheit für Zeilenumbruch, Ausrichtung und
Zeilenposition. Canvas und PDF-Export rufen dieselbe Funktion:

```rust
pub fn layout(fonts: &mut FontsView, el: &Element, scale: f32) -> TextLayout
```

`scale` ist beim Canvas der Zoomfaktor (damit Glyphen scharf bleiben) und
beim Export 1.0. Rückgabewerte sind **immer in pt**. Die Umbruchstellen sind
identisch, weil Schriftgröße und Umbruchbreite proportional mitskalieren —
abgesichert durch `text_layout::tests::umbruch_ist_zoomunabhaengig`.

`auto_height` im Modell entscheidet, ob `h` aus dem Inhalt berechnet wird
(dann ist `valign` wirkungslos) oder ob der Nutzer sie festgelegt hat (dann
richtet `valign` den Text darin aus). Der Reflow läuft als expliziter Schritt
in `canvas::reflow_text_heights` — nicht mehr als Seiteneffekt beim Zeichnen.

#### Textauszeichnung

Vier unabhängige Schalter am Element: `bold`, `italic`, `underline`,
`strikethrough`. Sie zerfallen in **zwei verschiedene Dinge**, und das ist der
Grund, warum sie an verschiedenen Stellen behandelt werden:

- **Schnitte** (`bold`, `italic`) wechseln die *Schriftdatei*. Sie ändern
  damit die Zeichenbreiten und folglich den Zeilenumbruch, müssen also schon
  ins **Layout** einfließen — `text_layout::font_id_for` wählt über
  `fonts::family_for_style` die passende Familie.
- **Auszeichnungslinien** (`underline`, `strikethrough`) sind gemalte Striche.
  Sie ändern nichts am Umbruch, brauchen aber überall dieselbe Lage:
  `text_layout::decoration_metrics` liefert sie einmal in pt relativ zur
  Grundlinie, Canvas, PDF und SVG bilden nur noch ab.

`fonts::has_style(key, style)` ist die gemeinsame Auskunft darüber, ob ein
**echter** Schnitt geladen ist. Ist er es, benutzen ihn Bildschirm und PDF;
ist er es nicht, ahmen ihn beide nach (Umriss für fett, Scherung für kursiv).
Ohne diese eine Quelle könnte der Bildschirm einen echten Schnitt zeigen, den
das PDF nachahmt — oder umgekehrt.

| Schrift | Fett/Kursiv |
|---|---|
| System-Schriften (Arial, Calibri, Georgia …) | echte Schnitte (`arialbd.ttf` usw.), auf dem Desktop |
| Eingebettete Schriften (Inter, Lora, Pacifico …) | nur Regular in der Binary → nachgeahmt |
| Standardschrift | egui liefert keinen Fettschnitt → nachgeahmt |
| Browser (WASM) | nur eingebettete Schriften → immer nachgeahmt |

Drei Fallen, die frühere Fassungen nicht kannten:

1. **Der Schnitt fiel beim Layout unter den Tisch.** `font_id_for` wertete
   `bold`/`italic` nur für die Standardschrift aus. Fettes Arial stand auf dem
   Bildschirm mager, im PDF fett — und brach an verschiedenen Stellen um.
2. **Die Alias-Familien „Bold"/„Italics" waren leer.** Sie enthielten
   denselben mageren Schnitt wie `Proportional`; fett gesetzter Text in der
   Standardschrift maß auf den Punkt genau so breit wie magerer. `has_style`
   meldet für sie deshalb `false`, damit die Nachahmung greift.
3. **Der Unterstrich riss den Grafikzustand mit.** Er setzte die Strichstärke
   auf seinen eigenen Wert, mitten in der Zeilenschleife — ab der zweiten
   Zeile trug der nachgeahmte Fettdruck damit um ein Vielfaches zu dick auf.
   Glyphen und Linien laufen jetzt in getrennten Durchgängen.

**Grenze:** Kursiv kann der Canvas nicht nachahmen — egui kann eine Galley
nicht scheren. Bei Schriften ohne echten Kursivschnitt steht der Text auf dem
Bildschirm also aufrecht, im PDF geschert. Fett wird auf beiden Seiten
nachgeahmt und stimmt überein.

### 7c. Formen-Geometrie (`src/geometry.rs`)

Analog zum Textlayout: Die Umrisse aller Formen entstehen genau einmal, in
**BoxDoc-Seitenkoordinaten** (pt, Ursprung oben links, y nach unten, Rotation
im Uhrzeigersinn).

```rust
pub fn line_endpoints(el)   -> (Pos2, Pos2)
pub fn rect_outline(el)     -> Vec<Pos2>   // inkl. corner_radius
pub fn ellipse_outline(el)  -> Vec<Pos2>
pub fn path_outline(el)     -> Vec<Pos2>   // freier Pfad, Kurven aufgelöst
pub fn quad_corners(el)     -> [Pos2; 4]   // Bilder, Auswahlrahmen
pub fn triangulate(&[Pos2]) -> Vec<[u32; 3]>   // Füllung, auch konkav
```

#### Pfade und Kurven

Ein Pfad ist eine Folge von `PathNode { anchor, in_h, out_h }` — Stützpunkt
plus die beiden kubischen Kontrollpunkte. Im Modell stehen sie **auf die Box
normalisiert** (`points[]` + `handles[]`), hier kommen sie in
Seitenkoordinaten heraus:

```rust
pub fn path_nodes(el)            -> Vec<PathNode>
pub fn set_path_nodes(el, nodes)               // einziger Schreibweg
pub fn path_segments(el)         -> Option<(Pos2, Vec<PathSeg>)>  // für PDF
pub fn path_nearest(el, p)       -> Option<PathHit>               // Hit-Test, Einfügen
pub fn insert_node / remove_node / move_node / move_handle
pub fn smooth_path / sharpen_path / toggle_node_smooth
pub fn simplify_polyline / nodes_from_polyline // Freihand
```

`set_path_nodes` ist der **einzige** Weg, Stützpunkte zu ändern, und hält dabei
die Invariante, an der alles andere hängt: *die Box umschließt den Pfad*.
Hit-Test, Auswahl-Rechteck, Snapping und Ausrichtung lesen ausschließlich
`x`/`y`/`w`/`h` — zöge ein Knoten aus der Box heraus, wäre der Pfad dort weder
anklickbar noch ausrichtbar. Die Box wird über die **gezeichnete Kurve** gelegt,
nicht über die Stützpunkte (eine Kurve beult dazwischen aus) und nicht über die
Griffe (die liegen oft weit außerhalb der Form). Gerechnet wird im unrotierten
Rahmen, damit die Drehung erhalten bleibt.

**Bildschirm löst auf, PDF nicht:** `path_outline` zerlegt Kurven in Strecken
(≤ 24 je Kurve, Fehler ≪ 1 Bildpunkt), weil egui füllen und triangulieren
muss. Der PDF-Export nimmt dagegen `path_segments` und schreibt echte
Bézier-Operatoren. Nur so kommt eine importierte Rundung auch wieder als
Rundung heraus statt als Vieleck mit zweihundert Ecken.

`triangulate` ist Ear Clipping. Der Canvas füllte Formen vorher als
Dreiecksfächer um den ersten Punkt — richtig für konvexe Umrisse, und mehr gab
es lange nicht. Seit dem PDF-Import gibt es konkave Pfade, und der Fächer malt
dort über jede Einbuchtung hinweg.

Beide Renderer bilden davon nur noch ab — Canvas mit `to_screen`, PDF mit
`printing::to_pdf`. Weil beide Abbildungen affin und uniform skalierend sind,
ist das Ergebnis per Konstruktion deckungsgleich; `tests/pdf_geometry.rs`
simuliert beide Wege und vergleicht sie.

Vorher rechnete jeder Renderer selbst, mit drei verschiedenen Konventionen:

| Form | Canvas | PDF (vorher) |
|---|---|---|
| Linie | Drehpunkt = Mittelpunkt | Drehpunkt = **Startpunkt** |
| Rechteck | im Uhrzeigersinn | **gegen** den Uhrzeigersinn |
| Ellipse | im Uhrzeigersinn | im Uhrzeigersinn (zufällig richtig) |

Bei `rotation = 0` fiel nichts davon auf — bei jedem anderen Winkel stand die
Linie im PDF an einer völlig anderen Stelle.

#### Transparenz

`printpdf` 0.7 stellt die PDF-Transparenz (`ExtGState` mit `ca`/`CA`) nicht
über `PdfLayerReference` bereit. Alpha wurde deshalb schlicht ignoriert — eine
zu 23 % deckende Füllung kam knallig deckend heraus. `blend_over_white()`
mischt die Farbe stattdessen über den (immer weißen) Seitenhintergrund. Für
Formen auf der Seite ist das exakt das Canvas-Bild. **Grenze:** Überlappen
zwei halbtransparente Formen, scheint die untere im PDF nicht durch.

### 7d. PDF-Import (`src/pdf_import.rs`)

Nativ, via `pdfium-render`. Leitgedanke: **nichts wegwerfen, nichts erfinden.**
Erst wird versucht, eine Grundform wiederzuerkennen (die lässt sich in BoxDoc
weiterbearbeiten), sonst bleibt die Geometrie unverändert als `Path` erhalten.

Der wiederkehrende Fehler dabei war immer derselbe — die **Bounding-Box**. Sie
ist achsparallel und kennt weder Richtung noch Drehung:

| Objekt | mit Hüllbox | Quelle der Wahrheit |
|---|---|---|
| Linie | alle waagrecht, senkrechte unsichtbar (Breite 0) | Stützpunkte des Pfads |
| Text | jede Beschriftung waagrecht | Zeichenmatrix (`a`,`b`) |
| Bild | flach in zu großer Box | Bildmatrix (Einheitsquadrat → Ecken) |
| Rechteck | gedrehte werden zur größeren Hüllbox | vier Eckpunkte |

**Kurven bleiben Kurven.** `collect_subpaths` sammelt neben dem aufgelösten
Streckenzug (den die Ellipsen-Erkennung braucht) auch die Knotenfolge mit
ihren Griffen: Der erste Kontrollpunkt eines `c`-Operators wird zum
Ausgangsgriff des vorigen Knotens, der zweite zum Eingangsgriff des neuen.
Vorher wurde jede Kurve in bis zu 24 Strecken zerlegt und anschließend auf
256 Punkte heruntergedünnt — aus einem Bogen wurde ein Vieleck, das sich nicht
mehr sinnvoll bearbeiten und beim Export nicht wiederherstellen ließ.

Bei gedrehtem Text liefert pdfium nur die achsparallele Hüllbox. Die gesuchte
Box folgt daraus durch Auflösen von
`HB_b = w·|cos θ| + h·|sin θ|`, `HB_h = w·|sin θ| + h·|cos θ|`
(Determinante `cos 2θ`; nur nahe 45° mehrdeutig, dort dient die Schriftgröße
als Höhe). Der Mittelpunkt ist unkritisch — die Hüllbox eines gedrehten
Rechtecks ist auf dessen Mittelpunkt zentriert.

Weitere Fallen, die der Import inzwischen kennt:

- **Form-XObjects** (eingebettete Miniatur-Dokumente) werden rekursiv gelesen;
  ihre Kinder liegen im Koordinatensystem des Formulars, die Matrizen werden
  verkettet (`Mat::then`). Vorher fehlte solcher Inhalt komplett.
- **`f` schließt implizit**: Ein gefüllter Pfad braucht kein `h`. Ohne diese
  Regel verschwand ein Großteil aller gefüllten Formen.
- **Unsichtbarer Text** (Render-Modus 3) wird ausgelassen — sonst steht der
  OCR-Layer eines Scans sichtbar über dem Seitenbild.
- **Nur gestrichene Pfade** werden nicht gefüllt (`fill_mode`), sonst deckt
  ihre Fläche darunterliegenden Inhalt zu.
- **Ellipsen** werden an der Form geprüft (Ellipsengleichung), nicht an der
  Zahl der Bögen. Sonst wird jedes abgerundete Rechteck zur Ellipse.

**Grenzen:** Flächen mit Löchern (mehrere Teilpfade mit Even-Odd/Nonzero)
werden je Teilpfad gefüllt, das Loch also mit; Farbverläufe und Muster
(Shading) werden ausgelassen; bei gespiegelten Bildern bleibt die Drehung
erhalten, die Spiegelung nicht.

Abgesichert durch `tests/pdf_import.rs` — die Test-PDFs entstehen dort **von
Hand aus Content-Stream-Operatoren**, nicht über BoxDocs eigenen Export: Ein
Import, der nur die eigenen Dateien versteht, wäre wertlos.

### 7b. Multiuser-Merge (`src/merge.rs`)

Drei-Wege-Merge auf Element-Ebene für gleichzeitiges Bearbeiten:

```rust
pub fn merge_documents(base, local, remote) -> (Document, MergeReport)
```

Möglich ohne CRDT, weil jedes Element eine stabile, nie wiederverwendete
`u64`-ID hat. Pro Element wird feldweise gemergt; ändern beide Seiten
dasselbe Feld, gewinnt der lokale Wert und der Konflikt wird im
`MergeReport` gemeldet. Löschung vs. Änderung entscheidet zugunsten der
Änderung — ein wiederauferstandenes Element ist mit einem Tastendruck weg,
verlorene Arbeit nicht.

Zusammenspiel mit dem Server (`src/web_sync.rs`):

1. `?meta=<slug>` liefert nur die Versionsnummer (Polling, wenige Bytes).
2. Ist sie höher, wird das Dokument geladen und **gemergt**, nicht ersetzt.
3. `PUT ...&version=<n>` schreibt optimistisch. Bei veralteter Version
   antwortet der Server mit 409 **und dem aktuellen Stand**; der Client
   merged und sendet erneut.
4. `WebDoc::base_doc` hält den zuletzt abgeglichenen Stand als Merge-Basis.

### 8. ODT (`src/odt.rs`)

- Import + Export via `zip`-Crate, **auf beiden Plattformen**. `zip` mit
  `deflate` ist reines Rust und baut für wasm32 ohne Änderung.
- **Der Kern arbeitet auf Bytes, nicht auf Pfaden:** `export_to_bytes()` und
  `import_from_bytes()` sind die eigentlichen Implementierungen (über
  `std::io::Cursor`), `export()`/`import()` sind dünne Datei-Wrapper für Native.
  Im Browser kommen die Bytes aus einem `FileReader` und gehen als Download
  wieder raus — es gibt dort keinen Pfad.
- **ZIP-Bomb-Limit aktiv** (SW-003 gefixt, siehe `SECURITY.md`):
  - `MAX_EXTRACT_SIZE = 100 MB` pro Archiveintrag, geprüft in `read_entry`.
  - `MAX_ARCHIVE_TOTAL_SIZE = 400 MB` über alle Einträge, geprüft in
    `import_from_bytes` — also auf beiden Plattformen, nicht nur nativ.
- Der ODT-Import setzt `file_path` auf Web bewusst nicht: ohne Dateisystem wäre
  jeder Pfad erfunden, und beim Speichern würde er eine Datei suggerieren, die es
  nicht gibt. Der Import ist ein Konvertierungsschritt, kein Öffnen.

---

## Plattform-Support

### Nativ

| Plattform | Status | Backend |
|----------|--------|---------|
| Windows | ✅ Voll | glow (OpenGL) |
| Linux (X11) | ✅ Voll | glow |
| Linux (Wayland) | ✅ Voll | glow |
| macOS | ✅ Voll | glow |

### Web (WASM)

Die Web-Version ist mit der EXE **angeglichen**: gleicher Menübaum, gleicher
Code, gleiche Ausgabebytes (abgesichert durch `tests/web_parity.rs`). Der
Unterschied ist nur das Ziel — nativ ein Dateidialog, im Browser ein Download.

| Feature | Status | Bemerkung |
|---------|--------|-----------|
| Grund-Rendering | ✅ | egui/eframe WebRunner |
| File-I/O (Open/Save) | ✅ | Browser File-Input + Blob-Download |
| Server-Sync | ✅ | Versions-Polling + Drei-Wege-Merge (`web_sync.rs`, `merge.rs`) |
| Mehrbenutzer | ✅ | Optimistische Nebenläufigkeit, 409 + Merge |
| **PDF-Export** | ✅ | `printing::pdf_bytes()` → Download; braucht `vendor/printpdf` |
| **SVG-Export (Seite + Auswahl)** | ✅ | `svg::svg_string()` → Download |
| **ODT-Import/-Export** | ✅ | `odt::export_to_bytes()` / `import_from_bytes()` |
| **Schrift laden (TTF/OTF)** | ✅ | File-Input → `PENDING_FONT` |
| **Bilder speichern** | ✅ | ein Download je ausgewähltem Bild (PNG) |
| File-Watcher (lokal) | ❌ | nur nativ; im Web übernimmt das der Server-Sync |
| PDF-Import | ❌ | braucht pdfium (native C++-Bibliothek); Menüeintrag ausgegraut |
| Drucken | ❌ | Browser druckt den Canvas als Pixelbild; stattdessen PDF exportieren |
| Bild in die Zwischenablage (Strg+C) | ❌ | `navigator.clipboard.write` ist async + braucht Nutzergeste |
| Responsiv / Mobile | ⚠️ | Phase 3 |
| IndexedDB-Persistenz | ❌ | Phase 3 |

Die drei fehlenden Aktionen sind im Menü **ausgegraut statt versteckt**, mit dem
Grund im Tooltip. Wer den Eintrag sucht, soll sehen, dass es ihn gibt.

---

## Modulstruktur (IST)

```
BoxDoc/
├── src/
│   ├── main.rs              # Entry points (nativ + WASM, ~90 Zeilen)
│   ├── model.rs             # Datenmodell + Serde (~920 Zeilen)
│   ├── io.rs                # File-I/O, native + web_impl Submodule (~880 Zeilen)
│   ├── history.rs           # Undo/Redo (~90 Zeilen)
│   ├── store.rs             # Bildspeicher + Texture-Cache (~75 Zeilen)
│   ├── geometry.rs          # Formen-Umrisse, Pfade & Kurven (~1475 Zeilen inkl. Tests)
│   ├── text_layout.rs       # Ein Layout-Pfad für Canvas und PDF (~290 Zeilen)
│   ├── merge.rs             # Drei-Wege-Merge (~670 Zeilen inkl. Tests)
│   ├── themes.rs            # Farb-Themes (~190 Zeilen)
│   ├── fonts.rs             # Font-Loading (~140 Zeilen)
│   ├── settings_io.rs       # Settings-Persistenz (~115 Zeilen)
│   ├── file_watch.rs        # File-Watcher (nativ) (~68 Zeilen)
│   ├── web_sync.rs          # Server-Sync (WASM) (~625 Zeilen)
│   ├── app.rs               # App-State + UI-Logik (~3620 Zeilen, Refactor offen)
│   ├── canvas.rs            # Canvas-Rendering + Interaktion (~2530 Zeilen, Refactor offen)
│   ├── pdf_import.rs        # PDF-Import via pdfium (~1230 Zeilen)
│   ├── odt.rs               # ODT-Import/-Export (~400 Zeilen)
│   ├── printing.rs          # PDF-Export (~600 Zeilen)
│   └── svg.rs               # SVG-Export, Seite oder Auswahl (~440 Zeilen)
├── assets/
│   └── fonts/               # Inter, Roboto, Lora, JetBrains, Pacifico (TTF)
├── vendor/
│   └── printpdf/            # printpdf 0.7.0 + wasm-Patch (BOXDOC-PATCH.md)
├── web/                     # Web-Quelle: PHP-Backend + KI-Dateien (web/README.md)
│   └── dist/                # Build-Ergebnis, gitignored — NICHT editieren
├── Cargo.toml              # Dependencies + Profile + [patch.crates-io]
├── Trunk.toml              # WASM-Build-Konfig (dist = "web/dist")
├── build-web.ps1           # Web-Build (nutzt Profil wasm-release)
└── index.html              # WASM-Entry-Point + copy-file-Links nach web/dist/
```

> Eine frühere Version dieses Dokuments behauptete eine `src/app/`- und
> `src/canvas/`-Submodulstruktur. Diese existiert im Code nicht; die
> Aufteilung ist als Ziel in `ROADMAP.md` Phase 7d/e hinterlegt.

> Ebenso falsch war die Behauptung, es gebe eine `src/ai.rs`. Diese Datei
> wurde nie angelegt. AI-Integration läuft ausschließlich über den
> File-Watcher.

---

## Dependencies

Siehe `Cargo.toml`. Zusammenfassung:

**Core:** `eframe`, `egui`, `image`, `serde`, `serde_json`, `base64`,
`printpdf`, `zip`. Die letzten zwei sind bewusst **nicht** nativ-exklusiv: PDF-
und ODT-Export laufen auch im Browser, beide Crates sind reines Rust.
**Nativ-only:** `rfd`, `notify`, `notify-debouncer-mini`, `crossbeam-channel`,
`pdfium-render` + `pdfium-bundled` (native C++-Bibliothek → kein WASM).
**WASM-only:** `wasm-bindgen`, `wasm-bindgen-futures`, `web-sys`, `js-sys`.

**`[patch.crates-io]`:** `printpdf` zeigt auf `vendor/printpdf`, eine Kopie von
0.7.0 mit zwei Korrekturen an seinem kaputten wasm-Datums-Polyfill. Ohne den
Patch baut printpdf für `wasm32-unknown-unknown` überhaupt nicht. Der native
Codepfad ist unverändert — beide Korrekturen stehen in
`cfg(target_arch = "wasm32")`-Blöcken. Siehe `vendor/printpdf/BOXDOC-PATCH.md`.

`gloo-storage`/`gloo-file` (in älterer Doku erwähnt) sind **nicht** Teil der
Dependencies und nicht nötig — IndexedDB wird in Phase 3 über
`web-sys`-Bindings direkt angebunden.

---

## Performance-Charakteristik (Richtwerte)

| Metrik | Aktuell | Bemerkung |
|---|---|---|
| FPS | 30–60 | je nach Dokumentgröße |
| Memory/Dokument | 1–10 MB | anhängig von Bildern |
| History | max. 200 Snapshots | konfigurierbar in `history.rs:11` |
| Load/Save | < 1 s | typische Dokumente |
| Max-File-Size | unlimitiert | für `.boxdoc` |
| Max-Extract-Size (ODT) | 100 MB pro Eintrag, 400 MB Gesamt | siehe `SECURITY.md` SW-003 |

Optimierungen (Viewport-Culling, Quad-Tree, Lazy-Loading) folgen in Phase 7f/g.

---

## Build

### Nativ

```bash
cargo run                              # Debug
cargo build --release                  # Binary: target/release/boxdoc
```

### WASM

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
trunk serve                             # Dev-Server (debug, schnelle Rebuilds)
.\build-web.ps1                         # Upload-Paket in web/dist/
```

`build-web.ps1` ruft `trunk build --release --cargo-profile wasm-release`. Das
Profil aus `Cargo.toml` (`opt-level="s"`, `lto`, `panic="abort"`) spart rund
1,8 MB gegenüber dem normalen release-Profil — 5,7 statt 7,5 MB WASM. Es steht
absichtlich **nicht** in `Trunk.toml`: sonst würde `trunk serve` beim Entwickeln
auch damit bauen, zwei Minuten pro Rebuild statt Sekunden.

**Ordnerstruktur:** `web/` ist die Quelle (in Git), `web/dist/` das Ergebnis
(gitignored, wird bei jedem Build neu geschrieben). Vorher lagen beide als
`web/` und `dist/` nebeneinander im Wurzelverzeichnis und sahen gleichrangig
aus — mit dem Ergebnis, dass `llms.txt` in beiden von Hand editiert wurde und
`dist/index.php` wochenlang veraltet war. Siehe `web/README.md`.

### Cross-Compile

```bash
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target x86_64-apple-darwin
```

---

## Lizenz

Dual-lizenziert **MIT OR Apache-2.0**. Siehe [`LICENSE`](LICENSE).
