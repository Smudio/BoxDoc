# BoxDoc Architecture

> *Technische Architektur-Dokumentation — Letztes Update: 18. Juli 2026*
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
    // Phase 1 (geplant): Ellipse
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
}
```

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
| PDF | `.pdf` | ❌ (Phase 2) | ⚠️ nur Text+Bild, keine Shapes | printpdf |

Projektstruktur:

```json
{
  "doc": { "format": "A4", "orientation": "Portrait", "pages": [...] },
  "images": [{ "id": 1, "png_base64": "..." }]
}
```

Plattform-spezifische Implementierungen:

- `mod native` (`io.rs:149`) — `std::fs`, `rfd`-Dialoge.
- `mod web_impl` (`io.rs:361`) — Browser File-Input, Blob-Download.

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
- **Status:** Text + Bilder; **Rechtecke und Linien werden aktuell
  ignoriert** (`printing.rs:104-105`, leerer Match-Arm).
- **WASM:** Status-Text "nicht unterstützt".
- Vollständiger Shape-Export + PDF-Import folgen in Phase 2.

### 8. ODT (`src/odt.rs`)

- Import + Export nativ via `zip`-Crate.
- Kein ZIP-Bomb-Limit (siehe `SECURITY.md` SW-003).
- WASM: nicht unterstützt.

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

| Feature | Status | Bemerkung |
|---------|--------|-----------|
| Grund-Rendering | ✅ | egui/eframe WebRunner |
| File-I/O (Open/Save) | ✅ | Browser File-Input + Blob-Download (`io.rs:514`) |
| File-Watcher | ❌ | Fehler-Stub; Phase 4 bringt Remote-Polling |
| ODT-Import/-Export | ❌ | Status-Text; `zip`-Crate nicht in WASM-Variante |
| PDF-Export | ❌ | Status-Text |
| Responsiv / Mobile | ⚠️ | Phase 3 |
| IndexedDB-Persistenz | ❌ | Phase 3 |

---

## Modulstruktur (IST)

```
BoxDoc/
├── src/
│   ├── main.rs              # Entry points (nativ + WASM, ~78 Zeilen)
│   ├── model.rs             # Datenmodell + Serde (~700 Zeilen)
│   ├── io.rs                # File-I/O, native + web_impl Submodule (~630 Zeilen)
│   ├── history.rs           # Undo/Redo (~90 Zeilen)
│   ├── store.rs             # Bildspeicher + Texture-Cache (~40 Zeilen)
│   ├── geometry.rs          # Geometrie-Helfer (~50 Zeilen)
│   ├── themes.rs            # Farb-Themes (~180 Zeilen)
│   ├── fonts.rs             # Font-Loading (~105 Zeilen)
│   ├── settings_io.rs       # Settings-Persistenz (~115 Zeilen)
│   ├── file_watch.rs        # File-Watcher (nativ) (~68 Zeilen)
│   ├── app.rs               # App-State + UI-Logik (~2050 Zeilen, Refactor offen)
│   ├── canvas.rs            # Canvas-Rendering + Interaktion (~1430 Zeilen, Refactor offen)
│   ├── odt.rs               # ODT-Import/-Export (~330 Zeilen)
│   └── printing.rs          # PDF-Export (~265 Zeilen)
├── assets/
│   └── fonts/               # Inter, Roboto, Lora, JetBrains, Pacifico (TTF)
├── Cargo.toml              # Dependencies + Profile
├── Trunk.toml              # WASM-Build-Konfig
└── index.html              # WASM-Entry-Point
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

**Core:** `eframe`, `egui`, `image`, `serde`, `serde_json`, `base64`.
**Nativ-only:** `rfd`, `zip`, `printpdf`, `notify`, `notify-debouncer-mini`,
`crossbeam-channel`.
**WASM-only:** `wasm-bindgen`, `wasm-bindgen-futures`, `web-sys`, `js-sys`.

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
| Max-File-Size | unlimitiert | siehe `SECURITY.md` SW-003 |

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
trunk serve                             # Dev-Server
trunk build --release                   # Production-WASM in dist/
```

### Cross-Compile

```bash
cargo build --release --target x86_64-pc-windows-msvc
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target x86_64-apple-darwin
```

---

## Lizenz

Dual-lizenziert **MIT OR Apache-2.0**. Siehe [`LICENSE`](LICENSE).
