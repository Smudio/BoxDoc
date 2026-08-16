# BoxDoc — Kritischer Review

> Stand: 14. August 2026 · Basis: `main` @ `ed79bcd` + uncommittete Z-Order/Snap-Arbeit
> Ziel-Massstab: *„Extrem zuverlaessiger, simpler Dokumenten-Editor"*

Dieses Dokument ist eine Bestandsaufnahme, kein Planungsdokument.
Verbindliche Planung bleibt [`ROADMAP.md`](ROADMAP.md).

---

## Gesamturteil

BoxDoc ist ein solides, ueberdurchschnittlich sauber geschriebenes Projekt — aber
derzeit weit vom Anspruch „bester Dokumenten-Editor" entfernt. Nicht wegen
fehlender Features, sondern weil die **Grundlagen der Verlaesslichkeit** fehlen.

Ein Dokumenten-Editor wird nicht an seinem Funktionsumfang gemessen, sondern
daran, dass man ihm seine Arbeit anvertrauen kann.

**Positiv hervorzuheben:**

- Die Dokumentation ist **ehrlich**. `ARCHITECTURE.md` korrigiert aktiv frueher
  aufgestellte Falschbehauptungen. Das ist selten und wertvoll.
- Der Code ist dicht kommentiert, Modulgrenzen sind vernuenftig gezogen.
- Die AI-first-Idee (die Datei selbst *ist* die Schnittstelle) ist konzeptionell
  stark und ungewoehnlich konsequent umgesetzt.
- `cargo check` laeuft ohne Fehler durch.

**Kern des Problems:** Darunter liegen Fehler, die Nutzerarbeit *verlieren*.

---

## 1. Blocker — Datenverlust

### 1.1 Kein Schutz gegen ungespeicherte Aenderungen · KRITISCH

`src/main.rs` hat kein Close-Handling. `new_document()` (`app.rs:389`) und
„Oeffnen…" pruefen `self.modified` nicht.

> **Fenster schliessen = alles weg, ohne Nachfrage.**

Das `modified`-Flag existiert bereits — es wird nur nirgends ausgewertet.

### 1.2 Kein Strg+S · KRITISCH

Vollstaendige Liste der Tastenkuerzel im Code:

| Vorhanden | Fehlt |
|---|---|
| `Strg+C`, `Strg+V`, `Strg+Z`, `Strg+Y`, `Entf`, `Esc`, Pfeiltasten, `L` | `Strg+S`, `Strg+O`, `Strg+N`, `Strg+A`, `Strg+X`, `Strg+D`, `Strg+P` |

Speichern geht ausschliesslich ueber Menue → Datei → Speichern. Die Menueeintraege
zeigen ausserdem keine Shortcuts an.

### 1.3 Texteingabe ist nicht undo-bar · KRITISCH

`canvas.rs:173-184`, beim Commit des bearbeiteten Textes:

```rust
el.text = text;
app.touch();          // <- kein push_history()
```

Weder beim Start der Bearbeitung (`canvas.rs:712`) noch beim Commit wird ein
Snapshot erzeugt. `Strg+Z` macht getippten Text nicht rueckgaengig.

Dasselbe beim Pfeiltasten-Verschieben (`canvas.rs:864-875`).

### 1.4 Die Web-Version speichert nie · KRITISCH

`app.rs:1047-1070` in `tick_web_sync`:

```rust
// Schritt 3 — Auto-Save nach 2 s Ruhe
if self.modified && ... && now - self.web_last_modified > 2.0 { /* save */ }

// Schritt 5 — am Ende JEDES Frames
if self.modified { self.web_last_modified = now; }
```

`modified` ist ein Dirty-Flag, kein Event. Solange es `true` ist, wird der Timer
in jedem Frame zurueckgesetzt; die Bedingung `> 2.0` wird nie wahr.

> **Die Web-Version speichert niemals automatisch.** Alle Aenderungen im Browser
> gehen beim Schliessen des Tabs verloren.

Zweiter Fehler im selben Pfad: `spawn_save` (`web_sync.rs:271-277`) aktualisiert
`last_content_hash` nicht. Der Kommentar sagt „hash wird im Haupt-Thread
gesetzt" — der `WebEvent::Saved`-Handler (`app.rs:1034`) tut es aber nicht. Jedes
erfolgreiche Speichern wuerde beim naechsten Poll als „externe Aenderung"
fehlinterpretiert → Voll-Reload + falscher Undo-Eintrag.

### 1.5 Web-Polling ueberschreibt lokale Arbeit · KRITISCH

`apply_web_doc` (`app.rs:1076`) ersetzt `self.doc` bedingungslos. Der Kommentar
bei `spawn_poll` behauptet „Bei ungespeicherten eigenen Aenderungen nicht
reloaden" — diese Pruefung steht im Code nirgends.

Nativ gibt es einen Konflikt-Dialog (`show_conflict_dialog`), im Web nicht.

### 1.6 Panic beim PDF-Export

`printing.rs:122`:

```rust
doc.add_builtin_font(BuiltinFont::Helvetica)
   .unwrap_or_else(|_| panic!("keine Schrift gefunden"))
```

Ein PDF-Export auf einem System ohne die vier hartkodierten Font-Pfade laesst die
App abstuerzen — mitsamt dem ungespeicherten Dokument.

---

## 2. Das Kernversprechen bricht: WYSIWYG stimmt nicht

Ein Dokumenten-Editor hat genau einen Vertrag: *Was ich sehe, kommt raus.*

| Feature | Canvas | PDF | ODT |
|---|---|---|---|
| Textumbruch | ja (`painter.layout` mit Wrap-Breite) | **nein — nur `\n`-Split** | nein |
| Text-Rotation | **nein — ignoriert** | **nein — ignoriert** | ja |
| `corner_radius` | **nein — `let _ = radius;`** | ja | — |
| Rechteck / Linie | ja | ja | **nein — verworfen** |
| Textformatierung | ja | ja | **nein — alles `"Standard"`** |

### 2.1 Kein Zeilenumbruch im PDF · KRITISCH

`printing.rs:209` macht `el.text.split('\n')`. Ein Absatz, der auf dem Bildschirm
ueber fuenf Zeilen umbricht, laeuft im PDF als **eine Zeile aus der Seite heraus**.

Dazu: `approx_text_width()` (`printing.rs:189`) schaetzt `Zeichen x 0,5 x Groesse`.
Zentrierter und rechtsbuendiger Text sitzt im PDF sichtbar falsch.

Ausserdem rechnet der PDF-Export mit `line_h = font_size * 1.25`, waehrend der
Canvas die echte Galley-Hoehe nutzt — `valign: Middle/Bottom` ist damit im PDF
immer dann falsch, wenn Text umbricht.

### 2.2 `corner_radius` ist ein kaputtes Bedienelement

Es hat einen `DragValue` in den Eigenschaften (`app.rs:1796`), tut auf dem Canvas
nachweislich nichts (`canvas.rs:1026`: `let _ = radius;`), erscheint aber im PDF.
Der Nutzer dreht am Regler, sieht nichts und haelt das Feature fuer kaputt.

### 2.3 Text-Rotation existiert nur auf dem Papier

Das Feld existiert, die `Rotate`-Interaktion existiert, es steht im AI-Hint — aber
`draw_selection` (`canvas.rs:1152`) bietet den Rotationsgriff nur fuer Bilder an,
und `draw_element` rendert Text ohne jede Rotation.

### 2.4 Render-Funktion mutiert das Modell

`canvas.rs:921`:

```rust
el.h = (galley.size().y / zoom).max(el.font_size * 1.2);
```

Eine Zeichenfunktion veraendert das Dokument — zoom-abhaengig, ohne History, ohne
`modified`-Flag. Architekturfehler, der spaeter schwer zu debuggende Diffs erzeugt.

---

## 3. Bedienbarkeit

### 3.1 Eingabekonflikte (mechanisch nachweisbar)

`canvas.rs:728-771` benutzt `ctx.input_mut(|i| i.events.retain(...))` und
**entfernt** Copy/Paste-Events, sobald `app.editing.is_none()`. `app.editing`
bezieht sich aber nur auf die Textbearbeitung *auf dem Canvas* — nicht auf das
Eigenschaften-Panel und nicht auf den JSON-Editor.

Konsequenzen:

- **`Strg+C` im Eigenschaften-Textfeld** kopiert die *Objekte* statt des
  markierten Textes.
- **`Strg+V` im JSON-Editor funktioniert nicht.** Das Event wird vorher
  weggefiltert. Damit ist die zentrale Funktion des JSON-Editors kaputt.

Analog, ohne jede Focus-Pruefung:

- `canvas.rs:819` — **`Entf`** loescht das ausgewaehlte Objekt, auch waehrend man
  im JSON-Editor tippt.
- `canvas.rs:837` — **`L`** schaltet den Linien-Modus um, auch beim Tippen im
  Eigenschaften-Panel.

Fix ueberall identisch: `ctx.memory(|m| m.focused()).is_none()` als Bedingung.

### 3.2 Weitere Luecken

- **Keine Toolbar.** Alles steckt in Menues. „Simple und extrem benutzerfreundlich"
  heisst: die sechs wichtigsten Werkzeuge sind einen Klick entfernt.
- **Textbearbeitung ist nicht WYSIWYG.** `canvas.rs:165` legt ein rohes
  `TextEdit::multiline` ueber das Element, ohne Font, Groesse, Farbe oder
  Ausrichtung zu uebernehmen. Doppelklick auf 48pt-Pacifico-Rot-Zentriert →
  man tippt in 14px-Schwarz-Linksbuendig.
- **Dupliziertes Bedienelement.** `app.rs:1755-1777`: „Rahmenstaerke" steht
  zweimal untereinander, beide auf `stroke_width`. Bei einer Linie heisst es
  „Linienstaerke" *und* „Rahmenstaerke".
- **Einstellungen werden nicht gespeichert.** `settings_io::save()` wird nur fuer
  `theme` und `panel_side` gerufen (`app.rs:1367`, `1379`). `units`,
  `scroll_mode` und `page_align` sind nach jedem Neustart wieder auf Default.
- **Undo ohne Feedback.** `can_undo` / `can_redo` existieren in `history.rs` und
  werden nirgends benutzt (Compiler-Warnung). Keine ausgegrauten Menueeintraege.
- **Leere Textelemente bleiben liegen.** Doppelklick → `Esc` hinterlaesst ein
  unsichtbares, aber klickbares Element.

---

## 4. Sicherheit — das PHP-Backend ist offen

Der schwaechste Teil des Projekts. `SECURITY.md` behandelt sorgfaeltig Shell-Args
und ZIP-Bomben und uebersieht das Backend vollstaendig.

### 4.1 Token-Schutz ist trivial umgehbar · KRITISCH

`web/index.php:131-140` prueft `check_access()` beim API-Endpunkt `?get=<slug>`.
Der **Direktpfad** `/<slug>` (Zeilen 197-208 und 221-229) prueft gar nichts:

```php
if ($slug !== '' && !str_contains($_SERVER['HTTP_ACCEPT'] ?? '*/*', 'text/html')) {
    readfile($doc_path);   // <- kein check_access()
    exit;
}
```

`curl https://boxdoc.at/geheimdoc` liefert das tokengeschuetzte Dokument im
Klartext. Der Token ist wirkungslos.

### 4.2 `?list=1` gibt alle Dokumente preis · KRITISCH

`web/index.php:165-179`, ohne jede Authentifizierung: alle Slugs aller Nutzer,
inklusive der „geschuetzten". Zusammen mit 4.1 ist **jedes Dokument auf dem Server
von jedem lesbar** — und da PUT ohne Token bei ungeschuetzten Docs erlaubt ist,
auch von jedem ueberschreibbar.

### 4.3 Stored XSS · KRITISCH

`web/index.php:225-227`:

```php
$inject = '<script type="application/json" id="boxdoc-content">' . "\n"
        . $doc_json . "\n</script>\n";
```

`$doc_json` ist frei vom Nutzer kontrollierter Inhalt. Ein Textelement mit
`</script><script>...</script>` bricht aus dem Tag aus.

### 4.4 Weiteres

- **Token in der URL** (`?t=...`) landet in Server-Logs, Referrern und
  Browser-History. Gehoert in einen Header.
- **`stream.php` blockiert einen PHP-Worker 120 Sekunden** pro Verbindung. Auf
  typischem Shared Hosting (5-10 Worker) reichen ein Dutzend Tabs, um die Seite
  lahmzulegen.
- **Kein Rate-Limit** auf `POST ?new=1` — Festplatte in Minuten voll (10 MB/Doc).
- **Slug-Regex inkonsistent:** `{4,32}` in `index.php`, `{8,32}` in `stream.php`.

---

## 5. Multiuser — konzeptionell nicht tragfaehig

Der aktuelle Ansatz ist:

1. Volles Dokument alle 2 s per GET pollen.
2. Volles Dokument per PUT schreiben.
3. Bei Unterschied: lokalen Stand komplett ersetzen.

Das ist **Last-Write-Wins auf Dokumentebene**. Zwei Nutzer, die gleichzeitig an
verschiedenen Ecken derselben Seite arbeiten, loeschen sich gegenseitig die
Arbeit — nicht als Randfall, sondern als Regelverhalten.

Dazu kommt: Der Poll laedt jedes Mal das **komplette Dokument inklusive aller
base64-Bilder**. Bei einem 3-MB-Dokument sind das 1,5 MB/s pro offenem Tab. Die
API liefert bereits ein `modified`-Feld — es wird nicht genutzt.

Die gute Nachricht: Das Datenmodell ist fuer eine saubere Loesung fast ideal
gebaut. `Vec<Element>` mit stabilen `u64`-IDs erlaubt einen **Drei-Wege-Merge auf
Element-Ebene** (base / local / remote) ohne CRDT-Komplexitaet.

---

## 6. Architektur & Code-Qualitaet

### 6.1 Das fette `Element`-Struct

`model.rs:484` — jedes Element traegt 20 Felder. Eine Linie hat `crop`,
`image_w`, `font`, `valign`, `indent`, `corner_radius`. Deshalb sind
`new_line()`, `new_rectangle()`, `new_ellipse()` und `new_text()` fast identische
25-Zeilen-Bloecke.

Es blaeht ausserdem das JSON auf — was direkt gegen die AI-first-Praemisse
arbeitet, weil die KI mehr irrelevante Felder liest und schreibt.

### 6.2 Der `AI_HINT`-String

`io.rs:26-148`, ~5 KB, wird in *jede* `.boxdoc`-Datei geschrieben. Die Idee ist
clever, aber die Spec ist handgepflegt und driftet vom Modell weg — sie behauptet
z. B. `corner_radius` sei fuer Rectangle relevant, obwohl es auf dem Canvas
ignoriert wird.

### 6.3 Hot-Path-Mutex

`fonts.rs:128` — `family_for()` nimmt einen globalen Mutex und macht eine lineare
Suche, **pro Textelement, pro Frame**.

### 6.4 Monolithen

`app.rs` mit 2812 Zeilen, `canvas.rs` mit 1820. Die Roadmap kennt das (Phase
7d/e). Frueher angehen, nicht spaeter: die Fehler aus Abschnitt 1 und 3 sind genau
die Sorte, die in grossen Dateien ueberlebt.

### 6.5 81 Compiler-Warnungen

Darunter **27 mit „will become a hard error in a future release"**
(f32/f64-Inferenz-Fallback) und ~56 deprecated egui-APIs (`close_menu`,
`SidePanel`, `Panel::show`, `menu::bar`). Beim naechsten egui-Update bricht das.

### 6.6 Roadmap vs. Realitaet

`ROADMAP.md` nennt sich „die einzige verbindliche Quelle fuer Status und
Planung", steht auf `v0.4.2-dev` (Cargo.toml: `0.5.0`) und listet Phase 1
(Ellipse) als offen — obwohl Commit `f866e01` sie implementiert hat.
Leitplanke 3 sagt „Auto-Save ist bewusst nicht Teil der Roadmap", waehrend
`web_sync.rs` Auto-Save implementiert.

---

## 7. Tests: null

```
grep -rn "#[test]" src/ tests/   ->  0 Treffer
```

`tests/` enthaelt nur drei PDF-Fixtures. Die vier `src/bin/test_pdf_*.rs` sind
manuelle Binaries, keine Tests.

`ROADMAP.md` Leitplanke 5: *„Keine Tests als Blocker — sie kommen spaeter, wenn
sich das Modell stabilisiert hat."*

**Diese Entscheidung ist der eigentliche Grund fuer die Haelfte der Fehler in
diesem Review.** Jeder Punkt aus Abschnitt 1 und 2 waere von einem trivialen Test
gefangen worden:

```rust
#[test] fn roundtrip_erhaelt_alle_felder()
#[test] fn debounce_feuert_nach_zwei_sekunden()    // faengt 1.4
#[test] fn pdf_zeilen_entsprechen_canvas_zeilen()  // faengt 2.1
#[test] fn settings_ueberleben_speichern_laden()   // faengt 3.2
```

`model.rs`, `history.rs`, `geometry.rs` und `io.rs` sind reine Logik ohne UI —
dort kosten Tests fast nichts.

---

## 8. Empfohlene Reihenfolge

### Sprint 1 — Vertrauen

1. Close-Guard + Bestaetigungsdialog bei `modified`; dito Neu/Oeffnen
2. `Strg+S` / `O` / `N` / `A` / `X` / `D` / `P`, Shortcuts in den Menues anzeigen
3. `push_history()` bei Textbearbeitung und Pfeiltasten-Nudge
4. Focus-Guards fuer alle globalen Tasten
5. `panic!` in `printing.rs:122` durch `Result` ersetzen
6. Alle Settings persistieren
7. Tests fuer genau diese Punkte

### Sprint 2 — PDF

8. Zeilenumbruch: **ein** Layout-Pfad fuer Canvas und PDF
9. `corner_radius` auf dem Canvas rendern
10. Text-Rotation rendern + Rotationsgriff fuer alle Element-Typen
11. Modell-Mutation aus `draw_element` herausziehen

### Sprint 3 — Multiuser

12. Drei-Wege-Merge auf Element-Ebene
13. Versionierung + `If-Match` / `409`
14. Guenstiges Polling ueber Meta-Endpunkt
15. Auto-Save-Debounce reparieren

### Sprint 4 — Backend-Sicherheit

16. `check_access()` auf dem `/<slug>`-Pfad
17. `?list=1` hinter Auth
18. `</` im injizierten JSON escapen
19. Token per Header; Rate-Limit; `stream.php`-Lifetime senken

### Sprint 5 — Bedienung & Aufraeumen

20. Toolbar mit den sechs Kernwerkzeugen
21. WYSIWYG-Textbearbeitung im Overlay
22. ODT: Rechteck/Linie + Textformatierung
23. `app.rs`/`canvas.rs` aufteilen; Warnungen auf 0; CI mit `-D warnings`

---

## Fazit

Die Idee traegt, das Handwerk ist ordentlich, die Dokumentation ist ehrlich.

Aber solange das Programm beim Schliessen kommentarlos die Arbeit wegwirft, kein
`Strg+S` kennt und im PDF anders aussieht als auf dem Bildschirm, ist es kein
Editor, dem jemand ein wichtiges Dokument anvertraut.

**Sprint 1 ist keine Politur, sondern die Eintrittskarte.**
