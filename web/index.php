<?php
// =============================================================================
// BoxDoc — Single-File Backend (Front-Controller + API)
// =============================================================================
//   boxdoc.at/              → index.html (SPA + KI-Anleitung im <head>/<noscript>)
//   boxdoc.at/<slug>        → index.html (SPA, mit Doc-Inhalt injiziert)
//   boxdoc.at/index.php?new=1&name=X   → API: neues Doc
//   boxdoc.at/index.php?get=X          → API: Doc lesen
//   boxdoc.at/index.php?put=X          → API: Doc überschreiben
//   boxdoc.at/index.php?list=1         → API: alle Docs auflisten
//
// Die komplette KI-Anleitung (Format-Spec, Element-Typen, Beispiele) steckt
// in index.html: als <meta>-Tags im <head>, als JSON-LD, und als <noscript>-Block.
// Sie ist IMMER im HTML-Quelltext — egal wer fragt.
// =============================================================================

declare(strict_types=1);

const DOCS_DIR = __DIR__ . '/docs/';
const MAX_SIZE = 10 * 1024 * 1024;

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

function check_access(string $slug, ?string $token): void {
    $meta = load_meta($slug);
    if ($meta === null) send_json(['error' => 'Document not found'], 404);
    if (empty($meta['token'])) return;
    if ($token === null || $token === '') send_json(['error' => 'Token required'], 401);
    if (!hash_equals((string) $meta['token'], (string) $token))
        send_json(['error' => 'Invalid token'], 403);
}

function base_url(): string {
    $scheme = ($_SERVER['HTTPS'] ?? 'off') !== 'off' ? 'https' : 'http';
    $host = $_SERVER['HTTP_HOST'] ?? 'localhost';
    return $scheme . '://' . $host;
}

function ensure_docs_dir(): void {
    if (!is_dir(DOCS_DIR)) @mkdir(DOCS_DIR, 0700, true);
}

// =============================================================================
// API-ROUTING
// =============================================================================

$method  = $_SERVER['REQUEST_METHOD'] ?? 'GET';
$has_api = isset($_GET['new']) || isset($_GET['get']) || isset($_GET['put']) || isset($_GET['list']);

if ($has_api) {

    header('Access-Control-Allow-Origin: *');
    header('Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS');
    header('Access-Control-Allow-Headers: Content-Type, X-BoxDoc-Token');
    header('Cache-Control: no-store, no-cache, must-revalidate');

    if ($method === 'OPTIONS') { http_response_code(204); exit; }

    ensure_docs_dir();

    // POST ?new=1 — Neues Dokument anlegen
    if ($method === 'POST' && isset($_GET['new'])) {
        $slug = $_GET['name'] ?? '';
        if ($slug === '') {
            $slug = gen_slug();
        } elseif (!valid_slug($slug)) {
            send_json(['error' => 'Invalid name (4-32 chars a-z0-9)'], 400);
        } elseif (is_file(slug_path($slug))) {
            send_json(['error' => 'Name already exists'], 409);
        }
        $token = $_GET['token'] ?? null;
        $content = '{"doc":{"format":"A4","orientation":"Portrait","pages":[{"elements":[]}]},"images":[]}';
        $tmp = slug_path($slug) . '.tmp';
        file_put_contents($tmp, $content);
        rename($tmp, slug_path($slug));
        $meta = ['created' => time(), 'modified' => time()];
        if ($token !== null && $token !== '') $meta['token'] = $token;
        save_meta($slug, $meta);
        $b = base_url();
        $tq = $token ? '?t=' . $token : '';
        send_json([
            'slug' => $slug,
            'token' => $token,
            'protected' => $token !== null && $token !== '',
            'url' => $b . '/' . $slug . $tq,
            'api_get' => $b . '/index.php?get=' . $slug . $tq,
            'api_put' => $b . '/index.php?put=' . $slug . $tq,
        ], 201);
    }

    // GET ?get=<slug> — Dokument lesen
    if ($method === 'GET' && isset($_GET['get'])) {
        $slug = $_GET['get'];
        if (!valid_slug($slug)) send_json(['error' => 'Invalid slug'], 400);
        check_access($slug, $_GET['t'] ?? null);
        $path = slug_path($slug);
        if (!is_file($path)) send_json(['error' => 'Document not found'], 404);
        header('Content-Type: application/json; charset=utf-8');
        readfile($path);
        exit;
    }

    // PUT ?put=<slug> — Dokument überschreiben
    if ($method === 'PUT' && isset($_GET['put'])) {
        $slug = $_GET['put'];
        if (!valid_slug($slug)) send_json(['error' => 'Invalid slug'], 400);
        check_access($slug, $_GET['t'] ?? ($_SERVER['HTTP_X_BOXDOC_TOKEN'] ?? null));
        $body = file_get_contents('php://input');
        if (strlen($body) > MAX_SIZE)
            send_json(['error' => 'Document too large (max ' . MAX_SIZE . ' bytes)'], 413);
        $decoded = json_decode($body);
        if ($decoded === null && json_last_error() !== JSON_ERROR_NONE)
            send_json(['error' => 'Invalid JSON: ' . json_last_error_msg()], 400);
        $tmp = slug_path($slug) . '.tmp';
        file_put_contents($tmp, $body);
        rename($tmp, slug_path($slug));
        $meta = load_meta($slug) ?? [];
        $meta['modified'] = time();
        save_meta($slug, $meta);
        @file_put_contents(DOCS_DIR . $slug . '.events.log',
            json_encode(['ts' => time(), 'type' => 'update']) . "\n", FILE_APPEND);
        send_json(['ok' => true, 'slug' => $slug, 'modified' => $meta['modified']]);
    }

    // GET ?list=1 — Alle Dokumente auflisten
    if ($method === 'GET' && isset($_GET['list'])) {
        ensure_docs_dir();
        $docs = [];
        foreach (glob(DOCS_DIR . '*.boxdoc') as $f) {
            $s = basename($f, '.boxdoc');
            $m = load_meta($s);
            $docs[] = [
                'slug' => $s,
                'modified' => $m['modified'] ?? filemtime($f),
                'size' => filesize($f),
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

// Slug + kein Accept: text/html → rohes JSON (für curl/KI)
if ($slug !== '' && !str_contains($_SERVER['HTTP_ACCEPT'] ?? '*/*', 'text/html')) {
    $doc_path = slug_path($slug);
    if (!is_file($doc_path)) {
        http_response_code(404);
        header('Content-Type: application/json; charset=utf-8');
        echo json_encode(['error' => 'Document not found', 'slug' => $slug]);
        exit;
    }
    header('Content-Type: application/json; charset=utf-8');
    header('Cache-Control: no-store');
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
    if (is_file($doc_path)) {
        $doc_json = file_get_contents($doc_path);
        $inject = '<script type="application/json" id="boxdoc-content">' . "\n"
                . $doc_json . "\n</script>\n";
        $spa = str_replace('</body>', $inject . '</body>', $spa);
    }
}

echo $spa;
