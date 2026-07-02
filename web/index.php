<?php
// =============================================================================
// BoxDoc Front-Controller
// =============================================================================
//   boxdoc.at/              → Browser: SPA | Agent: SSR Startseite
//   boxdoc.at/lebenslauf    → Browser: SPA | Agent: SSR mit Doc-Inhalt
//   boxdoc.at/api.php       → API (lesen/schreiben)
//
// Agent-Erkennung: Echte Browser haben "Mozilla" + Engine (AppleWebKit/Gecko/
// Trident/Chrome). Alles andere (curl, opencode, python, bots) bekommt eine
// reine SSR-HTML-Seite OHNE WASM/Canvas — sofort lesbar, kein JS nötig.
// =============================================================================

declare(strict_types=1);

const DOCS_DIR = __DIR__ . '/docs/';

$request = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH) ?? '/';
$path = trim($request, '/');

// Slug extrahieren
$slug = '';
if (preg_match('/^([a-z0-9]{4,32})$/', $path, $m)) {
    $slug = $m[1];
}

$ua = $_SERVER['HTTP_USER_AGENT'] ?? '';

// Echten Browser erkennen.
// Zuverlässigstes Kriterium: echte Browser senden bei Navigation zwingend
// "Sec-Fetch-Dest: document" und "Sec-Fetch-Mode: navigate". WebFetch-Tools
// (opencode, curl, python-requests, bots) senden diese Header NICHT, selbst
// wenn sie einen Mozilla-User-Agent vortäuschen.
function is_real_browser(): bool {
    $dest = $_SERVER['HTTP_SEC_FETCH_DEST'] ?? '';
    $mode = $_SERVER['HTTP_SEC_FETCH_MODE'] ?? '';
    return $dest === 'document' && $mode === 'navigate';
}

// --- Rohe JSON-Anfrage (curl ohne Accept: text/html) → sofort JSON ---
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

// --- Browser-Pfad: SPA ausliefern ---
if (is_real_browser()) {
    $spa_path = __DIR__ . '/index.html';
    if (is_file($spa_path)) {
        $spa = file_get_contents($spa_path);

        // Doc-Inhalt für die SPA einbetten
        if ($slug !== '') {
            $doc_path = DOCS_DIR . $slug . '.boxdoc';
            if (is_file($doc_path)) {
                $doc_json = file_get_contents($doc_path);
                $inject = '<script type="application/json" id="boxdoc-content">' . "\n"
                        . $doc_json . "\n</script>\n";
                $spa = str_replace('</body>', $inject . '</body>', $spa);
            }
        }
        echo $spa;
        exit;
    }
    http_response_code(503);
    echo 'SPA nicht gefunden.';
    exit;
}

// =============================================================================
// --- Agent-Pfad: SSR-HTML (kein WASM, kein Canvas, reiner Text) ---
// =============================================================================
// Das hier bekommen opencode, curl -H "Accept: text/html", bots, etc.
// Eine einfache HTML-Seite, die auch nach Markdown-Konvertierung lesbar ist.

$scheme = (!empty($_SERVER['HTTPS']) && $_SERVER['HTTPS'] !== 'off') ? 'https' : 'http';
$host = $_SERVER['HTTP_HOST'] ?? 'localhost';
$base = $scheme . '://' . $host;

header('Content-Type: text/html; charset=utf-8');

// Doc laden
$doc_json = '';
$doc_pretty = '';
$doc_found = false;
if ($slug !== '') {
    $doc_path = DOCS_DIR . $slug . '.boxdoc';
    if (is_file($doc_path)) {
        $raw = file_get_contents($doc_path);
        $doc_json = $raw;
        $decoded = json_decode($raw);
        if ($decoded !== null) {
            $doc_pretty = json_encode($decoded,
                JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
        }
        $doc_found = true;
    }
}

// Titel aus dem ersten Text-Element extrahieren (für <title>)
$page_title = $slug ?: 'BoxDoc';
if ($doc_pretty) {
    if (preg_match('/"text"\s*:\s*"([^"\\\\]*(?:\\\\.[^"\\\\]*)*)"/', $doc_pretty, $tm)) {
        $page_title = substr($tm[1], 0, 60);
    }
}

echo '<!DOCTYPE html>';
echo '<html lang="de"><head>';
echo '<meta charset="utf-8">';
echo '<meta name="viewport" content="width=device-width, initial-scale=1">';
echo '<title>' . htmlspecialchars($page_title) . ' — BoxDoc</title>';
echo '<style>
body { font-family: -apple-system, system-ui, sans-serif; max-width: 900px; margin: 2rem auto; padding: 0 1rem; line-height: 1.5; color: #222; }
h1 { color: #1f3a5f; border-bottom: 3px solid #4fc3c5; padding-bottom: .3rem; }
h2 { color: #1f3a5f; margin-top: 2rem; }
pre { background: #f4f4f4; padding: 1rem; border-radius: 6px; overflow-x: auto; font-size: 0.85rem; }
code { background: #f4f4f4; padding: .1rem .3rem; border-radius: 3px; }
.api { background: #fff8e1; padding: 1rem 1.5rem; border-left: 4px solid #c8a24e; margin: 1rem 0; border-radius: 0 6px 6px 0; }
.doc-content { background: #f9f9f9; padding: 1rem; border-radius: 6px; border: 1px solid #e0e0e0; }
.hint { color: #666; font-style: italic; }
</style>';
echo '</head><body>';

if ($slug === '') {
    // Startseite
    echo '<h1>BoxDoc</h1>';
    echo '<p>BoxDoc ist ein Dokumenten-Editor. Dokumente liegen als JSON vor und können per HTTP bearbeitet werden.</p>';
    echo '<h2>Dokument erstellen</h2>';
    echo '<div class="api"><pre>curl -X POST "' . $base . '/api.php?new=1&amp;name=meindokument"</pre></div>';
    echo '<h2>Vorhandene Dokumente auflisten</h2>';
    echo '<div class="api"><pre>curl "' . $base . '/api.php?list=1"</pre></div>';
    echo '<h2>Dokument lesen</h2>';
    echo '<p>Jedes Dokument ist unter <code>' . $base . '/&lt;slug&gt;</code> erreichbar.</p>';
    echo '<div class="api"><pre>curl "' . $base . '/meindokument"</pre></div>';
} elseif (!$doc_found) {
    // Nicht gefunden
    echo '<h1>Dokument nicht gefunden</h1>';
    echo '<p>Das Dokument <code>' . htmlspecialchars($slug) . '</code> existiert nicht.</p>';
    echo '<p>Erstellen:</p>';
    echo '<div class="api"><pre>curl -X POST "' . $base . '/api.php?new=1&amp;name=' . htmlspecialchars($slug) . '"</pre></div>';
} else {
    // Dokument anzeigen
    echo '<h1>BoxDoc: ' . htmlspecialchars($slug) . '</h1>';
    echo '<p class="hint">Dies ist ein BoxDoc-Dokument. Du kannst es lesen und per HTTP ändern.</p>';

    // Inhalt
    echo '<h2>Dokument-Inhalt (JSON)</h2>';
    echo '<div class="doc-content"><pre>' . htmlspecialchars($doc_pretty) . '</pre></div>';

    // API-Anleitung
    echo '<h2>Dieses Dokument ändern</h2>';

    echo '<p><strong>1. Lesen</strong> (als JSON):</p>';
    echo '<div class="api"><pre>curl "' . $base . '/api.php?get=' . $slug . '"</pre></div>';

    echo '<p><strong>2. Ändern</strong> (komplettes JSON per PUT senden):</p>';
    echo '<div class="api"><pre>curl -X PUT --data-binary @doc.json \\
    "' . $base . '/api.php?put=' . $slug . '"</pre></div>';

    echo '<h2>Dokumentformat</h2>';
    echo '<ul>';
    echo '<li><code>doc.format</code>: A4 | A3 | A5 | Letter | Legal</li>';
    echo '<li><code>doc.orientation</code>: Portrait | Landscape</li>';
    echo '<li><code>doc.pages[]</code>: jede Seite hat <code>elements[]</code></li>';
    echo '<li><code>element.kind</code>: Text | Image | Rectangle | Line</li>';
    echo '<li>Koordinaten in Punkt (1pt = 1/72 Zoll), Ursprung oben-links, y nach unten</li>';
    echo '<li>A4 Portrait: 595 × 842 pt</li>';
    echo '<li>IDs sind u64 und stabil. Neue Elemente: höchste ID + 1</li>';
    echo '<li>Bilder (images[], png_base64) unverändert lassen</li>';
    echo '</ul>';

    echo '<h2>Element-Typen</h2>';
    echo '<ul>';
    echo '<li><strong>Text</strong>: text, font_size, font, color[r,g,b,a], bold, italic, underline, align, valign</li>';
    echo '<li><strong>Rectangle</strong>: fill_color, stroke_width, stroke_color, corner_radius</li>';
    echo '<li><strong>Line</strong>: stroke_width, stroke_color (h=0, rotation für Winkel)</li>';
    echo '<li><strong>Image</strong>: id (verweist auf images[].id), crop, image_w, image_h</li>';
    echo '</ul>';

    echo '<h2>Beispiel: Neues Text-Element</h2>';
    echo '<pre>{"id": 99, "kind": "Text",
  "x": 100, "y": 200, "w": 400, "h": 40, "rotation": 0,
  "text": "Neuer Text", "font_size": 14, "font": "default",
  "color": [20,20,20,255], "bold": false, "italic": false, "underline": false,
  "align": "Left", "valign": "Top", "indent": 0,
  "crop": {"x":0,"y":0,"w":1,"h":1}, "image_w": 0, "image_h": 0,
  "fill_color": [80,140,220,60], "stroke_width": 0,
  "stroke_color": [40,100,180,255], "corner_radius": 0}</pre>';

    echo '<p class="hint">Workflow: JSON lesen → Felder ändern → per PUT zurückschicken. Nutzer mit offenem Tab sehen die Änderung live.</p>';
}

echo '</body></html>';
