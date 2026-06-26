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

### WASM-Portierung

BoxDoc soll direkt im Browser laufen — ohne Installation, ohne Plugin.

- **Kompilierung nach WebAssembly** — Rust + egui kompilieren nativ nach WASM
- **Statischer Webserver** — die kompilierte App besteht aus statischen Dateien (`index.html`, `.wasm`, `.js`, Assets), die auf jedem einfachen Webserver liegen können (nginx, Apache, `python -m http.server`, GitHub Pages)
- **Kein Backend erforderlich** — die gesamte Logik läuft clientseitig im Browser
- **Dateizugriff im Browser** — Save/Open über die File System Access API oder Download/Upload

### Responsive Layout

- Anpassung an verschiedene Bildschirmgrößen (Desktop, Tablet)
- Touch-Bedienung für mobile Geräte (grundlegend)

---

## Phase 3 — Echtzeit-Kollaboration

### Multi-User Zusammenarbeit über WebRTC

Mehrere Nutzer arbeiten gleichzeitig am selben Dokument — in Echtzeit, ohne zentralen Server.

- **WebRTC Direct Connections** — Browser verbinden sich direkt miteinander (Peer-to-Peer). Kein zusätzlicher Application-Server nötig; lediglich ein STUN/TURN-Server für die Verbindungsherstellung (kostenlose öffentliche STUN-Server verfügbar)
- **Dezentraler Ansatz** — jeder Peer hält eine lokale Kopie des Dokuments. Änderungen werden über den Datenkanal (DataChannel) an alle verbundenen Peers gesendet
- **Echtzeit-Synchronisation** — wenn ein Nutzer ein Objekt verschiebt, Text bearbeitet oder ein Bild einfügt, sehen alle anderen Peers die Änderung sofort
- **Konfliktlösung** — Object-basierte CRDT-ähnliche Synchronisation: jedes Objekt hat eine eindeutige ID, Änderungen werden pro Objekt angewendet (Last-Write-Wins pro Element)
- **Präsenz** — farbige Cursor oder Markierungen zeigen, wo andere Nutzer arbeiten

### Session-Verwaltung

- **Room-System** — ein Dokument entspricht einem Raum; Nutzer treten über einen Link bei
- **WebSocket-Signalisierung** — für die initiale Verbindungsherstellung (Wer ist im Raum?) reicht ein minimaler WebSocket-Server oder eine signalisierungsfreie Verbindung über geteilte Links
- **Skalierbarkeit** — für kleine Teams (2–10 Peers) optimiert; Broadcasting über Mesh-Topologie

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
| v0.2 (aktuell) | Lokaler Editor, Objekte, ODT/PDF |
| v0.3 | Undo/Redo, Auto-Save |
| v0.4 | WASM-Portierung, Browser-Version |
| v0.5 | Echtzeit-Kollaboration (WebRTC) |
| v0.6 | Mobile (iOS/Android) |
| v1.0 | Stabilisiert, poliert, bereit für breiten Einsatz |
