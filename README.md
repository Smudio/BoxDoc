# BoxDoc

Ein simples, leistungsstarkes und zuverlässiges Werkzeug zum Erstellen, Bearbeiten und Speichern von Dokumenten.

BoxDoc verbindet die Einfachheit eines Objekt-Canvas mit der Vertrautheit klassischer Textverarbeitung — nativ, schnell und plattformübergreifend.

---

## Funktionen

**Objekte**
- Frei verschiebbare Text- und Bild-Objekte
- Text mit Inline-Bearbeitung (Doppelklick), Einzug, Farbe, Schriftart und -größe
- Auszeichnung: **fett**, *kursiv*, unterstrichen, durchgestrichen — frei
  kombinierbar und vollständig im PDF- wie im SVG-Export. Wo die Schrift einen
  echten Schnitt mitbringt, wird er benutzt; sonst wird er nachgeahmt
- Bilder per Drag & Drop — skalieren, frei drehen, zuschneiden (Crop)
- Horizontale und vertikale Textausrichtung
- Formen: Rechteck, Ellipse, Linie

**Pfade & Kurven**
- **Einfügen → Pfad**: legt eine fertige, bearbeitbare Kurve auf die Seite und
  öffnet gleich die Knotenbearbeitung — der Weg ohne Zeichnen
- Pfad-Werkzeug: Klick setzt eine Ecke, Klick+Ziehen zieht Kurvengriffe heraus
- Freihand-Werkzeug: gezogene Spur wird ausgedünnt und automatisch geglättet
- Knotenbearbeitung: Knoten und Bézier-Griffe ziehen, einfügen, löschen,
  zwischen Ecke und weichem Übergang umschalten
- Offen (Linienzug) oder geschlossen (Fläche), mit Füllung und Kontur
- Kurven bleiben beim PDF-Export echte Kurven und kommen beim Import auch
  wieder als solche zurück

**Auswahl & Bearbeitung**
- Auswahl-Rechteck: mehrere Objekte gleichzeitig auswählen
- Mehrere Objekte gleichzeitig verschieben
- Ausrichtungs-Buttons: links / mittig / rechts, oben / mittig / unten
- Copy & Paste (Strg+C / Strg+V) mit Ghost-Vorschau und Snap an die Originalposition

**Seiten & Layout**
- Papierformate: A3, A4, A5, Letter, Legal
- Hoch- und Querformat
- Mehrere Seiten pro Dokument
- Papier mittig, links- oder rechtsbündig ausrichtbar

**Positionierung**
- Koordinaten relativ zum Text-Ursprungspunkt (abhängig von Ausrichtung)
- Referenzpunkt-Raster (3×3) für Mehrfachauswahl
- Einheiten wählbar: cm, mm, pt, Zoll

**Dateiformate**
- Natives `.boxdoc`-Format (Speichern/Öffnen)
- ODT-Import und -Export (OpenDocument, LibreOffice-kompatibel)
- PDF-Export und Drucken — mit **eingebetteter** Schrift, und zwar genau der,
  mit der auch auf dem Bildschirm gezeichnet wurde
- SVG-Export — ganze Seite **oder nur die Auswahl**
- Bild-Export — ein ausgewähltes Bild als PNG oder JPEG wieder herausspeichern,
  und mit Strg+C auch direkt in die Zwischenablage (Paint, Word, Chat)

---

## Bedienung

| Aktion | Tasten |
|---|---|
| Zoomen | Strg + Scroll |
| Ansicht verschieben | Mittlere Maustaste |
| Text erstellen | Doppelklick auf leere Fläche |
| Mehrere Objekte auswählen | Ziehen auf leerer Fläche |
| Zur Auswahl hinzufügen | Shift + Klick |
| Kopieren | Strg + C |
| Einfügen | Strg + V (Klick zum Platzieren) |
| Löschen | Entf |
| Abbrechen | Esc |

### Der Zeiger sagt, was ein Klick tut

| Zeiger | Bedeutung |
|---|---|
| Pfeil | leere Fläche — Ziehen spannt ein Auswahl-Rechteck auf |
| Hand | ein Objekt liegt darunter, Klick wählt es aus |
| Verschieben-Kreuz | das Objekt ist schon ausgewählt, Ziehen bewegt es |
| Größenpfeil | Kanten- oder Eckgriff — Ziehen skaliert (folgt der Drehung) |
| Greifhand | Drehgriff, Linien-Endpunkt oder Pfad-Knoten |
| Fadenkreuz | ein Zeichenwerkzeug ist aktiv |

Cursor und Klick gehen durch dieselbe Trefferprüfung — was der Zeiger
anzeigt, ist auch das, was der Klick auswählt.

### Werkzeuge

| Werkzeug | Taste |
|---|---|
| Auswahl | V |
| Linie | L |
| Pfad (Pen) | P |
| Freihand | F |
| Knoten des ausgewählten Pfads bearbeiten | N |

### Pfad zeichnen (P)

| Aktion | Bedienung |
|---|---|
| Eckknoten setzen | Klick |
| Kurvenknoten setzen | Klick + Ziehen (zieht die Griffe heraus) |
| Auf 45°-Raster legen | Shift halten |
| Pfad schließen | Klick auf den Startknoten |
| Offen beenden | Enter oder Doppelklick |
| Letzten Knoten zurücknehmen | Rücktaste |
| Abbrechen | Esc |

### Bestehenden Pfad bearbeiten

**Hinein:** Doppelklick auf den Pfad — oder Pfad auswählen und `N` drücken,
oder im Eigenschaften-Panel auf „Knoten bearbeiten".

Der Doppelklick muss die Kontur nicht genau treffen: Ein paar Pixel daneben
genügen, und bei einem bereits ausgewählten Pfad reicht die grobe Richtung.
Der Zeiger wird zur Hand, sobald er nah genug ist.

| Aktion | Bedienung |
|---|---|
| Punkt verschieben | Knoten ziehen |
| Kurve verformen | Griff ziehen |
| Punkt hinzufügen | Doppelklick auf ein Segment |
| Punkt löschen | Knoten anklicken, dann `Entf` — oder „Knoten löschen" im Panel |
| Ecke ⇄ weicher Übergang | Alt + Klick auf den Knoten — oder „Ecke / Kurve" im Panel |
| Griffe unabhängig setzen (Spitze) | Alt + Griff ziehen |
| Beenden | `N` oder `Esc` |

Die Knotenform zeigt den Zustand: **Quadrat** = Ecke, **Kreis** = weicher
Übergang, **Raute** = Spitze (Griffe mit Knick). Fährt der Cursor über ein
Segment, zeigt ein Kreuz, wo ein Doppelklick den neuen Punkt setzen würde.

### SVG exportieren

Die **Auswahl** exportierst du am schnellsten über den Knopf
**„Auswahl als SVG…"** ganz unten im Eigenschaften-Panel — er steht dort, wo
du die Auswahl gerade in der Hand hast, und zeigt die Anzahl mit an.

Im Menü gibt es beides:
**Datei → Seite als SVG exportieren…** legt die aktuelle Seite ab.
**Datei → Auswahl als SVG exportieren…** nimmt nur die gewählten Objekte;
der Eintrag ist ausgegraut, solange nichts ausgewählt ist.

Was dabei herauskommt:

- Jedes Objekt bleibt sein eigenes Vektorprimitiv — ein Rechteck ein `<rect>`,
  eine Kurve ein `<path>` mit echten Bézier-Griffen. Nichts wird zu einem
  Vieleck aufgelöst, alles ist in Illustrator oder Inkscape wieder
  bearbeitbar.
- Halbdurchsichtige Farben bleiben halbdurchsichtig (der PDF-Export muss sie
  über Weiß mischen, SVG nicht).
- Text bleibt Text — markierbar und durchsuchbar. Die Schrift wird benannt,
  nicht eingebettet; fehlt sie auf dem Zielrechner, greift eine generische
  Rückfallebene.
- Bilder liegen als data-URI **in** der Datei, nicht daneben.
- Die Auswahl wird auf ihre Hüllbox plus 8 pt Rand beschnitten und behält
  einen **durchsichtigen** Hintergrund, damit sie sich in ein anderes
  Dokument legen lässt.

Exportiert wird immer nur die **aktuelle** Seite: SVG kennt keine Seiten,
alle übereinanderzustapeln wäre nicht das, was jemand will.

---

### Bilder herausspeichern und kopieren

Ist ein Bild ausgewählt, steht im Eigenschaften-Panel neben „Auswahl als SVG…"
der Knopf **„Bild speichern…"** (im Menü: **Datei → Ausgewähltes Bild
speichern…**). Gespeichert wird die **Bilddatei selbst**, nicht die
Seitendarstellung:

- in **Originalauflösung**, nicht in der Größe, auf die du das Bild auf der
  Seite gezogen hast,
- auf den **Crop** beschnitten — was du siehst, ist was du bekommst,
- aber **ohne Drehung**: `rotation` beschreibt, wie das Bild auf der Seite
  liegt, nicht wie die Datei aussieht. Eine schräge Kopie mit transparenten
  Ecken wäre in Paint & Co. nur im Weg. Wer die Seitendarstellung will, nimmt
  den SVG- oder PDF-Export.

Als Format stehen PNG und JPEG zur Wahl. Ein unbeschnittenes PNG wird
**byteweise durchgereicht** statt neu kodiert — die Datei kommt genau so
heraus, wie sie hineingegangen ist. Bei JPEG landen durchsichtige Bereiche auf
Weiß, weil JPEG kein Alpha kennt (sonst würden sie schwarz).

Sind mehrere Bilder ausgewählt, fragt der Dialog nach einem **Ordner** und legt
sie dort einzeln ab; vorhandene Dateien werden nicht überschrieben.

**Strg+C** kopiert nicht nur das BoxDoc-Objekt: Ist genau **ein Bild**
ausgewählt, liegt es danach auch als Pixelbild in der System-Zwischenablage und
lässt sich direkt in Paint, Word oder einen Chat einfügen. Programme, die es
können, bekommen die Version **mit** Transparenz; für das klassische
Bitmap-Format (Paint) wird zusätzlich eine Variante auf weißem Grund abgelegt —
sonst würde Paint durchsichtige Bereiche schwarz zukleben. Bei mehreren Bildern
in der Auswahl bleibt es bei den BoxDoc-Objekten: welches gemeint wäre, ist
nicht zu erraten. Der Weg läuft über PowerShell und braucht dadurch etwa eine
Sekunde — die Statuszeile sagt danach, ob es geklappt hat. Nur unter Windows.

---

## Tech-Stack

**Rust** + **egui** — nativ kompiliert, ohne Laufzeit-Abhängigkeiten.

Die Architektur ist sauber in Module getrennt:

```
src/
├── main.rs       Einstiegspunkt
├── app.rs        Anwendungszustand & UI-Logik
├── canvas.rs     Zeichenfläche & Interaktion
├── model.rs      Datenmodell (Dokument, Seite, Element)
├── geometry.rs   Geometrie-Helfer (Rotation)
├── fonts.rs      Schriftverwaltung
├── store.rs      Bildspeicher
├── io.rs         Datei-Dialoge & Projektformat
├── odt.rs        OpenDocument-Import/Export
├── printing.rs   PDF-Export & Drucken
└── svg.rs        SVG-Export (Seite oder Auswahl)
```

Rust + egui laufen sowohl nativ als auch im Browser (WASM) — beide Fassungen
teilen denselben Code.

---

## Build

### Desktop

```sh
cargo build --release
```

Die fertige Binary liegt unter `target/release/boxdoc`.

### Web

```powershell
.\build-web.ps1
```

Das Upload-Paket landet in `web/dist/`; dessen Inhalt auf den Webserver laden.
`web/` ist die Quelle, `web/dist/` das Ergebnis — Details in
[`web/README.md`](web/README.md).

Die Web-Version ist mit der Desktop-Version angeglichen: gleicher Menübaum,
gleiche Ausgabebytes. PDF-, SVG- und ODT-Export laufen im Browser als Download.
Nicht dabei sind PDF-Import (braucht pdfium, eine native Bibliothek) und Drucken
(dafür PDF exportieren) — beide Menüeinträge sind ausgegraut und erklären sich
im Tooltip.

---

## Lizenz

MIT oder Apache-2.0.
