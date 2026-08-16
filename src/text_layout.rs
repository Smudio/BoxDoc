//! Textlayout — die **einzige** Quelle der Wahrheit für Zeilenumbruch,
//! Ausrichtung und Zeilenposition.
//!
//! Vorher rechneten Canvas und PDF-Export jeweils eigenständig: Der Canvas
//! nutzte egui's Galley (mit echtem Umbruch), der PDF-Export nur
//! `text.split('\n')` plus eine grobe Breitenschätzung. Ergebnis: Ein Absatz,
//! der auf dem Bildschirm über fünf Zeilen umbrach, lief im PDF als eine Zeile
//! aus der Seite heraus.
//!
//! Dieses Modul löst das, indem beide Seiten dasselbe Layout benutzen — egui's
//! Schriftsystem mit den echten Font-Metriken. Was der Nutzer sieht, ist damit
//! per Konstruktion das, was im PDF landet.
//!
//! **Einheiten:** Alle Rückgabewerte sind in Punkten (pt), unabhängig vom
//! übergebenen `scale`. Der Canvas layoutet mit `scale = zoom` (damit Glyphen
//! bei hohem Zoom scharf bleiben), der PDF-Export mit `scale = 1.0` — die
//! Umbruchstellen sind identisch, weil Schriftgröße und Umbruchbreite
//! proportional mitskalieren.

use crate::model::{Element, FontStyle, TextAlign, VAlign};

/// Eine fertig umgebrochene, ausgerichtete Textzeile.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidLine {
    /// Der Text dieser Zeile, ohne Zeilenumbruch-Zeichen.
    pub text: String,
    /// X-Offset relativ zur linken Kante der Element-Box, in pt.
    /// Enthält bereits Einzug und horizontale Ausrichtung.
    pub x: f32,
    /// Y-Offset der **Grundlinie** relativ zur Oberkante der Element-Box, in pt.
    pub baseline_y: f32,
    /// Gemessene Breite der Zeile in pt (für Unterstreichung und Debugging).
    pub width: f32,
}

/// Das vollständige Layout eines Textelements.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextLayout {
    pub lines: Vec<LaidLine>,
    /// Gesamthöhe des umgebrochenen Textblocks in pt.
    pub height: f32,
}

/// Bricht den Text von `el` um und richtet ihn aus.
///
/// `scale` skaliert die Layout-Auflösung (Canvas: Zoomfaktor, Export: 1.0).
/// Die Rückgabe ist immer in pt.
pub fn layout(
    fonts: &mut egui::epaint::text::FontsView<'_>,
    el: &Element,
    scale: f32,
) -> TextLayout {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };

    let font_id = font_id_for(el, scale);
    // Umbruchbreite: die Box-Breite abzüglich Einzug. Mindestens 1 px, sonst
    // bricht egui nach jedem Zeichen um.
    let wrap_width = ((el.w - el.indent) * scale).max(1.0);
    let galley = fonts.layout(
        el.text.clone(),
        font_id,
        egui::Color32::WHITE, // Farbe ist hier irrelevant, gemalt wird woanders
        wrap_width,
    );

    let block_h = galley.rect.height() / scale;

    // Vertikale Ausrichtung des gesamten Blocks innerhalb der Box.
    let y_offset = match el.valign {
        VAlign::Top => 0.0,
        VAlign::Middle => (el.h - block_h).max(0.0) / 2.0,
        VAlign::Bottom => (el.h - block_h).max(0.0),
    };

    let mut lines = Vec::with_capacity(galley.rows.len());
    for placed in &galley.rows {
        let text = placed.row.text();
        let width = placed.row.size.x / scale;

        // Horizontale Ausrichtung **je Zeile** — nicht für den Block als
        // Ganzes. Zentrierter Mehrzeilentext soll zeilenweise zentriert sein,
        // so wie es jede Textverarbeitung macht.
        let align_x = match el.align {
            TextAlign::Left => 0.0,
            TextAlign::Center => ((el.w - el.indent) - width).max(0.0) / 2.0,
            TextAlign::Right => ((el.w - el.indent) - width).max(0.0),
        };

        // Grundlinie = Zeilenoberkante + Ascent der Schrift. Bei leeren Zeilen
        // gibt es keine Glyphe, aus der wir den Ascent lesen könnten — dann
        // nähern wir über die übliche Ascent-Proportion.
        let ascent = placed
            .row
            .glyphs
            .first()
            .map(|g| g.font_ascent)
            .unwrap_or(el.font_size * scale * DEFAULT_ASCENT_RATIO);

        lines.push(LaidLine {
            text,
            x: el.indent + align_x,
            baseline_y: y_offset + (placed.pos.y + ascent) / scale,
            width,
        });
    }

    TextLayout {
        lines,
        height: block_h,
    }
}

/// Misst, wie breit der Text von `el` **ohne Umbruch** wäre — in pt.
///
/// Bei mehrzeiligem Text (harte Umbrüche) ist das die Breite der längsten
/// Zeile. Wird vom PDF-Import gebraucht, um Boxen zu weiten, die mit BoxDocs
/// Schriften sonst ungewollt umbrechen würden.
pub fn natural_width(fonts: &mut egui::epaint::text::FontsView<'_>, el: &Element) -> f32 {
    let galley = fonts.layout(
        el.text.clone(),
        font_id_for(el, 1.0),
        egui::Color32::WHITE,
        f32::INFINITY, // kein Umbruch
    );
    galley.rect.width()
}

/// Anteil der Schriftgröße, der typischerweise über der Grundlinie liegt.
/// Nur Rückfallwert für Zeilen ohne Glyphen (Leerzeilen).
const DEFAULT_ASCENT_RATIO: f32 = 0.8;

/// Bestimmt die egui-`FontId` für ein Element, inklusive Fett-/Kursiv-Schnitt.
///
/// Früher galt das nur für die Standardschrift — bei jeder anderen wurde der
/// Schnitt beim Layout stillschweigend fallen gelassen. Auf dem Bildschirm
/// stand dann magere Schrift, im PDF fette; und weil fette Glyphen breiter
/// sind, brachen beide auch an verschiedenen Stellen um.
pub fn font_id_for(el: &Element, scale: f32) -> egui::FontId {
    egui::FontId::new(
        el.font_size * scale,
        crate::fonts::family_for_style(&el.font, FontStyle::of(el)),
    )
}

/// Lage und Stärke der Auszeichnungslinien eines Textelements — in pt,
/// relativ zur **Grundlinie** der jeweiligen Zeile.
///
/// Warum hier und nicht dreimal beim Zeichnen: Unterstreichung und
/// Durchstreichung sind gemalte Linien, keine Glyphen. Rechnete jeder Renderer
/// ihre Lage selbst aus, säße derselbe Strich auf dem Bildschirm, im PDF und
/// im SVG an drei verschiedenen Stellen — genau der Fehler, den dieses Modul
/// für den Zeilenumbruch bereits behoben hat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecorationMetrics {
    /// Abstand der Unterstreichung **unterhalb** der Grundlinie.
    pub underline_dy: f32,
    /// Abstand der Durchstreichung **oberhalb** der Grundlinie. Etwa halbe
    /// x-Höhe, damit der Strich durch die Mitte der Kleinbuchstaben läuft.
    pub strike_dy: f32,
    /// Strichstärke beider Linien.
    pub thickness: f32,
}

/// Die Auszeichnungs-Maße zu einer Schriftgröße.
///
/// Die Anteile sind die üblichen typografischen Verhältnisse; sie aus der
/// Schriftdatei zu lesen wäre genauer, ist über egui aber nicht zugänglich —
/// und eine falsche, aber überall **gleiche** Lage ist hier mehr wert als eine
/// richtige, die nur einer der drei Renderer kennt.
pub fn decoration_metrics(font_size: f32) -> DecorationMetrics {
    DecorationMetrics {
        underline_dy: font_size * 0.14,
        strike_dy: font_size * 0.26,
        thickness: (font_size * 0.06).max(0.4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Element, TextAlign, VAlign};

    /// Baut einen egui-Kontext mit den Standardschriften, wie ihn die App hat.
    fn test_fonts() -> egui::Context {
        let ctx = egui::Context::default();
        // Ein Frame durchlaufen, damit die Schriften geladen sind.
        let _ = ctx.run(Default::default(), |_| {});
        ctx
    }

    fn text_el(text: &str, w: f32, size: f32) -> Element {
        let mut el = Element::new_text(1, 0.0, 0.0);
        el.text = text.to_string();
        el.w = w;
        el.h = 500.0;
        el.font_size = size;
        el
    }

    #[test]
    fn langer_text_wird_umgebrochen() {
        let ctx = test_fonts();
        let el = text_el(
            "Dies ist ein bewusst langer Absatz, der in einer schmalen Box \
             zwingend über mehrere Zeilen umbrechen muss.",
            120.0,
            12.0,
        );
        let layout = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        assert!(
            layout.lines.len() > 1,
            "Text muss umbrechen, hat aber nur {} Zeile(n)",
            layout.lines.len()
        );
    }

    #[test]
    fn umbruch_ist_zoomunabhaengig() {
        // Der Kern der WYSIWYG-Garantie: Canvas (scale = zoom) und PDF
        // (scale = 1.0) müssen an denselben Stellen umbrechen.
        let ctx = test_fonts();
        let el = text_el(
            "Ein Absatz mittlerer Länge, der über mehrere Zeilen läuft und \
             damit den Umbruch tatsächlich beansprucht.",
            160.0,
            11.0,
        );
        let at_1 = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        for scale in [0.5_f32, 2.0, 3.0] {
            let scaled = ctx.fonts_mut(|f| layout(f, &el, scale));
            let a: Vec<&str> = at_1.lines.iter().map(|l| l.text.as_str()).collect();
            let b: Vec<&str> = scaled.lines.iter().map(|l| l.text.as_str()).collect();
            assert_eq!(a, b, "Umbruch weicht bei scale={scale} ab");
        }
    }

    #[test]
    fn explizite_zeilenumbrueche_bleiben_erhalten() {
        let ctx = test_fonts();
        let el = text_el("Zeile A\nZeile B\nZeile C", 400.0, 12.0);
        let layout = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        let texts: Vec<&str> = layout.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, vec!["Zeile A", "Zeile B", "Zeile C"]);
    }

    #[test]
    fn zentrierung_wirkt_je_zeile() {
        let ctx = test_fonts();
        let mut el = text_el("kurz\nviel laengere Zeile hier", 400.0, 12.0);
        el.align = TextAlign::Center;
        let layout = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        assert_eq!(layout.lines.len(), 2);
        // Die kürzere Zeile muss weiter eingerückt sein als die längere.
        assert!(
            layout.lines[0].x > layout.lines[1].x,
            "kurze Zeile x={} muss > lange Zeile x={} sein",
            layout.lines[0].x,
            layout.lines[1].x
        );
    }

    #[test]
    fn rechtsbuendig_endet_buendig() {
        let ctx = test_fonts();
        let mut el = text_el("kurz\nviel laengere Zeile hier", 400.0, 12.0);
        el.align = TextAlign::Right;
        let layout = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        // Rechte Kante = x + width muss für alle Zeilen gleich sein.
        let right0 = layout.lines[0].x + layout.lines[0].width;
        let right1 = layout.lines[1].x + layout.lines[1].width;
        assert!(
            (right0 - right1).abs() < 0.01,
            "rechte Kanten weichen ab: {right0} vs {right1}"
        );
    }

    #[test]
    fn valign_verschiebt_den_block() {
        let ctx = test_fonts();
        let mut el = text_el("eine Zeile", 400.0, 12.0);
        el.h = 200.0;

        el.valign = VAlign::Top;
        let top = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        el.valign = VAlign::Middle;
        let middle = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        el.valign = VAlign::Bottom;
        let bottom = ctx.fonts_mut(|f| layout(f, &el, 1.0));

        assert!(top.lines[0].baseline_y < middle.lines[0].baseline_y);
        assert!(middle.lines[0].baseline_y < bottom.lines[0].baseline_y);
    }

    #[test]
    fn einzug_verschiebt_nach_rechts_und_verengt() {
        let ctx = test_fonts();
        let mut el = text_el("Text", 400.0, 12.0);
        el.indent = 50.0;
        let layout = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        assert!((layout.lines[0].x - 50.0).abs() < 0.01);
    }

    #[test]
    fn leerer_text_ergibt_kein_layout_chaos() {
        let ctx = test_fonts();
        let el = text_el("", 400.0, 12.0);
        let layout = ctx.fonts_mut(|f| layout(f, &el, 1.0));
        // Genau eine (leere) Zeile, endliche Höhe.
        assert!(layout.height.is_finite());
        assert!(layout.lines.len() <= 1);
    }
}
