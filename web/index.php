<?php
// =============================================================================
// BoxDoc SSR — Server-Side Rendering für KI-Agenten
// =============================================================================
// URL: /d/<slug>?t=<token>
//
// Was passiert hier:
//   1. Für Browser: liefert die SPA (index.html) — aber mit eingebettetem
//      JSON-Inhalt des Docs in einem <script type="application/json"> Tag.
//      Die SPA liest das beim Start aus.
//   2. Für KI-Agenten (opencode, curl, wget): sieht den Doc-Inhalt + eine
//      vollständige Anleitung wie man das Doc per PUT ändern kann. Alles
//      direkt im HTML-Quellcode sichtbar.
// =============================================================================

declare(strict_types=1);

const DOCS_DIR = __DIR__ . '/docs/';

$slug = '';
$token = '';

// Pfad parsen: /d/<slug>?t=<token>
$request_uri = parse_url($_SERVER['REQUEST_URI'] ?? '/', PHP_URL_PATH);
if (preg_match('#^/?d/([a-z0-9]{8,32})#', $request_uri, $m)) {
    $slug = $m[1];
} elseif (isset($_GET['doc'])) {
    $slug = $_GET['doc'];
}
$token = $_GET['t'] ?? '';

$doc_content = '';
$doc_found = false;
$doc_modified = 0;

if ($slug !== '') {
    $doc_path = DOCS_DIR . $slug . '.boxdoc';
    if (is_file($doc_path)) {
        $doc_content = file_get_contents($doc_path);
        $doc_found = true;
        $doc_modified = filemtime($doc_path);
    }
}

// Basis-URL ermitteln (für Beispiele in der Anleitung)
$scheme = ($_SERVER['HTTPS'] ?? 'off') !== 'off' ? 'https' : 'http';
$host = $_SERVER['HTTP_HOST'] ?? 'localhost';
$script_dir = dirname($_SERVER['SCRIPT_NAME'] ?? '/');
$base = $scheme . '://' . $host . $script_dir;
$api_get_url = $base . '/api.php?get=' . $slug . '&t=' . $token;
$api_put_url = $base . '/api.php?put=' . $slug . '&t=' . $token;

// AI-Anleitung (wird in den HTML-Code eingebettet)
$ai_instructions = <<<HTML
<!--
==========================================================================
BOXDOC DOKUMENT — ANLEITUNG FÜR KI-AGENTEN (z. B. opencode)
==========================================================================

Du (die KI) hast diese Seite aufgerufen und kannst das Dokument direkt
sehen und bearbeiten. Hier ist alles, was du brauchst:

DAS DOKUMENT
-----------
Der vollständige Inhalt dieses BoxDoc-Dokuments steht weiter unten in der
Seite als JSON eingebettet:
  <script type="application/json" id="boxdoc-content">
      { ...vollständiges Dokument als JSON... }
  </script>

Du kannst es auch separat als reines JSON herunterladen:
  curl "$api_get_url"

WIE DU ES ÄNDERST
----------------
Sende das geänderte JSON per PUT an die API:
  curl -X PUT \\
       --data-binary @geändertes-dokument.json \\
       "$api_put_url"

WICHTIG: Der Token in der URL (t=...) MUSS erhalten bleiben. Ohne ihn
schlägt der Schreibzugriff fehl. Verwende am besten genau diese URL.

DOKUMENTFORMAT (.boxdoc = JSON)
-------------------------------
{
  "doc": {
    "format": "A4" | "A3" | "A5" | "Letter" | "Legal",
    "orientation": "Portrait" | "Landscape",
    "pages": [ { "elements": [ <Element>, ... ] } ]
  },
  "images": [ { "id": <u64>, "png_base64": "<base64-PNG>" } ]
}

ELEMENT (je nach "kind" sind verschiedene Felder relevant)
---------------------------------------------------------
{
  "id": <u64>,                       // stabil, nie ändern beim Update
  "kind": "Text" | "Image" | "Rectangle" | "Line",
  "x": <f32 pt>, "y": <f32 pt>,      // linke obere Ecke
  "w": <f32 pt>, "h": <f32 pt>,      // Breite, Höhe (Line: h=0)
  "rotation": <Grad>,
  "text": "<Inhalt>",                // kann \\n enthalten
  "font_size": <pt>,
  "font": "default"|"inter"|"roboto"|"lora"|"jetbrains"|"pacifico",
  "color": [r, g, b, a],             // 0..255; a=255 deckend
  "bold": <bool>, "italic": <bool>, "underline": <bool>,
  "align": "Left"|"Center"|"Right", "valign": "Top"|"Middle"|"Bottom",
  "indent": <pt>,
  "crop": {"x":0,"y":0,"w":1,"h":1}, // Image: normalisiert 0..1
  "image_w": <px>, "image_h": <px>,  // Image
  "fill_color": [r,g,b,a],           // Shape; Alpha 0 = transparent
  "stroke_width": <pt>,              // Shape; 0 = kein Rahmen
  "stroke_color": [r,g,b,a],         // Shape
  "corner_radius": <pt>              // Rectangle
}

ELEMENT-TYPEN
-------------
  Text       : text, font_size, font, color, bold, italic, underline, align, valign
  Rectangle  : fill_color, stroke_width, stroke_color, corner_radius
  Line       : stroke_width, stroke_color (Linie = Box mit h=0 + rotation)
  Image      : id (verweist auf images[].id), crop, image_w, image_h

KOOORDINATENSYSTEM
------------------
- Punkt: 1 pt = 1/72 Zoll (1 Zoll = 25,4 mm). Ursprung: oben-links, y nach unten.
- A4 Hochformat: 595 × 842 pt. 1 cm ≈ 28,3 pt.
- Z-Order: Elemente weiter hinten im Array liegen OBEN.

REGELN
------
1. Ändere Felder direkt im JSON, schicke das komplette Dokument per PUT.
2. IDs sind u64 und stabil. Neue Elemente: höchste vorhandene ID + 1.
3. Bilder (images[], png_base64, image_w, image_h) UNVERÄNDERT lassen.
4. Der Token (?t=...) MUSS in der PUT-URL enthalten sein.
5. Wenn andere Nutzer das Dokument gerade offen haben, sehen sie deine
   Änderung in Echtzeit (<200 ms via SSE).

DESIGN-TIPPS
------------
- Titel 28-36pt bold, Überschrift 18-22pt bold, Fließtext 10-12pt, Footer 8-9pt
- Zeilenabstand ca. 1,3× Schriftgröße
- Seitenrand mind. 50 pt (≈ 1,8 cm)
- Eine Hauptfarbe + eine Akzentfarbe
- Aufzählungen mit "• " prefixen
- Z-Order: Dekorationen (Hintergrundbalken) VORNE im Array, Text HINTEN

WORKFLOW
--------
1. Aktuelles Doc abrufen:
     curl "$api_get_url" > doc.json
2. doc.json mit deinen Edit-Tools ändern (Text, Position, Farben, etc.)
3. Zurückschreiben:
     curl -X PUT --data-binary @doc.json "$api_put_url"

oder in einem Rutsch: JSON direkt per PUT senden.

==========================================================================
-->
HTML;

// SPA-HTML lesen (falls vorhanden) — sonst Inline-Template
$spa_html = '';
$spa_path = __DIR__ . '/index.html';
if (is_file($spa_path)) {
    $spa_html = file_get_contents($spa_path);
}

// Wenn das SPA eine fertige index.html ist (Trunk-Build), müssen wir unser
// JSON + AI-Hint injizieren. Wir hängen es VOR </head> oder </body> ein.
// Falls keine index.html gefunden wurde (z. B. Dev-Mode), rendern wir ein
// einfaches HTML-Gerüst.

if ($spa_html === '') {
    // Simples HTML-Gerüst (Fallback / Dev)
    ?>
<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<title>BoxDoc — <?= htmlspecialchars($slug) ?></title>
</head>
<body>
<p>Dies ist ein BoxDoc-Dokument. Dokument-ID: <code><?= htmlspecialchars($slug) ?></code></p>
<?php if (!$doc_found): ?>
<p style="color:#c00">Dokument nicht gefunden.</p>
<?php endif; ?>
    <?php
} else {
    echo $spa_html;
    // Wenn die SPA ihr eigenes <html> ausgespuckt hat, fügen wir die
    // JSON/Anleitung als HTML-Kommentar am Ende ein.
}

// Doc-Inhalt einbetten (für SPA zum Auslesen UND für opencode sichtbar)
if ($doc_found) {
    echo '<script type="application/json" id="boxdoc-content" data-modified="' . $doc_modified . '">'
       . $doc_content
       . '</script>' . "\n";
} else {
    echo '<script type="application/json" id="boxdoc-content" data-error="not-found">{}</script>' . "\n";
}

// Metadaten einbetten
echo '<script type="application/json" id="boxdoc-meta">'
   . json_encode([
       'slug' => $slug,
       'api_get' => $api_get_url,
       'api_put' => $api_put_url,
       'modified' => $doc_modified,
   ])
   . '</script>' . "\n";

// AI-Anleitung als HTML-Kommentar
echo $ai_instructions;

// Falls wir das HTML-Gerüst selbst erzeugt haben, schließen
if ($spa_html === '') {
    echo "</body></html>\n";
}
