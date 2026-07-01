# BoxDoc Web

```
src/      → Rust-Sourcecode (App)
web/      → PHP-Dateien für den Server (Backend)
dist/     → Web-Build-Output (wird von `trunk build` erzeugt)
```

## Bauen (unverändert)

```bash
trunk build --release
# erzeugt dist/ mit: index.html, boxdoc-*.js, boxdoc-*.wasm
```

## Deployen (ein Ordner auf dem Server)

Inhalte von `dist/` UND `web/` in denselben Ordner auf dem Server kopieren:

```
server/
├── index.html          ← aus dist/
├── boxdoc-*.js         ← aus dist/
├── boxdoc-*.wasm       ← aus dist/
├── index.php           ← aus web/  (Front-Controller: saubere URLs)
├── api.php             ← aus web/  (Mutationen: new/put/list)
├── stream.php          ← aus web/  (SSE für Phase 3, harmlos wenn ungenutzt)
├── .htaccess           ← aus web/  (optional, Apache)
└── docs/               ← wird von PHP automatisch angelegt (chmod 0700)
```

## URLs

| URL | Methode | Wer | Liefert |
|-----|---------|-----|---------|
| `boxdoc.at/` | GET | Browser | SPA (Startseite) |
| `boxdoc.at/lebenslauf` | GET | Browser | SPA (öffnet das Doc) |
| `boxdoc.at/lebenslauf` | GET | KI (curl) | Doc als JSON (+ `_ai_hint` Anleitung) |
| `boxdoc.at/api.php?put=lebenslauf` | PUT | KI | Doc überschreiben |
| `boxdoc.at/api.php?new=1&name=lebenslauf` | POST | KI | Neues Doc erstellen |

### Wie die KI zwischen Browser und JSON unterscheidet
Content-Negotiation via `Accept`-Header:
- Browser sendet `Accept: text/html,...` → bekommt die SPA
- curl/opencode senden `Accept: */*` → bekommen das rohe JSON

## Für KI-Agenten (opencode) — maximal einfach

```bash
# Lesen (saubere URL, JSON + Anleitung)
curl https://boxdoc.at/lebenslauf

# Ändern (PUT an api.php)
curl -X PUT --data-binary @doc.json https://boxdoc.at/api.php?put=lebenslauf

# Neues Doc anlegen
curl -X POST https://boxdoc.at/api.php?new=1&name=lebenslauf
```

Kein Token nötig (Default = öffentlich). Nur wenn der Nutzer Schutz gesetzt
hat, `?t=<token>` anhängen.

## Token (optional)

Default: öffentlich. Schutz nur, wenn beim Erstellen `?token=<wert>` gesetzt:
```bash
curl -X POST "https://boxdoc.at/api.php?new=1&name=lebenslauf&token=geheim"
# Ab dann ist ?t=geheim für Lesen/Schreiben nötig.
```

## Sicherheit

- Slug-Format `^[a-z0-9]{4,32}$`, reservierte Namen blockiert (`docs`, `api`, …)
- Token-Vergleich mit `hash_equals` (timing-safe)
- Atomic Writes (tmp + rename)
- 10 MB Limit pro Request
- `docs/` per `.htaccess` gesperrt

## Anforderungen

PHP 7.4+. Optional: Apache mit `mod_rewrite` für saubere URLs.
Ohne `.htaccess` funktioniert es auch (via `?doc=<slug>` URLs).
