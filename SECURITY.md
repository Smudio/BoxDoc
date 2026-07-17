# Security Policy

> Letztes Update: 18. Juli 2026 · Aktuelle Version: **v0.4.2-dev**

## Supported Versions

| Version | Supported | Notes |
|---------|-----------|-------|
| `main` (dev, v0.4.2-dev) | ✅ | SW-001/002/003 gefixt (Phase 0 abgeschlossen) |
| >= v0.4.2 | ✅ | SW-001/002/003 gefixt |
| v0.4.1 | ⚠️ | 3 fixed-point issues (SW-001/002/003) — auf v0.4.2 aktualisieren |
| <= v0.4.0 | ⚠️ | Nicht unterstützt |

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

## Bekannte Schwachstellen (Status)

Die folgenden drei Punkte waren im Code-Stand ≤ v0.4.1 offen und sind in
**`main` / kommende v0.4.2** gefixt (Phase 0 der Roadmap, siehe
`AGENTS_TASKS.md` → `phase0-security`).

### SW-001: Unsichere Shell-Argument-Übergabe (Windows) — **Fixed in main / v0.4.2**
- **Schwere:** Mittel (Pfad aktuell hartcodiert, daher nicht direkt ausnutzbar)
- **Datei:** `src/printing.rs:49-56`
- **Beschreibung:** `Command::new("cmd").args(["/C", "start", "", &path…])`
  verwendete ein Array-Argument statt einzelne `.arg()`-Aufrufe.
- **Fix:** `.arg("/C").arg("start").arg("").arg(path…)` — separate Argumente.

### SW-002: Fehlende Pfad-Validierung — **Fixed in main / v0.4.2**
- **Schwere:** Niedrig (Pfade kommen aus `rfd::FileDialog`, nutzergesteuert)
- **Datei:** `src/io.rs` (`load_project`, `save_project`, `import_odt_dialog`)
- **Beschreibung:** Keine Prüfung, ob der gewählte Pfad canonicalisierbar ist.
- **Fix:** `is_safe_path()`-Helper + `ensure_canonicalizable()` als Defensiv-Check
  in `load_project`, `save_project`, `import_odt_dialog`. Eine strikte
  Base-Dir-Prüfung gegen `current_dir` wurde bewusst weggelassen, da BoxDoc
  Dateien über `rfd::FileDialog` aus einem beliebigen Verzeichnis laden darf —
  eine solche Prüfung würde alle legitimen Nutzungs-Wege brechen. Kaputte und
  symlink-basierte Pfade werden abgewiesen.
- **Kompromiss:** Siehe Kommentarblock an `is_safe_path` in `src/io.rs`.

### SW-003: Fehlende Größenbegrenzung beim ZIP-Entpacken (ODT) — **Fixed in main / v0.4.2**
- **Schwere:** Mittel (DoS durch große ODTs)
- **Datei:** `src/odt.rs` (`import`, `read_entry`)
- **Beschreibung:** `read_to_end` ohne Limit, `ZipArchive::new` ohne
  Vorab-Prüfung der dekomprimierten Größe.
- **Fix:** `MAX_EXTRACT_SIZE = 100 * 1024 * 1024` (100 MB) pro Eintrag,
  `MAX_ARCHIVE_TOTAL_SIZE = 400 MB` über alle Einträge summiert. Beide Limits
  werden in `import` bzw. `read_entry` geprüft und schlagen bei Überschreitung
  mit Fehler fehl.

> **Hinweis:** Eine frühere Version dieses Dokuments behauptete, alle drei
> Punkte seien in einer "v0.3.1" bereits gefixt. Diese Version wurde nie
> veröffentlicht; die Behauptung war falsch und wurde am 18. Juli 2026
> korrigiert. Der Fix ist nun in `main` eingecheckt und kommt mit v0.4.2.

---

## Best Practices (für Entwickler)

1. **Shell-Aufrufe immer mit separaten `.arg()`** — nie String-Konkatenation.
2. **Dateipfade validieren** — `canonicalize` + `starts_with(base)` für
   Automatisierung; `ensure_canonicalizable` für File-Dialog-Pfade.
3. **Ressourcen-Verbrauch begrenzen** — `MAX_FILE_SIZE`, `MAX_EXTRACT_SIZE`,
   `MAX_ARCHIVE_TOTAL_SIZE`.
4. **Sichere Parser verwenden** — `serde_json`, `image`, `zip` (mit Limits).
5. **Kein `unsafe`** außer in gut begründeten, reviewten Fällen.

## Best Practices (für Nutzer)

1. Aktuelle Version verwenden (>= v0.4.2).
2. Keine `.boxdoc`-Dateien aus unvertrauenswürdigen Quellen öffnen.
3. Keine `.odt`-Dateien aus unvertrauenswürdigen Quellen importieren.
4. Auffälliges Verhalten melden.

---

## Audits

| Datum | Auditor | Scope | Befund |
|------|---------|-------|--------|
| 18. Juli 2026 | Manuell (Code-Review) | `printing.rs`, `io.rs`, `odt.rs` | 3 offene Punkte (SW-001/002/003) |
| 18. Juli 2026 | Manuell (Code-Review) | Phase-0-Fixes | SW-001/002/003 als in `main` gefixt bestätigt |

Ein formelles externes Audit hat nicht stattgefunden.
