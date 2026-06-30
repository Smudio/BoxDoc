<?php
// =============================================================================
// BoxDoc SSE-Stream — Live-Updates für Multiplayer (Phase 3)
// =============================================================================
// Browser öffnen EventSource('stream.php?slug=<slug>&t=<token>').
// Bei jedem PUT auf das Doc pusht dieser Stream ein Event.
//
// Implementation: der lange offene Request pollt intern das Event-Log
// (docs/<slug>.events.log) alle ~100 ms. Sobald eine neue Zeile dazukommt,
// wird sie als SSE-Event gesendet. Simple, robust, funktioniert auf jedem
// PHP-Hosting (kein Swoole/Ratchet nötig).
// =============================================================================

declare(strict_types=1);

header('Content-Type: text/event-stream; charset=utf-8');
header('Cache-Control: no-store, no-cache, must-revalidate');
header('Connection: keep-alive');
header('Access-Control-Allow-Origin: *');

// Konfiguration
const DOCS_DIR = __DIR__ . '/docs/';
const POLL_INTERVAL_US = 100_000; // 100 ms
const MAX_LIFETIME_S = 120;       // Request-Timeout Schutz

$slug = $_GET['slug'] ?? '';
$token = $_GET['t'] ?? '';

// Slug validieren
if (!preg_match('/^[a-z0-9]{8,32}$/', $slug)) {
    http_response_code(400);
    echo "event: error\ndata: invalid slug\n\n";
    exit;
}

// Token prüfen
$meta_path = DOCS_DIR . $slug . '.meta.json';
if (!is_file($meta_path)) {
    http_response_code(404);
    echo "event: error\ndata: document not found\n\n";
    exit;
}
$meta = json_decode((string) file_get_contents($meta_path), true);
if (!hash_equals((string) ($meta['token'] ?? ''), (string) $token)) {
    http_response_code(403);
    echo "event: error\ndata: invalid token\n\n";
    exit;
}

$events_log = DOCS_DIR . $slug . '.events.log';

// Start-Position: aktuelle Zeilenzahl des Logs
$last_pos = is_file($events_log) ? filesize($events_log) : 0;

// SSE-Heartbeat: alle 15 Sekunden einen Kommentar schicken (hält Verbindung offen)
$last_heartbeat = time();
$start_time = time();

// Output-Buffer flushen
while (ob_get_level() > 0) ob_end_flush();

echo "event: ready\ndata: {\"slug\":\"$slug\"}\n\n";
flush();

// Hauptschleife
while (time() - $start_time < MAX_LIFETIME_S) {
    // Connection abgebrochen? (Browser zu)
    if (connection_aborted()) break;

    // Neue Events aus dem Log lesen
    clearstatcache(true, $events_log);
    $current_size = is_file($events_log) ? filesize($events_log) : 0;
    if ($current_size > $last_pos) {
        $fp = fopen($events_log, 'rb');
        fseek($fp, $last_pos);
        while (($line = fgets($fp)) !== false) {
            $line = trim($line);
            if ($line === '') continue;
            $data = json_decode($line, true);
            if ($data === null) continue;
            // Event senden
            echo "event: update\ndata: " . json_encode($data) . "\n\n";
            flush();
        }
        fclose($fp);
        $last_pos = $current_size;
    }

    // Heartbeat
    if (time() - $last_heartbeat >= 15) {
        echo ": heartbeat\n\n";
        flush();
        $last_heartbeat = time();
    }

    usleep(POLL_INTERVAL_US);
}

// Soft-Timeout: Client reconnectet automatisch (EventSource)
echo "event: close\ndata: session timeout\n\n";
flush();
