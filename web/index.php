<?php
// =============================================================================
// BoxDoc Front-Controller
// =============================================================================
//   boxdoc.at/              → SPA Startseite
//   boxdoc.at/lebenslauf    → Doc-Inhalt EINGEBETTET (für KI + Browser)
//   boxdoc.at/api.php       → API (lesen/schreiben)
//
// Der Doc-Inhalt wird ZWEIFACH eingebettet:
//   1. <script type="application/json">  — für die SPA (Browser mit JS)
//   2. <noscript> mit Klartext            — für KI-Agenten/WebFetch (kein JS)
//      WebFetch-Tools konvertieren HTML→Markdown und strippen <script>-Tags,
//      aber <noscript>-Inhalt überlebt die Konvertierung.
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

// --- KI-Pfad: wenn kein HTML gewollt, rohes JSON ---
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

// --- HTML ausliefern ---

// Doc-Inhalt laden
$doc_json = '{}';
$doc_found = false;
$doc_pretty = '{}';
if ($slug !== '') {
    $doc_path = DOCS_DIR . $slug . '.boxdoc';
    if (is_file($doc_path)) {
        $raw = file_get_contents($doc_path);
        $doc_json = $raw;
        $doc_found = true;
        // Pretty-print für bessere Lesbarkeit im <noscript>
        $decoded = json_decode($raw);
        if ($decoded !== null) {
            $doc_pretty = json_encode($decoded, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        }
    }
}

// SPA-HTML laden
$spa_html = '';
$spa_path = __DIR__ . '/index.html';
if (is_file($spa_path)) {
    $spa_html = file_get_contents($spa_path);
}

// Basis-URL
$scheme = (!empty($_SERVER['HTTPS']) && $_SERVER['HTTPS'] !== 'off') ? 'https' : 'http';
$host = $_SERVER['HTTP_HOST'] ?? 'localhost';
$base = $scheme . '://' . $host;

// Einbettung bauen
$embedded = '';

if ($slug !== '' && $doc_found) {

    // --- 1. Für die SPA: <script> Tag ---
    $embedded .= '<script type="application/json" id="boxdoc-content">' . "\n";
    $embedded .= $doc_json . "\n";
    $embedded .= '</script>' . "\n\n";

    // --- 2. Für KI-Agenten: <noscript> mit Klartext-Anleitung ---
    // WebFetch-Tools (opencode, etc.) führen kein JS aus. Sie konvertieren
    // HTML→Markdown/Text. <script> wird dabei entfernt, aber <noscript>
    // Inhalt bleibt erhalten. Deshalb steht hier alles wichtige als Text.
    $embedded .= "<noscript>\n";
    $embedded .= "<h1>BoxDoc Dokument: " . htmlspecialchars($slug) . "</h1>\n";
    $embedded .= "<p>Dies ist ein BoxDoc-Dokument. BoxDoc ist ein Dokumenten-Editor.</p>\n";
    $embedded .= "<p>Du (die KI) kannst dieses Dokument lesen und per HTTP verändern.</p>\n\n";

    $embedded .= "<h2>Vollständiger Dokument-Inhalt (JSON)</h2>\n";
    $embedded .= "<pre>\n" . htmlspecialchars($doc_pretty) . "\n</pre>\n\n";

    $embedded .= "<h2>Wie du dieses Dokument ändern kannst</h2>\n";
    $embedded .= "<p><strong>Lesen</strong> (reines JSON, ohne HTML):</p>\n";
    $embedded .= "<pre>curl -H \"Accept: application/json\" \"" . $base . "/" . $slug . "\"</pre>\n";
    $embedded .= "<p>oder kürzer:</p>\n";
    $embedded .= "<pre>curl \"" . $base . "/api.php?get=" . $slug . "\"</pre>\n\n";

    $embedded .= "<p><strong>Ändern</strong> (komplettes JSON per PUT senden):</p>\n";
    $embedded .= "<pre>curl -X PUT --data-binary @doc.json \"" . $base . "/api.php?put=" . $slug . "\"</pre>\n\n";

    $embedded .= "<p><strong>Neues Dokument erstellen</strong>:</p>\n";
    $embedded .= "<pre>curl -X POST \"" . $base . "/api.php?new=1&amp;name=neuername\"</pre>\n\n";

    $embedded .= "<h2>Dokumentformat</h2>\n";
    $embedded .= "<ul>\n";
    $embedded .= "<li><strong>doc.format</strong>: A4, A3, A5, Letter, Legal</li>\n";
    $embedded .= "<li><strong>doc.orientation</strong>: Portrait oder Landscape</li>\n";
    $embedded .= "<li><strong>doc.pages</strong>: Array von Seiten, jede mit elements-Array</li>\n";
    $embedded .= "<li><strong>Element.kind</strong>: Text, Image, Rectangle, Line</li>\n";
    $embedded .= "<li><strong>Koordinaten</strong>: Punkt (1pt = 1/72 Zoll), Ursprung oben-links, y nach unten</li>\n";
    $embedded .= "<li><strong>A4 Portrait</strong>: 595 × 842 pt</li>\n";
    $embedded .= "<li><strong>IDs</strong>: u64, stabil. Neue Elemente = höchste ID + 1</li>\n";
    $embedded .= "<li><strong>Bilder</strong>: images[], png_base64, image_w, image_h unverändert lassen</li>\n";
    $embedded .= "</ul>\n\n";

    $embedded .= "<h2>Element-Typen</h2>\n";
    $embedded .= "<ul>\n";
    $embedded .= "<li><strong>Text</strong>: text, font_size, font, color[r,g,b,a], bold, italic, underline, align, valign</li>\n";
    $embedded .= "<li><strong>Rectangle</strong>: fill_color, stroke_width, stroke_color, corner_radius</li>\n";
    $embedded .= "<li><strong>Line</strong>: stroke_width, stroke_color (h=0, rotation für Winkel)</li>\n";
    $embedded .= "<li><strong>Image</strong>: id (verweist auf images[].id), crop, image_w, image_h</li>\n";
    $embedded .= "</ul>\n\n";

    $embedded .= "<p><strong>Workflow:</strong> 1) JSON lesen. 2) Mit Edit-Tools ändern. 3) Per PUT zurückschicken. Andere Nutzer mit offenem Tab sehen die Änderung live.</p>\n";
    $embedded .= "</noscript>\n\n";

} elseif ($slug !== '' && !$doc_found) {

    $embedded .= "<noscript>\n";
    $embedded .= "<h1>BoxDoc: Dokument nicht gefunden</h1>\n";
    $embedded .= "<p>Das Dokument '" . htmlspecialchars($slug) . "' existiert nicht.</p>\n";
    $embedded .= "<p>Neues Dokument erstellen:</p>\n";
    $embedded .= "<pre>curl -X POST \"" . $base . "/api.php?new=1&amp;name=" . htmlspecialchars($slug) . "\"</pre>\n";
    $embedded .= "</noscript>\n\n";
}

// In SPA einfügen
if ($spa_html !== '') {
    if (str_contains($spa_html, '</body>')) {
        $spa_html = str_replace('</body>', $embedded . '</body>', $spa_html);
    } else {
        $spa_html .= $embedded;
    }
    echo $spa_html;
} else {
    http_response_code(503);
    echo 'BoxDoc SPA nicht gefunden. Bitte mit trunk build bauen.';
}
