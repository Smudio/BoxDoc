//! Datei-Dialoge sowie Speichern/Laden des Projektformats (.boxdoc).
//!
//! Auf Native: rfd-Dialoge + std::fs.
//! Auf Web: Drag&Drop (läuft über egui) + Browser-Download.

use serde::{Deserialize, Serialize};

use crate::app::EditorApp;
use crate::model::Document;
use crate::store::{FontStore, ImageStore};

#[derive(Serialize, Deserialize)]
pub struct ProjectImage {
    pub id: u64,
    pub png_base64: String,
}

#[derive(Serialize, Deserialize)]
pub struct ProjectFont {
    pub name: String,
    pub ttf_base64: String,
}

/// Vollständige AI-Anleitung, die in jede .boxdoc-Datei eingebettet wird,
/// damit opencode (und jeder andere Agent) sofort weiß, wie das Format
/// funktioniert und wie man es bearbeitet. BoxDoc ignoriert dieses Feld beim
/// Laden; es dient ausschließlich der Maschinen-Lesbarkeit.
pub const AI_HINT: &str = r#"BoxDoc-Dokument-Format (.boxdoc) — Anleitung für KI-Agenten
=======================================================

Diese Datei ist die komplette AI-Schnittstelle. Es gibt kein zusätzliches
Protokoll, keine API, keine Sockets. Du (die KI) liest und bearbeitest die
Datei direkt mit deinen normalen Datei-Werkzeugen — wie eine Quellcode-Datei.

Wenn BoxDoc läuft und diese Datei geöffnet hat, übernimmt es jede externe
Änderung automatisch (≤ 300 ms) als Undo-Schritt. Der Nutzer sieht sie live
und kann sie mit Strg+Z zurückrollen.

DATEIFORMAT
-----------
{
  "doc": {
    "format": "A4" | "A3" | "A5" | "Letter" | "Legal" |
              { "Custom": { "name": "<Name>", "w_mm": <mm>, "h_mm": <mm> } },
    "orientation": "Portrait" | "Landscape",
    "custom_formats": [ { "name": "<Name>", "w_mm": <mm>, "h_mm": <mm> }, ... ],
    "background": [r, g, b, a] | null,
    "pages": [ { "elements": [ <Element>, ... ] } ]
  },
  "fonts": [ { "name": "<key>", "ttf_base64": "<base64-TTF-Bytes>" } ],
  "images": [ { "id": <u64>, "png_base64": "<base64-PNG-Bytes>" } ]
}

Reihenfolge in der Datei: doc → _ai_hint → fonts → images.
Die Felder `fonts[]` und `images[]` enthalten nur große base64-Blöcke und
stehen daher am Ende — du kannst sie ignorieren, wenn du nur Layout/Text
änderst. Beim Speichern schreibt BoxDoc sie unverändert zurück.

EIGENES SEITENFORMAT
--------------------
Neben den Standardformaten ("A4", "A3", "A5", "Letter", "Legal") kannst du
ein eigenes Format setzen:
  "format": { "Custom": { "name": "Mein Format", "w_mm": 210.0, "h_mm": 297.0 } }
w_mm/h_mm sind Breite und Höhe in Millimetern (Hochformat; "orientation"
tauscht sie). Zusätzlich kannst du in "custom_formats" (Array, gleiche
Objekte) weitere benannte Formate hinterlegen — sie erscheinen dann im
Format-Menü und bleiben im Dokument gespeichert.

SEITENHINTERGRUND
-----------------
"background" im doc-Objekt: [r,g,b,a] mit a = Deckkraft (255 deckend) oder
null für GAR KEINEN Hintergrund — das SVG bekommt dann kein Hintergrund-
Rechteck und bleibt transparent (praktisch für Icons/Schemata). Fehlt das
Feld, gilt Weiß. Beispiel: "background": [255, 246, 230, 255].

ELEMENT (je nach "kind" sind verschiedene Felder relevant)
---------------------------------------------------------
{
  "id": <u64>,                       // stabil, niemals ändern beim Update
  "kind": "Text" | "Image" | "Rectangle" | "Line" | "Ellipse" | "Path",
  "x": <f32 pt>,                     // linke obere Ecke (unrotiert)
  "y": <f32 pt>,
  "w": <f32 pt>,                     // Breite
  "h": <f32 pt>,                     // Höhe (bei "Line" = 0)
  "rotation": <Grad>,                // gegen Uhrzeigersinn
  "text": "<Inhalt>",                // kann \n enthalten
  "font_size": <pt>,
  "font": "default"|"inter"|"roboto"|"lora"|"jetbrains"|"pacifico"|<custom-font-name>,
  "color": [r, g, b, a],             // 0..255; a=255 deckend
  "bold": <bool>, "italic": <bool>,
  "underline": <bool>, "strikethrough": <bool>,
  "align": "Left"|"Center"|"Right", "valign": "Top"|"Middle"|"Bottom",
  "indent": <pt>,
  "auto_height": <bool>,             // Text; true = h waechst mit dem Inhalt
  "crop": { "x":0.0, "y":0.0, "w":1.0, "h":1.0 },  // Image; normalisiert 0..1
  "image_w": <px>, "image_h": <px>,                // Image
  "fill_color": [r,g,b,a],           // Shape; Alpha 0 = transparent
  "stroke_width": <pt>,              // Shape; 0 = kein Rahmen
  "stroke_color": [r,g,b,a],         // Shape
  "corner_radius": <pt>,             // Rectangle
  "points": [[0.0,0.0], [1.0,0.5]],  // Path; auf die Box normalisiert (0..1)
  "handles": [[ix,iy, ox,oy], ...],  // Path; Kurvengriffe, leer/fehlend = Strecken
  "path_closed": <bool>              // Path; offen = nie gefuellt
}

ELEMENT-TYPEN
-------------
Text       : text, font_size, font, color, bold, italic, underline, strikethrough,
             align, valign, indent, auto_height
Rectangle  : fill_color, stroke_width, stroke_color, corner_radius
Line       : stroke_width, stroke_color (Linie = Box mit h=0 + rotation)
Ellipse    : fill_color, stroke_width, stroke_color (Kreis = w==h; corner_radius ignoriert)
Path       : points, handles, path_closed, fill_color, stroke_width, stroke_color
Image      : id (verweist auf images[].id), crop, image_w, image_h

PFADE UND KURVEN
----------------
"points[i]" = [nx, ny] mit 0,0 = linke obere und 1,1 = rechte untere Ecke der
Box. Der Punkt auf der Seite ist also x + nx*w bzw. y + ny*h (danach um die
Boxmitte um "rotation" gedreht). Verschieben, Skalieren und Drehen laufen
damit ueber x/y/w/h/rotation wie bei jeder anderen Form — zum Verschieben
aenderst du x/y, NICHT die Punkte.

"handles[i]" = [in_x, in_y, out_x, out_y] sind die beiden kubischen
Bezier-Kontrollpunkte des Knotens i, im SELBEN normalisierten Raum wie
"points" — also Positionen, keine Abstaende zum Stuetzpunkt. "in" gilt fuer
das Segment vor dem Knoten, "out" fuer das danach.

  - Eckknoten: beide Griffe liegen auf ihrem Stuetzpunkt, also
    [nx, ny, nx, ny]. Beide Nachbarsegmente werden dann Geraden.
  - Glatter Uebergang: in, Stuetzpunkt und out liegen auf einer Geraden,
    ueblich als in = p - t und out = p + t.
  - Zwischen zwei Knoten liegt eine Gerade, wenn out des einen und in des
    anderen jeweils auf ihrem Stuetzpunkt liegen — sonst eine kubische Kurve
    mit genau diesen beiden Kontrollpunkten.
  - Bei "path_closed": true gibt es zusaetzlich das Segment vom letzten
    zurueck zum ersten Knoten. Ein offener Pfad wird nie gefuellt.

REGEL: "handles" ist entweder leer/weggelassen (reiner Streckenzug — der
Normalfall fuer Vielecke) oder GENAU so lang wie "points". Andere Laengen
repariert BoxDoc beim Laden, indem es die fehlenden Knoten zu Ecken macht.

TEXT-HOEHE UND UMBRUCH
----------------------
Text bricht automatisch an der Boxbreite `w` um — genau so auf dem Bildschirm
wie im PDF (gemeinsames Layout, siehe src/text_layout.rs). Du musst also keine
Zeilenumbrueche selbst setzen; `\n` erzwingt lediglich einen zusaetzlichen.

- "auto_height": true  → `h` wird von BoxDoc aus dem Inhalt berechnet.
                         Ein von dir gesetztes `h` wird ueberschrieben.
                         "valign" hat keine Wirkung (Box = Inhaltshoehe).
- "auto_height": false → `h` gilt wie angegeben, "valign" richtet den Text
                         darin aus. Nutze das fuer Kaesten fester Groesse.

Fuer die meisten Faelle ist auto_height: true richtig. Setze `w` passend und
lass BoxDoc die Hoehe bestimmen.

TEXTAUSZEICHNUNG
----------------
Vier Schalter, alle unabhaengig kombinierbar und alle im PDF- wie im
SVG-Export enthalten:

  "bold"          fett
  "italic"        kursiv
  "underline"     unterstrichen
  "strikethrough" durchgestrichen

Sie gelten immer fuer das GANZE Element — es gibt keine Auszeichnung einzelner
Woerter innerhalb eines Textblocks. Willst du ein einzelnes Wort hervorheben,
mach daraus ein eigenes Text-Element und setze es daneben.

"bold"/"italic" waehlen einen echten Schriftschnitt, wo die Schrift einen
mitbringt (die System-Schriften auf dem Desktop). Wo nicht — bei den
eingebetteten Schriften und im Browser — werden sie nachgeahmt und sehen
etwas anders aus. Der Umbruch stimmt in beiden Faellen.

KOOORDINATENSYSTEM
------------------
- Maßeinheit: Punkt (1 pt = 1/72 Zoll; 1 Zoll = 25,4 mm)
- Ursprung: oben-links, y zeigt nach UNTEN
- A4 Hochformat: 595 x 842 pt | A4 Querformat: 842 x 595 pt
- 1 cm ≈ 28,3 pt | 1 mm ≈ 2,83 pt
- Z-Order: Elemente weiter hinten im Array liegen OBEN (werden zuletzt gezeichnet)

REGELN FÜR DIE KI
-----------------
1. Lies die Datei als JSON, verstehe das Dokument.
2. Ändere Felder direkt mit deinen normalen Edit-Tools.
3. IDs sind u64 und stabil. Beim Aktualisieren nur existierende IDs verwenden.
   Für neue Elemente: höchste vorhandene ID + 1.
4. Bilder (images, png_base64, image_w, image_h) UNVERÄNDERT lassen.
   Custom-Fonts (fonts[], ttf_base64) ebenfalls UNVERÄNDERT lassen.
5. Nach jedem Speichern übernimmt BoxDoc die Änderung automatisch (≤ 300 ms).
6. Der Nutzer kann mit Strg+Z zurückrollen; BoxDoc schreibt den alten Stand zurück.
7. Ungültiges JSON wird still ignoriert — teilschreibende Dateien sind unkritisch.

DESIGN-LEITFADEN
----------------
- Titel 28-36pt bold, Überschrift 18-22pt bold, Fließtext 10-12pt, Footer 8-9pt
- Zeilenabstand: ca. 1,3x Schriftgröße
- Seitenrand: mindestens ~50 pt (≈ 1,8 cm)
- Eine Hauptfarbe + eine Akzentfarbe für ein ruhiges Bild
- Aufzählungen mit "• " prefixen, eine Zeile pro Punkt
- Z-Order: Dekorationen (Hintergrundbalken) VORNE im Array, Text HINTEN

BEISPIEL: Neues Text-Element hinzufügen (an elements anhängen)
--------------------------------------------------------------
{
  "id": <nächste freie ID>, "kind": "Text",
  "x": 100.0, "y": 200.0, "w": 400.0, "h": 40.0, "rotation": 0.0,
  "text": "Neuer Absatz", "font_size": 14.0, "font": "default",
  "color": [20,20,20,255], "bold": false, "italic": false,
  "underline": false, "strikethrough": false,
  "align": "Left", "valign": "Top", "indent": 0.0, "auto_height": true,
  "crop": {"x":0,"y":0,"w":1,"h":1}, "image_w": 0, "image_h": 0,
  "fill_color": [80,140,220,60], "stroke_width": 2.0,
  "stroke_color": [40,100,180,255], "corner_radius": 0.0
}

Hinweis: Dieses Feld (_ai_hint) wird von BoxDoc beim Laden ignoriert.
Du kannst es beim Bearbeiten unverändert lassen oder löschen — BoxDoc wird
es beim nächsten Speichern wieder automatisch einfügen.

WEB / SERVER (falls das Doc auf boxdoc.at liegt)
------------------------------------------------
Wenn dieses Dokument auf einem BoxDoc-Server liegt (z. B. boxdoc.at),
kannst du es über einfache HTTP-Requests lesen und ändern:

  Lesen:    GET  https://boxdoc.at/<slug>
  Version:  GET  https://boxdoc.at/api.php?meta=<slug>
  Ändern:   PUT  https://boxdoc.at/api.php?put=<slug>&version=<n>
  Neu:      POST https://boxdoc.at/api.php?new=1&name=<slug>
  Auflisten:GET  https://boxdoc.at/api.php?list=1

Der <slug> ist der Dokumentname in der URL (z. B. "lebenslauf").

AUTHENTIFIZIERUNG
- Neue Dokumente bekommen standardmäßig einen Token (nicht öffentlich).
- Token gehört in den Header:  X-BoxDoc-Token: <token>
  Der ältere ?t=<token>-Parameter funktioniert weiterhin, ist aber schlechter
  (landet in Server-Logs, Referrern und der Browser-History).

NEBENLÄUFIGKEIT — WICHTIG
Jedes Dokument hat eine `version`. Beim Schreiben schickst du die Version mit,
auf der deine Änderung aufsetzt. Ist sie veraltet, antwortet der Server mit
HTTP 409 und liefert den aktuellen Stand im Feld "document" mit. Dann gilt:
neuen Stand übernehmen, deine Änderung erneut darauf anwenden, nochmal senden.
Schreibe NIE ohne Version — sonst überschreibst du fremde Arbeit.

Beispiele:
  curl -H 'X-BoxDoc-Token: abc' https://boxdoc.at/lebenslauf
  curl -H 'X-BoxDoc-Token: abc' 'https://boxdoc.at/api.php?meta=lebenslauf'
  curl -X PUT -H 'X-BoxDoc-Token: abc' --data-binary @doc.json \
       'https://boxdoc.at/api.php?put=lebenslauf&version=7'
  curl -X POST 'https://boxdoc.at/api.php?new=1&name=lebenslauf'
"#;

#[derive(Serialize, Deserialize)]
pub struct Project {
    pub doc: Document,
    /// Selbst-Dokumentation für KI-Agenten. Wird beim Speichern automatisch
    /// eingefügt und beim Laden ignoriert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _ai_hint: Option<String>,
    /// Eingebettete Custom-Fonts (base64-TTF). Bleiben nach _ai_hint, damit
    /// eine KI nicht durch base64-Blöcke scrollen muss, um das Layout zu
    /// verstehen.
    #[serde(default)]
    pub fonts: Vec<ProjectFont>,
    #[serde(default)]
    pub images: Vec<ProjectImage>,
}

impl Project {
    /// Erzeugt ein Project mit eingebettetem AI-Hint (für den regulären
    /// Speichern-Fluss).
    pub fn for_save(
        doc: Document,
        fonts: Vec<ProjectFont>,
        images: Vec<ProjectImage>,
    ) -> Self {
        Project {
            doc,
            _ai_hint: Some(AI_HINT.to_string()),
            fonts,
            images,
        }
    }
}

// ===========================================================================
// Native
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::PathBuf;

    use base64::Engine;

    use super::{
        Document, EditorApp, FontStore, ImageStore, Project, ProjectFont, ProjectImage,
    };

    type IoResult<T> = std::io::Result<T>;

    /// Überprüft, ob `path` innerhalb von `base` liegt (Path-Traversal-Schutz).
    ///
    /// Hinweis: Diese Funktion wird aktuell nicht gegen `current_dir` als `base`
    /// erzwungen, da BoxDoc Dateien aus `rfd::FileDialog` lädt — die Nutzer
    /// dürfen beliebige Pfade wählen. Sie steht als Defensiv-Check für
    /// Automatisierungszwecke (z. B. künftige Server-Variante) bereit.
    /// Die regulären `load_project`/`save_project`-Pfade prüfen lediglich, ob
    /// der Pfad canonicalisierbar ist (blockiert kaputte / symlink-basierte
    /// Pfade), brechen aber legitime Nutzungs-Wege nicht.
    #[allow(dead_code)]
    fn is_safe_path(path: &std::path::Path, base: &std::path::Path) -> bool {
        let ok_base = match base.canonicalize() {
            Ok(b) => b,
            Err(_) => return false,
        };
        match path.canonicalize() {
            Ok(p) => p.starts_with(&ok_base),
            Err(_) => false,
        }
    }

    /// Defensiv-Check: stellt sicher, dass der Pfad canonicalisierbar ist.
    /// Blockiert kaputte Pfade und das Auflösen problematischer Symlinks,
    /// ohne legitime File-Dialog-Pfade einzuschränken (siehe `is_safe_path`-
    /// Kommentar). Bei noch nicht existierenden Pfaden (z. B. erstes Speichern)
    /// wird das Elternverzeichnis geprüft.
    fn ensure_canonicalizable(path: &std::path::Path) -> std::io::Result<()> {
        let ok = if path.exists() {
            path.canonicalize().is_ok()
        } else {
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.canonicalize().is_ok())
                .unwrap_or(false)
        };
        if ok {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "path cannot be canonicalized",
            ))
        }
    }

    pub fn open_project_dialog(app: &mut EditorApp) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("BoxDoc-Projekt", &["boxdoc"])
            .set_title("Dokument öffnen")
            .pick_file()
        else {
            return;
        };
        match load_project(&path) {
            Ok((doc, images, fonts, next_id)) => {
                app.doc = doc;
                app.images = images;
                app.fonts = fonts;
                app.fonts_dirty = true;
                app.page_index = 0;
                app.next_id = next_id;
                app.clear_selection();
                app.editing = None;
                app.crop_mode = false;
                app.interaction = crate::app::Interaction::None;
                app.file_path = Some(path);
                app.modified = false;
                app.set_status("Dokument geöffnet.");
            }
            Err(e) => app.set_status(format!("Fehler beim Öffnen: {e}")),
        }
    }

    pub fn save_project_dialog(app: &mut EditorApp, save_as: bool) {
        let path = if !save_as {
            app.file_path.clone()
        } else {
            None
        };
        let path = match path {
            Some(p) => p,
            None => {
                let mut dlg = rfd::FileDialog::new()
                    .add_filter("BoxDoc-Projekt", &["boxdoc"])
                    .set_title("Dokument speichern");
                if let Some(start) = default_name(app) {
                    dlg = dlg.set_file_name(start);
                }
                match dlg.save_file() {
                    Some(p) => ensure_ext(p, "boxdoc"),
                    None => return,
                }
            }
        };

        match save_project(&path, app) {
            Ok(()) => {
                app.file_path = Some(path);
                app.modified = false;
                app.set_status("Gespeichert.");
            }
            Err(e) => app.set_status(format!("Fehler beim Speichern: {e}")),
        }
    }

    pub fn open_image_dialog(app: &mut EditorApp) {
        let files = rfd::FileDialog::new()
            .add_filter("Bild", &["png", "jpg", "jpeg", "bmp", "webp", "ico"])
            .set_title("Bild auswählen")
            .pick_files();
        for f in files.unwrap_or_default() {
            if let Ok(bytes) = std::fs::read(&f) {
                app.add_image_from_bytes(bytes, None);
            }
        }
    }

    /// Liefert ein Bild aus der Zwischenablage (Windows).
    #[cfg(target_os = "windows")]
    pub fn poll_clipboard_image() -> Option<Vec<u8>> {
        // Clipboard-Bild als BMP über eine temporäre Datei via PowerShell lesen.
        let temp = std::env::temp_dir().join("boxdoc_clip.png");
        let ps = format!(
            r#"$ErrorActionPreference='SilentlyContinue'; Add-Type -AssemblyName System.Windows.Forms; $img=[System.Windows.Forms.Clipboard]::GetImage(); if ($img) {{ $img.Save('{}') }}"#,
            temp.display()
        );
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .output();
        if temp.exists() {
            let bytes = std::fs::read(&temp).ok();
            let _ = std::fs::remove_file(&temp);
            bytes.filter(|b| b.starts_with(&[0x89, b'P', b'N', b'G']))
        } else {
            None
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn poll_clipboard_image() -> Option<Vec<u8>> {
        None
    }

    /// Schrift laden (TTF/OTF).
    ///
    /// Lag früher direkt im Menü-Code in `app.rs`. Hierher gezogen, weil die
    /// Web-Version dieselbe Aktion braucht und der Menü-Code sonst zwei
    /// plattformabhängige Zweige hätte.
    pub fn open_font_dialog(app: &mut EditorApp) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Schrift", &["ttf", "otf"])
            .set_title("Schriftdatei auswählen")
            .pick_file()
        else {
            return;
        };
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| String::from("custom"));
        match std::fs::read(&path) {
            Ok(bytes) => app.add_font_from_bytes(name, bytes),
            Err(e) => app.set_status(format!("Schrift laden fehlgeschlagen: {e}")),
        }
    }

    pub fn export_odt_dialog(app: &mut EditorApp) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("OpenDocument", &["odt"])
            .set_title("Als ODT exportieren");
        if let Some(start) = default_name(app) {
            let n = format!("{}.odt", start.trim_end_matches(".boxdoc"));
            dlg = dlg.set_file_name(n);
        }
        let Some(path) = dlg.save_file().map(|p| ensure_ext(p, "odt")) else {
            return;
        };
        match crate::odt::export(&path, &app.doc, &app.images) {
            Ok(()) => app.set_status(format!("ODT exportiert: {}", path.display())),
            Err(e) => app.set_status(format!("ODT-Export fehlgeschlagen: {e}")),
        }
    }

    pub fn import_odt_dialog(app: &mut EditorApp) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("OpenDocument", &["odt"])
            .set_title("ODT öffnen")
            .pick_file()
        else {
            return;
        };
        if let Err(e) = ensure_canonicalizable(&path) {
            app.set_status(format!("Fehler beim ODT-Lesen: {e}"));
            return;
        }
        match crate::odt::import(&path) {
            Ok((doc, images, next_id)) => {
                app.doc = doc;
                app.images = images;
                app.page_index = 0;
                app.next_id = next_id;
                app.clear_selection();
                app.editing = None;
                app.crop_mode = false;
                app.interaction = crate::app::Interaction::None;
                app.file_path = Some(path);
                app.modified = false;
                app.set_status("ODT geöffnet.");
            }
            Err(e) => app.set_status(format!("Fehler beim ODT-Lesen: {e}")),
        }
    }

    pub fn import_pdf_dialog(app: &mut EditorApp, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_title("PDF öffnen")
            .pick_file()
        else {
            return;
        };
        if let Err(e) = ensure_canonicalizable(&path) {
            app.set_status(format!("Fehler beim PDF-Lesen: {e}"));
            return;
        }
        // Der erste Aufruf kann dauern, weil pdfium-bundled die native
        // Bibliothek herunterlädt. Status vorher setzen, damit der Nutzer
        // Feedback bekommt.
        app.set_status("PDF wird importiert (erster Aufruf kann etwas dauern)…");
        match crate::pdf_import::import_pdf(&path) {
            Ok((mut doc, images, next_id)) => {
                // Textboxen an BoxDocs Schriften anpassen, sonst brechen Zeilen
                // um, die im PDF einzeilig waren.
                crate::pdf_import::fit_text_widths(ctx, &mut doc);
                app.doc = doc;
                app.images = images;
                app.fonts = Default::default();
                app.fonts_dirty = true;
                app.page_index = 0;
                app.next_id = next_id;
                app.clear_selection();
                app.editing = None;
                app.crop_mode = false;
                app.interaction = crate::app::Interaction::None;
                app.file_path = Some(path);
                app.modified = false;
                app.set_status("PDF geöffnet.");
            }
            Err(e) => app.set_status(format!("Fehler beim PDF-Lesen: {e}")),
        }
    }

    pub fn export_pdf_dialog(app: &mut crate::app::EditorApp, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_title("Als PDF exportieren");
        if let Some(stem) = app
            .file_path
            .as_ref()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        {
            dlg = dlg.set_file_name(format!("{stem}.pdf"));
        }
        let Some(path) = dlg.save_file() else { return; };
        let path = if path.extension().and_then(|e| e.to_str()) == Some("pdf") {
            path
        } else {
            path.with_extension("pdf")
        };
        let layouts = crate::printing::collect_layouts(ctx, &app.doc);
        match crate::printing::export_pdf(&path, &app.doc, &app.images, &layouts) {
            Ok(()) => app.set_status(format!("PDF exportiert: {}", path.display())),
            Err(e) => app.set_status(format!("PDF-Export fehlgeschlagen: {e}")),
        }
    }

    /// SVG-Export. `selection_only` exportiert nur die ausgewählten Objekte,
    /// sonst die aktuelle Seite.
    ///
    /// Warum die **aktuelle** Seite und nicht das ganze Dokument: SVG hat kein
    /// Seitenkonzept. Alle Seiten in eine Datei zu legen hieße, sie
    /// übereinanderzustapeln — das will niemand. Mehrere Dateien stillschweigend
    /// anzulegen wäre eine Überraschung. Also genau das, was auf dem Schirm ist.
    pub fn export_svg_dialog(
        app: &mut crate::app::EditorApp,
        ctx: &egui::Context,
        selection_only: bool,
    ) {
        let scope = if selection_only {
            if app.selection.is_empty() {
                app.set_status("Nichts ausgewählt — es gibt nichts zu exportieren.");
                return;
            }
            crate::svg::Scope::Selection {
                page: app.page_index,
                ids: app.selection.clone(),
            }
        } else {
            crate::svg::Scope::Page(app.page_index)
        };

        let title = if selection_only {
            "Auswahl als SVG exportieren"
        } else {
            "Seite als SVG exportieren"
        };
        let mut dlg = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .set_title(title);
        if let Some(stem) = app
            .file_path
            .as_ref()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        {
            // Seitennummer bzw. „auswahl" mit in den Namen: Sonst überschreibt
            // der zweite Export stillschweigend den ersten.
            let suffix = if selection_only {
                String::from("auswahl")
            } else {
                format!("seite{}", app.page_index + 1)
            };
            dlg = dlg.set_file_name(format!("{stem}_{suffix}.svg"));
        }
        let Some(path) = dlg.save_file() else { return };
        let path = if path.extension().and_then(|e| e.to_str()) == Some("svg") {
            path
        } else {
            path.with_extension("svg")
        };

        let layouts = crate::printing::collect_layouts(ctx, &app.doc);
        match crate::svg::export_svg(&path, &app.doc, &app.images, &layouts, &scope) {
            Ok(()) => {
                let what = if selection_only {
                    format!("{} Objekt(e)", app.selection.len())
                } else {
                    format!("Seite {}", app.page_index + 1)
                };
                app.set_status(format!("SVG exportiert ({what}): {}", path.display()));
            }
            Err(e) => app.set_status(format!("SVG-Export fehlgeschlagen: {e}")),
        }
    }

    /// Speichert die ausgewählten Bilder als Bilddateien (PNG oder JPEG).
    ///
    /// Gespeichert wird die **Bilddatei**, nicht das Seitenobjekt: in
    /// Originalauflösung, auf den Crop beschnitten, ohne Drehung und ohne
    /// Seitenhintergrund (siehe `ImageStore::cropped_rgba`). Wer die
    /// Seitendarstellung will, nimmt den SVG- oder PDF-Export.
    ///
    /// Bei mehreren ausgewählten Bildern fragt der Dialog nach einem Ordner —
    /// ein Speichern-Dialog pro Bild wäre bei zehn Bildern eine Zumutung.
    pub fn export_image_dialog(app: &mut EditorApp) {
        let images = app.selected_images();
        if images.is_empty() {
            app.set_status("Kein Bild ausgewählt.");
            return;
        }
        let stem = app
            .file_path
            .as_ref()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| String::from("boxdoc"));

        if images.len() == 1 {
            let el = &images[0];
            let mut dlg = rfd::FileDialog::new()
                .add_filter("PNG", &["png"])
                .add_filter("JPEG", &["jpg", "jpeg"])
                .set_title("Bild speichern")
                .set_file_name(format!("{stem}_bild.png"));
            if let Some(dir) = app.file_path.as_ref().and_then(|p| p.parent()) {
                dlg = dlg.set_directory(dir);
            }
            let Some(path) = dlg.save_file() else { return };
            let path = if path.extension().is_some() {
                path
            } else {
                path.with_extension("png")
            };
            match write_element_image(app, el, &path) {
                Ok(()) => app.set_status(format!("Bild gespeichert: {}", path.display())),
                Err(e) => app.set_status(format!("Bild speichern fehlgeschlagen: {e}")),
            }
            return;
        }

        let Some(dir) = rfd::FileDialog::new()
            .set_title("Ordner für die Bilder wählen")
            .pick_folder()
        else {
            return;
        };
        let mut saved = 0usize;
        let mut first_error = None;
        for (i, el) in images.iter().enumerate() {
            // Vorhandene Dateien nicht überschreiben: Ein Ordner-Export soll
            // nicht stillschweigend die Bilder des letzten Exports ersetzen.
            let path = unique_path(&dir, &format!("{stem}_bild{}", i + 1), "png");
            match write_element_image(app, el, &path) {
                Ok(()) => saved += 1,
                Err(e) => {
                    if first_error.is_none() {
                        first_error = Some(e.to_string());
                    }
                }
            }
        }
        match first_error {
            None => app.set_status(format!("{saved} Bilder gespeichert: {}", dir.display())),
            Some(e) => app.set_status(format!(
                "{saved} von {} Bildern gespeichert — Fehler: {e}",
                images.len()
            )),
        }
    }

    /// Schreibt ein einzelnes Bild-Element; das Format folgt der Dateiendung.
    fn write_element_image(
        app: &EditorApp,
        el: &crate::model::Element,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        ensure_canonicalizable(path)?;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let bytes = if ext == "jpg" || ext == "jpeg" {
            app.images.element_jpeg(el, 92)
        } else {
            app.images.element_png(el)
        };
        let Some(bytes) = bytes else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Bilddaten konnten nicht gelesen werden",
            ));
        };
        std::fs::write(path, bytes)
    }

    /// `dir/name.ext`, bei Bedarf mit `_2`, `_3`, … bis der Name frei ist.
    fn unique_path(dir: &std::path::Path, name: &str, ext: &str) -> PathBuf {
        let mut path = dir.join(format!("{name}.{ext}"));
        let mut n = 2;
        while path.exists() && n < 1000 {
            path = dir.join(format!("{name}_{n}.{ext}"));
            n += 1;
        }
        path
    }

    /// Legt ein Bild in die System-Zwischenablage, damit es in Paint, Word
    /// oder einem Chat eingefügt werden kann.
    ///
    /// `png` behält die Transparenz (Format „PNG", das z. B. GIMP und
    /// Inkscape lesen), `flat_png` ist dieselbe Grafik auf weißem Grund für
    /// das klassische Bitmap-Format — Windows-Bitmaps kennen kein Alpha, und
    /// ohne die Weiß-Variante klebt Paint transparente Bereiche schwarz zu.
    ///
    /// Der Weg über PowerShell ist derselbe wie beim Lesen der Zwischenablage
    /// (`poll_clipboard_image`): Er kostet nichts an Abhängigkeiten und
    /// braucht keine eigene Fensterklasse. Der Aufruf läuft synchron und
    /// blockiert dabei rund eine Sekunde (fast alles davon PowerShell-Start).
    /// Das ist bewusst so: Nur so kann die Statuszeile ehrlich sagen, ob das
    /// Bild wirklich in der Zwischenablage liegt — ein Hintergrund-Thread
    /// müsste raten oder eine spätere Meldung über die aktuelle schreiben.
    #[cfg(target_os = "windows")]
    pub fn set_clipboard_image(png: &[u8], flat_png: &[u8]) -> bool {
        let dir = std::env::temp_dir();
        let p_png = dir.join("boxdoc_clip_out.png");
        let p_flat = dir.join("boxdoc_clip_out_flat.png");
        if std::fs::write(&p_png, png).is_err() || std::fs::write(&p_flat, flat_png).is_err() {
            let _ = std::fs::remove_file(&p_png);
            let _ = std::fs::remove_file(&p_flat);
            return false;
        }
        // Beide Bilder werden über einen MemoryStream geladen, nicht über
        // Image::FromFile — sonst hält PowerShell die Dateien offen und das
        // Aufräumen unten schlägt fehl.
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             Add-Type -AssemblyName System.Windows.Forms; \
             Add-Type -AssemblyName System.Drawing; \
             $msPng=New-Object System.IO.MemoryStream(,[System.IO.File]::ReadAllBytes('{png}')); \
             $msBmp=New-Object System.IO.MemoryStream(,[System.IO.File]::ReadAllBytes('{flat}')); \
             $img=[System.Drawing.Image]::FromStream($msBmp); \
             $data=New-Object System.Windows.Forms.DataObject; \
             $data.SetData('PNG',$false,$msPng); \
             $data.SetImage($img); \
             [System.Windows.Forms.Clipboard]::SetDataObject($data,$true,10,100)",
            png = ps_quote(&p_png.display().to_string()),
            flat = ps_quote(&p_flat.display().to_string()),
        );
        let ok = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-STA",
                "-Command",
                &script,
            ])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        let _ = std::fs::remove_file(&p_png);
        let _ = std::fs::remove_file(&p_flat);
        ok
    }

    /// Escaped einen Pfad für ein einfach-gequotetes PowerShell-Literal.
    #[cfg(target_os = "windows")]
    fn ps_quote(s: &str) -> String {
        s.replace('\'', "''")
    }

    #[cfg(not(target_os = "windows"))]
    pub fn set_clipboard_image(_png: &[u8], _flat_png: &[u8]) -> bool {
        // Nur Windows: Auf Linux/macOS hängt der Weg vom Sitzungstyp ab
        // (xclip/wl-copy/pbcopy). Ohne getestete Umsetzung lieber ehrlich
        // „nicht unterstützt" melden als still nichts tun.
        false
    }

    pub fn print_dialog(app: &mut crate::app::EditorApp, ctx: &egui::Context) {
        let dir = std::env::temp_dir();
        let path = dir.join("boxdoc_drucken.pdf");
        let layouts = crate::printing::collect_layouts(ctx, &app.doc);
        match crate::printing::export_pdf(&path, &app.doc, &app.images, &layouts) {
            Ok(()) => {
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd")
                    .arg("/C")
                    .arg("start")
                    .arg("")
                    .arg(path.display().to_string())
                    .spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&path).spawn();
                app.set_status("PDF erzeugt und Drucker-Dialog geöffnet.");
            }
            Err(e) => app.set_status(format!("Drucken fehlgeschlagen: {e}")),
        }
    }


    // Hinweis: Der PDF-Export läuft nicht mehr über dieses Modul, sondern über
    // `printing::export_pdf_dialog`. Grund: Er braucht den `egui::Context`, um
    // den Text mit demselben Schriftsystem zu layouten, das der Canvas benutzt
    // (siehe `text_layout`). Ohne diese gemeinsame Layout-Quelle wäre der
    // Zeilenumbruch im PDF wieder ein anderer als auf dem Bildschirm.

    fn default_name(app: &EditorApp) -> Option<String> {
        Some(
            app.file_path
                .as_ref()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                .unwrap_or_else(|| String::from("dokument")),
        )
    }

    fn ensure_ext(path: PathBuf, ext: &str) -> PathBuf {
        if path.extension().and_then(|e| e.to_str()) == Some(ext) {
            path
        } else {
            path.with_extension(ext)
        }
    }

    pub fn save_project(path: &std::path::Path, app: &EditorApp) -> std::io::Result<()> {
        ensure_canonicalizable(path)?;
        let fonts: Vec<ProjectFont> = app
            .fonts
            .map
            .iter()
            .map(|(name, e)| ProjectFont {
                name: name.clone(),
                ttf_base64: base64::engine::general_purpose::STANDARD.encode(&e.ttf),
            })
            .collect();
        let images: Vec<ProjectImage> = app
            .images
            .map
            .iter()
            .map(|(id, e)| ProjectImage {
                id: *id,
                png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
            })
            .collect();
        let project = Project::for_save(app.doc.clone(), fonts, images);
        let json = serde_json::to_string_pretty(&project)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    pub fn load_project(
        path: &std::path::Path,
    ) -> std::io::Result<(Document, ImageStore, FontStore, u64)> {
        ensure_canonicalizable(path)?;
        let json = std::fs::read_to_string(path)?;
        let project: Project = serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut images = ImageStore::default();
        let mut fonts = FontStore::default();
        let mut max_id = 0u64;
        for img in project.images {
            let png = base64::engine::general_purpose::STANDARD
                .decode(&img.png_base64)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let dim = image::load_from_memory(&png)
                .map(|i| (i.width(), i.height()))
                .unwrap_or((0, 0));
            images.insert(img.id, png, dim);
        }
        for pf in project.fonts {
            let ttf = base64::engine::general_purpose::STANDARD
                .decode(&pf.ttf_base64)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            fonts.insert(pf.name, ttf);
        }
        for page in &project.doc.pages {
            for el in &page.elements {
                max_id = max_id.max(el.id);
            }
        }
        Ok((project.doc, images, fonts, max_id + 1))
    }
}

#[cfg(target_arch = "wasm32")]
mod web_impl {
    use super::{EditorApp, Project, ProjectFont, ProjectImage};
    use base64::Engine;

    pub fn open_project_dialog(app: &mut EditorApp) {
        super::trigger_project_file_input();
        app.set_status(".boxdoc-Datei auswählen…");
    }

    /// Speichern im Browser: Ziel ist ein Dokument auf dem Server.
    ///
    /// Auf Native schreibt „Speichern" in die gebundene Datei und „Speichern
    /// unter…" fragt nach einer neuen. Im Browser ist die Entsprechung ein
    /// Serverdokument: gebunden → hochladen, ungebunden → nach einem Namen
    /// fragen. Der Download ist bewusst nicht mehr dieser Weg — er erzeugt
    /// eine Kopie, die von jeder weiteren Änderung nichts mehr mitbekommt,
    /// und liegt deshalb als `download_project` neben den Exporten.
    pub fn save_project_dialog(app: &mut EditorApp, save_as: bool) {
        if save_as {
            app.open_server_save_dialog();
        } else {
            app.save_to_server_now();
        }
    }

    /// Lädt den aktuellen Stand als `.boxdoc`-Datei herunter.
    pub fn download_project(app: &mut EditorApp) {
        let fonts: Vec<ProjectFont> = app
            .fonts
            .map
            .iter()
            .map(|(name, e)| ProjectFont {
                name: name.clone(),
                ttf_base64: base64::engine::general_purpose::STANDARD.encode(&e.ttf),
            })
            .collect();
        let images: Vec<ProjectImage> = app
            .images
            .map
            .iter()
            .map(|(id, e)| ProjectImage {
                id: *id,
                png_base64: base64::engine::general_purpose::STANDARD.encode(&e.png),
            })
            .collect();
        let project = Project::for_save(app.doc.clone(), fonts, images);
        match serde_json::to_string_pretty(&project) {
            Ok(json) => {
                let name = match &app.web_doc {
                    Some(w) => format!("{}.boxdoc", w.slug),
                    None => format!("{}.boxdoc", stem(app)),
                };
                download_file(&json, &name, "application/json");
                // `modified` nur löschen, wenn der Download tatsächlich das
                // einzige Ziel ist. Hängt das Dokument am Server, wäre der
                // Stand dort weiterhin älter — und ein grüner Punkt in der
                // Statuszeile würde das Gegenteil behaupten.
                if app.web_doc.is_none() {
                    app.modified = false;
                }
                app.set_status(format!("Heruntergeladen: {name}"));
            }
            Err(e) => app.set_status(format!("Fehler: {e}")),
        }
    }

    pub fn open_image_dialog(app: &mut EditorApp) {
        super::trigger_file_input();
        app.set_status("Bild auswählen…");
    }

    pub fn poll_clipboard_image() -> Option<Vec<u8>> {
        None // auf Web übernimmt der paste-Listener + take_pending_image
    }

    /// Bild-Export im Browser: Download je ausgewähltem Bild.
    ///
    /// Native fragt bei mehreren Bildern nach einem Ordner; im Browser gibt es
    /// keinen Ordner, also ein Download pro Bild — durchnummeriert wie dort.
    pub fn export_image_dialog(app: &mut EditorApp) {
        let images = app.selected_images();
        if images.is_empty() {
            app.set_status("Kein Bild ausgewählt.");
            return;
        }
        let stem = stem(app);
        let single = images.len() == 1;
        let mut saved = 0usize;
        for (i, el) in images.iter().enumerate() {
            let Some(png) = app.images.element_png(el) else {
                continue;
            };
            let name = if single {
                format!("{stem}_bild.png")
            } else {
                format!("{stem}_bild{}.png", i + 1)
            };
            download_bytes(&png, &name, "image/png");
            saved += 1;
        }
        match saved {
            0 => app.set_status("Bilddaten konnten nicht gelesen werden."),
            1 if single => app.set_status("Bild heruntergeladen."),
            n => app.set_status(format!("{n} von {} Bildern heruntergeladen.", images.len())),
        }
    }

    /// Im Browser gibt es keinen synchronen Weg, ein Bild in die
    /// Zwischenablage zu schreiben (`navigator.clipboard.write` ist async und
    /// braucht eine Nutzergeste). Strg+C kopiert dort weiterhin nur die
    /// BoxDoc-Objekte.
    pub fn set_clipboard_image(_png: &[u8], _flat_png: &[u8]) -> bool {
        false
    }

    /// Namensstamm für Downloads — aus dem geöffneten Dokument, sonst
    /// „dokument". Auf Web ist `file_path` nur ein Anzeigename (es gibt keinen
    /// Dateisystem-Pfad), taugt als Stamm aber genauso.
    fn stem(app: &EditorApp) -> String {
        app.file_path
            .as_ref()
            .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| String::from("dokument"))
    }

    pub fn export_odt_dialog(app: &mut EditorApp) {
        match crate::odt::export_to_bytes(&app.doc, &app.images) {
            Ok(bytes) => {
                let name = format!("{}.odt", stem(app));
                download_bytes(
                    &bytes,
                    &name,
                    "application/vnd.oasis.opendocument.text",
                );
                app.set_status(format!("ODT heruntergeladen: {name}"));
            }
            Err(e) => app.set_status(format!("ODT-Export fehlgeschlagen: {e}")),
        }
    }

    pub fn import_odt_dialog(app: &mut EditorApp) {
        super::trigger_odt_file_input();
        app.set_status("ODT-Datei auswählen…");
    }

    /// PDF-Import bleibt Desktop-exklusiv: er setzt auf pdfium auf, eine
    /// native C++-Bibliothek. Die gibt es im Browser nicht, und ein zweiter,
    /// eigener PDF-Parser für Web würde andere Ergebnisse liefern als die EXE —
    /// genau die Art Abweichung, die diese Angleichung vermeiden soll.
    pub fn import_pdf_dialog(app: &mut EditorApp, _ctx: &egui::Context) {
        app.set_status("PDF-Import gibt es nur in der Desktop-Version (braucht pdfium).");
    }

    pub fn export_pdf_dialog(app: &mut EditorApp, ctx: &egui::Context) {
        let layouts = crate::printing::collect_layouts(ctx, &app.doc);
        match crate::printing::pdf_bytes(&app.doc, &app.images, &layouts) {
            Ok(bytes) => {
                let name = format!("{}.pdf", stem(app));
                download_bytes(&bytes, &name, "application/pdf");
                app.set_status(format!("PDF heruntergeladen: {name}"));
            }
            Err(e) => app.set_status(format!("PDF-Export fehlgeschlagen: {e}")),
        }
    }

    /// SVG-Export als Download. `selection_only` exportiert nur die
    /// ausgewählten Objekte, sonst die aktuelle Seite — dieselbe Regel wie auf
    /// Native (SVG kennt keine Seiten, also genau das, was auf dem Schirm ist).
    pub fn export_svg_dialog(
        app: &mut EditorApp,
        ctx: &egui::Context,
        selection_only: bool,
    ) {
        let scope = if selection_only {
            if app.selection.is_empty() {
                app.set_status("Nichts ausgewählt — es gibt nichts zu exportieren.");
                return;
            }
            crate::svg::Scope::Selection {
                page: app.page_index,
                ids: app.selection.clone(),
            }
        } else {
            crate::svg::Scope::Page(app.page_index)
        };

        let layouts = crate::printing::collect_layouts(ctx, &app.doc);
        match crate::svg::svg_string(&app.doc, &app.images, &layouts, &scope) {
            Ok(svg) => {
                // Seitennummer bzw. „auswahl" mit in den Namen, damit der
                // zweite Export nicht wie der erste heißt.
                let suffix = if selection_only {
                    String::from("auswahl")
                } else {
                    format!("seite{}", app.page_index + 1)
                };
                let name = format!("{}_{suffix}.svg", stem(app));
                download_file(&svg, &name, "image/svg+xml");
                let what = if selection_only {
                    format!("{} Objekt(e)", app.selection.len())
                } else {
                    format!("Seite {}", app.page_index + 1)
                };
                app.set_status(format!("SVG heruntergeladen ({what}): {name}"));
            }
            Err(e) => app.set_status(format!("SVG-Export fehlgeschlagen: {e}")),
        }
    }

    /// Drucken läuft im Browser über den Druckdialog des Browsers — und der
    /// druckt die HTML-Seite, also den Canvas als Pixelbrei. Der PDF-Export
    /// ist der richtige Weg und liefert dasselbe Ergebnis wie die EXE.
    pub fn print_dialog(app: &mut EditorApp, _ctx: &egui::Context) {
        app.set_status("Drucken: bitte PDF exportieren und das PDF drucken.");
    }

    /// Schrift laden — Datei-Dialog des Browsers, Ergebnis landet asynchron
    /// in `PENDING_FONT`.
    pub fn open_font_dialog(app: &mut EditorApp) {
        super::trigger_font_file_input();
        app.set_status("Schriftdatei auswählen…");
    }

    fn download_file(content: &str, filename: &str, mime: &str) {
        download_bytes(content.as_bytes(), filename, mime);
    }

    fn download_bytes(bytes: &[u8], filename: &str, mime: &str) {
        use js_sys::Uint8Array;
        use wasm_bindgen::JsCast;
        use web_sys::{Blob, BlobPropertyBag};

        let array = Uint8Array::new_with_length(bytes.len() as u32);
        array.copy_from(bytes);

        let mut props = BlobPropertyBag::new();
        props.type_(mime);
        let blob = Blob::new_with_u8_array_sequence_and_options(
            &js_sys::Array::of1(&array.into()),
            &props,
        )
        .unwrap();

        let url = web_sys::Url::create_object_url_with_blob(&blob).unwrap();

        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        let anchor = document
            .create_element("a")
            .unwrap()
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .unwrap();
        anchor.set_href(&url);
        anchor.set_download(filename);
        anchor.click();
        web_sys::Url::revoke_object_url(&url).ok();
    }
}

// ===========================================================================
// Öffentliche API — dispatch je nach Plattform
// ===========================================================================

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
pub use web_impl::*;

// ===========================================================================
// Web: Globaler Puffer für asynchron geladene Dateien
// ===========================================================================

#[cfg(target_arch = "wasm32")]
static PENDING_IMAGE: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

#[cfg(target_arch = "wasm32")]
static PENDING_PROJECT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Rohbytes einer gewählten ODT-Datei.
#[cfg(target_arch = "wasm32")]
static PENDING_ODT: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

/// Name + Rohbytes einer gewählten Schriftdatei (TTF/OTF).
#[cfg(target_arch = "wasm32")]
static PENDING_FONT: std::sync::Mutex<Option<(String, Vec<u8>)>> = std::sync::Mutex::new(None);

/// Liest die gewählte Datei binär ein und legt die Bytes über `store` ab.
///
/// Die drei Binär-Inputs (Bild, ODT, Schrift) unterschieden sich vorher nur in
/// zwei Zeilen — Accept-Filter und Zielpuffer. Einmal geschrieben statt dreimal
/// kopiert, sonst driften die Fassungen auseinander.
#[cfg(target_arch = "wasm32")]
fn trigger_binary_file_input(
    accept: &str,
    store: impl Fn(&web_sys::File, Vec<u8>) + 'static,
) {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlInputElement;

    let document = web_sys::window().unwrap().document().unwrap();
    let input = document
        .create_element("input")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    input.set_type("file");
    input.set_accept(accept);
    input.set_multiple(false);

    // `Rc`, weil der äußere change-Handler ein `FnMut` sein muss (er kann
    // mehrfach feuern) und `store` daher nicht in den inneren load-Handler
    // *verschoben* werden darf, sondern geteilt wird.
    let store = std::rc::Rc::new(store);

    let onchange: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> =
        wasm_bindgen::closure::Closure::new(move |event: web_sys::Event| {
            let input: Option<HtmlInputElement> = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
            let Some(input) = input else { return };
            let Some(file) = input.files().and_then(|f| f.get(0)) else {
                return;
            };

            let reader = web_sys::FileReader::new().unwrap();
            let _ = reader.read_as_array_buffer(&file);

            let onload: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> = {
                let reader = reader.clone();
                let store = store.clone();
                wasm_bindgen::closure::Closure::new(move |_e: web_sys::Event| {
                    if let Ok(result) = reader.result() {
                        let bytes = js_sys::Uint8Array::new(&result).to_vec();
                        store(&file, bytes);
                    }
                })
            };
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
        });

    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    input.click();
}

#[cfg(target_arch = "wasm32")]
pub fn trigger_odt_file_input() {
    trigger_binary_file_input(
        ".odt,application/vnd.oasis.opendocument.text",
        |_file, bytes| {
            if let Ok(mut p) = PENDING_ODT.lock() {
                *p = Some(bytes);
            }
        },
    );
}

#[cfg(target_arch = "wasm32")]
pub fn trigger_font_file_input() {
    trigger_binary_file_input(".ttf,.otf,font/ttf,font/otf", |file, bytes| {
        // Der Schriftname ist der Dateiname ohne Endung — genauso wie auf
        // Native (`path.file_stem()`).
        let name = file.name();
        let stem = name
            .rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or(name);
        let stem = if stem.is_empty() {
            String::from("custom")
        } else {
            stem
        };
        if let Ok(mut p) = PENDING_FONT.lock() {
            *p = Some((stem, bytes));
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn trigger_file_input() {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlInputElement;

    let document = web_sys::window().unwrap().document().unwrap();
    let input = document
        .create_element("input")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    input.set_type("file");
    input.set_accept("image/png,image/jpeg,image/bmp,image/webp");
    input.set_multiple(false);

    let onchange: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> =
        wasm_bindgen::closure::Closure::new(move |event: web_sys::Event| {
            let input: Option<HtmlInputElement> = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
            let Some(input) = input else { return };
            let Some(file) = input.files().and_then(|f| f.get(0)) else {
                return;
            };

            let reader = web_sys::FileReader::new().unwrap();
            let _ = reader.read_as_array_buffer(&file);

            let onload: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> = {
                let reader = reader.clone();
                wasm_bindgen::closure::Closure::new(move |_e: web_sys::Event| {
                    if let Ok(result) = reader.result() {
                        let uint8 = js_sys::Uint8Array::new(&result).to_vec();
                        if let Ok(mut p) = PENDING_IMAGE.lock() {
                            *p = Some(uint8);
                        }
                    }
                })
            };
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
        });

    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    input.click();
}

#[cfg(target_arch = "wasm32")]
pub fn trigger_project_file_input() {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlInputElement;

    let document = web_sys::window().unwrap().document().unwrap();
    let input = document
        .create_element("input")
        .unwrap()
        .dyn_into::<HtmlInputElement>()
        .unwrap();
    input.set_type("file");
    input.set_accept(".boxdoc,application/json");
    input.set_multiple(false);

    let onchange: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> =
        wasm_bindgen::closure::Closure::new(move |event: web_sys::Event| {
            let input: Option<HtmlInputElement> = event
                .target()
                .and_then(|t| t.dyn_into::<HtmlInputElement>().ok());
            let Some(input) = input else { return };
            let Some(file) = input.files().and_then(|f| f.get(0)) else {
                return;
            };

            let reader = web_sys::FileReader::new().unwrap();
            let _ = reader.read_as_text(&file);

            let onload: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> = {
                let reader = reader.clone();
                wasm_bindgen::closure::Closure::new(move |_e: web_sys::Event| {
                    if let Ok(result) = reader.result() {
                        if let Some(text) = result.as_string() {
                            if let Ok(mut p) = PENDING_PROJECT.lock() {
                                *p = Some(text);
                            }
                        }
                    }
                })
            };
            reader.set_onload(Some(onload.as_ref().unchecked_ref()));
            onload.forget();
        });

    input.set_onchange(Some(onchange.as_ref().unchecked_ref()));
    onchange.forget();
    input.click();
}

/// Registriert einen paste-Listener auf Window-Ebene, der Bilder aus der
/// Zwischenablage abfängt. Muss einmal beim Start aufgerufen werden.
#[cfg(target_arch = "wasm32")]
pub fn install_clipboard_paste_listener() {
    use wasm_bindgen::JsCast;
    let cb: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> =
        wasm_bindgen::closure::Closure::new(|event: web_sys::Event| {
            let Some(evt) = event.dyn_ref::<web_sys::ClipboardEvent>() else {
                return;
            };
            let Some(data) = evt.clipboard_data() else {
                return;
            };
            let Some(files) = data.files() else { return };
            for i in 0..files.length() {
                let Some(file) = files.get(i) else { continue };
                if file.type_().starts_with("image/") {
                    let reader = web_sys::FileReader::new().unwrap();
                    let _ = reader.read_as_array_buffer(&file);
                    let onload: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)> = {
                        let reader = reader.clone();
                        wasm_bindgen::closure::Closure::new(move |_e: web_sys::Event| {
                            if let Ok(result) = reader.result() {
                                let bytes = js_sys::Uint8Array::new(&result).to_vec();
                                if let Ok(mut p) = PENDING_IMAGE.lock() {
                                    *p = Some(bytes);
                                }
                            }
                        })
                    };
                    reader.set_onload(Some(onload.as_ref().unchecked_ref()));
                    onload.forget();
                    break;
                }
            }
        });
    let window = web_sys::window().unwrap();
    let _ = window.add_event_listener_with_callback("paste", cb.as_ref().unchecked_ref());
    cb.forget();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn install_clipboard_paste_listener() {}

/// Auf Web: gibt die zuletzt geladene Projekt-JSON zurück und leert den Puffer.
#[cfg(target_arch = "wasm32")]
pub fn take_pending_project() -> Option<String> {
    PENDING_PROJECT.lock().unwrap().take()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_pending_project() -> Option<String> {
    None
}

/// Auf Web: gibt die zuletzt geladenen Bild-Bytes zurück (falls vorhanden)
/// und leert den Puffer. Auf Native immer `None`.
#[cfg(target_arch = "wasm32")]
pub fn take_pending_image() -> Option<Vec<u8>> {
    PENDING_IMAGE.lock().unwrap().take()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_pending_image() -> Option<Vec<u8>> {
    None
}

/// Auf Web: Bytes der zuletzt gewählten ODT-Datei.
#[cfg(target_arch = "wasm32")]
pub fn take_pending_odt() -> Option<Vec<u8>> {
    PENDING_ODT.lock().unwrap().take()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_pending_odt() -> Option<Vec<u8>> {
    None
}

/// Auf Web: Name und Bytes der zuletzt gewählten Schriftdatei.
#[cfg(target_arch = "wasm32")]
pub fn take_pending_font() -> Option<(String, Vec<u8>)> {
    PENDING_FONT.lock().unwrap().take()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn take_pending_font() -> Option<(String, Vec<u8>)> {
    None
}
