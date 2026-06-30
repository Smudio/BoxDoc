<?php
// =============================================================================
// BoxDoc API — Dokumenten-Backend (framework-frei, PHP 7.4+)
// =============================================================================
// Endpunkte (alle via ?action= oder各自的 Query-Param):
//   POST   ?new=1                      Neues Doc anlegen → {slug, token, url}
//   GET    ?get=<slug>&t=<token>       Doc-Inhalt liefern (JSON)
//   PUT    ?put=<slug>&t=<token>       Doc überschreiben (Body = JSON)
//   POST   ?rename=<slug>&t=<token>    Doc umbenennen (Body: {"new_name":"..."})
//   GET    ?list=1                     Liste aller Docs (slug, modified)
//
// Storage: web/docs/<slug>.boxdoc (Inhalt) + <slug>.meta.json (Token)
// =============================================================================

declare(strict_types=1);

header('Content-Type: application/json; charset=utf-8');
header('Access-Control-Allow-Origin: *');
header('Access-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS');
header('Access-Control-Allow-Headers: Content-Type, X-BoxDoc-Token');
header('Cache-Control: no-store, no-cache, must-revalidate');

if ($_SERVER['REQUEST_METHOD'] === 'OPTIONS') {
    http_response_code(204);
    exit;
}

const DOCS_DIR = __DIR__ . '/docs/';
const MAX_SIZE = 10 * 1024 * 1024; // 10 MB

// -----------------------------------------------------------------------------
// Hilfsfunktionen
// -----------------------------------------------------------------------------

function send_json(array $data, int $code = 200): void
{
    http_response_code($code);
    echo json_encode($data, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    exit;
}

function error(string $msg, int $code = 400): void
{
    send_json(['error' => $msg], $code);
}

function slug_path(string $slug): string
{
    // Strenge Validierung: nur a-z0-9, 8-32 Zeichen. Kein Path-Traversal möglich.
    if (!preg_match('/^[a-z0-9]{8,32}$/', $slug)) {
        error('Invalid slug format', 400);
    }
    return DOCS_DIR . $slug . '.boxdoc';
}

function meta_path(string $slug): string
{
    if (!preg_match('/^[a-z0-9]{8,32}$/', $slug)) {
        error('Invalid slug format', 400);
    }
    return DOCS_DIR . $slug . '.meta.json';
}

function gen_slug(): string
{
    // 10 Zeichen kryptografischer Zufall, kollisionsarm.
    return bin2hex(random_bytes(5));
}

function gen_token(): string
{
    // 32 Zeichen Token für Capability-URL.
    return bin2hex(random_bytes(16));
}

function load_meta(string $slug): ?array
{
    $path = meta_path($slug);
    if (!is_file($path)) return null;
    $m = json_decode((string) file_get_contents($path), true);
    return is_array($m) ? $m : null;
}

function save_meta(string $slug, array $meta): void
{
    file_put_contents(meta_path($slug), json_encode($meta,
        JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES));
}

function check_token(string $slug, ?string $token): void
{
    $meta = load_meta($slug);
    if ($meta === null) error('Document not found', 404);
    if (empty($meta['token'])) error('Document has no token', 403);
    if ($token === null || $token === '') error('Token required', 401);
    if (!hash_equals((string) $meta['token'], (string) $token)) {
        error('Invalid token', 403);
    }
}

function create_doc(string $slug, string $token, string $content = ''): void
{
    if (!is_dir(DOCS_DIR)) mkdir(DOCS_DIR, 0700, true);
    // Atomic write: tmp + rename.
    $tmp = slug_path($slug) . '.tmp';
    file_put_contents($tmp, $content);
    rename($tmp, slug_path($slug));
    save_meta($slug, [
        'token' => $token,
        'created' => time(),
        'modified' => time(),
    ]);
}

// -----------------------------------------------------------------------------
// Routing
// -----------------------------------------------------------------------------

$method = $_SERVER['REQUEST_METHOD'];

// --- Neues Doc anlegen ---
if ($method === 'POST' && isset($_GET['new'])) {
    $slug = gen_slug();
    $token = gen_token();
    $content = '{"doc":{"format":"A4","orientation":"Portrait","pages":[{"elements":[]}]},"images":[]}';
    create_doc($slug, $token, $content);
    $scheme = ($_SERVER['HTTPS'] ?? 'off') !== 'off' ? 'https' : 'http';
    $host = $_SERVER['HTTP_HOST'] ?? 'localhost';
    $base = $scheme . '://' . $host . dirname($_SERVER['SCRIPT_NAME'] ?? '/');
    send_json([
        'slug' => $slug,
        'token' => $token,
        'url' => $base . '/d/' . $slug . '?t=' . $token,
        'api_get' => $base . '/api.php?get=' . $slug . '&t=' . $token,
        'api_put' => $base . '/api.php?put=' . $slug . '&t=' . $token,
    ], 201);
}

// --- Doc lesen ---
if ($method === 'GET' && isset($_GET['get'])) {
    $slug = $_GET['get'];
    $path = slug_path($slug);
    if (!is_file($path)) error('Document not found', 404);
    header('Content-Type: application/json; charset=utf-8');
    readfile($path);
    exit;
}

// --- Doc überschreiben ---
if ($method === 'PUT' && isset($_GET['put'])) {
    $slug = $_GET['put'];
    $token = $_GET['t'] ?? ($_SERVER['HTTP_X_BOXDOC_TOKEN'] ?? null);
    check_token($slug, $token);
    $body = file_get_contents('php://input');
    if (strlen($body) > MAX_SIZE) error('Document too large (max ' . MAX_SIZE . ' bytes)', 413);
    // JSON validieren
    $decoded = json_decode($body);
    if ($decoded === null && json_last_error() !== JSON_ERROR_NONE) {
        error('Invalid JSON: ' . json_last_error_msg(), 400);
    }
    // Atomic write
    $tmp = slug_path($slug) . '.tmp';
    file_put_contents($tmp, $body);
    rename($tmp, slug_path($slug));
    // Meta aktualisieren
    $meta = load_meta($slug) ?? [];
    $meta['modified'] = time();
    save_meta($slug, $meta);
    // Event-Log für SSE (später Phase 3) — hänge einfach an.
    @file_put_contents(DOCS_DIR . $slug . '.events.log',
        json_encode(['ts' => time(), 'type' => 'update']) . "\n",
        FILE_APPEND);
    send_json(['ok' => true, 'slug' => $slug, 'modified' => $meta['modified']]);
}

// --- Doc umbenennen (Slug ändern) ---
if ($method === 'POST' && isset($_GET['rename'])) {
    $old_slug = $_GET['rename'];
    $token = $_GET['t'] ?? null;
    check_token($old_slug, $token);
    $body = json_decode(file_get_contents('php://input'), true);
    $new_name = $body['new_name'] ?? '';
    if (!preg_match('/^[a-z0-9]{8,32}$/', $new_name)) {
        error('Invalid new_name (8-32 chars a-z0-9)', 400);
    }
    if (is_file(slug_path($new_name))) error('Target name already exists', 409);
    // Dateien verschieben
    rename(slug_path($old_slug), slug_path($new_name));
    rename(meta_path($old_slug), meta_path($new_name));
    $meta = load_meta($new_name);
    $meta['modified'] = time();
    save_meta($new_name, $meta);
    send_json(['ok' => true, 'slug' => $new_name]);
}

// --- Liste aller Docs (nur slug + modified, öffentlich) ---
if ($method === 'GET' && isset($_GET['list'])) {
    $docs = [];
    foreach (glob(DOCS_DIR . '*.boxdoc') as $f) {
        $slug = basename($f, '.boxdoc');
        $meta = load_meta($slug);
        $docs[] = [
            'slug' => $slug,
            'modified' => $meta['modified'] ?? filemtime($f),
            'size' => filesize($f),
        ];
    }
    send_json(['documents' => $docs]);
}

// --- Fallback ---
error('Unknown endpoint. Use ?new=1, ?get=<slug>, ?put=<slug>, ?list=1', 404);
