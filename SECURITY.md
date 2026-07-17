# Security Policy

> Letztes Update: 18. Juli 2026 · Aktuelle Version: **v0.3.0**

## Supported Versions

| Version | Supported | Notes |
|---------|-----------|-------|
| `main` (dev) | ✅ | Aktiver Entwicklungsstand |
| v0.3.0 | ⚠️ | **3 bekannte Schwachstellen (offen)** — siehe unten |
| < v0.3.0 | ❌ | Nicht unterstützt |

## Reporting a Vulnerability

Bitte **kein öffentliches GitHub Issue** für Sicherheitslücken öffnen.

Stattdessen E-Mail an: **security@boxdoc.at** (wird noch eingerichtet).

In dermeldung bitte:
- Detaillierte Beschreibung
- Reproduktionsschritte
- Impact-Einschätzung
- Optional: Fix-Vorschlag

Antwort innerhalb von **48 Stunden**.

---

## Bekannte Schwachstellen (offen)

Die folgenden drei Punkte sind im Code-Stand v0.3.0 vorhanden und in
Phase 0 der Roadmap zur Behebung vorgesehen (siehe `ROADMAP.md` und
`AGENTS_TASKS.md` → `phase0-security`).

### SW-001: Unsichere Shell-Argument-Übergabe (Windows)
- **Schwere:** Mittel (Pfad aktuell hartcodiert, daher nicht direkt ausnutzbar)
- **Datei:** `src/printing.rs:49-52`
- **Beschreibung:** `Command::new("cmd").args(["/C", "start", "", &path…])`
  verwendet ein Array-Argument statt einzelne `.arg()`-Aufrufe. Defensive
  Härtung.
- **Fix:** `.arg("/C").arg("start").arg("").arg(path…)`.

### SW-002: Fehlende Pfad-Validierung
- **Schwere:** Niedrig (Pfade kommen aus `rfd::FileDialog`, Nutzergesteuert)
- **Datei:** `src/io.rs` (`load_project`, `save_project`, `import_odt_dialog`)
- **Beschreibung:** Keine Prüfung, ob der gewählte Pfad innerhalb eines
  erlaubten Basisverzeichnisses liegt. Theoretisch Path-Traversal möglich,
  praktisch nur über explizite Nutzer-Auswahl.
- **Fix:** `is_safe_path()` mit `canonicalize` + `starts_with(base)`.

### SW-003: Fehlende Größenbegrenzung beim ZIP-Entpacken (ODT)
- **Schwere:** Mittel (DoS durch große ODTs)
- **Datei:** `src/odt.rs:257-265`
- **Beschreibung:** `read_to_end` ohne Limit, `ZipArchive::new` ohne
  Vorab-Prüfung der dekomprimierten Größe.
- **Fix:** `MAX_EXTRACT_SIZE: u64 = 100 * 1024 * 1024` pro Eintrag prüfen.

> **Hinweis:** Eine frühere Version dieses Dokuments behauptete, alle drei
> Punkte seien in einer "v0.3.1" bereits gefixt. Diese Version wurde nie
> veröffentlicht; die Behauptung war falsch und wurde am 18. Juli 2026
> korrigiert.

---

## Best Practices (für Entwickler)

1. **Shell-Aufrufe immer mit separaten `.arg()`** — nie String-Konkatenation.
2. **Dateipfade validieren** — `canonicalize` + `starts_with(base)`.
3. **Ressourcen-Verbrauch begrenzen** — `MAX_FILE_SIZE`, `MAX_EXTRACT_SIZE`.
4. **Sichere Parser verwenden** — `serde_json`, `image`, `zip` (mit Limits).
5. **Kein `unsafe`** außer in gut begründeten, reviewten Fällen.

## Best Practices (für Nutzer)

1. Aktuelle Version verwenden.
2. Keine `.boxdoc`-Dateien aus unvertrauenswürdigen Quellen öffnen.
3. Keine `.odt`-Dateien aus unvertrauenswürdigen Quellen importieren.
4. Auffälliges Verhalten melden.

---

## Audits

| Datum | Auditor | Scope | Befund |
|------|---------|-------|--------|
| 18. Juli 2026 | Manuell (Code-Review) | `printing.rs`, `io.rs`, `odt.rs` | 3 offene Punkte (SW-001/002/003) |

Ein formelles externes Audit hat nicht stattgefunden.
