//! PDF-Import via pdfium-render.
//!
//! Liest ein PDF ein und konvertiert Seiten/Elemente in das BoxDoc-Modell:
//! Text (Position, Größe, Drehung, Bold/Italic-Heuristik), eingebettete Bilder
//! (als PNG, mit Drehung) und Vektorgrafik (Linien, Polygonzüge, gedrehte
//! Rechtecke, Ellipsen und beliebige freie Pfade).
//!
//! # Leitgedanke: nichts wegwerfen, nichts erfinden
//!
//! Zwei Fehlerarten sind hier möglich, und beide sind schlecht. Etwas
//! **wegzulassen** heißt, dass der Nutzer eine leere Stelle sieht, wo im PDF
//! etwas stand. Etwas **anzunähern** heißt, dass an dieser Stelle etwas
//! Falsches steht. Der Import versucht deshalb erst, eine Grundform
//! wiederzuerkennen (die lässt sich bearbeiten), und behält andernfalls die
//! Geometrie unverändert als freien Pfad.
//!
//! Konkret hieß das an drei Stellen, eine reine Bounding-Box-Auswertung
//! aufzugeben — eine Hüllbox kennt weder Richtung noch Drehung:
//!
//! * **Pfade** kommen aus ihren Stützpunkten; sonst wird jede Linie waagrecht.
//! * **Text** kommt aus der Zeichenmatrix; sonst steht jede Beschriftung
//!   waagrecht, auch die senkrechte Tabellenüberschrift.
//! * **Bilder** kommen aus der Bildmatrix; sonst liegt jedes gedrehte Bild
//!   flach in einer zu großen Box.
//!
//! Ebenso wird in **Form-XObjects** hineingelesen (verschachtelte Inhalte, die
//! früher komplett fehlten) und unsichtbarer OCR-Text übersprungen.
//!
//! # Was BoxDoc nicht abbilden kann
//!
//! * Flächen mit Löchern (Even-Odd/Nonzero über mehrere Teilpfade) — jeder
//!   Teilpfad wird für sich gefüllt, das Loch also mit.
//! * Farbverläufe und Muster (Shading Patterns) werden ausgelassen.
//! * Gespiegelte Bilder: Die Drehung stimmt, die Spiegelung geht verloren.
//!
//! Native-only: PDFium ist eine C/C++-Bibliothek und steht auf WASM nicht zur
//! Verfügung. Web-Import via pdf.js ist für eine spätere Phase geplant.

use crate::model::{
    mm_to_pt, Document, Element, ElementKind, Orientation, Page, PaperFormat, TextAlign, VAlign,
};
use crate::geometry::PathNode;
use crate::store::ImageStore;
use egui::Pos2;
use pdfium_render::prelude::{PdfPageTextRenderMode, PdfPathFillMode, PdfPathSegmentType};
use pdfium_render::prelude::{
    PdfColor, PdfPageImageObject, PdfPageObject, PdfPageObjectCommon, PdfPageObjectsCommon,
    PdfPagePathObject, PdfPathSegments, PdfRect, Pdfium,
};

type E = Box<dyn std::error::Error>;

/// Maximale Anzahl Seiten im Import (Schutz vor pathologischen PDFs).
const MAX_PAGES: i32 = 200;
/// Maximale Anzahl Elemente pro Seite (Schutz vor pathologischen PDFs).
const MAX_ELEMENTS_PER_PAGE: usize = 4000;

/// Liefert eine `Pdfium`-Instanz. Beim allerersten Aufruf lädt `pdfium_bundled`
/// die native Bibliothek in ein Cache-Verzeichnis herunter und bindet sie
/// global ein. Jeder weitere Aufruf gibt eine dünne Pdfium-Instanz zurück,
/// die auf den bereits gebundenen globalen State zugreift.
fn open_pdfium() -> Result<Pdfium, String> {
    match pdfium_bundled::bind_pdfium_silent() {
        Ok(p) => Ok(p),
        Err(_) => {
            // Wahrscheinlich already initialized — Pdfium::default() findet
            // die bestehenden Bindings und gibt eine funktionierende
            // Instanz zurück.
            Ok(Pdfium::default())
        }
    }
}

/// Importiert eine PDF-Datei und wandelt sie in ein BoxDoc-`Document` um.
///
/// Rückgabe: `(Document, ImageStore, next_id)` — analog zu `odt::import`.
pub fn import_pdf(path: &std::path::Path) -> Result<(Document, ImageStore, u64), E> {
    let pdfium = open_pdfium().map_err(|e| -> E { e.into() })?;
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| format!("PDF konnte nicht geladen werden: {e}"))?;

    let mut images = ImageStore::default();
    let mut next_id: u64 = 1;
    let mut pages: Vec<Page> = Vec::new();

    // Gesamtanzahl Seiten begrenzen (Schutz vor pathologischen PDFs).
    let page_count = document.pages().len().min(MAX_PAGES);

    // Papierformat anhand der ersten Seite raten. Wir merken es uns am
    // Dokument, auch wenn spätere Seiten abweichende Maße haben können —
    // BoxDoc unterstützt nur ein einheitliches Format pro Dokument.
    let (format, orientation) = if page_count > 0 {
        let first = document
            .pages()
            .get(0)
            .map_err(|e| -> E { e.to_string().into() })?;
        detect_paper_format(first.width().value, first.height().value)
    } else {
        (PaperFormat::A4, Orientation::Portrait)
    };

    // Über alle Seiten iterieren. PdfPagesCommon::iter liefert einen
    // PdfPagesIterator, den wir direkt verarbeiten können.
    let pages_iter = document.pages().iter();
    let mut processed = 0i32;
    for page in pages_iter {
        if processed >= page_count {
            break;
        }
        processed += 1;

        let page_h = page.height().value;

        let mut elements = extract_page(&page, &document, page_h, &mut images, &mut next_id);

        if elements.len() > MAX_ELEMENTS_PER_PAGE {
            elements.truncate(MAX_ELEMENTS_PER_PAGE);
        }

        pages.push(Page { elements });
    }

    if pages.is_empty() {
        pages.push(Page::default());
    }

    let doc = Document {
        format,
        orientation,
        custom_formats: Vec::new(),
        pages,
    };
    Ok((doc, images, next_id))
}

/// Weitet importierte Textboxen, bis der Text mit BoxDocs Schriften wieder in
/// **eine** Zeile passt.
///
/// Warum das nötig ist: Die Box-Breite kommt aus der Bounding-Box, die das PDF
/// mit seiner *eigenen* eingebetteten Schrift gemessen hat. BoxDoc setzt den
/// Text danach mit einer Ersatzschrift — und die ist fast immer ein paar
/// Prozent breiter. Das letzte Wort passt dann nicht mehr und rutscht in eine
/// zweite Zeile, obwohl im PDF alles auf einer Zeile stand.
///
/// Ein Textsegment von pdfium liegt per Definition auf **einer** Zeile
/// (pdfium fasst nur Zeichen derselben Zeile und desselben Stils zusammen).
/// Deshalb ist „muss einzeilig bleiben" hier keine Heuristik, sondern die
/// Eigenschaft, die der Import zu erhalten hat.
///
/// Muss nach [`import_pdf`] aufgerufen werden, sobald ein `egui::Context` mit
/// geladenen Schriften vorliegt.
pub fn fit_text_widths(ctx: &egui::Context, doc: &mut Document) {
    ctx.fonts_mut(|fonts| {
        for page in doc.pages.iter_mut() {
            for el in page.elements.iter_mut() {
                if el.kind != ElementKind::Text || el.text.is_empty() {
                    continue;
                }
                let natural = crate::text_layout::natural_width(fonts, el);
                let needed = el.indent + natural + wrap_margin(natural);
                if needed > el.w {
                    el.w = needed;
                }
            }
        }
    });
}

/// Sicherheitszuschlag auf die gemessene Textbreite, in pt.
///
/// Der Canvas layoutet mit `scale = zoom`; Glyphenbreiten skalieren dabei nicht
/// exakt linear (Hinting, Rundung auf Pixel). Eine Zeile, die bei Zoom 1.0
/// haargenau passt, kann bei Zoom 3.0 umbrechen. Der Zuschlag hält Abstand zur
/// Umbruchkante, ohne die Box sichtbar aufzublähen.
fn wrap_margin(natural_width: f32) -> f32 {
    (natural_width * 0.02).clamp(0.5, 6.0)
}

/// Erkennt das Papierformat aus einer Mediabox-Größe (in pt).
/// BoxDoc unterstützt nur ein Format pro Dokument — bei gemischten Größen
/// wird die erste Seite maßgeblich, der Rest ggf. beschnitten.
fn detect_paper_format(w_pt: f32, h_pt: f32) -> (PaperFormat, Orientation) {
    let (w, h) = (w_pt.max(h_pt), w_pt.min(h_pt)); // Hochformat-Normierung
    let tolerance = 6.0; // pt — Wikimedia/PDF-Encoder haben leichte Abweichungen

    for fmt in PaperFormat::all() {
        let (fw_mm, fh_mm) = fmt.size_mm();
        let (fw_pt, fh_pt) = (mm_to_pt(fw_mm), mm_to_pt(fh_mm));
        if (w - fw_pt).abs() < tolerance && (h - fh_pt).abs() < tolerance {
            let orientation = if w_pt >= h_pt {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            };
            return (fmt, orientation);
        }
    }
    let orientation = if w_pt >= h_pt {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    };
    (PaperFormat::A4, orientation)
}

/// Affine Transformation in PDF-Konvention (`a b c d e f`).
///
/// Gebraucht, weil ein PDF seine Objekte schachteln kann: Ein Form-XObject
/// bringt eine eigene Matrix mit, und alles darin liegt in **seinem**
/// Koordinatensystem. Ohne die Verkettung landen verschachtelte Inhalte an
/// der Stelle, an der sie im Formular stehen — nicht dort, wo das Formular
/// auf der Seite sitzt.
#[derive(Debug, Clone, Copy)]
pub struct Mat {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Mat {
    pub const IDENTITY: Mat = Mat {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn from_pdfium(m: pdfium_render::prelude::PdfMatrix) -> Mat {
        Mat {
            a: m.a(),
            b: m.b(),
            c: m.c(),
            d: m.d(),
            e: m.e(),
            f: m.f(),
        }
    }

    /// Wendet die Transformation auf einen Punkt an.
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// Verkettung: erst `inner`, dann `self`.
    pub fn then(&self, inner: Mat) -> Mat {
        Mat {
            a: inner.a * self.a + inner.b * self.c,
            b: inner.a * self.b + inner.b * self.d,
            c: inner.c * self.a + inner.d * self.c,
            d: inner.c * self.b + inner.d * self.d,
            e: inner.e * self.a + inner.f * self.c + self.e,
            f: inner.e * self.b + inner.f * self.d + self.f,
        }
    }

    /// Gleichmäßiger Skalierungsfaktor (für Strichstärken).
    pub fn scale(&self) -> f32 {
        let det = (self.a * self.d - self.b * self.c).abs().sqrt();
        if det.is_finite() && det > 0.0 {
            det
        } else {
            1.0
        }
    }
}

/// Sammelzustand beim Ablaufen des Objektbaums einer Seite.
struct PageWalk<'a, 'b> {
    document: &'a pdfium_render::prelude::PdfDocument<'b>,
    page_h: f32,
    /// Elemente mit ihrem Zeichenrang.
    layered: Vec<(usize, Element)>,
    /// Rechtecke aller Textobjekte (in Seitenkoordinaten des PDFs) samt Rang —
    /// Grundlage für die Einsortierung der gröber geschnittenen Textsegmente.
    text_objects: Vec<(usize, PdfRect)>,
    /// Laufender Zeichenrang über alle Schachtelungsebenen hinweg.
    z: usize,
}

/// Wie tief in Form-XObjects hinein importiert wird. Schützt vor Dateien, die
/// sich (versehentlich oder mutwillig) tief schachteln.
const MAX_FORM_DEPTH: usize = 12;

/// Baut alle Elemente einer Seite — **in der Zeichenreihenfolge des PDFs**.
///
/// Das ist der springende Punkt: Ein PDF malt seine Objekte in der Reihenfolge,
/// in der sie im Content-Stream stehen; was später kommt, liegt oben. Früher
/// hat BoxDoc erst allen Text, dann alle Bilder, dann alle Pfade eingelesen —
/// damit landete jedes farbige Hintergrundrechteck zwangsläufig **über** dem
/// Text, den es im Original hinterlegt hat.
///
/// Bilder und Pfade tragen ihren Index direkt (`page.objects()` liefert sie in
/// Zeichenreihenfolge). Text kommt aus `PdfPageText::segments()`, weil pdfium
/// dort bereits zeilenweise gruppiert — ein einzelnes Textobjekt ist oft nur
/// ein Wortfragment. Die Segmente bekommen ihren Rang deshalb über die
/// Textobjekte, die in ihnen liegen (siehe `z_for_text`).
fn extract_page(
    page: &pdfium_render::prelude::PdfPage,
    document: &pdfium_render::prelude::PdfDocument,
    page_h: f32,
    images: &mut ImageStore,
    next_id: &mut u64,
) -> Vec<Element> {
    let mut walk = PageWalk {
        document,
        page_h,
        layered: Vec::new(),
        text_objects: Vec::new(),
        z: 0,
    };
    walk_objects(page.objects().iter(), Mat::IDENTITY, 0, &mut walk, images, next_id);

    let PageWalk {
        mut layered,
        text_objects,
        ..
    } = walk;

    for (bounds, el) in extract_text(page, page_h, next_id) {
        layered.push((z_for_text(&bounds, &text_objects), el));
    }

    // Stabil: Elemente mit gleichem Rang behalten ihre Reihenfolge.
    layered.sort_by_key(|(z, _)| *z);
    layered.into_iter().map(|(_, el)| el).collect()
}

/// Läuft eine Objektliste ab und legt die Ergebnisse in `walk` ab.
///
/// `parent` bildet den Raum, in dem diese Objekte liegen, auf die Seite ab.
/// Auf oberster Ebene ist das die Identität; innerhalb eines Form-XObjects die
/// verkettete Matrix aller umgebenden Formulare.
fn walk_objects(
    objects: pdfium_render::prelude::PdfPageObjectsIterator,
    parent: Mat,
    depth: usize,
    walk: &mut PageWalk,
    images: &mut ImageStore,
    next_id: &mut u64,
) {
    for obj in objects {
        let z = walk.z;
        walk.z += 1;
        match obj {
            PdfPageObject::Text(ref t) => {
                if let Ok(b) = PdfPageObjectCommon::bounds(t) {
                    walk.text_objects.push((z, transformed_rect(b, parent)));
                }
            }
            PdfPageObject::Image(ref img) => {
                if let Some(el) =
                    image_element(img, walk.document, walk.page_h, parent, images, next_id)
                {
                    walk.layered.push((z, el));
                }
            }
            PdfPageObject::Path(ref path) => {
                // Ein Pfadobjekt kann mehrere Teilpfade enthalten (z. B. einen
                // Polygonzug aus mehreren Strecken). Alle Teile teilen sich den
                // Zeichenrang des Objekts.
                for el in path_elements(path, walk.page_h, parent, next_id) {
                    walk.layered.push((z, el));
                }
            }
            PdfPageObject::XObjectForm(ref form) => {
                // Ein Form-XObject ist ein eingebettetes Miniatur-Dokument.
                // Seine Kinder liegen in seinem eigenen Koordinatensystem —
                // ohne diese Verkettung wurden sie früher schlicht ignoriert
                // und die halbe Seite fehlte.
                if depth >= MAX_FORM_DEPTH {
                    continue;
                }
                let inner = form
                    .matrix()
                    .map(Mat::from_pdfium)
                    .unwrap_or(Mat::IDENTITY);
                walk_objects(
                    form.iter(),
                    parent.then(inner),
                    depth + 1,
                    walk,
                    images,
                    next_id,
                );
            }
            _ => {}
        }
    }
}

/// Bildet ein Objekt-Viereck mit `m` ab und liefert dessen Hüllrechteck.
fn transformed_rect(q: pdfium_render::prelude::PdfQuadPoints, m: Mat) -> PdfRect {
    let corners = [
        m.apply(q.x1().value, q.y1().value),
        m.apply(q.x2().value, q.y2().value),
        m.apply(q.x3().value, q.y3().value),
        m.apply(q.x4().value, q.y4().value),
    ];
    let (mut min_x, mut min_y) = corners[0];
    let (mut max_x, mut max_y) = corners[0];
    for (x, y) in corners {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    PdfRect::new_from_values(min_y, min_x, max_y, max_x)
}

/// Bestimmt den Zeichenrang eines Textsegments aus den Textobjekten, die darin
/// liegen. Bei mehreren gewinnt der **kleinste** Rang: Das zusammengefasste
/// Segment wird so früh gemalt wie sein frühestes Teilstück.
///
/// Findet sich kein enthaltenes Objekt (Rundungen, Rotation), greift das
/// nächstgelegene. Ohne jedes Textobjekt bleibt es bei 0 — dann gibt es auch
/// nichts, wogegen sich der Text sortieren müsste.
fn z_for_text(segment: &PdfRect, text_objects: &[(usize, PdfRect)]) -> usize {
    /// Toleranz in pt, damit Rundungsfehler die Zuordnung nicht kippen.
    const TOL: f32 = 1.0;

    let sx = (segment.left().value + segment.right().value) / 2.0;
    let sy = (segment.bottom().value + segment.top().value) / 2.0;

    let mut contained: Option<usize> = None;
    let mut nearest: Option<(f32, usize)> = None;

    for (z, b) in text_objects {
        let bx = (b.left().value + b.right().value) / 2.0;
        let by = (b.bottom().value + b.top().value) / 2.0;

        if bx >= segment.left().value - TOL
            && bx <= segment.right().value + TOL
            && by >= segment.bottom().value - TOL
            && by <= segment.top().value + TOL
        {
            contained = Some(contained.map_or(*z, |c| c.min(*z)));
        }

        let d = (bx - sx) * (bx - sx) + (by - sy) * (by - sy);
        if nearest.is_none_or(|(bd, _)| d < bd) {
            nearest = Some((d, *z));
        }
    }

    contained.or(nearest.map(|(_, z)| z)).unwrap_or(0)
}

/// Extrahiert Text aus der Seite. Verwendet `PdfPageText::segments()`, das
/// pdfium bereits zu logischen Text-Runs zusammenfasst (mit Leerzeichen
/// pro Wort). Wir übernehmen die Run-Grenzen und ergänzen Stil-Infos aus dem
/// ersten Zeichen jeder Run (Bold/Italic/Farbe/Schriftgröße).
///
/// Liefert je Segment auch dessen PDF-Rechteck, damit der Aufrufer den
/// Zeichenrang bestimmen kann.
fn extract_text(
    page: &pdfium_render::prelude::PdfPage,
    page_h: f32,
    next_id: &mut u64,
) -> Vec<(PdfRect, Element)> {
    let mut out = Vec::new();
    let text = match page.text() {
        Ok(t) => t,
        Err(_) => return out,
    };

    for segment in text.segments().iter() {
        let seg_text = segment.text();
        let trimmed = seg_text.trim();
        if trimmed.is_empty() {
            continue;
        }

        let chars = segment.chars().ok();
        let first = chars.as_ref().and_then(|c| c.first().ok());

        // Unsichtbaren Text auslassen. Gescannte PDFs legen die
        // OCR-Erkennung als unsichtbare Textebene **über** das Seitenbild —
        // sichtbar importiert stünde jedes Wort doppelt auf der Seite.
        if first
            .as_ref()
            .and_then(|c| c.render_mode().ok())
            .is_some_and(|m| m == PdfPageTextRenderMode::Invisible)
        {
            continue;
        }

        // Drehung aus der Zeichenmatrix: Sie bildet den Textraum auf die Seite
        // ab, ihre erste Spalte (a, b) ist die Schreibrichtung. PDF-y zeigt
        // nach oben, BoxDoc-y nach unten — deshalb das Vorzeichen.
        let matrix = first.as_ref().and_then(|c| c.matrix().ok());
        let theta_pdf = matrix
            .map(|m| m.b().atan2(m.a()))
            .filter(|t| t.is_finite())
            .unwrap_or(0.0);
        let rotation = -theta_pdf.to_degrees();

        // Stil aus dem ersten Zeichen der Run übernehmen.
        let (size, bold, italic, color) = first
            .as_ref()
            .map(|c| {
                // `scaled_font_size` skaliert nur mit `matrix.d` — bei einer
                // 90-Grad-Drehung ist das 0. Deshalb selbst rechnen: die
                // Schriftgröße mal dem gleichmäßigen Maßstab der Matrix.
                let scale = matrix.map(|m| Mat::from_pdfium(m).scale()).unwrap_or(1.0);
                let size = (c.unscaled_font_size().value * scale)
                    .max(c.scaled_font_size().value)
                    .max(4.0);
                let bold = c.font_is_bold_reenforced();
                let italic = c.font_is_italic();
                let color = c
                    .fill_color()
                    .map(|fc| {
                        [
                            fc.red() as u8,
                            fc.green() as u8,
                            fc.blue() as u8,
                            fc.alpha() as u8,
                        ]
                    })
                    .unwrap_or([20, 20, 20, 255]);
                (size, bold, italic, color)
            })
            .unwrap_or((12.0, false, false, [20, 20, 20, 255]));

        let bounds = segment.bounds();
        let id = *next_id;
        *next_id += 1;

        let mut el = Element::new_text(id, 0.0, 0.0);
        if rotation.abs() < 0.05 {
            // Ungedreht: die Hüllbox des Segments **ist** die Textbox.
            // (PDF: y oben = top, BoxDoc: y wächst nach unten.)
            el.x = bounds.left().value;
            el.y = page_h - bounds.top().value;
            el.w = bounds.width().value.max(size * 0.5);
            el.h = bounds.height().value.max(size);
        } else {
            let (x, y, w, h) = rotated_text_box(rotation, page_h, size, &bounds);
            el.x = x;
            el.y = y;
            el.w = w;
            el.h = h;
            el.rotation = rotation;
            // Gedrehte Boxen werden um ihre **Mitte** gedreht. Würde der
            // Canvas die Höhe später nachrechnen, wanderte der Text seitwärts
            // aus seiner Zeile heraus — deshalb die Höhe hier festhalten und
            // die eine Zeile mittig setzen.
            el.auto_height = false;
            el.valign = VAlign::Middle;
        }
        el.text = trimmed.to_string();
        el.font_size = size;
        el.bold = bold;
        el.italic = italic;
        el.color = color;
        el.align = TextAlign::Left;
        el.valign = VAlign::Top;
        out.push((bounds, el));
    }
    out
}

/// Bestimmt Lage und Größe der Box eines **gedrehten** Textsegments aus deren
/// achsparalleler Hüllbox.
///
/// Direkt übernehmen lässt sich die Hüllbox nicht: Bei 90 Grad wären Breite
/// und Höhe vertauscht, bei schrägen Winkeln beide zu groß. Man kann die
/// gesuchte Box aber ausrechnen. Für eine Box `w × h`, um `θ` gedreht, gilt
///
/// ```text
/// HB_breite = w·|cos θ| + h·|sin θ|
/// HB_hoehe  = w·|sin θ| + h·|cos θ|
/// ```
///
/// — zwei Gleichungen für zwei Unbekannte. Ihre Determinante ist `cos 2θ`; nur
/// nahe 45 Grad wird sie null (dort ist die Hüllbox eines Rechtecks tatsächlich
/// mehrdeutig). Für diesen Fall dient die Schriftgröße als Höhe.
///
/// Der **Mittelpunkt** ist unproblematisch: Die Hüllbox eines gedrehten
/// Rechtecks hat aus Symmetriegründen denselben Mittelpunkt wie das Rechteck.
///
/// Rückgabe: `(x, y, w, h)` der unrotierten Box in Seitenkoordinaten — genau
/// das, was `Element` zusammen mit `rotation` erwartet.
fn rotated_text_box(rotation_deg: f32, page_h: f32, size: f32, aabb: &PdfRect) -> (f32, f32, f32, f32) {
    let aw = aabb.width().value;
    let ah = aabb.height().value;
    let r = rotation_deg.to_radians();
    let (c, s) = (r.cos().abs(), r.sin().abs());
    let det = c * c - s * s;

    let (w, h) = if det.abs() > 0.05 {
        ((c * aw - s * ah) / det, (c * ah - s * aw) / det)
    } else {
        // Nahe 45 Grad: Höhe aus der Schriftgröße, Breite daraus ableiten.
        let h = size;
        (((aw - h * s) / c.max(0.05)).max(size * 0.5), h)
    };

    let w = w.max(size * 0.5);
    // Genau eine Zeile — dieselbe Mindesthöhe, die der Canvas für Textboxen
    // ansetzt (`reflow_auto_height`).
    let h = h.max(size * 1.2);

    let cx = (aabb.left().value + aabb.right().value) / 2.0;
    let cy = page_h - (aabb.bottom().value + aabb.top().value) / 2.0;
    (cx - w / 2.0, cy - h / 2.0, w, h)
}

/// Wandelt ein einzelnes Bild-Objekt in ein Element und legt das PNG im
/// ImageStore ab. `None`, wenn das Bild nicht dekodierbar ist.
fn image_element(
    img: &PdfPageImageObject,
    document: &pdfium_render::prelude::PdfDocument,
    page_h: f32,
    parent: Mat,
    images: &mut ImageStore,
    next_id: &mut u64,
) -> Option<Element> {
    // Bitmap als RGBA holen. `get_processed_bitmap` berücksichtigt Filter und
    // Maskierung (Transparenz), liefert die Pixel aber in der **Rasterlage**
    // des Bilds — eine Drehung der Platzierung steckt nicht darin. Die tragen
    // wir unten selbst nach.
    let bitmap = img.get_processed_bitmap(document).ok()?;
    let px_w = bitmap.width();
    let px_h = bitmap.height();
    if px_w == 0 || px_h == 0 {
        return None;
    }
    let rgba = bitmap.as_rgba_bytes();
    if rgba.is_empty() {
        return None;
    }
    let png = encode_rgba_to_png(&rgba, px_w as u32, px_h as u32);
    if png.is_empty() {
        return None;
    }

    // Die Bildmatrix bildet das Einheitsquadrat auf die Platzierung ab. Ihre
    // Ecken geben Lage, Größe **und** Drehung — eine Hüllbox täte das nicht:
    // Ein um 30 Grad gedrehtes Bild hat eine viel größere Hüllbox als das Bild
    // selbst, und ein um 90 Grad gedrehtes wäre darin schlicht falsch herum.
    let m = parent.then(img.matrix().map(Mat::from_pdfium).unwrap_or(Mat::IDENTITY));
    // In Bildkoordinaten liegt die erste Pixelzeile **oben**, also bei v = 1.
    let to_page = |x: f32, y: f32| {
        let (px, py) = m.apply(x, y);
        Pos2::new(px, page_h - py)
    };
    let top_left = to_page(0.0, 1.0);
    let top_right = to_page(1.0, 1.0);
    let bottom_left = to_page(0.0, 0.0);
    let bottom_right = to_page(1.0, 0.0);

    let across = top_right - top_left;
    let down = bottom_left - top_left;
    let w = across.length();
    let h = down.length();
    if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rotation = across.y.atan2(across.x).to_degrees();
    let center = Pos2::new(
        (top_left.x + top_right.x + bottom_left.x + bottom_right.x) / 4.0,
        (top_left.y + top_right.y + bottom_left.y + bottom_right.y) / 4.0,
    );

    let id = *next_id;
    *next_id += 1;
    images.insert(id, png, (px_w as u32, px_h as u32));
    let mut el = Element::new_image(id, 0, 0, px_w as u32, px_h as u32);
    el.x = center.x - w / 2.0;
    el.y = center.y - h / 2.0;
    el.w = w;
    el.h = h;
    el.rotation = if rotation.abs() < 0.01 { 0.0 } else { rotation };
    Some(el)
}

/// Ein Teilpfad eines PDF-Pfadobjekts, bereits in **BoxDoc-Seitenkoordinaten**
/// (Ursprung oben links, y wächst nach unten).
#[derive(Debug, Clone, Default)]
pub struct SubPath {
    /// **Eckpunkte** in Zeichenreihenfolge — Bézier-Kontrollpunkte sind hier
    /// nicht enthalten. Grundlage der Formerkennung (Rechteck, Linie).
    pub pts: Vec<Pos2>,
    /// Derselbe Zug als **Streckenzug**: Kurven sind in kurze Geraden
    /// aufgelöst. Grundlage der Ellipsen-Prüfung und der Größenabschätzung.
    pub outline: Vec<Pos2>,
    /// Derselbe Zug als **Knotenfolge mit Kurvengriffen** — die Form, in der
    /// BoxDoc einen freien Pfad speichert. Hier bleiben die Bézier-Daten des
    /// PDFs erhalten, statt in Strecken zerlegt zu werden.
    pub nodes: Vec<PathNode>,
    /// Wurde der Teilpfad mit `closepath` geschlossen?
    pub closed: bool,
    /// Anzahl gerader Strecken (LineTo).
    pub lines: usize,
    /// Anzahl **Bézier-Kurven** (nicht Segmente: pdfium liefert je Kurve drei).
    pub curves: usize,
}

/// Eine aus einem Teilpfad erkannte Form — in Seitenkoordinaten, noch ohne
/// Farbe und ID.
#[derive(Debug, Clone, PartialEq)]
pub enum PathShape {
    /// Strecke von `a` nach `b`.
    Line { a: Pos2, b: Pos2 },
    /// Rechteck über seine unrotierte linke obere Ecke, Größe und Drehung
    /// (Grad, im Uhrzeigersinn — dieselbe Konvention wie `Element::rotation`).
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rotation: f32,
    },
    /// Achsparallele Ellipse über ihre Hüllbox.
    Ellipse { x: f32, y: f32, w: f32, h: f32 },
    /// Alles, was keiner Grundform entspricht: der Zug selbst, mit seinen
    /// Kurvengriffen.
    Free {
        nodes: Vec<PathNode>,
        closed: bool,
    },
}

/// Kürzeste Strecke, die noch als Linie übernommen wird (pt).
const MIN_SEGMENT_LEN: f32 = 0.05;

/// Wandelt ein Pfadobjekt in null oder mehr BoxDoc-Elemente.
///
/// Der wesentliche Punkt gegenüber einer reinen Bounding-Box-Auswertung: Eine
/// Linie hat in ihrer Box **keine** erkennbare Richtung. Eine senkrechte und
/// eine waagrechte Linie unterscheiden sich nur darin, welche Seite der Box
/// null ist, eine Diagonale gar nicht von ihrer Gegendiagonale. Deshalb lesen
/// wir die tatsächlichen Stützpunkte des Pfads und leiten Länge und Winkel
/// daraus ab.
fn path_elements(
    path: &PdfPagePathObject,
    page_h: f32,
    parent: Mat,
    next_id: &mut u64,
) -> Vec<Element> {
    // Die Objektmatrix gehört zu den Segmentkoordinaten dazu (`segments()`
    // liefert die rohen Werte), die Matrix des umgebenden Form-XObjects
    // ebenfalls.
    let matrix = parent.then(path.matrix().map(Mat::from_pdfium).unwrap_or(Mat::IDENTITY));
    let mut subpaths = collect_subpaths(path, page_h, matrix);
    if subpaths.is_empty() {
        return Vec::new();
    }
    // Winzige Pfade übergehen — Rundungsartefakte, keine Form.
    let extent = subpaths
        .iter()
        .flat_map(|s| s.outline.iter())
        .fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |(l, t, r, b), p| {
            (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y))
        });
    if extent.2 - extent.0 < 1.0 && extent.3 - extent.1 < 1.0 {
        return Vec::new();
    }

    // Farben übernehmen, falls gesetzt. PdfColor implementiert kein Default,
    // daher nutzen wir einen expliziten Transparenz-Wert als Fallback.
    let transparent = PdfColor::new(0, 0, 0, 0);
    let fill = path.fill_color().unwrap_or(transparent);
    let stroke = path.stroke_color().unwrap_or(transparent);
    // Die Strichstärke steht im Grafikzustand und ist damit noch nicht
    // transformiert; die Matrix skaliert sie mit.
    let stroke_w = path.stroke_width().map(|p| p.value).unwrap_or(0.0) * matrix.scale();
    let has_stroke = path.is_stroked().unwrap_or(false) && stroke_w > 0.0;
    // Ein Pfad ohne Füllmodus zeichnet nur seine Kontur — ihn zu füllen würde
    // im PDF sichtbaren Inhalt überdecken.
    let is_filled = path
        .fill_mode()
        .map(|m| m != PdfPathFillMode::None)
        .unwrap_or(true);

    let fill_color = if is_filled {
        [
            fill.red() as u8,
            fill.green() as u8,
            fill.blue() as u8,
            fill.alpha() as u8,
        ]
    } else {
        [0, 0, 0, 0]
    };
    let stroke_color = [
        stroke.red() as u8,
        stroke.green() as u8,
        stroke.blue() as u8,
        stroke.alpha() as u8,
    ];

    // Füllen schließt einen Teilpfad implizit: `f` braucht kein `h` davor.
    // Ohne diese Regel verschwand jede gefüllte Form, die ihren Umriss nicht
    // ausdrücklich geschlossen hat — und das tun die wenigsten Erzeuger.
    if is_filled {
        for sub in subpaths.iter_mut() {
            if sub.outline.len() > 2 {
                sub.closed = true;
            }
        }
    }

    let mut out = Vec::new();
    for sub in &subpaths {
        for shape in classify_subpath(sub) {
            let id = *next_id;
            let mut el = match shape {
                PathShape::Line { a, b } => {
                    // Eine nicht gestrichene Linie hat im PDF keine sichtbare
                    // Ausdehnung — sie zu importieren erzeugte nur ein
                    // unsichtbares, aber anklickbares Element.
                    if !has_stroke {
                        continue;
                    }
                    line_element(id, a, b)
                }
                PathShape::Rect {
                    x,
                    y,
                    w,
                    h,
                    rotation,
                } => {
                    let mut el = Element::new_rectangle(id, x, y);
                    el.w = w;
                    el.h = h;
                    el.rotation = rotation;
                    el.fill_color = fill_color;
                    el
                }
                PathShape::Ellipse { x, y, w, h } => {
                    let mut el = Element::new_ellipse(id, x, y);
                    el.w = w;
                    el.h = h;
                    el.fill_color = fill_color;
                    el
                }
                PathShape::Free { nodes, closed } => {
                    // Ein offener Zug ohne Kontur wäre unsichtbar.
                    if !closed && !has_stroke {
                        continue;
                    }
                    if nodes.len() < 2 {
                        continue;
                    }
                    let mut el = crate::geometry::path_from_nodes(id, &nodes, closed);
                    el.fill_color = if closed { fill_color } else { [0, 0, 0, 0] };
                    el
                }
            };
            if has_stroke {
                el.stroke_color = stroke_color;
                el.stroke_width = stroke_w;
            } else {
                el.stroke_width = 0.0;
            }
            *next_id += 1;
            out.push(el);
        }
    }
    out
}

/// Obergrenze für Stützpunkte eines importierten Pfads.
///
/// Ein Pfad wird gefüllt, solange das Dokument offen ist, und ein konkaver
/// Umriss kostet dabei quadratisch viel (Ear Clipping). Diese Grenze hält den
/// Aufwand im Rahmen.
///
/// Seit Kurven als Kurven ankommen, greift sie deutlich seltener: Ein Bogen
/// braucht jetzt zwei Knoten statt der bis zu 24 Punkte seiner Auflösung.
/// Was hier noch anschlägt, sind echte Punktwolken — nachgezeichnete
/// Landkarten, Schriftzüge in Umrissen.
const MAX_PATH_POINTS: usize = 256;

/// Dünnt eine Knotenfolge gleichmäßig aus, falls sie zu lang ist.
/// Der letzte Knoten bleibt immer erhalten, damit die Form nicht am Ende
/// abgeschnitten wirkt.
fn decimate_nodes(nodes: &[PathNode]) -> Vec<PathNode> {
    if nodes.len() <= MAX_PATH_POINTS {
        return nodes.to_vec();
    }
    let step = nodes.len().div_ceil(MAX_PATH_POINTS);
    let mut out: Vec<PathNode> = nodes.iter().step_by(step).copied().collect();
    if let Some(last) = nodes.last() {
        if out.last().map(|n| n.anchor) != Some(last.anchor) {
            out.push(*last);
        }
    }
    out
}

/// Baut ein Linien-Element, das exakt von `a` nach `b` verläuft.
///
/// BoxDoc beschreibt eine Linie über ihren **Mittelpunkt**: `x`/`y` sind die
/// linke obere Ecke der (null hohen) Box, `w` die Länge, `rotation` der Winkel
/// um die Boxmitte. Genau diese Umrechnung passiert hier — sie ist die
/// Umkehrung von [`crate::geometry::line_endpoints`].
pub fn line_element(id: u64, a: Pos2, b: Pos2) -> Element {
    let mid = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let len = (b - a).length();
    let mut el = Element::new_line(id, mid.x - len / 2.0, mid.y);
    el.w = len;
    el.h = 0.0;
    el.rotation = (b.y - a.y).atan2(b.x - a.x).to_degrees();
    el.fill_color = [0, 0, 0, 0];
    el
}

/// Liest die Stützpunkte eines Pfadobjekts und zerlegt sie an jedem `MoveTo`
/// in Teilpfade. Die Punkte kommen in Seitenkoordinaten zurück
/// (PDF: y nach oben, BoxDoc: y nach unten → `y = page_h - y_pdf`).
fn collect_subpaths(path: &PdfPagePathObject, page_h: f32, matrix: Mat) -> Vec<SubPath> {
    let mut out: Vec<SubPath> = Vec::new();
    let mut current = SubPath::default();
    // pdfium meldet eine Bézier-Kurve als **drei** Segmente vom Typ BezierTo:
    // zwei Kontrollpunkte und den Endpunkt. Hier gesammelt, unten aufgelöst.
    let mut ctrl: Vec<Pos2> = Vec::with_capacity(2);

    let flush = |current: &mut SubPath, out: &mut Vec<SubPath>| {
        if current.outline.len() > 1 {
            out.push(std::mem::take(current));
        } else {
            *current = SubPath::default();
        }
    };

    for seg in path.segments().iter() {
        let (px, py) = seg.point();
        let (tx, ty) = matrix.apply(px.value, py.value);
        let p = Pos2::new(tx, page_h - ty);
        match seg.segment_type() {
            PdfPathSegmentType::MoveTo => {
                ctrl.clear();
                flush(&mut current, &mut out);
                current.pts.push(p);
                current.outline.push(p);
                current.nodes.push(PathNode::corner(p));
            }
            PdfPathSegmentType::LineTo => {
                ctrl.clear();
                current.lines += 1;
                current.pts.push(p);
                current.outline.push(p);
                current.nodes.push(PathNode::corner(p));
            }
            PdfPathSegmentType::BezierTo => {
                ctrl.push(p);
                if ctrl.len() == 3 {
                    current.curves += 1;
                    let start = *current.outline.last().unwrap_or(&ctrl[0]);
                    flatten_bezier(start, ctrl[0], ctrl[1], ctrl[2], &mut current.outline);
                    current.pts.push(ctrl[2]);
                    // Die Kurve als Kurve behalten: Der erste Kontrollpunkt
                    // gehört als Ausgangsgriff an den vorigen Knoten, der
                    // zweite als Eingangsgriff an den neuen.
                    if let Some(last) = current.nodes.last_mut() {
                        last.out_h = ctrl[0];
                    }
                    current.nodes.push(PathNode {
                        anchor: ctrl[2],
                        in_h: ctrl[1],
                        out_h: ctrl[2],
                    });
                    ctrl.clear();
                }
            }
            PdfPathSegmentType::Unknown => {}
        }
        if seg.is_close() {
            current.closed = true;
        }
    }
    flush(&mut current, &mut out);
    out
}

/// Entfernt aus einer Knotenfolge, was der Formerkennung im Weg steht:
/// Wiederholungen desselben Stützpunkts und — bei geschlossenen Zügen — einen
/// Endknoten, der wieder auf dem Startpunkt liegt.
///
/// Das Gegenstück zu [`dedup_points`], nur dass hier die Griffe mitwandern
/// müssen: Wird der letzte Knoten mit dem ersten verschmolzen, gehört sein
/// Eingangsgriff an den ersten Knoten — sonst verlöre der Abschluss einer
/// geschlossenen Kurve seine Rundung.
fn dedup_nodes(nodes: &[PathNode], closed: bool) -> Vec<PathNode> {
    let mut out: Vec<PathNode> = Vec::with_capacity(nodes.len());
    for n in nodes {
        match out.last_mut() {
            Some(last) if (n.anchor - last.anchor).length() < MIN_SEGMENT_LEN => {
                // Derselbe Punkt zweimal: Griffe zusammenführen, statt einen
                // Knoten mit Länge null stehen zu lassen.
                last.out_h = n.out_h;
            }
            _ => out.push(*n),
        }
    }
    if closed && out.len() > 2 {
        let (first, last) = (out[0], out[out.len() - 1]);
        if (last.anchor - first.anchor).length() < MIN_SEGMENT_LEN {
            out[0].in_h = last.in_h;
            out.pop();
        }
    }
    out
}

/// Löst eine kubische Bézier-Kurve in Strecken auf und hängt sie an `out`
/// (ohne den Startpunkt, der dort schon steht).
///
/// Die Anzahl der Stützstellen richtet sich nach der groben Länge der Kurve:
/// Eine 2 pt lange Rundung braucht keine 24 Punkte, ein großer Bogen schon.
fn flatten_bezier(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, out: &mut Vec<Pos2>) {
    let rough = (p1 - p0).length() + (p2 - p1).length() + (p3 - p2).length();
    let steps = ((rough / 3.0).ceil() as usize).clamp(4, 24);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let u = 1.0 - t;
        let x = u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x;
        let y = u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y;
        out.push(Pos2::new(x, y));
    }
}

/// Erkennt die Form eines Teilpfads.
///
/// Die Reihenfolge ist Absicht: Zuerst wird versucht, eine **Grundform**
/// wiederzuerkennen (Linie, Rechteck, Ellipse), denn die lässt sich in BoxDoc
/// sinnvoll weiterbearbeiten — Größe ziehen, Eckradius, Drehung. Erst wenn das
/// nicht trägt, bleibt der Streckenzug als freier Pfad erhalten. Verworfen
/// wird nichts mehr: Was das PDF zeigt, zeigt BoxDoc auch.
///
/// * offen und gerade → je Strecke eine Linie (ein Polygonzug also mehrere),
/// * offen mit Kurven → freier, offener Pfad,
/// * geschlossen und als echtes Rechteck erkennbar → gedrehtes Rechteck,
/// * geschlossen und nachweislich elliptisch → Ellipse,
/// * sonst geschlossen → freier, geschlossener Pfad.
pub fn classify_subpath(sub: &SubPath) -> Vec<PathShape> {
    let corners = dedup_points(&sub.pts, sub.closed);
    let outline = dedup_points(&sub.outline, sub.closed);
    let nodes = decimate_nodes(&dedup_nodes(&sub.nodes, sub.closed));
    if outline.len() < 2 {
        return Vec::new();
    }

    if !sub.closed {
        if sub.curves == 0 && corners.len() >= 2 {
            return corners
                .windows(2)
                .filter(|w| (w[1] - w[0]).length() >= MIN_SEGMENT_LEN)
                .map(|w| PathShape::Line { a: w[0], b: w[1] })
                .collect();
        }
        return vec![PathShape::Free {
            nodes,
            closed: false,
        }];
    }

    if sub.curves == 0 {
        if let Some(r) = rect_from_corners(&corners) {
            return vec![r];
        }
    }
    // Ellipsen auch ohne Kurvensegmente prüfen: BoxDoc schreibt seine eigenen
    // Ellipsen als Vieleck ins PDF, damit Bildschirm und Druck garantiert
    // dieselbe Form zeigen. Ohne diesen Zweig käme eine exportierte Ellipse
    // als Pfad zurück.
    if let Some(e) = ellipse_from_outline(&outline) {
        return vec![e];
    }

    if outline.len() < 3 {
        return Vec::new();
    }
    vec![PathShape::Free { nodes, closed: true }]
}

/// Prüft, ob ein geschlossener Streckenzug eine achsparallele Ellipse ist, und
/// liefert sie über ihre Hüllbox.
///
/// Geprüft wird nicht die Anzahl der Bögen, sondern die Form selbst: Jeder
/// Punkt muss die Ellipsengleichung erfüllen. Damit fällt ein abgerundetes
/// Rechteck — vier Bögen und vier Geraden, früher als Ellipse importiert —
/// zuverlässig durch und bleibt als Pfad erhalten.
fn ellipse_from_outline(pts: &[Pos2]) -> Option<PathShape> {
    if pts.len() < 8 {
        return None;
    }
    let (mut min_x, mut min_y) = (f32::MAX, f32::MAX);
    let (mut max_x, mut max_y) = (f32::MIN, f32::MIN);
    for p in pts {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    let (w, h) = (max_x - min_x, max_y - min_y);
    if w < MIN_SEGMENT_LEN || h < MIN_SEGMENT_LEN {
        return None;
    }
    let (cx, cy) = (min_x + w / 2.0, min_y + h / 2.0);
    let (rx, ry) = (w / 2.0, h / 2.0);

    // Toleranz: rund 1 % der Halbachse. Die geprüften Punkte liegen exakt auf
    // der Kurve (wir werten die Bézier-Formel an Parameterwerten aus) — der
    // Fehler des Abflachens steckt in den Sehnen dazwischen, nicht in den
    // Punkten. Die übliche Vier-Bogen-Näherung eines Kreises weicht um unter
    // 0,03 % ab, ein Vieleck gar nicht. Alles andere fällt durch.
    for p in pts {
        let dx = (p.x - cx) / rx;
        let dy = (p.y - cy) / ry;
        if (dx * dx + dy * dy - 1.0).abs() > 0.02 {
            return None;
        }
    }
    Some(PathShape::Ellipse {
        x: min_x,
        y: min_y,
        w,
        h,
    })
}

/// Entfernt aufeinanderfolgende Dopplungen und — bei geschlossenen Pfaden —
/// einen abschließenden Punkt, der wieder auf dem Startpunkt liegt.
fn dedup_points(pts: &[Pos2], closed: bool) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = Vec::with_capacity(pts.len());
    for p in pts {
        if out
            .last()
            .is_none_or(|last| (*p - *last).length() >= MIN_SEGMENT_LEN)
        {
            out.push(*p);
        }
    }
    if closed && out.len() > 2 {
        if let (Some(first), Some(last)) = (out.first().copied(), out.last().copied()) {
            if (last - first).length() < MIN_SEGMENT_LEN {
                out.pop();
            }
        }
    }
    out
}

/// Erkennt vier Punkte als Rechteck (auch gedreht) und liefert es in
/// BoxDoc-Konvention: unrotierte linke obere Ecke, Größe, Drehwinkel.
fn rect_from_corners(pts: &[Pos2]) -> Option<PathShape> {
    if pts.len() != 4 {
        return None;
    }
    let e = [
        pts[1] - pts[0],
        pts[2] - pts[1],
        pts[3] - pts[2],
        pts[0] - pts[3],
    ];
    let (mut w, mut h) = (e[0].length(), e[1].length());
    if w < MIN_SEGMENT_LEN || h < MIN_SEGMENT_LEN {
        return None;
    }
    // Gegenüberliegende Kanten müssen gleich lang und entgegengesetzt sein …
    let tol = w.max(h) * 0.01 + 0.05;
    if (e[0] + e[2]).length() > tol || (e[1] + e[3]).length() > tol {
        return None;
    }
    // … benachbarte senkrecht aufeinander stehen.
    if e[0].dot(e[1]).abs() > w * h * 0.02 {
        return None;
    }

    let cx = pts.iter().map(|p| p.x).sum::<f32>() / 4.0;
    let cy = pts.iter().map(|p| p.y).sum::<f32>() / 4.0;

    // Ein Rechteck ist punktsymmetrisch: Drehungen um 180° zeigen dasselbe
    // Bild. Wir wählen den Winkel im Bereich (-90°, 90°] …
    let mut rotation = e[0].y.atan2(e[0].x).to_degrees();
    if rotation > 90.0 {
        rotation -= 180.0;
    } else if rotation <= -90.0 {
        rotation += 180.0;
    }
    // … und richten achsparallele Rechtecke exakt aus, statt sie mit einer
    // Drehung von 90° zu beschreiben.
    if rotation.abs() < 0.05 {
        rotation = 0.0;
    } else if (rotation.abs() - 90.0).abs() < 0.05 {
        std::mem::swap(&mut w, &mut h);
        rotation = 0.0;
    }

    Some(PathShape::Rect {
        x: cx - w / 2.0,
        y: cy - h / 2.0,
        w,
        h,
        rotation,
    })
}

/// Kodiert eine RGBA-Pixel-Liste als PNG-Bytes. Nutzt die `image`-Crate,
/// die bereits in BoxDoc gelinkt ist.
fn encode_rgba_to_png(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    use image::{ImageBuffer, RgbaImage};
    let needed = (w as usize).saturating_mul(h as usize).saturating_mul(4);
    if rgba.len() < needed {
        return Vec::new();
    }
    let img: RgbaImage = ImageBuffer::from_raw(w, h, rgba[..needed].to_vec())
        .unwrap_or_else(|| ImageBuffer::new(w, h));
    let mut buf = std::io::Cursor::new(Vec::new());
    if image::DynamicImage::ImageRgba8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .is_err()
    {
        return Vec::new();
    }
    buf.into_inner()
}
