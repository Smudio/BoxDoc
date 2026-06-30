# Roadmap

BoxDoc wird schrittweise weiterentwickelt. Diese Roadmap beschreibt die geplanten Features und die technische Ausrichtung.

---

## Status

**Aktuell: v0.2.0** — Lokaler Dokumenten-Editor mit Objekt-Canvas, Multi-Selection, Copy/Paste, ODT/PDF-Export.

---

## Phase 1 — Fundament erweitern

### Undo / Redo System

Ein vollständiges Versionsprotokoll, das jede Aktion erfasst und zeitlich zurückverfolgt werden kann.

- **Unbegrenzte History** — der Nutzer kann beliebig weit in die Vergangenheit zurückgehen und jede gemachte Änderung schrittweise rückgängig machen
- **Granular auf Aktionsebene** — jede Einzelaktion ist ein History-Eintrag: Objekt verschieben, Text ändern, Bild drehen, Objekt löschen, Crop anpassen, Eigenschaften bearbeiten, Seite hinzufügen
- **Redo** — verworfene Schritte können wiederhergestellt werden, bevor eine neue Aktion den Redo-Stack überschreibt
- **Snapshots** — der komplette Dokumentzustand wird bei jeder Aktion als Snapshot gespeichert (einfach, zuverlässig, deterministisch)
- **Bedienung**: Strg+Z (Rückgängig), Strg+Y / Strg+Shift+Z (Wiederherstellen)

### Auto-Save

- Automatisches Speichern im Hintergrund in einem temporären Verzeichnis
- Wiederherstellung nach Absturz oder unerwartetem Schließen

---

## Phase 2 — Web & Browser

### PHP-Dokumenten-Backend

BoxDoc soll direkt im Browser laufen — ohne Installation, ohne Plugin.

- **WASM-Kompilierung** — Rust + egui kompilieren nativ nach WebAssembly
- **Statischer Webserver** — Frontend (index.html, .wasm, .js) liegt als statische Datei auf jedem Webserver (nginx, Apache, `python -m http.server`, GitHub Pages)
- **PHP-Dokumenten-Backend** — kleines, framework-freies `api.php` (~120 Zeilen) speichert Dokumente serverseitig in `docs/<slug>.boxdoc`. URL-basierter Zugriff via `boxdoc.at/d/<slug>?t=<token>`.
- **Server-Side Rendering für KI** — `index.php` bettet den Dokument-Inhalt + vollständige AI-Anleitung direkt in den HTML-Quellcode ein. opencode (oder jeder andere Agent) sieht beim `curl` alles Nötige: JSON-Inhalt + Anleitung wie man per PUT ändert.
- **Kein Framework, kein Build** — das Backend ist eine einzige `api.php` + optionale `.htaccess`. Läuft auf jedem Shared-Hosting mit PHP 7.4+.
- **Token-Schutz** — jedes Dokument hat ein 32-Zeichen-Token (Capability URL, wie Google Docs). Schreibzugriff nur mit Token.

### Responsives Layout

- Anpassung an verschiedene Bildschirmgrößen (Desktop, Tablet)
- Touch-Bedienung für mobile Geräte (grundlegend)

---

## Phase 3 — Echtzeit-Kollaboration

### Hybrid: Server-Sent Events (Daten) + WebRTC (Cursor)

Mehrere Nutzer arbeiten gleichzeitig am selben Dokument — in Echtzeit.

- **SSE für Dokument-Sync** — Browser öffnen eine `EventSource`-Verbindung zu `stream.php`. Jede Änderung (von anderem Tab oder von opencode per PUT) wird an alle verbundene Clients gepusht (<200 ms). Datei-basierte Event-Queue, kein Redis nötig.
- **WebRTC für Präsenz** — für low-latency Cursor und Auswahl-Markierungen verbinden sich Browser direkt (Peer-to-Peer via DataChannel). STUN-Server kostenlos; nur für die Cursor, nicht für die Dokumentdaten.
- **Last-Write-Wins pro Element** — jedes Objekt hat eindeutige `id`. Bei Konflikten gewinnt die zuletzt geschriebene Version pro Element (einfach, deterministisch).
- **Präsenz** — farbige Cursor, Namen, Auswahl-Markierungen zeigen, wo andere Nutzer arbeiten.
- **opencode-freundlich** — KI-Agenten brauchen nur HTTP (PUT/GET), kein WebSocket. Browser nutzen SSE für Live-Updates.

### Session-Verwaltung

- **Room-System** — ein Dokument entspricht einem Raum; Nutzer treten über die geteilte URL bei
- **Skalierbarkeit** — für kleine Teams (2–10 Peers) optimiert; SSE für Daten, WebRTC-Mesh für Cursor

---

## Phase 4 — Mobile & Plattform

### Mobile Apps

- **iOS und Android** — egui/eframe unterstützt Touch-Input und mobile Rendering-Backends
- **Native Datei-Dialoge** — Integration in die mobile Dateiauswahl
- **Geteilte Codebase** — gleiche Rust-Logik, nur das Rendering-Backend unterscheidet sich

### Druckverbesserungen

- System-Druckdialog direkt (ohne PDF-Umweg)
- Druckvorschau

---

## Technische Leitsätze

1. **Simple first** — jedes Feature wird so einfach wie möglich implementiert, ohne die Architektur zu verkomplizieren
2. **Zuverlässig** — Datenverlust ist inakzeptabel; Auto-Save und Undo sind Fundament, nicht Afterthought
3. **Elegant** — die UI bleibt ruhig, aufgeräumt und schnell
4. **Portabel** — eine Codebase, drei Zielplattformen (Desktop, Web, Mobil)
5. **Ohne Server** — BoxDoc funktioniert ohne zentrale Infrastruktur; Kollaboration ist Peer-to-Peer

---

## Versionsübersicht

| Version | Fokus |
|---|---|
| v0.3 (aktuell) | Lokaler Editor, Objekte, ODT/PDF, AI-Datei-Schnittstelle |
| v0.4 | WASM-Portierung, PHP-Dokumenten-Backend, SSR für KI |
| v0.5 | Echtzeit-Kollaboration (SSE + WebRTC für Cursor) |
| v0.6 | Mobile (iOS/Android) |
| v1.0 | Stabilisiert, poliert, bereit für breiten Einsatz |
