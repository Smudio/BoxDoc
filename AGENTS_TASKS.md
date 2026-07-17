# Agent Tasks für BoxDoc

Delegierbare Aufgabenbeschreibungen für Subagents (z. B. `opencode` Task-Tool).
Jede Phase aus `ROADMAP.md` hat hier einen Agent-Stub mit klarem Scope,
klar abgegrenzt von anderen Phasen.

> Letztes Update: 18. Juli 2026

---

## Nutzungs-Hinweis

- Agenten arbeiten **isoliert** auf dem Codebase-Snapshot.
- Jeder Agent muss am Ende `cargo build --release` (und für Web/WASM-Phasen
  `trunk build`) erfolgreich durchführen können.
- **Kein Commit/PR ohne explizite Anweisung** — der Nutzer reviewt manuell.
- Bei Konflikten mit `AGENTS.md` (Dateiformat-Spec) ist `AGENTS.md` maßgeblich.

---

## Phase 0 — `phase0-security`

**Scope:** Sicherheitslücken schließen + Plandoku konsolidieren.

**Inputs:**
- `SECURITY.md` (Liste der 3 CVEs)
- `src/printing.rs:49-52`, `src/io.rs`, `src/odt.rs:257-265`

**Aufgaben:**
1. `printing.rs:49-52` auf `.arg("/C").arg("start").arg("").arg(...)` umstellen.
2. In `io.rs` eine Funktion `fn is_safe_path(path: &Path, base: &Path) -> bool`
   hinzufügen und in `load_project`, `save_project`, `import_odt_dialog` aufrufen.
3. In `odt.rs` Konstante `MAX_EXTRACT_SIZE: u64 = 100 * 1024 * 1024` definieren
   und in `read_entry` / `ZipArchive::new`-Umgebung prüfen.
4. `SECURITY.md`, `ARCHITECTURE.md` so anpassen, dass keine "Fixed in v0.3.1"-
   Behauptungen mehr drinstehen (Status: "open" bzw. "fixed in this commit").
5. `Cargo.toml` Version auf `0.3.1-dev` setzen.

**Done-Kriterien:**
- `cargo build --release` erfolgreich.
- 3 manuelle Tests: (a) ungültigen Pfad abweisen, (b) 200 MB ODT abweisen,
  (c) normaler Workflow (Open/Save/Print) noch funktionsfähig.

**Nicht im Scope:** Test-Framework, Refactoring, andere Phasen.

---

## Phase 1 — `phase1-shapes`

**Scope:** `Ellipse` als neue Element-Art.

**Inputs:**
- `src/model.rs` (`ElementKind`, `Element`, `default_element`)
- `AGENTS.md` (Dateiformat-Spec — muss erweitert werden)

**Aufgaben:**
1. In `src/model.rs`:
   - `ElementKind::Ellipse` zum Enum hinzufügen.
   - `default_element()` um Ellipse-Default ergänzen (Fill hellgrau, Stroke
     schwarz, `corner_radius` ignoriert).
   - Serialisation testen (mit `serde_json`).
2. In `src/canvas.rs`:
   - Match-Arm bei `:870` (Rectangle) erweitern.
   - Ellipse zeichnen: `Painter::add(PathShape::ellipse(...))` mit
     `fill_color` und `stroke_color`/`stroke_width`.
   - Hit-Test `point_in_element` erweitern:
     `(dx/rx)^2 + (dy/ry)^2 <= 1` inkl. Rotation (Rücktransform).
   - Resize-Handles: 4 (wie Rectangle) oder 8 — Rectangle-Logik reicht.
3. In `src/app.rs:1289` (Properties-Panel):
   - Match-Arm für Ellipse, gleiche Felder wie Rectangle (Fill, Stroke,
     Stroke-Width); `corner_radius` ausblenden.
   - Toolbar-Button "Ellipse" + Werkzeug-Auswahl erweitern.
4. In `src/printing.rs:88`:
   - Match-Arm für Ellipse → `printpdf` Ellipse/Circle-Primitive.
5. In `src/odt.rs:176`:
   - `<draw:ellipse svg:x="…" svg:y="…" svg:width="…" svg:height="…">`.
6. `AGENTS.md` aktualisieren:
   - `"kind"` um `"Ellipse"` erweitern, Tabelle + Beispiel ergänzen.

**Done-Kriterien:**
- Neue Ellipse per Toolbar erzeugbar, mit Handles verschiebbar/resize-bar.
- Hit-Test stimmt (auch bei Rotation).
- PDF-Export enthält die Ellipse.
- ODT in LibreOffice öffnet die Ellipse sichtbar.
- `.boxdoc` JSON enthält `"kind": "Ellipse"` und ist pretty.

**Nicht im Scope:** Polygon, Arrow, Callout, PDF-Import.

---

## Phase 2 — `phase2-pdf`

**Scope:** PDF-Import + vollständiger PDF-Export (mit Shapes).

**Inputs:**
- `src/printing.rs`, `src/io.rs`, `Cargo.toml`

**Aufgaben:**
1. PDF-Export verbessern:
   - Shapes (Rect, Line, Ellipse) rendern.
   - `bold`/`italic` korrekt im Font-Simulator setzen.
   - `align`, `valign`, `rotation` für Text berücksichtigen.
   - Mehrseitiger Export.
2. PDF-Import:
   - Crate wählen: `pdfium-render` (Google PDFium Binding) bevorzugt,
     Fallback `lopdf` für reine Text-Extraktion.
   - Nur **nativ** (WASM folgt später via `pdf.js`).
   - `import_pdf_dialog()` in `io.rs` (analog `import_odt_dialog`).
   - Parser-Logik in neuer Datei `src/pdf_import.rs`:
     - Text-Runs → `Text`-Elemente mit `x`, `y`, `w`, `h`, `font_size`,
       `bold` (heuristisch), `color`.
     - Vektor-Pfade → `Rectangle`/`Line`/`Ellipse` je nach Form.
     - Eingebettete Bilder → `Image` (PNG via pdfium-Render).
3. Sample-Dateien: 3 PDFs in `tests/fixtures/` (Rechnung, Flyer, Bericht)
   für manuellen Roundtrip-Check.

**Done-Kriterien:**
- Import: öffnet eine Standard-PDF, Text/Bilder sind Positionsgenau im Editor.
- Export: alle 4 Element-Typen sichtbar, Schriften nicht verschoben.
- Roundtrip (`.boxdoc` → PDF → `.boxdoc`) ist visuell nachvollziehbar.

**Nicht im Scope:** WASM-PDF, perfektes Font-Matching, OCR.

---

## Phase 3 — `phase3-mobile`

**Scope:** Responsives WASM für Tablet/Smartphone.

**Inputs:**
- `src/canvas.rs` (Mouse-Handler), `src/app.rs` (UI-Layout), `index.html`

**Aufgaben:**
1. Touch-Input in `canvas.rs`:
   - `egui::Event::Touch` anstatt/zusätzlich zu Mouse.
   - Pinch-Zoom (2 Finger), Pan (1 Finger), Long-Press-Auswahl,
     Doppel-Tap-Edit (Text).
   - Bestehende Mouse-Logik bleibt erhalten (Desktop).
2. Responsive UI in `app.rs`:
   - Bei `window_width < 800` auf Kompaktmodus umschalten: Toolbar unten,
     Panel seitlich ausklappbar.
   - Gröbere Resize-Handles (mind. 24 px Touch-Target).
3. `index.html`:
   - `<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">`
   - Vollbild-Canvas, Safe-Area-Insets berücksichtigen.
4. IndexedDB-Persistierung der letzten Sitzung in `src/io.rs` (`web_impl`):
   - Bei `unload` → `localforage.setItem("last_session", json)`.
   - Bei App-Start → Auto-Laden wenn vorhanden.
5. Manuelles QA: Android Chrome, iOS Safari, iPad Safari.

**Done-Kriterien:**
- Auf 7" Tablet (1024×600) sind alle Funktionen nutzbar.
- Pinch-Zoom flüssig (>30 FPS).
- Letzte Sitzung kehrt nach Tab-Schließen zurück.

**Nicht im Scope:** Native Apps (Phase 5), Auto-Save ins Dateisystem.

---

## Phase 4 — `phase4-server`

**Scope:** PHP-Server-Variante, BoxDoc-Dateien am Webserver verwalten.

**Inputs:**
- Neu: `server/` Verzeichnis (gibt es noch nicht).
- `AGENTS.md` (AI-Anleitung, in `index.php` eingebettet).

**Aufgaben:**
1. `server/api.php` (~150 Zeilen, framework-frei):
   - `GET  /api.php?slug=<slug>&t=<token>` → JSON-Inhalt der Datei.
   - `PUT  /api.php?slug=<slug>&t=<token>` Body = JSON → speichern.
   - `DELETE /api.php?slug=<slug>&t=<token>` → Datei löschen.
   - `slug`: `[a-z0-9-]{1,64}`, sonst 400.
   - `docs/<slug>.boxdoc` Pretty-JSON, UTF-8, atomic write via temp+rename.
   - Token-Check aus `docs/<slug>.meta.json`.
2. `server/index.php`:
   - Wenn `?slug=<slug>` (ohne Token): SSR HTML mit eingebettetem
     Dokument-JSON + gekürzter AI-Anleitung (für `curl`-Agenten).
   - Sonst: Startseite mit "Neues Dokument" → generiert Slug + Token,
     leitet auf Editor weiter.
3. `server/.htaccess`:
   - `/d/<slug>` → `/api.php?slug=<slug>` für Pretty URLs.
   - Caching-Header, MIME-Types.
4. `server/setup.php` (CLI):
   - Erzeugt `docs/` Verzeichnis + `.htaccess` Schutz.
   - Generiert Slug + Token für ein neues Dokument.
5. `server/README.md`:
   - Deployment-Anleitung für Shared-Hosting (PHP 7.4+).
   - Sicherheitshinweise (HTTPS, Token-Handhabung).
6. BoxDoc-Client in `src/io.rs` (`web_impl`):
   - Neuer Menüpunkt "Open from URL".
   - HTTP-PUT beim Speichern (wenn `remote_url` gesetzt ist).
   - Optional: Remote-Polling (300 ms) statt lokalem File-Watcher.

**Done-Kriterien:**
- `curl https://example.com/d/test42` zeigt HTML mit JSON + AI-Anleitung.
- `curl -X PUT -d @file.boxdoc "https://…/api.php?slug=test42&t=SECRET"` speichert.
- BoxDoc-Web-App kann URL+Token laden und speichern.
- Demo auf einem Shared-Hosting läuft.

**Nicht im Scope:** Multiuser-Echtzeit (Phase 6), Nutzerverwaltung.

---

## Phase 5 — `phase5-native-mobile`

**Scope:** Native iOS- und Android-Apps aus der Rust-Codebase.

**Aufgaben:**
1. Build-Setup via `cargo-mobile2` (oder `xbuild`).
2. egui Mobile-Rendering-Backend (Touch-Events aus Phase 3 sind Voraussetzung).
3. Native Datei-Dialoge und Share-Intent.
4. Sandbox-Speicherung (`Documents` Verzeichnis der App).
5. Build-Pipeline für iOS (`.ipa`) und Android (`.apk`/`.aab`).
6. App-Store-Metadaten: Icons, Screenshots, Beschreibung.

**Done-Kriterien:**
- iOS-App startet auf iPhone 12+, öffnet und speichert `.boxdoc`.
- Android-App startet ab API 28, gleicher Funktionsumfang.
- Beide im jeweiligen Store hochladbar.

**Nicht im Scope:** Provider-Integration, Push-Benachrichtigungen.

---

## Phase 6 — `phase6-collab` (optional)

**Scope:** SSE-basierte Multiuser-Echtzeit, kein WebRTC.

**Aufgaben:**
1. `server/stream.php`:
   - SSE-Endpoint, hält Verbindung offen.
   - Pollt `docs/<slug>.boxdoc` alle 200 ms; bei Änderung → Event an alle
     Subscriber.
2. Browser-Client:
   - `EventSource` bei Open-from-URL.
   - Eingehende Events → Reload als Undo-Schritt (bestehende Logik in
     `app.rs:642`).
3. Cursor-Präsenz:
   - 10×/s Position über SSE senden (kleines JSON-Event).
   - Remote-Cursor als farbigen Punkt + Namen rendern in `canvas.rs`.
4. Last-Write-Wins pro `element.id`:
   - Beim Empfang: Element mit passender ID ersetzen, sonst neu anlegen.
   - Konflikte (gleiche ID, unterschiedliche Felder) → zuletzt empfangene
     Version gewinnt.

**Done-Kriterien:**
- 2 Tabs im selben Browser, beide sehen Änderungen des anderen in <500 ms.
- Cursor des anderen sichtbar, farbig abgesetzt.
- Keine Endlosschleife bei gleichzeitiger Bearbeitung.

**Nicht im Scope:** WebRTC, Operational Transform, CRDT.

---

## Phase 7 — `phase7-polish` (langfristig)

**Scope:** Weitere Shapes, Refactoring, Tests, Performance.

**Aufgaben (Teil-Bausteine, einzeln delegierbar):**
- `phase7a-polygon` — `ElementKind::Polygon` (Punkte-Liste, offen/geschlossen).
- `phase7b-arrow` — `ElementKind::Arrow` (Linie + Pfeilspitze).
- `phase7c-callout` — zusammengesetztes Element (Box + Linie + Text).
- `phase7d-refactor-app` — `src/app.rs` (2053 Zeilen) aufteilen in
  `src/app/{state,actions,file_ops,panels}.rs`.
- `phase7e-refactor-canvas` — `src/canvas.rs` (1429 Zeilen) aufteilen in
  `src/canvas/{rendering,interaction,paste,handles}.rs`.
- `phase7f-viewport-culling` — Nur sichtbare Elemente rendern.
- `phase7g-quadtree` — Spatial Index für Hit-Testing.
- `phase7h-ci` — `.github/workflows/`: build + lint für native + WASM.
- `phase7i-tests` — Unit-Tests für `model.rs`, `geometry.rs`, `history.rs`.
- `phase7j-autosave` — Lokale Crash-Recovery ins Temp-Verzeichnis (optional).

**Done-Kriterien je Teil-Baustein:** einzeln in Task-Beschreibung klären.

---

## Agent-Auswahlhilfe

| Wenn… | Agent |
|---|---|
| Sicherheitslücken fixen | `phase0-security` |
| Neue Shape hinzufügen | `phase1-shapes` (Ellipse), `phase7a-c` (Polygon/Arrow/Callout) |
| PDF-Funktionen erweitern | `phase2-pdf` |
| Browser/Touch tauglich machen | `phase3-mobile` |
| Webserver-Variante bauen | `phase4-server` |
| Native Mobile-App bauen | `phase5-native-mobile` |
| Multiuser | `phase6-collab` (nur bei Bedarf) |
| Code aufräumen, CI, Tests | `phase7d-j` |
