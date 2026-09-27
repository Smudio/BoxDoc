# Roadmap

BoxDoc ist ein nativer, KI-freundlicher Dokumenten-Editor (Rust + egui).
Diese Datei ist die einzige verbindliche Quelle für Status und Planung.

> Letztes Update: 16. August 2026 · Aktuelle Version: **v0.7.0** (Cargo.toml)

---

## Status Quo (IST, v0.7.0)

| Bereich | Stand | Wo |
|---|---|---|
| Elemente | `Text`, `Image`, `Rectangle`, `Line`, `Ellipse`, `Path` (mit Bézier-Kurven) | `src/model.rs` |
| Text | fett, kursiv, unterstrichen, durchgestrichen; echte Schriftschnitte, sonst nachgeahmt | `src/text_layout.rs`, `src/fonts.rs` |
| Editor | Multi-Select, Copy/Paste-Ghost, Crop, Rotation, Resize, Snapping | `src/canvas.rs` |
| Zeichnen | Auswahl · Linie · Pfad (Pen) · Freihand, dazu Knotenbearbeitung | `src/canvas.rs`, `src/geometry.rs` |
| Undo/Redo | Snapshot-basiert, max. 200 Einträge | `src/history.rs` |
| AI-Sync | Native `notify`-File-Watcher, Reload als Undo-Schritt | `src/file_watch.rs` |
| Multiuser | Drei-Wege-Merge, optimistische Nebenläufigkeit über `version` | `src/merge.rs`, `src/web_sync.rs` |
| ODT | Import + Export (Desktop **und** Web) mit ZIP-Limit, **ohne Shapes** | `src/odt.rs` |
| PDF | Import (pdfium, nur Desktop) + Export (printpdf, **auch Web**), alle Shapes, echte Kurven, eingebettete Schriften | `src/pdf_import.rs`, `src/printing.rs` |
| SVG | Export (Desktop **und** Web): ganze Seite **oder Auswahl**, echte Primitive und Transparenz | `src/svg.rs` |
| Sicherheit | Shell-Args separiert, Pfad-Checks, ZIP-Limit, Backend-Token | `SECURITY.md` |
| WASM | Mit der EXE **angeglichen**: gleicher Menübaum, gleiche Ausgabebytes. Nur PDF-Import und Drucken fehlen (ausgegraut, Grund im Tooltip) | `src/io.rs` (`web_impl`), `tests/web_parity.rs` |
| Papier | A3, A4, A5, Letter, Legal · Portrait/Landscape · Mehrere Seiten | `src/model.rs` |
| Tests | 260 bestanden, `cargo test` | `src/`, `tests/` |

**Offen (bekannt):** kein responsives Mobile, keine CI, ODT-Export ohne
Shapes/Textformatierung, Text-Rotation wird nicht gerendert, keine eigene
Toolbar (Werkzeugwahl liegt im Eigenschaften-Panel), WYSIWYG-Textbearbeitung
fehlt. Bestandsaufnahme: [`REVIEW.md`](REVIEW.md).

---

## Leitplanken für alle Phasen

1. **Eine Codebase** — Rust für Desktop, WASM für Web/Mobile, optional PHP für Server-Variante. Keine zweite Sprache für die App-Logik.
2. **AI-first** — die `.boxdoc`-Datei bleibt die einzige AI-Schnittstelle. Jede Phase muss file-basiert funktionieren.
3. **Simple first** — pro Phase nur das Nötigste. WebRTC, Plugin-System, Auto-Save sind bewusst **nicht** Teil der Roadmap.
4. **Server-optional** — BoxDoc läuft lokal ohne jeden Server. Server-Variante (Phase 4) ist additiv.
5. **Tests gehören zur Definition-of-Done** — *geändert am 14.08.2026.* Die frühere Regel
   ("Tests kommen später") war die Ursache für die Hälfte der in `REVIEW.md` dokumentierten
   Fehler: nie speichernder Auto-Save, nicht undo-barer Text, PDF ohne Zeilenumbruch. Jeder
   davon wäre von einem trivialen Test gefangen worden. Neue Logik in `model`, `history`,
   `merge`, `text_layout` und `io` braucht ab sofort einen Test.

---

## Phase 0 — Sicherheits-Stabilisierung · v0.4.2 ✅ erledigt

**Ziel:** Bekannte Schwachstellen schließen, Doku an Realität anpassen.

- [x] `printing.rs:49-56` — `.arg()` statt `.args([...])` (SW-001)
- [x] `io.rs` — `is_safe_path()` + `ensure_canonicalizable()` in `load_project`,
      `save_project`, `import_odt_dialog` (SW-002)
- [x] `odt.rs` — `MAX_EXTRACT_SIZE` (100 MB) + `MAX_ARCHIVE_TOTAL_SIZE` (400 MB)
      beim ZIP-Lesen (SW-003)
- [x] `SECURITY.md` — Fix-Status korrigiert (Fixed in main / v0.4.2)
- [x] `ARCHITECTURE.md` — Security-Passagen auf IST-Stand gebracht
- [x] `Cargo.toml` auf `0.4.2-dev` gesetzt

**Agent:** `phase0-security` (siehe `AGENTS_TASKS.md`)

---

## Phase 1 — Shapes · v0.4.0

**Ziel:** `Ellipse` als neue Element-Art (Kreis = Spezialfall `w == h`).

- [ ] `ElementKind::Ellipse` in `src/model.rs`, Default-Werte, `default_element()`
- [ ] Canvas-Rendering in `src/canvas.rs:870` (Match-Arm) → `Painter::add(circle/ellipse)`
- [ ] Properties-Panel in `src/app.rs:1289` (Fill, Stroke, Radius-X/Radius-Y)
- [ ] Resize-Logik in `src/canvas.rs` (zusätzlicher Handle oder w/h wie Rectangle)
- [ ] Hit-Test in `point_in_element` (`src/canvas.rs`)
- [ ] PDF-Export in `src/printing.rs:88` (printpdf circle/ellipse-Primitive)
- [ ] ODT-Export in `src/odt.rs:176` (`<draw:ellipse>`)
- [ ] JSON-Schema-Doku in `AGENTS.md` ergänzen
- [ ] Toolbar-Button + Shortcut

**Nicht in Phase 1:** Polygon, Arrow, Callout → später (Phase 7).

**Agent:** `phase1-shapes`

---

## Phase 2 — PDF-Roundtrip · v0.5.0

**Ziel:** PDF komplett **importieren** und vollständig **exportieren** können.

### Import
- [ ] `pdf-extract` oder `lopdf`/`pdfium-render` als native Dependency
- [ ] `import_pdf_dialog()` in `src/io.rs`
- [ ] Parser: Text-Runs mit Position + Font-Size → `Text`-Elemente; Vektorpfade → `Rectangle`/`Line`/`Ellipse`; eingebettete Bilder → `Image`
- [ ] Multi-Page-PDF → Multi-Page-Doc
- [ ] Schätzung für fehlende Metriken (Bold, Font-Family → `default`)

### Export
- [ ] Shapes in `printing.rs` rendern (Rect, Line, Ellipse)
- [ ] Vollständige Font-Übersetzung (Bold, Italic, Inter/Roboto/Lora/JetBrains/Pacifico)
- [ ] Korrekte Positionierung inkl. `align`, `valign`, `rotation`
- [ ] Mehrseitiger Export
- [ ] CI-Test-Setup: Sample `.boxdoc` → PDF → neu einlesen → Diff

**WASM:** ✅ **PDF-Export läuft im Browser** — ohne JS-Library, mit demselben
`printpdf`-Code wie nativ (`printing::pdf_bytes()`). Dafür war ein Patch an
printpdf 0.7 nötig, dessen wasm-Datums-Polyfill nicht baut; siehe
`vendor/printpdf/BOXDOC-PATCH.md`. PDF-**Import** bleibt Desktop-exklusiv
(pdfium ist eine native C++-Bibliothek); ein zweiter Parser via `pdf.js` würde
andere Ergebnisse liefern als die EXE und ist deshalb ausdrücklich nicht geplant.

**Agent:** `phase2-pdf`

---

## Phase 3 — Mobile & Responsive WASM · v0.6.0

**Ziel:** BoxDoc im Browser auf Smartphone/Tablet nutzbar.

- [ ] Touch-Interaktion: Pinch-Zoom, Two-Finger-Pan, Long-Press-Selektion, Tap-Edit
- [ ] `eframe` Touch-Events an `canvas.rs` anbinden ( aktuell nur Mouse)
- [ ] Responsive UI: Toolbar unten als Bottom-Sheet, kollabierbare Seitenleiste, gröbere Handles
- [ ] Viewport-Meta-Tag + `index.html` für Mobile optimiert
- [ ] Test auf Android Chrome + iOS Safari (manuelles QA)
- [ ] Last-Sitzung in IndexedDB persistieren (Dok + Scroll-Position)
- [x] Kleineres WASM: `build-web.ps1` baut mit dem Profil `wasm-release`
      (`opt-level="s"`, `lto`, `panic="abort"`) — 5,7 statt 7,5 MB
- [ ] Lazy-Fonts (Schriften erst bei Bedarf laden)

**Kein Auto-Save:** Keine automatische Speicherung der Daten — nur die letzte Sitzung zur Wiederherstellung.

**Agent:** `phase3-mobile`

---

## Phase 4 — Server-Variante mit PHP · v0.7.0

**Ziel:** BoxDoc-Dateien am Webserver verwalten, AI über HTTP-PUT, SSR für Agenten.

- [ ] `server/api.php` (~150 Zeilen): `GET/PUT/DELETE /docs/<slug>?t=<token>` — speichert in `server/docs/<slug>.boxdoc`
- [ ] `server/index.php`: SSR — bettet Dokument-JSON + vollständige AI-Anleitung in HTML ein (für `curl` von Agenten)
- [ ] Capability-Token (32 Zeichen) pro Dokument; Lese-URL optional public, Schreib-URL nur mit Token
- [ ] `server/.htaccess`: Rewrite-Regeln + Caching-Header
- [ ] `server/README.md`: Deployment auf Shared-Hosting (PHP 7.4+)
- [ ] BoxDoc-Client: neuer "Open from URL"-Dialog (`URL + Token`), Schreiben via HTTP-PUT
- [ ] File-Watcher-Erweiterung: optional Polling einer Remote-URL (300 ms) statt lokales FS
- [ ] Demo-Deployment auf `boxdoc.at`

**Single-User-Cloud:** Keine Multiuser-Echtzeit in Phase 4. Jeder Client lädt/speichert isoliert.

**Agent:** `phase4-server`

---

## Phase 5 — Native Mobile Apps · v0.8.0

**Ziel:** BoxDoc als native iOS/Android-App (gleiche Rust-Logik, mobile Backend).

- [ ] Build-Setup via `cargo-mobile2` oder `xbuild`
- [ ] egui Mobile-Backend testen (Touch-Input bereits unterstützt)
- [ ] Native Datei-Auswahl, Sharing-Intent (Share-Sheet)
- [ ] Sandbox-Speicherung (App-Documents)
- [ ] iOS App Store + Android Play Store Vorbereitung (Icons, Metadaten)
- [ ] Crash-Reporting optional

**Agent:** `phase5-native-mobile`

---

## Phase 6 — Multiuser-Echtzeit (optional) · v0.9.0

**Nur wenn echter Bedarf aus Nutzerfeedback.** Bewusst SIMPEL, kein WebRTC.

- [ ] `server/stream.php` — SSE-Endpoint, pusht `PUT`-Events an alle Subscriber der `<slug>`
- [ ] Browser: `EventSource` auf `/stream.php?slug=…&t=…`
- [ ] Cursor-Präsenz: Position 10×/s über SSE mitschicken (nicht über WebRTC)
- [ ] Last-Write-Wins pro `element.id` (id-basiert, kein OT/CRDT)
- [ ] Präsenz-Indikator: Name + Farbe, lokal pro Session generiert

**Kein WebRTC, kein OT.** Für 2–10 Peers reicht SSE völlig.

**Agent:** `phase6-collab`

---

## Phase 7 — Weitere Shapes & Polish · v1.0+

Später, nicht zeitkritisch:

- [ ] `Polygon` (beliebige Punkte, geschlossen oder offen)
- [ ] `Arrow` (Linie mit Pfeilspitze, konfigurierbar)
- [ ] `Callout` (Rechteck + Linie + Text kombiniert)
- [ ] Auto-Save (Crash-Recovery, lokal, optional)
- [ ] Code-Refactoring: `app.rs` und `canvas.rs` aufteilen (siehe `AGENTS_TASKS.md`)
- [ ] Performance: Viewport-Culling, Quad-Tree Hit-Test, Texture Lazy-Loading
- [ ] Tests + CI (GitHub Actions: native + WASM)
- [ ] Plugin-System für AI-Agenten

---

## Versionstabelle

| Version | Fokus | Phase | Status |
|---|---|---|---|
| v0.4.1 | Vor-Phase-0-Stand | — | ⚠️ 3 Security-Fixes fehlten |
| v0.4.2 | Sicherheits-Stabilisierung | 0 | ✅ erledigt |
| v0.4.x | Ellipse-Shape | 1 | ✅ erledigt |
| v0.5.0 | Web-Sync (PHP-Backend), JSON-Editor, Datei-Browser | 4 | ✅ erledigt |
| v0.5.x | PDF-Roundtrip (Import + vollständiger Export) | 2 | ✅ erledigt |
| v0.6.0 | Zuverlässigkeit & Multiuser-Merge | 8 | ✅ erledigt |
| v0.7.0 | Pfade, Kurven & PDF-Kurven-Rundlauf | 10 | ✅ erledigt |
| — | Mobile & Responsive WASM | 3 | ⬜ offen |
| — | Bedienung (Toolbar, WYSIWYG-Text, ODT-Shapes) | 9 | ⬜ offen |
| — | Native Mobile Apps | 5 | ⬜ offen |
| — | SSE-Multiuser-Echtzeit (optional) | 6 | ⬜ offen |
| v1.0+ | Weitere Shapes, Polish, CI | 7 | ⬜ offen |

> Die Versionsspalte folgt der `Cargo.toml`-Historie, nicht der ursprünglich
> geplanten Nummerierung — die Phasen wurden in anderer Reihenfolge umgesetzt
> als 2026 angenommen.

---

## Was bewusst NICHT auf der Roadmap steht

- **Auto-Save** — bewusst weggelassen; AI-Workflow + manuelles Speichern reicht.
- **WebRTC** — zu komplex, SSE reicht für die Nutzerzahlen.
- **Operational Transformation / CRDTs** — Last-Write-Wins pro `id` reicht.
- **WebSocket** — SSE ist simpler und passengerechte genug.
- **Tests als DoD** — werden erst in Phase 7 relevant.
- **Plugin-System** — kann folgen, wenn sich das Modell stabilisiert hat.
- **CI/CD-Pipeline** — folgt mit Phase 7; aktuell manueller Build.

---

## Phase 8 — Zuverlässigkeit & Multiuser · v0.6.0 ✅ erledigt

**Ziel:** Die in `REVIEW.md` dokumentierten Datenverlust-Pfade schließen und
gleichzeitiges Bearbeiten korrekt lösen.

### Datenverlust (Sprint 1)
- [x] Close-Guard: Fenster schließen bei `modified` fragt nach
      (`app.rs` → `PendingAction`, `show_unsaved_dialog`)
- [x] Neu/Öffnen/Datei-Browser laufen über `request_action`
- [x] `Strg+S`, `Strg+Umschalt+S`, `Strg+O`, `Strg+N`, `Strg+P`,
      `Strg+A`, `Strg+D`, `Strg+X` (`app.rs` → `handle_shortcuts`)
- [x] Shortcuts im Menü sichtbar (`menu_entry`)
- [x] Neues Menü „Bearbeiten" mit ausgegrautem Undo/Redo
- [x] Textbearbeitung ist undo-bar (`canvas.rs`, Snapshot vor dem Commit)
- [x] Pfeiltasten-Verschieben ist undo-bar, als ein Schritt zusammengefasst
- [x] Focus-Guards: `Entf`, `L`, Pfeiltasten und Strg+C/V wirken nicht mehr
      in Textfeldern — behebt u. a. „Strg+V im JSON-Editor tut nichts"
- [x] `panic!` im PDF-Export durch `Result` ersetzt (`printing.rs`)
- [x] Alle Settings werden persistiert (vorher nur Theme + Panel-Position)
- [x] Doppeltes „Rahmenstärke"-Bedienelement entfernt

### PDF / WYSIWYG (Sprint 2)
- [x] `src/text_layout.rs` — **ein** Layout-Pfad für Canvas und PDF
- [x] Zeilenumbruch im PDF (vorher nur `\n`-Split)
- [x] Exakte Zeilenbreiten statt `Zeichen × 0,5 × Größe`
- [x] Ausrichtung wirkt je Zeile, nicht auf den Block
- [x] `corner_radius` wird auf dem Canvas gerendert (war `let _ = radius`)
- [x] Modell-Mutation aus `draw_element` in expliziten Reflow-Schritt gezogen
- [x] `auto_height` im Modell — Auto-Höhe und `valign` schließen sich nicht
      mehr stillschweigend gegenseitig aus

### Multiuser (Sprint 3)
- [x] `src/merge.rs` — Drei-Wege-Merge auf Element-Ebene
- [x] Optimistische Nebenläufigkeit über `version` + HTTP 409
- [x] Konflikt liefert den Serverstand gleich mit (kein zweiter Roundtrip)
- [x] Polling über `?meta=` statt Volldokument
- [x] Auto-Save-Debounce repariert (feuerte nie)
- [x] `base_doc` als gemeinsame Merge-Basis

### Backend-Sicherheit (Sprint 4)
- [x] `has_access()` auf dem `/<slug>`-Pfad und bei der HTML-Auslieferung
- [x] `?list=1` zeigt nur zugängliche Dokumente
- [x] XSS beim JSON-Inject behoben (`</` → `<\/`)
- [x] Token per `X-BoxDoc-Token`-Header
- [x] Neue Dokumente sind standardmäßig token-geschützt
- [x] Rate-Limit auf `?new=1`; atomares Schreiben mit eindeutigem Temp-Namen
- [x] `stream.php`: Lebensdauer 120 s → 25 s (Worker-Erschöpfung)

### Formen-Geometrie (Nachtrag)
- [x] `geometry.rs` — Umrisse einmalig in Seitenkoordinaten
- [x] Linien drehen um ihren Mittelpunkt (PDF drehte um den Startpunkt)
- [x] Rechteck-Rotationsrichtung im PDF korrigiert
- [x] Alpha wird im PDF berücksichtigt (`blend_over_white`)
- [x] ~5 200 Zeichen duplizierte Formenlogik entfernt

### Tests
- [x] 92 Tests: `merge`, `text_layout`, `geometry`, `history`, Dateiformat,
      PDF-Roundtrip, Canvas/PDF-Deckungsgleichheit
- [x] PDF-Test liest das erzeugte PDF mit pdfium zurück und vergleicht den Text

---

## Phase 10 — Pfade, Kurven & PDF-Rundlauf · v0.7.0 ✅ erledigt

**Ziel:** Freie Pfade zeichnen und bearbeiten; Kurven überstehen den Weg durch
ein PDF unbeschadet.

### Datenmodell
- [x] `Element::handles` — kubische Bézier-Griffe je Stützpunkt, auf die Box
      normalisiert wie `points`. Leer = reiner Streckenzug, dann fehlt das
      Feld in der Datei ganz (`skip_serializing_if`)
- [x] Reparatur falsch langer Griff-Vektoren an **einer** Stelle: der
      Deserialisierung von `Page::elements` — kein Ladepfad kann sie umgehen
- [x] `merge.rs`: `points` und `handles` als **ein** Feld mergen, sonst
      landeten die Griffe der einen Seite an den Stützpunkten der anderen

### Geometrie (`geometry.rs`)
- [x] `PathNode` + `path_nodes` / `set_path_nodes` als einziger Schreibweg,
      hält die Invariante „Box umschließt den Pfad" (über die gezeichnete
      Kurve, nicht über Stützpunkte oder Griffe), Drehung bleibt erhalten
- [x] `path_segments` (echte Kurven fürs PDF) vs. `path_outline` (aufgelöst
      für den Bildschirm)
- [x] Bearbeitung: `insert_node` (De Casteljau, formtreu), `remove_node`,
      `move_node`, `move_handle` (gespiegelt oder frei), `toggle_node_smooth`,
      `smooth_path`, `sharpen_path`
- [x] Freihand: `simplify_polyline` (Ramer-Douglas-Peucker) +
      `nodes_from_polyline` (Catmull-Rom-Tangenten)

### Werkzeuge & Bearbeitung
- [x] `Tool { Select, Line, Pen, Freehand }` — **ein** Feld für den
      Werkzeugzustand; der frühere Linienmodus als eigenes `Option`-Feld ließ
      zwei Modi gleichzeitig zu
- [x] Pen (P): Klick = Ecke, Klick+Ziehen = Kurvengriffe, Klick auf den
      Startknoten schließt, Enter/Doppelklick beendet, Rücktaste nimmt zurück
- [x] Freihand (F): Spur wird beim Loslassen ausgedünnt und geglättet,
      schließt sich selbst, wenn sie am Startpunkt endet
- [x] „Einfügen → Pfad": fertige Kurve mittig auf der Seite, wie Rechteck und
      Ellipse — mit sofort geöffneter Knotenbearbeitung, weil ein Pfad ohne
      sichtbare Knoten wie ein bloßer Strich wirkt
- [x] Knotenbearbeitung: Knoten und Griffe ziehen, Doppelklick auf ein
      Segment fügt ein, Entf löscht, Alt+Klick schaltet Ecke/Kurve um
- [x] Drei Wege hinein, weil eine Funktion, die man nicht findet, keine ist:
      **Doppelklick auf den Pfad**, Taste `N`, Panel-Schalter. Der Doppelklick
      legte vorher ein Textfeld über den Pfad — die Suche im Doppelklick-Zweig
      kannte nur Text-Elemente, alles andere galt als leere Fläche
- [x] Löschen und Ecke/Kurve auch als Panel-Schalter, samt Anzeige des
      ausgewählten Knotens — vorher nur über Entf und Alt+Klick erreichbar
- [x] Statuszeile weist beim Auswählen eines Pfads auf die Bearbeitung hin
- [x] Ecken als Quadrat, weiche Knoten als Kreis
- [x] Eigenschaften-Panel: Knotenzahl, Glätten/Ecken, Geschlossen
- [x] Werkzeugleiste im Panel; Werkzeugtasten nur ohne Strg (vorher hätte
      Strg+P neben dem Drucken auch das Werkzeug gewechselt)

### PDF
- [x] Import: Bézier-Segmente werden zu Knoten mit Griffen, statt in bis zu
      24 Strecken zerlegt und auf 256 Punkte gedünnt zu werden
- [x] Export: echte Kurven-Operatoren über printpdfs Kontrollpunkt-Flags
- [x] Ende-zu-Ende-Test: Kurve → PDF → Import → immer noch eine Kurve mit
      vier Knoten an derselben Stelle

### Nebenbei behoben
- [x] Hit-Test für Pfade prüfte die Hüllbox statt der Kontur — ein offenes
      Häkchen fing Klicks über seine gesamte Box ab

### Tests
- [x] `cargo test` meldet 165 bestandene Tests (vorher 126); neu sind 22
      eigene: Kurven-Rundlauf durch PDF und JSON, Box-Invariante, formtreues
      Einfügen, Reparatur kaputter Griffe, Spiegeln von Griffen, Ausdünnen
- [x] Das Pfad-Beispiel aus `AGENTS.md` wird als Test ausgeführt — die
      KI-Schnittstelle kann nicht mehr unbemerkt von der Implementierung
      abdriften

**Nicht enthalten:** ODT-Export von Pfaden (ODT kennt bis heute überhaupt
keine Shapes, siehe Phase 9), zusammengesetzte Pfade mit Löchern,
gestrichelte Linien.

---

## Phase 11 — SVG-Export & Zeiger-Rückmeldung · v0.7.0 ✅ erledigt

**Ziel:** Was auf der Seite steht, soll sich als Vektor herausholen lassen —
besonders eine **Auswahl**. Und der Zeiger soll vorher sagen, was ein Klick tut.

### SVG (`src/svg.rs`)
- [x] `Scope::Page` (Seitenformat, weißer Grund) und `Scope::Selection`
      (Hüllbox der gewählten Objekte + 8 pt Rand, **durchsichtig** — eine
      Auswahl soll sich woanders einfügen lassen, ohne weißen Kasten)
- [x] Jedes Objekt bleibt sein Primitiv: `<rect>`, `<ellipse>`, `<line>`,
      `<path>` mit kubischen Bézier-Segmenten, `<text>` mit `<tspan>` je Zeile.
      Nichts wird zu einem Vieleck aufgelöst
- [x] Echte Transparenz über `fill-opacity` — der PDF-Export muss über Weiß
      mischen, weil printpdf 0.7 kein `ExtGState` freigibt
- [x] Bilder als data-URI eingebettet; Crop über `<clipPath>` + Versatz statt
      über neu berechnete Pixel, damit der Ausschnitt verschiebbar bleibt
- [x] `geometry::element_bounds` / `elements_bounds` — Leinwand aus der
      Kontur, nicht aus `x/y/w/h`: Gedrehtes ragt hinaus, eine Linie hat
      `h == 0`
- [x] Modul ohne UI- und Plattform-Abhängigkeit (erzeugt nur eine
      Zeichenkette), deshalb vollständig testbar
- [x] Menü: „Seite als SVG exportieren…" und „Auswahl als SVG exportieren…"
      (ausgegraut mit Grund statt versteckt)
- [x] Knopf „Auswahl als SVG…" **im Eigenschaften-Panel**, für Einzel- und
      Mehrfachauswahl. Eine Auswahl zu exportieren ist eine Aktion auf der
      Auswahl — sie gehört dorthin, wo man sie gerade in der Hand hat, nicht
      zwei Menüebenen entfernt zwischen Öffnen und Drucken

### Zeiger-Rückmeldung (`canvas.rs`)
- [x] Hand über Auswählbarem, Verschieben-Kreuz über Ausgewähltem,
      Größenpfeile über den Griffen (per Winkel, also drehungsrichtig),
      Greifhand über Drehgriff/Endpunkt/Knoten, Fadenkreuz beim Zeichnen
- [x] `topmost_at` als **eine** Trefferauflösung für Klick und Cursor — getrennt
      gerechnet wären sie irgendwann auseinandergelaufen, und ein Cursor, der
      etwas anderes verspricht als der Klick tut, ist schlimmer als keiner

### Nebenbei behoben
- [x] Pfad-Trefferprüfung lief **nach** einer randlosen Hüllbox-Prüfung. Die
      Box eines flachen Pfads ist `PATH_MIN_EXTENT` (0,5 pt) hoch — die
      Toleranz kam damit nie an, ein waagerechter Zug war auf ein Viertelpixel
      genau zu treffen. Daher „passiert nichts" und „ständig Textfelder"
- [x] Klick-Radius 8 px, Doppelklick 16 px, Doppelklick auf einen bereits
      ausgewählten Pfad 40 px

### Tests
- [x] `cargo test` meldet 227 bestandene Tests (vorher 165); neu sind u. a.
      25 zum SVG-Export, darunter eine **XML-Prüfung mit echtem Parser**
      (`quick-xml`, nur `dev-dependency`) samt Gegenprobe, dass sie bei
      kaputtem SVG auch wirklich anschlägt

**Nicht enthalten:** SVG-**Import**, mehrseitige SVG-Ausgabe, Einbetten der
Schriften in die SVG-Datei (sie werden nur benannt, mit generischer
Rückfallebene).

---

## Phase 12 — Textauszeichnung & Schrifteinbettung · v0.7.0 ✅ erledigt

**Ziel:** Fett, kursiv, unterstrichen und durchgestrichen sollen vollständig
funktionieren — auf dem Bildschirm, im PDF und im SVG. Auslöser war, dass eine
ausgezeichnete Seite nicht sauber als PDF herauskam.

Die Schalter gab es größtenteils schon. Was fehlte, war, dass sie *ankamen*.

### Neu
- [x] `strikethrough` (durchgestrichen) im Modell, im Panel, im Canvas, im
      PDF- und im SVG-Export sowie im Merge — der letzte fehlende der vier
      klassischen Auszeichnungen
- [x] `FontStyle` (Regular/Bold/Italic/BoldItalic) als eigener Typ statt
      zweier durchgereichter `bool`
- [x] **Echte Schnitte** je Schrift: `FontDef` kennt jetzt `bold_paths`,
      `italic_paths` und `bold_italic_paths`; `fonts.rs` registriert sie als
      eigene Familien, `printing.rs` bettet die passende Datei ein
- [x] `fonts::has_style` als **gemeinsame** Auskunft für Bildschirm und PDF,
      ob ein echter Schnitt vorliegt oder nachgeahmt werden muss
- [x] Nachahmung von fett auch auf dem Canvas (mehrfach versetzt gezeichnet),
      passend zum Umriss, den der PDF-Export zeichnet
- [x] `text_layout::decoration_metrics` — Lage und Stärke der
      Auszeichnungslinien einmal in pt, für Canvas, PDF und SVG

### Behoben
- [x] **Der Schnitt fiel beim Layout unter den Tisch.** `font_id_for` wertete
      `bold`/`italic` nur für die Standardschrift aus. Fettes Arial stand auf
      dem Bildschirm mager und im PDF fett — und weil fette Glyphen breiter
      sind, brachen beide an verschiedenen Stellen um
- [x] **Die Alias-Familien „Bold"/„Italics" waren leer** — sie enthielten
      denselben mageren Schnitt wie `Proportional`. Fett gesetzter Text in der
      Standardschrift maß auf den Punkt genau so breit wie magerer
- [x] **Eingebettete Schriften kamen nie ins PDF.** Der Export suchte sie über
      `paths`, das bei ihnen leer ist, und fiel auf Arial zurück — im
      Browser-Build, wo es *nur* diese Schriften gibt, betraf das alles
- [x] **Für „default" stand Helvetica im PDF**, nicht die Schrift, mit der
      egui zeichnet. Andere Zeichenbreiten als die gemessenen: Der Text lief
      über seine Breite hinaus, die Unterstreichung endete sichtbar vor dem
      letzten Buchstaben. Jetzt kommen die Bytes aus
      `egui::FontDefinitions::default()`
- [x] **Der Unterstrich riss den Grafikzustand mit** — er setzte die
      Strichstärke mitten in der Zeilenschleife auf seinen eigenen Wert, sodass
      ab der zweiten Zeile der nachgeahmte Fettdruck um ein Vielfaches zu dick
      auftrug. Glyphen und Linien laufen jetzt in getrennten Durchgängen
- [x] **Die Linienfarbe war ererbt:** ohne eigene Zuweisung nahm ein
      Unterstrich die Randfarbe des zuletzt gezeichneten Rechtecks an
- [x] Der Hit-Test rechnete mit dem mageren Schnitt und griff bei fettem Text
      daneben

### Tests
- [x] `cargo test` meldet 247 bestandene Tests (vorher 227); neu ist
      `tests/text_styles.rs` (18) plus zwei zum SVG-Export

**Nicht enthalten:** Auszeichnung einzelner Wörter innerhalb eines Textblocks
(sie gilt je Element), Kursiv-Nachahmung auf dem Canvas (egui kann eine Galley
nicht scheren — im PDF wird geschert), Einbetten von Custom-Fonts aus der
`.boxdoc`-Datei in das PDF.

---

## Phase 9 — Bedienung (offen)

- [ ] Toolbar mit den sechs Kernwerkzeugen (Werkzeugwahl liegt derzeit im
      Eigenschaften-Panel, siehe Phase 10)
- [ ] WYSIWYG-Textbearbeitung (Font/Größe/Farbe/Ausrichtung im Overlay)
- [ ] Text-Rotation rendern + Rotationsgriff für alle Element-Typen
- [ ] ODT-Export: Rechteck/Linie und Textformatierung
- [ ] Leere Textelemente nach `Esc` aufräumen
- [ ] 48 deprecated egui-APIs migrieren; CI mit `-D warnings`
- [ ] `app.rs` / `canvas.rs` aufteilen (Phase 7d/e)
