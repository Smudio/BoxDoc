# BoxDoc Web-Backend

PHP-Dateien für den Server. Der Build-Prozess ist **unverändert**:
`trunk build --release` erzeugt `dist/` (wie bisher). Diese PHP-Dateien
werden beim Deploy einfach mitkopiert.

## Build (unverändert)

```bash
trunk build --release
# → dist/index.html, dist/boxdoc-*.js, dist/boxdoc-*.wasm
```

## Deploy (ein Ordner auf dem Server)

```
server/
├── index.html          ← aus dist/
├── boxdoc-*.js         ← aus dist/
├── boxdoc-*.wasm       ← aus dist/
├── api.php             ← aus web/
├── stream.php          ← aus web/ (für Phase 3, harmlos wenn ungenutzt)
├── .htaccess           ← aus web/ (optional, Apache)
└── docs/               ← wird von PHP angelegt (chmod 0700)
```

Also: Inhalt von `dist/` + Inhalt von `web/` in denselben Ordner auf dem Server.

## Für KI-Agenten (opencode)

Zwei einfache HTTP-Requests. Das wars.

```bash
# Lesen (JSON enthält automatisch _ai_hint mit Anleitung)
curl "https://boxdoc.at/api.php?get=<slug>&t=<token>"

# Ändern (komplettes JSON zurücksenden)
curl -X PUT --data-binary @doc.json \
     "https://boxdoc.at/api.php?put=<slug>&t=<token>"
```

Keine Authentifizierung, keine Headers, kein komplexes Protokoll.
Der Token (`?t=...`) ist das einzige, was nötig ist.

## Endpunkte

| URL | Methode | Zweck |
|-----|---------|-------|
| `api.php?new=1` | POST | Neues Doc → `{slug, token, url}` |
| `api.php?get=<slug>` | GET | Doc-Inhalt als JSON |
| `api.php?put=<slug>&t=<token>` | PUT | Doc überschreiben |
| `api.php?list=1` | GET | Alle Docs auflisten |
| `stream.php?slug=<slug>&t=<token>` | GET | SSE-Live-Stream (Phase 3) |
