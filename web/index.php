<?php
// =============================================================================
// BoxDoc Front-Controller
// =============================================================================
//   boxdoc.at/              → SPA Startseite
//   boxdoc.at/lebenslauf    → Doc-Inhalt EINGEBETTET im HTML
//                            (sichtbar für Browser UND für KI-Agenten)
//   boxdoc.at/api.php       → API (lesen/schreiben)
//
// WICHTIG: Der Doc-Inhalt wird IMMER in die Seite eingebettet, damit auch
// Tools die kein JavaScript ausführen (z.B. opencode/web-fetch) den Inhalt
// sehen können. Browser lesen ihn via <script id="boxdoc-content">,
// KI-Agenten sehen ihn im HTML-Quelltext.
// =============================================================================

declare(strict_types=1);

const DOCS_DIR = __DIR__ . '/docs/';

$request = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?? '/';
$path = trim($request, '/');

// Ist es ein reiner Slug? (a-z0-9, 4-32 Zeichen)
$slug = '';
if (preg_match('/^([a-z0-9]{4,32})$/', $path, $m)) {
    $slug = $m[1];
}

$token = $_GET['t'] ?? '';

// --- KI-Pfad: wenn kein HTML gewollt (curl mit Accept: */*), rohes JSON ---
if ($slug !== '' && !str_contains($_SERVER['HTTP_ACCEPT'] ?? '*/*', 'text/html')) {
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

// --- HTML ausliefern (Browser und alle HTML-lesenden Tools) ---

// Doc-Inhalt laden (falls Slug vorhanden)
$doc_json = '{}';
$doc_found = false;
if ($slug !== '') {
    $doc_path = DOCS_DIR . $slug . '.boxdoc';
    if (is_file($doc_path)) {
        $doc_json = file_get_contents($doc_path);
        $doc_found = true;
    }
}

// SPA-HTML laden
$spa_html = '';
$spa_path = __DIR__ . '/index.html';
if (is_file($spa_path)) {
    $spa_html = file_get_contents($spa_path);
}

// Basis-URL für API-Beispiele
$scheme = (!empty($_SERVER['HTTPS']) && $_SERVER['HTTPS'] !== 'off') ? 'https' : 'http';
$host = $_SERVER['HTTP_HOST'] ?? 'localhost';
$base = $scheme . '://' . $host;

// Doc-Inhalt EINBETTEN: als <script type="application/json"> — der Browser
// (SPA) liest es beim Start aus, und KI-Agenten sehen es direkt im Quelltext.
$embedded = '';
if ($slug !== '') {
    $embedded .= "\n<!-- ============================================================ -->\n";
    $embedded .= "<!-- BOXDOC DOKUMENT-INHALT (für KI-Agenten sichtbar)            -->\n";
    $embedded .= "<!-- ============================================================ -->\n";
    $embedded .= "<!-- Dieses Dokument wurde von BoxDoc generiert.                  -->\n";
    $embedded .= "<!-- Der komplette Inhalt steht unten als JSON.                   -->\n";
    $embedded .= "<!--                                                              -->\n";
    $embedded .= "<!-- ÄNDERN per curl:                                             -->\n";
    $embedded .= "<!--   curl -X PUT --data-binary @doc.json \\                      -->\n";
    $embedded .= "<!--        \"$base/api.php?put=$slug\"               -->\n";
    $embedded .= "<!--                                                              -->\n";
    $embedded .= "<!-- LESEN als reines JSON:                                       -->\n";
    $embedded .= "<!--   curl -H \"Accept: application/json\" \"$base/$slug\"      -->\n";
    $embedded .= "<!-- ============================================================ -->\n";
    $embedded .= '<script type="application/json" id="boxdoc-content">' . "\n";
    $embedded .= $doc_json . "\n";
    $embedded .= '</script>' . "\n";
    $embedded .= "<!-- ============================================================ -->\n";
}

// Einbettung vor </body> oder am Ende einfügen
if ($spa_html !== '') {
    // Vor </body> injizieren, falls vorhanden
    if (str_contains($spa_html, '</body>')) {
        $spa_html = str_replace('</body>', $embedded . '</body>', $spa_html);
    } else {
        $spa_html .= $embedded;
    }
    echo $spa_html;
} else {
    // Keine SPA vorhanden
    http_response_code(503);
    echo 'BoxDoc SPA nicht gefunden. Bitte mit trunk build bauen.';
}
