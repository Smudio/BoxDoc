# web/ — die Web-Version

**Es gibt genau einen Web-Ordner: diesen.** `web/dist/` darin ist kein zweiter,
sondern das Ergebnis des Builds.

```
web/                  ← Quelle. Hier editieren. In Git.
├── index.php         PHP-Backend (Dokumentenliste, GET/PUT, KI-Schnittstelle)
├── api.php           API-Einstieg
├── stream.php        Long-Polling fuer Live-Aktualisierung
├── .htaccess         Apache-Regeln
├── docs/.htaccess    sperrt den Dokumentenordner nach aussen
├── llms.txt          Kurz-Spezifikation fuer KI-Agenten
├── llms-full.txt     vollstaendige Spezifikation
├── sample.boxdoc     Beispiel-Dokument
├── robots.txt
└── dist/             ← ERGEBNIS. Gitignored. NICHT editieren.
```

## Bauen

```powershell
.\build-web.ps1          # aus dem Projektwurzelverzeichnis
```

Danach den **Inhalt von `web/dist/`** auf den Webserver laden. `docs/` muss fuer
PHP schreibbar sein (`chmod 0700`).

`trunk build` erzeugt `web/dist/` bei jedem Lauf komplett neu: das WASM-Modul und
den JS-Loader aus dem Rust-Code, dazu Kopien aller oben genannten Dateien. Diese
Kopien stehen als `copy-file`-Links in `index.html` im Wurzelverzeichnis — eine
neue Datei hier braucht dort auch einen neuen Link, sonst landet sie nie im
Upload-Paket.

## Warum diese Struktur

Vorher lagen `web/` und `dist/` nebeneinander im Wurzelverzeichnis und sahen wie
zwei gleichrangige Web-Ordner aus. Das ging schief: `llms.txt` und `llms-full.txt`
wurden in **beiden** von Hand editiert, und `dist/index.php` war wochenlang die
alte Fassung — wer die hochgeladen hat, hat eine veraltete Seite ausgeliefert.

Als `web/dist/` ist die Beziehung nicht mehr zu verwechseln. Aenderungen dort sind
beim naechsten Build weg; das ist Absicht und die einzige Richtung, in die es
gehen darf.

## Was die Web-Version kann

Sie ist mit der EXE angeglichen — gleiche Menues, gleicher Code, gleiche
Ausgabebytes. Der Unterschied ist nur das Ziel: Native oeffnet einen
Dateidialog, der Browser laedt herunter.

| Funktion | Web | Desktop |
| --- | --- | --- |
| Bearbeiten, Undo/Redo, Themes, JSON-Editor | ja | ja |
| `.boxdoc` oeffnen / speichern | Download | Datei |
| PDF exportieren | Download | Datei |
| SVG exportieren (Seite + Auswahl) | Download | Datei |
| ODT exportieren / oeffnen | Download / Datei-Dialog | Datei |
| Bilder speichern | Download (PNG) | Datei (PNG/JPEG) |
| Bild einfuegen, Schrift laden | ja | ja |
| Zwischenablage: Objekte | ja | ja |
| Zwischenablage: Bild hinein (Strg+V) | ja | ja |
| Zwischenablage: Bild hinaus (Strg+C) | nein¹ | ja |
| **PDF oeffnen (Import)** | **nein²** | ja |
| **Drucken** | **nein³** | ja |
| Datei-Watch (KI aendert Datei extern) | Web-Sync via PHP | Watcher |

1. `navigator.clipboard.write` ist asynchron und braucht eine Nutzergeste —
   Strg+C kopiert im Browser weiterhin die BoxDoc-Objekte.
2. Der PDF-Import setzt auf pdfium auf, eine native C++-Bibliothek. Ein zweiter,
   eigener Parser fuer Web wuerde andere Ergebnisse liefern als die EXE.
3. Der Browser-Druckdialog druckt die HTML-Seite, also den Canvas als Pixelbild.
   Richtiger Weg: PDF exportieren, PDF drucken. Der Menueeintrag ist deshalb
   ausgegraut statt versteckt, mit Hinweis im Tooltip.

Beide fehlenden Punkte sind im Menue sichtbar und ausgegraut — wer sie sucht,
findet sie und erfaehrt im Tooltip, woran es hakt.
