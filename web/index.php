<?php
// =============================================================================
// BoxDoc Front-Controller — bedient saubere URLs (boxdoc.at/<slug>)
// =============================================================================
//   boxdoc.at/              → SPA (index.html)
//   boxdoc.at/lebenslauf    → Browser: SPA  |  KI (curl/opencode): JSON
//   boxdoc.at/api.php       → direkt durch (Mutationen: new/put/list)
//
// Content-Negotiation:
//   Accept: text/html  → SPA (Browser)
//   sonst (curl: */*)  → rohes Doc-JSON (für KI-Agenten)
// =============================================================================

declare(strict_types=1);

const DOCS_DIR = __DIR__ . '/docs/';

// Pfad ohne führenden Schrägstrich.
$request = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?? '/';
$path = trim($request, '/');

// Ist es ein reiner Slug? (a-z0-9, 4-32 Zeichen)
$slug = '';
if (preg_match('/^([a-z0-9]{4,32})$/', $path, $m)) {
    $slug = $m[1];
}

// Will der Client HTML? (Browser) Sonst JSON liefern (curl, opencode, fetch).
$wants_html = str_contains($_SERVER['HTTP_ACCEPT'] ?? '*/*', 'text/html');

// --- KI-Pfad: JSON direkt ausliefern ---
if ($slug !== '' && !$wants_html) {
    $doc_path = DOCS_DIR . $slug . '.boxdoc';
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

// --- Browser-Pfad: SPA ausliefern (index.html) ---
$spa = __DIR__ . '/index.html';
if (is_file($spa)) {
    readfile($spa);
    exit;
}

// Fallback: kein index.html vorhanden (Dev-Modus?)
http_response_code(404);
echo 'BoxDoc SPA (index.html) nicht gefunden. Bitte mit `trunk build` bauen.';
