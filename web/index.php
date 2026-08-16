<?php
// =============================================================================
// BoxDoc — Single-File Backend (Front-Controller + API)
// =============================================================================
//   boxdoc.at/              → index.html (SPA + KI-Anleitung im <head>/<noscript>)
//   boxdoc.at/<slug>        → index.html (SPA, mit Doc-Inhalt injiziert)
//   boxdoc.at/index.php?new=1&name=X   → API: neues Doc
//   boxdoc.at/index.php?get=X          → API: Doc lesen
//   boxdoc.at/index.php?meta=X         → API: nur Versionsnummer (billig)
//   boxdoc.at/index.php?put=X&version=N→ API: Doc überschreiben (optimistisch)
//   boxdoc.at/index.php?list=1         → API: eigene Docs auflisten
//
// -----------------------------------------------------------------------------
// SICHERHEITSMODELL
// -----------------------------------------------------------------------------
// * Ein Dokument ist entweder oeffentlich (kein Token) oder geschuetzt (Token).
// * Der Token wird auf JEDEM Lesepfad geprueft — auch auf dem huebschen
//   /<slug>-Pfad und bei der HTML-Auslieferung. Frueher pruefte nur der
//   ?get=-Endpunkt, wodurch `curl https://host/geheimdoc` den Schutz komplett
//   umging.
// * ?list=1 listet keine fremden Dokumente mehr auf. Ohne gueltigen Token
//   werden nur oeffentliche Dokumente gezeigt.
// * Dokumentinhalt wird beim Einbetten ins HTML escaped (`</` → `<\/`),
//   sonst kann ein Textelement mit `</script>` aus dem Tag ausbrechen.
// * Token gehoeren in den Header (X-BoxDoc-Token). Der ?t=-Parameter bleibt
//   aus Kompatibilitaetsgruenden erlaubt, ist aber der schlechtere Weg
//   (landet in Logs, Referrern und der Browser-History).
//
// -----------------------------------------------------------------------------
// NEBENLAEUFIGKEIT
// -----------------------------------------------------------------------------
// Jedes Dokument hat eine monoton steigende `version` in der Meta-Datei.
// Ein PUT muss die Version mitschicken, auf der er aufsetzt. Stimmt sie nicht
// mehr, antwortet der Server mit 409 und liefert den aktuellen Stand mit —
// der Client merged ihn (siehe src/merge.rs) und speichert erneut.
// Ohne das war die Semantik "Last-Write-Wins": Wer zuletzt speichert, loescht
// die Arbeit aller anderen.
// =============================================================================

declare(strict_types=1);

const DOCS_DIR = __DIR__ . '/docs/';
const MAX_SIZE = 10 * 1024 * 1024;
/// Maximale Anzahl neuer Dokumente pro IP und Stunde.
const RATE_NEW_PER_HOUR = 20;

// -----------------------------------------------------------------------------
// Hilfsfunktionen
// -----------------------------------------------------------------------------

function send_json(array $data, int $code = 200): void {
    http_response_code($code);
    header('Content-Type: application/json; charset=utf-8');
    echo json_encode($data, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    exit;
}

function valid_slug(string $slug): bool {
    if (!preg_match('/^[a-z0-9]{4,32}$/', $slug)) return false;
    $reserved = ['docs', 'api', 'index', 'stream', 'assets', 'static', 'dist', 'web'];
    return !in_array($slug, $reserved, true);
}

function slug_path(string $slug): string {
    return DOCS_DIR . $slug . '.boxdoc';
}

function meta_path(string $slug): string {
    return DOCS_DIR . $slug . '.meta.json';
}

function gen_slug(): string {
    return bin2hex(random_bytes(5));
}

function load_meta(string $slug): ?array {
    $path = meta_path($slug);
    if (!is_file($path)) return null;
    $m = json_decode((string) file_get_contents($path), true);
    return is_array($m) ? $m : null;
}

function save_meta(string $slug, array $meta): void {
    file_put_contents(meta_path($slug), json_encode($meta,
        JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES));
}

/**
 * Liest den Token aus Header oder Query.
 * Header ist der bevorzugte Weg; der Query-Parameter bleibt fuer bestehende
 * Links und einfache curl-Aufrufe erhalten.
 */
function request_token(): ?string {
    $header = $_SERVER['HTTP_X_BOXDOC_TOKEN'] ?? null;
    if ($header !== null && $header !== '') return (string) $header;
    $query = $_GET['t'] ?? null;
    if ($query !== null && $query !== '') return (string) $query;
    return null;
}

/**
 * Darf der Aufrufer auf dieses Dokument zugreifen?
 * Oeffentliche Dokumente (ohne Token in der Meta) sind fuer alle offen.
 */
function has_access(string $slug, ?string $token): bool {
    $meta = load_meta($slug);
    if ($meta === null) return false;
    if (empty($meta['token'])) return true;         // oeffentlich
    if ($token === null || $token === '') return false;
    return hash_equals((string) $meta['token'], (string) $token);
}

/** Wie has_access(), bricht aber mit passendem Statuscode ab. */
function require_access(string $slug, ?string $token): void {
    $meta = load_meta($slug);
    if ($meta === null) send_json(['error' => 'Document not found'], 404);
    if (empty($meta['token'])) return;
    if ($token === null || $token === '') send_json(['error' => 'Token required'], 401);
    if (!hash_equals((string) $meta['token'], (string) $token))
        send_json(['error' => 'Invalid token'], 403);
}

/** Aktuelle Versionsnummer eines Dokuments (0, wenn unbekannt). */
function doc_version(string $slug): int {
    $meta = load_meta($slug);
    return (int) ($meta['version'] ?? 0);
}

function base_url(): string {
    $scheme = ($_SERVER['HTTPS'] ?? 'off') !== 'off' ? 'https' : 'http';
    $host = $_SERVER['HTTP_HOST'] ?? 'localhost';
    return $scheme . '://' . $host;
}

function ensure_docs_dir(): void {
    if (!is_dir(DOCS_DIR)) @mkdir(DOCS_DIR, 0700, true);
}

/**
 * Einfaches Rate-Limit pro IP, damit niemand den Server mit leeren Dokumenten
 * volllaufen lassen kann (10 MB pro Doc, vorher unbegrenzt viele).
 */
function rate_limit_ok(string $bucket, int $max_per_hour): bool {
    $ip = $_SERVER['REMOTE_ADDR'] ?? 'unknown';
    $file = DOCS_DIR . '.rate_' . $bucket . '_' . hash('sha256', $ip) . '.json';
    $now = time();
    $hits = [];
    if (is_file($file)) {
        $decoded = json_decode((string) file_get_contents($file), true);
        if (is_array($decoded)) $hits = $decoded;
    }
    // Nur Treffer der letzten Stunde behalten.
    $hits = array_values(array_filter($hits, fn($t) => is_int($t) && $now - $t < 3600));
    if (count($hits) >= $max_per_hour) return false;
    $hits[] = $now;
    @file_put_contents($file, json_encode($hits));
    return true;
}

/**
 * Schreibt eine Datei atomar. Der temporaere Name enthaelt einen Zufallsanteil,
 * damit zwei gleichzeitige Schreibvorgaenge sich nicht dieselbe Datei teilen.
 */
function atomic_write(string $path, string $content): bool {
    $tmp = $path . '.' . bin2hex(random_bytes(6)) . '.tmp';
    if (file_put_contents($tmp, $content) === false) return false;
    if (!rename($tmp, $path)) {
        @unlink($tmp);
        return false;
    }
    return true;
}

// =============================================================================
// API-ROUTING
// =============================================================================

$method  = $_SERVER['REQUEST_METHOD'] ?? 'GET';
$has_api = isset($_GET['new']) || isset($_GET['get']) || isset($_GET['put'])
        || isset($_GET['list']) || isset($_GET['meta']);

if ($has_api) {

    // CORS: Antworten sind fuer jeden Origin lesbar, aber der Token wandert
    // nicht automatisch mit — Credentials bleiben bewusst aus.
    header('Access-Control-Allow-Origin: *');
    header('Access-Control-Allow-Methods: GET, POST, PUT, OPTIONS');
    header('Access-Control-Allow-Headers: Content-Type, X-BoxDoc-Token');
    header('Cache-Control: no-store, no-cache, must-revalidate');

    if ($method === 'OPTIONS') { http_response_code(204); exit; }

    ensure_docs_dir();
    $token = request_token();

    // -------------------------------------------------------------------------
    // POST ?new=1 — Neues Dokument anlegen
    // -------------------------------------------------------------------------
    if ($method === 'POST' && isset($_GET['new'])) {
        if (!rate_limit_ok('new', RATE_NEW_PER_HOUR)) {
            send_json(['error' => 'Rate limit exceeded, try again later'], 429);
        }
        $slug = $_GET['name'] ?? '';
        if ($slug === '') {
            $slug = gen_slug();
        } elseif (!valid_slug($slug)) {
            send_json(['error' => 'Invalid name (4-32 chars a-z0-9)'], 400);
        } elseif (is_file(slug_path($slug))) {
            send_json(['error' => 'Name already exists'], 409);
        }
        // Token: Wenn keiner uebergeben wird, erzeugen wir einen. Das Dokument
        // ist damit standardmaessig NICHT oeffentlich beschreibbar. Wer bewusst
        // ein oeffentliches Dokument will, uebergibt public=1.
        $public = isset($_GET['public']) && $_GET['public'] === '1';
        $new_token = $_GET['token'] ?? null;
        if (!$public && ($new_token === null || $new_token === '')) {
            $new_token = bin2hex(random_bytes(16));
        }

        $content = '{"doc":{"format":"A4","orientation":"Portrait","pages":[{"elements":[]}]},"images":[]}';
        if (!atomic_write(slug_path($slug), $content)) {
            send_json(['error' => 'Could not write document'], 500);
        }
        $meta = ['created' => time(), 'modified' => time(), 'version' => 1];
        if ($new_token !== null && $new_token !== '') $meta['token'] = $new_token;
        save_meta($slug, $meta);

        $b = base_url();
        $tq = $new_token ? '?t=' . rawurlencode($new_token) : '';
        send_json([
            'slug'      => $slug,
            'token'     => $new_token,
            'protected' => $new_token !== null && $new_token !== '',
            'version'   => 1,
            'url'       => $b . '/' . $slug . $tq,
            'api_get'   => $b . '/index.php?get=' . $slug . $tq,
            'api_put'   => $b . '/index.php?put=' . $slug . $tq,
        ], 201);
    }

    // -------------------------------------------------------------------------
    // GET ?meta=<slug> — nur die Versionsnummer
    // -------------------------------------------------------------------------
    // Der Client pollt hierauf. Frueher lud er dafuer alle 2 Sekunden das
    // komplette Dokument inklusive aller base64-Bilder.
    if ($method === 'GET' && isset($_GET['meta'])) {
        $slug = (string) $_GET['meta'];
        if (!valid_slug($slug)) send_json(['error' => 'Invalid slug'], 400);
        require_access($slug, $token);
        $meta = load_meta($slug) ?? [];
        send_json([
            'slug'     => $slug,
            'version'  => (int) ($meta['version'] ?? 0),
            'modified' => (int) ($meta['modified'] ?? 0),
        ]);
    }

    // -------------------------------------------------------------------------
    // GET ?get=<slug> — Dokument lesen
    // -------------------------------------------------------------------------
    if ($method === 'GET' && isset($_GET['get'])) {
        $slug = (string) $_GET['get'];
        if (!valid_slug($slug)) send_json(['error' => 'Invalid slug'], 400);
        require_access($slug, $token);
        $path = slug_path($slug);
        if (!is_file($path)) send_json(['error' => 'Document not found'], 404);
        header('Content-Type: application/json; charset=utf-8');
        header('X-BoxDoc-Version: ' . doc_version($slug));
        readfile($path);
        exit;
    }

    // -------------------------------------------------------------------------
    // PUT ?put=<slug>&version=<n> — Dokument überschreiben
    // -------------------------------------------------------------------------
    if ($method === 'PUT' && isset($_GET['put'])) {
        $slug = (string) $_GET['put'];
        if (!valid_slug($slug)) send_json(['error' => 'Invalid slug'], 400);
        require_access($slug, $token);

        $path = slug_path($slug);
        if (!is_file($path)) send_json(['error' => 'Document not found'], 404);

        $body = file_get_contents('php://input');
        if ($body === false) send_json(['error' => 'Could not read body'], 400);
        if (strlen($body) > MAX_SIZE)
            send_json(['error' => 'Document too large (max ' . MAX_SIZE . ' bytes)'], 413);

        json_decode($body);
        if (json_last_error() !== JSON_ERROR_NONE)
            send_json(['error' => 'Invalid JSON: ' . json_last_error_msg()], 400);

        $current = doc_version($slug);

        // Optimistische Nebenlaeufigkeit. Wer keine Version mitschickt, wird
        // aus Kompatibilitaetsgruenden durchgelassen — Clients ab v0.6 tun es.
        if (isset($_GET['version'])) {
            $claimed = (int) $_GET['version'];
            if ($claimed !== $current) {
                // Nicht ablehnen und den Client raten lassen: den aktuellen
                // Stand gleich mitliefern, damit er sofort mergen kann.
                send_json([
                    'error'    => 'Version conflict',
                    'version'  => $current,
                    'document' => (string) file_get_contents($path),
                ], 409);
            }
        }

        if (!atomic_write($path, $body)) {
            send_json(['error' => 'Could not write document'], 500);
        }

        $meta = load_meta($slug) ?? [];
        $meta['modified'] = time();
        $meta['version']  = $current + 1;
        save_meta($slug, $meta);

        @file_put_contents(DOCS_DIR . $slug . '.events.log',
            json_encode(['ts' => time(), 'type' => 'update', 'version' => $meta['version']]) . "\n",
            FILE_APPEND);

        send_json([
            'ok'       => true,
            'slug'     => $slug,
            'version'  => $meta['version'],
            'modified' => $meta['modified'],
        ]);
    }

    // -------------------------------------------------------------------------
    // GET ?list=1 — Dokumente auflisten
    // -------------------------------------------------------------------------
    // Zeigt nur, worauf der Aufrufer auch zugreifen darf: oeffentliche
    // Dokumente und solche, deren Token er kennt. Frueher gab dieser Endpunkt
    // die Slugs ALLER Dokumente preis — zusammen mit dem ungeschuetzten
    // Lesepfad war damit jedes Dokument auf dem Server abrufbar.
    if ($method === 'GET' && isset($_GET['list'])) {
        $docs = [];
        foreach (glob(DOCS_DIR . '*.boxdoc') as $f) {
            $s = basename($f, '.boxdoc');
            if (!has_access($s, $token)) continue;
            $m = load_meta($s);
            $docs[] = [
                'slug'      => $s,
                'modified'  => $m['modified'] ?? filemtime($f),
                'size'      => filesize($f),
                'version'   => (int) ($m['version'] ?? 0),
                'protected' => !empty($m['token']),
            ];
        }
        send_json(['documents' => $docs]);
    }

    send_json(['error' => 'Unknown API endpoint'], 404);
}

// =============================================================================
// Kein API-Parameter → index.html ausliefern (SPA + KI-Anleitung)
// =============================================================================

$request = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?? '/';
$path = trim($request, '/');

$slug = '';
if (preg_match('/^([a-z0-9]{4,32})$/', $path, $m)) {
    $slug = $m[1];
}

$token = request_token();

// Slug + kein Accept: text/html → rohes JSON (für curl/KI)
if ($slug !== '' && !str_contains($_SERVER['HTTP_ACCEPT'] ?? '*/*', 'text/html')) {
    $doc_path = slug_path($slug);
    if (!is_file($doc_path)) {
        http_response_code(404);
        header('Content-Type: application/json; charset=utf-8');
        echo json_encode(['error' => 'Document not found', 'slug' => $slug]);
        exit;
    }
    // Der Token-Check fehlte hier frueher komplett.
    if (!has_access($slug, $token)) {
        http_response_code(load_meta($slug) === null ? 404 : 401);
        header('Content-Type: application/json; charset=utf-8');
        echo json_encode(['error' => 'Token required', 'slug' => $slug]);
        exit;
    }
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: no-store');
    header('X-BoxDoc-Version: ' . doc_version($slug));
    readfile($doc_path);
    exit;
}

// index.html ausliefern. Bei Slug: Doc-Inhalt injizieren.
$spa_path = __DIR__ . '/index.html';
if (!is_file($spa_path)) {
    http_response_code(503);
    echo 'index.html nicht gefunden.';
    exit;
}

$spa = file_get_contents($spa_path);

if ($slug !== '') {
    $doc_path = slug_path($slug);
    // Auch hier gilt der Token-Check: sonst kaeme der Inhalt eines
    // geschuetzten Dokuments einfach ueber die HTML-Seite heraus.
    if (is_file($doc_path) && has_access($slug, $token)) {
        $doc_json = (string) file_get_contents($doc_path);
        // XSS-Schutz: Ein Textelement mit `</script>` wuerde sonst aus dem
        // Tag ausbrechen. Innerhalb von JSON ist `<\/` aequivalent zu `</`,
        // der Inhalt bleibt also unveraendert.
        $doc_json = str_replace('</', '<\\/', $doc_json);
        $inject = '<script type="application/json" id="boxdoc-content">' . "\n"
                . $doc_json . "\n</script>\n"
                . '<meta name="boxdoc-version" content="' . doc_version($slug) . '">' . "\n";
        $spa = str_replace('</body>', $inject . '</body>', $spa);
    }
}

echo $spa;
