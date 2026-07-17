# Roadmap

BoxDoc ist ein nativer, KI-freundlicher Dokumenten-Editor (Rust + egui).
Diese Datei ist die einzige verbindliche Quelle für Status und Planung.

> Letztes Update: 18. Juli 2026 · Aktuelle Version: **v0.4.2-dev** (Cargo.toml)

---

## Status Quo (IST, v0.4.2-dev)

| Bereich | Stand | Wo |
|---|---|---|
| Elemente | `Text`, `Image`, `Rectangle`, `Line` | `src/model.rs` |
| Editor | Multi-Select, Copy/Paste-Ghost, Crop, Rotation, Resize | `src/canvas.rs` |
| Undo/Redo | Snapshot-basiert, max. 200 Einträge | `src/history.rs` |
| AI-Sync | Native `notify`-File-Watcher, Reload als Undo-Schritt | `src/file_watch.rs`, `src/app.rs:642` |
| ODT | Import + Export (nativ) mit ZIP-Limit | `src/odt.rs` |
| PDF | Export (nativ), **nur Text + Bilder, keine Shapes** | `src/printing.rs:104` |
| Sicherheit | Shell-Args separiert, Pfad-Checks, ZIP-Limit (Phase 0 erledigt) | `SECURITY.md` |
| WASM | Gerüst: `main.rs:46-77`, `index.html`, `Trunk.toml`, 18 `cfg`-Attribute; File-I/O via Browser-API | `src/io.rs:361` (`web_impl`) |
| Papier | A3, A4, A5, Letter, Legal · Portrait/Landscape · Mehrere Seiten | `src/model.rs` |

**Offen (bekannt):** keine Ellipse/Kreis, kein PDF-Import, kein responsives Mobile, keine Tests/CI.

---

## Leitplanken für alle Phasen

1. **Eine Codebase** — Rust für Desktop, WASM für Web/Mobile, optional PHP für Server-Variante. Keine zweite Sprache für die App-Logik.
2. **AI-first** — die `.boxdoc`-Datei bleibt die einzige AI-Schnittstelle. Jede Phase muss file-basiert funktionieren.
3. **Simple first** — pro Phase nur das Nötigste. WebRTC, Plugin-System, Auto-Save sind bewusst **nicht** Teil der Roadmap.
4. **Server-optional** — BoxDoc läuft lokal ohne jeden Server. Server-Variante (Phase 4) ist additiv.
5. **Keine Tests als Blocker** — Tests sind nicht Teil der Definition-of-Done der Phasen. Sie kommen später, wenn sich das Modell stabilisiert hat.

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

**WASM:** PDF-Import/-Export im Browser optional via JS-Library (`pdf.js`); nicht Teil von Phase 2.

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
- [ ] `Trunk.toml`-Optimierungen (kleinere WASM, lazy-Fonts)

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

| Version | Fokus | Status |
|---|---|---|
| v0.4.1 | Vor-Phase-0-Stand | ⚠️ 3 Security-Fixes fehlen |
| v0.4.2 | Sicherheits-Stabilisierung | ✅ Phase 0 erledigt |
| v0.5.0 | Ellipse-Shape | ⬜ Phase 1 |
| v0.6.0 | PDF-Roundtrip | ⬜ Phase 2 |
| v0.7.0 | Mobile WASM | ⬜ Phase 3 |
| v0.8.0 | PHP-Server | ⬜ Phase 4 |
| v0.9.0 | Native Mobile | ⬜ Phase 5 |
| v1.0.0 | SSE-Multiuser (optional) | ⬜ Phase 6 |
| v1.0+ | Weitere Shapes, Polish | ⬜ Phase 7 |

---

## Was bewusst NICHT auf der Roadmap steht

- **Auto-Save** — bewusst weggelassen; AI-Workflow + manuelles Speichern reicht.
- **WebRTC** — zu komplex, SSE reicht für die Nutzerzahlen.
- **Operational Transformation / CRDTs** — Last-Write-Wins pro `id` reicht.
- **WebSocket** — SSE ist simpler und passengerechte genug.
- **Tests als DoD** — werden erst in Phase 7 relevant.
- **Plugin-System** — kann folgen, wenn sich das Modell stabilisiert hat.
- **CI/CD-Pipeline** — folgt mit Phase 7; aktuell manueller Build.
