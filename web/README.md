# BoxDoc Web-Backend

Dieses Verzeichnis enthält das PHP-Backend für die BoxDoc-Web-Version.

## Deployment

1. **WASM-Build erstellen** (im Repo-Root):
   ```bash
   trunk build --release
   # Erzeugt dist/index.html, dist/boxdoc-*.js, dist/boxdoc-*.wasm
   ```

2. **Dateien auf den Server kopieren:**
   ```
   web/api.php            → server/index.php nicht überschreiben!
   web/stream.php         → server/
   web/index.php          → server/
   web/.htaccess          → server/
   dist/index.html        → server/
   dist/boxdoc-*.js       → server/
   dist/boxdoc-*.wasm     → server/
   ```

3. **`docs/` beschreibbar machen:**
   ```bash
   chmod 0700 docs/
   chown www-data:www-data docs/   # oder apache:apache
   ```

## Funktionsweise

| URL | Liefert |
|-----|---------|
| `boxdoc.at/` | SPA-Startseite (neues Doc erstellen / öffnen) |
| `boxdoc.at/d/<slug>?t=<token>` | SSR-Seite mit eingebettetem Doc-Inhalt + AI-Anleitung |
| `boxdoc.at/api.php?new=1` | Neues Doc erstellen → JSON mit slug, token, url |
| `boxdoc.at/api.php?get=<slug>` | Reines JSON des Docs |
| `boxdoc.at/api.php?put=<slug>&t=<token>` | Doc überschreiben (PUT) |
| `boxdoc.at/api.php?list=1` | Liste aller Docs (slug, modified, size) |
| `boxdoc.at/stream.php?slug=<slug>&t=<token>` | SSE-Live-Stream (für später) |

## Für opencode / KI-Agenten

Wenn opencode `boxdoc.at/d/<slug>?t=<token>` per `curl` aufruft, sieht es im
HTML-Quellcode:

1. **Den vollständigen Doc-Inhalt** als `<script type="application/json">`
2. **Eine komplette Anleitung** als HTML-Kommentar, die erklärt wie man per
   `curl -X PUT ...` Änderungen vornimmt.

Keine extra API-Dokumentation nötig — die Website ist selbst-dokumentierend.

## Sicherheit

- **Slug-Format:** `^[a-z0-9]{8,32}$` — kein Path-Traversal möglich
- **Token:** 32 Zeichen Zufall (Capability-URL-Prinzip)
- **`hash_equals`:** timing-sicherer Token-Vergleich
- **Atomic Writes:** tmp-Datei + rename (keine Korruption bei Absturz)
- **Größenlimit:** 10 MB pro Dokument
- **docs/ geschützt:** `.htaccess` sperrt direkten Zugriff (nur via api.php)

## Anforderungen

- PHP 7.4 oder neuer
- Schreibrechte für `docs/`
- Optional: Apache mit `mod_rewrite` für hübsche URLs (`/d/<slug>`)
- Alternativ: nginx, lighttpd — BoxDoc funktioniert auch ohne URL-Rewriting,
  dann via `?doc=<slug>` URLs direkt.
