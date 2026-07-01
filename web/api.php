<?php
// =============================================================================
// BoxDoc API — Dokumenten-Backend (framework-frei, PHP 7.4+)
// =============================================================================
// Endpunkte:
//   POST   ?new=1                     Neues Doc anlegen → {slug, url}
//                                        Optional: &name=<slug>   (eigener Name)
//                                        Optional: &token=<token> (Schutz setzen)
//   GET    ?get=<slug>                Doc-Inhalt liefern (JSON + _ai_hint)
//   PUT    ?put=<slug>                Doc überschreiben (Body = JSON)
//                                        Bei geschütztem Doc: &t=<token>
//   GET    ?list=1                    Liste aller Docs (slug, modified, size)
//
// Token-Modell:
//   Default = kein Token. Docs sind öffentlich (lesen/schreiben wie Pastebin).
//   Nur wenn beim Erstellen ?token=<wert> gesetzt wird, ist das Doc geschützt.
//   Dann ist bei GET/PUT der Token nötig: &t=<token>.
//
// Storage: docs/<slug>.boxdoc (Inhalt) + <slug>.meta.json (optional Token)
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

/** Validiert einen Slug: a-z0-9, 4-32 Zeichen, keine reservierten Namen. */
function valid_slug(string $slug): bool
{
    if (!preg_match('/^[a-z0-9]{4,32}$/', $slug)) return false;
    // Reservierte Namen (wg. Dateien/Verzeichnissen auf dem Server).
    $reserved = ['docs', 'api', 'index', 'stream', 'assets', 'static', 'dist'];
    return !in_array($slug, $reserved, true);
}

function slug_path(string $slug): string
{
    return DOCS_DIR . $slug . '.boxdoc';
}

function meta_path(string $slug): string
{
    return DOCS_DIR . $slug . '.meta.json';
}

function gen_slug(): string
{
    // 10 Zeichen Zufall — kollisionsarm.
    return bin2hex(random_bytes(5));
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

/**
 * Prüft den Zugriff. Wenn das Doc einen Token hat, muss er übereinstimmen.
 * Wenn das Doc KEINEN Token hat, ist der Zugriff frei (Default).
 */
function check_access(string $slug, ?string $token): void
{
    $meta = load_meta($slug);
    if ($meta === null) error('Document not found', 404);
    // Kein Token im Meta → öffentlich, Zugriff erlaubt.
    if (empty($meta['token'])) return;
    // Token gesetzt → muss übereinstimmen.
    if ($token === null || $token === '') error('Token required', 401);
    if (!hash_equals((string) $meta['token'], (string) $token)) {
        error('Invalid token', 403);
    }
}

function create_doc(string $slug, string $content, ?string $token = null): void
{
    if (!is_dir(DOCS_DIR)) mkdir(DOCS_DIR, 0700, true);
    // Atomic write: tmp + rename.
    $tmp = slug_path($slug) . '.tmp';
    file_put_contents($tmp, $content);
    rename($tmp, slug_path($slug));
    $meta = ['created' => time(), 'modified' => time()];
    if ($token !== null && $token !== '') {
        $meta['token'] = $token;
    }
    save_meta($slug, $meta);
}

function base_url(): string
{
    $scheme = ($_SERVER['HTTPS'] ?? 'off') !== 'off' ? 'https' : 'http';
    $host = $_SERVER['HTTP_HOST'] ?? 'localhost';
    $dir = dirname($_SERVER['SCRIPT_NAME'] ?? '/');
    $dir = ($dir === '/' || $dir === '\\') ? '' : $dir;
    return $scheme . '://' . $host . $dir;
}

// -----------------------------------------------------------------------------
// Routing
// -----------------------------------------------------------------------------

$method = $_SERVER['REQUEST_METHOD'];

// --- Neues Doc anlegen ---
if ($method === 'POST' && isset($_GET['new'])) {
    // Slug: vom Nutzer wählbar (?name=...) oder automatisch generiert.
    $slug = $_GET['name'] ?? '';
    if ($slug === '') {
        $slug = gen_slug();
    } elseif (!valid_slug($slug)) {
        error('Invalid name (4-32 chars a-z0-9)', 400);
    } elseif (is_file(slug_path($slug))) {
        error('Name already exists', 409);
    }
    // Token: optional. ?token=<wert> setzt Schutz, sonst öffentlich.
    $token = $_GET['token'] ?? null;

    $content = '{"doc":{"format":"A4","orientation":"Portrait","pages":[{"elements":[]}]},"images":[]}';
    create_doc($slug, $content, $token);

    $base = base_url();
    $tq = $token ? '?t=' . $token : '';
    send_json([
        'slug' => $slug,
        'token' => $token,
        'protected' => $token !== null,
        'url' => $base . '/d/' . $slug . $tq,
        'api_get' => $base . '/api.php?get=' . $slug . $tq,
        'api_put' => $base . '/api.php?put=' . $slug . $tq,
    ], 201);
}

// --- Doc lesen ---
if ($method === 'GET' && isset($_GET['get'])) {
    $slug = $_GET['get'];
    if (!valid_slug($slug)) error('Invalid slug', 400);
    $token = $_GET['t'] ?? null;
    check_access($slug, $token);
    $path = slug_path($slug);
    if (!is_file($path)) error('Document not found', 404);
    header('Content-Type: application/json; charset=utf-8');
    readfile($path);
    exit;
}

// --- Doc überschreiben ---
if ($method === 'PUT' && isset($_GET['put'])) {
    $slug = $_GET['put'];
    if (!valid_slug($slug)) error('Invalid slug', 400);
    $token = $_GET['t'] ?? ($_SERVER['HTTP_X_BOXDOC_TOKEN'] ?? null);
    check_access($slug, $token);
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
    // Event-Log für SSE (Phase 3)
    @file_put_contents(DOCS_DIR . $slug . '.events.log',
        json_encode(['ts' => time(), 'type' => 'update']) . "\n",
        FILE_APPEND);
    send_json(['ok' => true, 'slug' => $slug, 'modified' => $meta['modified']]);
}

// --- Liste aller Docs ---
if ($method === 'GET' && isset($_GET['list'])) {
    $docs = [];
    foreach (glob(DOCS_DIR . '*.boxdoc') as $f) {
        $slug = basename($f, '.boxdoc');
        $meta = load_meta($slug);
        $docs[] = [
            'slug' => $slug,
            'modified' => $meta['modified'] ?? filemtime($f),
            'size' => filesize($f),
            'protected' => !empty($meta['token']),
        ];
    }
    send_json(['documents' => $docs]);
}

// --- Fallback ---
error('Unknown endpoint. Use ?new=1, ?get=<slug>, ?put=<slug>, ?list=1', 404);
