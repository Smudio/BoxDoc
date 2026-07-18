<?php
// api.php — Dünner Wrapper. Alle Logik liegt in index.php.
// Falls diese Datei auf dem Server liegt, leitet sie an index.php weiter.
// Falls nicht: index.php allein reicht (API via /index.php?new=1 etc.).
include __DIR__ . '/index.php';
