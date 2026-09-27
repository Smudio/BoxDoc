//! Kern-Datenmodell des Dokuments.
//!
//! Alles arbeitet in "Punkten" (1/72 Zoll), damit die Darstellung auf dem
//! Bildschirm, beim Drucken und im PDF identisch ist.

use serde::{Deserialize, Serialize};

/// Benutzerdefiniertes Papierformat: Name plus Maße in Millimetern
/// (Hochformat). Wird mit Name und Abmessungen in der `.boxdoc`-Datei
/// gespeichert und ist damit portabel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomFormat {
    pub name: String,
    /// Breite in mm (Hochformat).
    pub w_mm: f32,
    /// Höhe in mm (Hochformat).
    pub h_mm: f32,
}

/// Papierformate. Die Größe wird in Millimetern angegeben (Hochformat).
///
/// `Custom` trägt seine Definition selbst (Name + Maße), damit überall, wo
/// nur ein `PaperFormat` übergeben wird, auch ein eigenes Format ohne
/// Zusatzkontext aufgelöst werden kann.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaperFormat {
    A3,
    A4,
    A5,
    Letter,
    Legal,
    /// Benutzerdefiniertes Format. In der Datei:
    /// `"format": { "Custom": { "name": "...", "w_mm": 210.0, "h_mm": 297.0 } }`
    Custom(CustomFormat),
}

impl PaperFormat {
    /// Anzeigename. Bei Standardformaten die bekannte Bezeichnung, bei
    /// Custom der vergebene Name.
    pub fn label(&self) -> String {
        match self {
            PaperFormat::A3 => String::from("A3"),
            PaperFormat::A4 => String::from("A4"),
            PaperFormat::A5 => String::from("A5"),
            PaperFormat::Letter => String::from("Letter"),
            PaperFormat::Legal => String::from("Legal"),
            PaperFormat::Custom(c) => c.name.clone(),
        }
    }

    pub fn all() -> [PaperFormat; 5] {
        [PaperFormat::A4, PaperFormat::A3, PaperFormat::A5, PaperFormat::Letter, PaperFormat::Legal]
    }

    /// (Breite, Höhe) in Millimeter, Hochformat.
    pub fn size_mm(&self) -> (f32, f32) {
        match self {
            PaperFormat::A3 => (297.0, 420.0),
            PaperFormat::A4 => (210.0, 297.0),
            PaperFormat::A5 => (148.0, 210.0),
            PaperFormat::Letter => (215.9, 279.4),
            PaperFormat::Legal => (215.9, 355.6),
            PaperFormat::Custom(c) => (c.w_mm, c.h_mm),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Portrait,
    Landscape,
}

/// Millimeter -> Punkt.
pub fn mm_to_pt(mm: f32) -> f32 {
    mm * 72.0 / 25.4
}

/// Punkt -> Millimeter.
pub fn pt_to_mm(pt: f32) -> f32 {
    pt * 25.4 / 72.0
}

/// (Breite, Höhe) der Seite in Punkten.
pub fn page_size_pt(format: &PaperFormat, orientation: Orientation) -> (f32, f32) {
    let (w, h) = format.size_mm();
    let (w, h) = (mm_to_pt(w), mm_to_pt(h));
    match orientation {
        Orientation::Portrait => (w, h),
        Orientation::Landscape => (h, w),
    }
}

// ===========================================================================
// Einstellungen (Settings)
// ===========================================================================

/// Maßeinheit für die Anzeige in den Eigenschaften. Intern wird immer in
/// Punkten (pt) gerechnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Units {
    Pt,
    Mm,
    Cm,
    Inch,
}

impl Default for Units {
    fn default() -> Self {
        Units::Cm
    }
}

impl Units {
    pub fn label(self) -> &'static str {
        match self {
            Units::Pt => "pt",
            Units::Mm => "mm",
            Units::Cm => "cm",
            Units::Inch => "zoll",
        }
    }

    pub fn all() -> [Units; 4] {
        [Units::Cm, Units::Mm, Units::Pt, Units::Inch]
    }

    /// Punkt-Wert in die Anzeige-Einheit umrechnen.
    pub fn from_pt(self, pt: f32) -> f32 {
        match self {
            Units::Pt => pt,
            Units::Mm => pt * 25.4 / 72.0,
            Units::Cm => pt * 2.54 / 72.0,
            Units::Inch => pt / 72.0,
        }
    }

    /// Anzeige-Wert zurück in Punkte umrechnen.
    pub fn to_pt(self, val: f32) -> f32 {
        match self {
            Units::Pt => val,
            Units::Mm => val * 72.0 / 25.4,
            Units::Cm => val * 72.0 / 2.54,
            Units::Inch => val * 72.0,
        }
    }

    /// Passende Zieh-Geschwindigkeit für DragValue pro Einheit — mm/pt
    /// ändern sich grob, cm/zoll fein.
    pub fn drag_speed(self) -> f32 {
        match self {
            Units::Pt => 1.0,
            Units::Mm => 1.0,
            Units::Cm => 0.1,
            Units::Inch => 0.05,
        }
    }
}

/// Horizontale Ausrichtung der Seite auf der Zeichenfläche.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PageAlign {
    Left,
    Center,
    Right,
}

impl Default for PageAlign {
    fn default() -> Self {
        PageAlign::Center
    }
}

impl PageAlign {
    pub fn label(self) -> &'static str {
        match self {
            PageAlign::Left => "Links",
            PageAlign::Center => "Mittig",
            PageAlign::Right => "Rechts",
        }
    }
}

/// Wie beim Scrollen zwischen Seiten gewechselt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScrollMode {
    /// Runterscrollen am Seitenende springt automatisch zur nächsten Seite.
    Continuous,
    /// Seiten werden nur über das Eigenschaften-Panel gewechselt.
    PageByPage,
}

impl Default for ScrollMode {
    fn default() -> Self {
        ScrollMode::PageByPage
    }
}

impl ScrollMode {
    pub fn label(self) -> &'static str {
        match self {
            ScrollMode::Continuous => "Fortlaufend (Scrollen wechselt Seite)",
            ScrollMode::PageByPage => "Seitenweise (über Eigenschaften)",
        }
    }
}

/// Farb-Thema der Anwendung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    Light,
    DarkClassic,
    DarkCalm,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Light
    }
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::Light => "Hell",
            Theme::DarkClassic => "Dunkel (Klassisch)",
            Theme::DarkCalm => "Dunkel (Calm)",
        }
    }

    pub fn all() -> [Theme; 3] {
        [Theme::Light, Theme::DarkClassic, Theme::DarkCalm]
    }
}

/// Position des Eigenschaften-Panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PanelSide {
    Right,
    Left,
    Bottom,
}

impl Default for PanelSide {
    fn default() -> Self {
        PanelSide::Right
    }
}

impl PanelSide {
    pub fn label(self) -> &'static str {
        match self {
            PanelSide::Right => "Rechts",
            PanelSide::Left => "Links",
            PanelSide::Bottom => "Unten",
        }
    }

    pub fn all() -> [PanelSide; 3] {
        [PanelSide::Right, PanelSide::Left, PanelSide::Bottom]
    }
}

/// Globale Anwendungseinstellungen.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub units: Units,
    pub page_align: PageAlign,
    pub scroll_mode: ScrollMode,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub panel_side: PanelSide,
}

// ===========================================================================
// Schriften
// ===========================================================================

/// Default-Schrift-Schlüssel (für `#[serde(default)]`).
pub fn default_font_key() -> String {
    String::from("default")
}

/// Schnitt einer Schrift — die vier Kombinationen aus fett und kursiv.
///
/// Eigener Typ statt zweier `bool`, weil der Schnitt durch drei Module
/// gereicht wird (Registrierung in `fonts`, Layout in `text_layout`, Einbetten
/// in `printing`). Ein Paar aus zwei gleichnamigen Wahrheitswerten lässt sich
/// beim Durchreichen lautlos vertauschen; ein benannter Wert nicht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    pub fn new(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => FontStyle::Regular,
            (true, false) => FontStyle::Bold,
            (false, true) => FontStyle::Italic,
            (true, true) => FontStyle::BoldItalic,
        }
    }

    /// Der Schnitt eines Textelements.
    pub fn of(el: &Element) -> Self {
        FontStyle::new(el.bold, el.italic)
    }

    /// Suffix des egui-Familiennamens. Der Regular-Schnitt bekommt keines,
    /// damit bestehende Dokumente und der `family_for`-Pfad unverändert
    /// weiterlaufen.
    pub fn suffix(self) -> &'static str {
        match self {
            FontStyle::Regular => "",
            FontStyle::Bold => ":b",
            FontStyle::Italic => ":i",
            FontStyle::BoldItalic => ":bi",
        }
    }

    pub fn bold(self) -> bool {
        matches!(self, FontStyle::Bold | FontStyle::BoldItalic)
    }

    pub fn italic(self) -> bool {
        matches!(self, FontStyle::Italic | FontStyle::BoldItalic)
    }

    pub fn all() -> [FontStyle; 4] {
        [
            FontStyle::Regular,
            FontStyle::Bold,
            FontStyle::Italic,
            FontStyle::BoldItalic,
        ]
    }
}

/// Ein kuratierter Satz schöner Schriften. Der Name dient als Schlüssel in
/// egui und als Anzeige im UI; `key` ist der technische Bezeichner, der im
/// Element gespeichert wird. So bleibt das Dokument portabel, auch wenn eine
/// Schrift auf dem Zielsystem fehlt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontDef {
    pub key: &'static str,
    pub display: &'static str,
    /// Kandidaten-Pfade (betriebssystemspezifisch); der erste Treffer wird
    /// geladen. Bleibt die Liste leer, fällt egui auf seinen Default zurück.
    pub paths: &'static [&'static str],
    /// Kandidaten-Pfade der **echten** Fett-, Kursiv- und Fett-Kursiv-Schnitte.
    ///
    /// Ein echter Schnitt ist einem nachgeahmten immer vorzuziehen: Er hat
    /// eigene Glyphenformen und eigene Breiten. Fehlt er, wird fett über einen
    /// Umriss und kursiv über eine Scherung angenähert — was anders aussieht
    /// und, schlimmer, anders breit ist. Leere Liste = kein echter Schnitt.
    pub bold_paths: &'static [&'static str],
    pub italic_paths: &'static [&'static str],
    pub bold_italic_paths: &'static [&'static str],
    /// Eingebettet via include_bytes! (funktioniert auch im Browser).
    pub bundled: bool,
}

impl FontDef {
    /// Kandidaten-Pfade für einen Schnitt.
    pub fn paths_for(&self, style: FontStyle) -> &'static [&'static str] {
        match style {
            FontStyle::Regular => self.paths,
            FontStyle::Bold => self.bold_paths,
            FontStyle::Italic => self.italic_paths,
            FontStyle::BoldItalic => self.bold_italic_paths,
        }
    }
}

/// Kuratierte Auswahl. Index 0 ist die Standard-Schrift.
///
/// Die eingebetteten Schriften liegen nur als Regular-Schnitt in der Binary —
/// je vier Schnitte würden sie vervierfachen, und im Browser zählt jedes
/// Kilobyte. Fett und kursiv werden dort nachgeahmt. Die System-Schriften
/// bringen ihre echten Schnitte mit.
pub const FONT_CHOICES: &[FontDef] = &[
    FontDef {
        key: "default",
        display: "Standard",
        paths: &[],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: false,
    },
    // --- Eingebettete Schriften (Web + Desktop) ---
    FontDef {
        key: "inter",
        display: "Inter",
        paths: &[],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: true,
    },
    FontDef {
        key: "roboto",
        display: "Roboto",
        paths: &[],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: true,
    },
    FontDef {
        key: "lora",
        display: "Lora",
        paths: &[],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: true,
    },
    FontDef {
        key: "jetbrains",
        display: "JetBrains Mono",
        paths: &[],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: true,
    },
    FontDef {
        key: "pacifico",
        display: "Pacifico",
        paths: &[],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: true,
    },
    // --- System-Schriften (nur Desktop) ---
    FontDef {
        key: "arial",
        display: "Arial",
        paths: &[
            "C:\\Windows\\Fonts\\arial.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ],
        bold_paths: &[
            "C:\\Windows\\Fonts\\arialbd.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        ],
        italic_paths: &[
            "C:\\Windows\\Fonts\\ariali.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Italic.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf",
        ],
        bold_italic_paths: &[
            "C:\\Windows\\Fonts\\arialbi.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-BoldItalic.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-BoldOblique.ttf",
        ],
        bundled: false,
    },
    FontDef {
        key: "calibri",
        display: "Calibri",
        paths: &[
            "C:\\Windows\\Fonts\\calibri.ttf",
            "/usr/share/fonts/truetype/calibri/Calibri-Regular.ttf",
        ],
        bold_paths: &["C:\\Windows\\Fonts\\calibrib.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\calibrii.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\calibriz.ttf"],
        bundled: false,
    },
    FontDef {
        key: "cambria",
        display: "Cambria",
        paths: &[
            "C:\\Windows\\Fonts\\cambria.ttc",
            "/usr/share/fonts/truetype/cambria/Cambria.ttf",
        ],
        bold_paths: &["C:\\Windows\\Fonts\\cambriab.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\cambriai.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\cambriaz.ttf"],
        bundled: false,
    },
    FontDef {
        key: "georgia",
        display: "Georgia",
        paths: &[
            "C:\\Windows\\Fonts\\georgia.ttf",
            "/usr/share/fonts/truetype/georgia/Georgia.ttf",
        ],
        bold_paths: &["C:\\Windows\\Fonts\\georgiab.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\georgiai.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\georgiaz.ttf"],
        bundled: false,
    },
    FontDef {
        key: "verdana",
        display: "Verdana",
        paths: &["C:\\Windows\\Fonts\\verdana.ttf"],
        bold_paths: &["C:\\Windows\\Fonts\\verdanab.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\verdanai.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\verdanaz.ttf"],
        bundled: false,
    },
    FontDef {
        key: "tahoma",
        display: "Tahoma",
        paths: &["C:\\Windows\\Fonts\\tahoma.ttf"],
        // Tahoma liefert nur einen Fett-Schnitt mit; kursiv wird geschert.
        bold_paths: &["C:\\Windows\\Fonts\\tahomabd.ttf"],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: false,
    },
    FontDef {
        key: "trebuc",
        display: "Trebuchet MS",
        paths: &["C:\\Windows\\Fonts\\trebuc.ttf"],
        bold_paths: &["C:\\Windows\\Fonts\\trebucbd.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\trebucit.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\trebucbi.ttf"],
        bundled: false,
    },
    FontDef {
        key: "palatino",
        display: "Palatino Linotype",
        paths: &["C:\\Windows\\Fonts\\pala.ttf"],
        bold_paths: &["C:\\Windows\\Fonts\\palab.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\palai.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\palabi.ttf"],
        bundled: false,
    },
    FontDef {
        key: "segoeui",
        display: "Segoe UI",
        paths: &["C:\\Windows\\Fonts\\segoeui.ttf"],
        bold_paths: &["C:\\Windows\\Fonts\\segoeuib.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\segoeuii.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\segoeuiz.ttf"],
        bundled: false,
    },
    FontDef {
        key: "consolas",
        display: "Consolas",
        paths: &[
            "C:\\Windows\\Fonts\\consola.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        ],
        bold_paths: &[
            "C:\\Windows\\Fonts\\consolab.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
        ],
        italic_paths: &[
            "C:\\Windows\\Fonts\\consolai.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Oblique.ttf",
        ],
        bold_italic_paths: &[
            "C:\\Windows\\Fonts\\consolaz.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-BoldOblique.ttf",
        ],
        bundled: false,
    },
    FontDef {
        key: "gabriola",
        display: "Gabriola",
        // Zierschrift, nur ein Schnitt.
        paths: &["C:\\Windows\\Fonts\\Gabriola.ttf"],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: false,
    },
    FontDef {
        key: "inkfree",
        display: "Ink Free",
        paths: &["C:\\Windows\\Fonts\\Inkfree.ttf"],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: false,
    },
    FontDef {
        key: "comic",
        display: "Comic Sans MS",
        paths: &["C:\\Windows\\Fonts\\comic.ttf"],
        bold_paths: &["C:\\Windows\\Fonts\\comicbd.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\comici.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\comicz.ttf"],
        bundled: false,
    },
    FontDef {
        key: "impact",
        display: "Impact",
        // Impact ist bereits ein fetter Schnitt und hat keine weiteren.
        paths: &["C:\\Windows\\Fonts\\impact.ttf"],
        bold_paths: &[],
        italic_paths: &[],
        bold_italic_paths: &[],
        bundled: false,
    },
    FontDef {
        key: "candara",
        display: "Candara",
        paths: &["C:\\Windows\\Fonts\\Candara.ttf"],
        bold_paths: &["C:\\Windows\\Fonts\\Candarab.ttf"],
        italic_paths: &["C:\\Windows\\Fonts\\Candarai.ttf"],
        bold_italic_paths: &["C:\\Windows\\Fonts\\Candaraz.ttf"],
        bundled: false,
    },
];

/// Schlüssel zur Anzeige.
pub fn font_display(key: &str) -> &'static str {
    FONT_CHOICES
        .iter()
        .find(|f| f.key == key)
        .map(|f| f.display)
        .unwrap_or("Unbekannt")
}

/// Index des Schlüssels (für ComboBox).
pub fn font_index(key: &str) -> usize {
    FONT_CHOICES
        .iter()
        .position(|f| f.key == key)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Vertikale Textausrichtung innerhalb der Element-Box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

impl Default for VAlign {
    fn default() -> Self {
        VAlign::Top
    }
}

/// Nicht-destruktiver Bildausschnitt, normalisiert auf [0.0, 1.0].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }
    }
}

impl Crop {
    pub fn clamp(self) -> Self {
        let x = self.x.clamp(0.0, 1.0);
        let y = self.y.clamp(0.0, 1.0);
        let w = self.w.clamp(0.01, 1.0 - x);
        let h = self.h.clamp(0.01, 1.0 - y);
        Crop { x, y, w, h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ElementKind {
    Text,
    Image,
    Rectangle,
    Line,
    Ellipse,
    /// Freier Streckenzug (siehe `Element::points`).
    Path,
}

/// Ein einzelnes Objekt auf der Seite: Text oder Bild.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub id: u64,
    pub kind: ElementKind,

    /// Position der linken oberen Ecke (unrotiert), in Punkten.
    pub x: f32,
    pub y: f32,
    /// Größe (unrotiert), in Punkten.
    pub w: f32,
    pub h: f32,
    /// Drehwinkel in Grad.
    pub rotation: f32,

    // --- Text ---
    pub text: String,
    pub font_size: f32,
    /// Schrift-Schlüssel (siehe FONT_CHOICES / font_index).
    #[serde(default = "default_font_key")]
    pub font: String,
    pub color: [u8; 4],
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    /// Durchgestrichen. Wie `underline` eine gezeichnete Linie, kein
    /// Schriftschnitt — sie liegt auf halber x-Höhe über der Grundlinie.
    #[serde(default)]
    pub strikethrough: bool,
    pub align: TextAlign,
    #[serde(default)]
    pub valign: VAlign,
    /// Einzug jeder Zeile in Punkten.
    pub indent: f32,
    /// Nur für `Text`: Wächst die Box automatisch mit dem Inhalt?
    ///
    /// `true` (Standard): `h` wird bei jedem Reflow aus dem umgebrochenen Text
    /// berechnet. `valign` hat dann keine Wirkung, weil die Box exakt so hoch
    /// ist wie ihr Inhalt.
    ///
    /// `false`: Der Nutzer hat die Höhe selbst festgelegt (Unterkante gezogen).
    /// Der Text wird innerhalb dieser festen Höhe gemäß `valign` ausgerichtet.
    #[serde(default = "default_true")]
    pub auto_height: bool,

    // --- Bild ---
    pub crop: Crop,
    /// Originale Pixelgröße des geladenen Bilds.
    pub image_w: u32,
    pub image_h: u32,

    // --- Shape (Rechteck / Linie) ---
    /// Füllfarbe (RGBA). Alpha = 0 → transparent.
    #[serde(default = "default_fill_color")]
    pub fill_color: [u8; 4],
    /// Rahmen-Stärke in pt (0 = kein Rahmen).
    #[serde(default = "default_stroke_width")]
    pub stroke_width: f32,
    /// Rahmen-Farbe.
    #[serde(default = "default_stroke_color")]
    pub stroke_color: [u8; 4],
    /// Eckradius für Rechtecke.
    #[serde(default)]
    pub corner_radius: f32,

    // --- Pfad ---
    /// Stützpunkte eines freien Streckenzugs, **normalisiert auf die Box**:
    /// `[0,0]` ist die linke obere Ecke, `[1,1]` die rechte untere.
    ///
    /// Warum normalisiert und nicht in Punkten? Weil damit Verschieben,
    /// Skalieren und Drehen eines Pfads dieselben Felder benutzen wie bei
    /// jeder anderen Form (`x`,`y`,`w`,`h`,`rotation`) — die Stützpunkte
    /// bleiben unangetastet. Absolute Punkte müssten bei jedem Ziehen
    /// mitgeführt werden, und jeder vergessene Pfad wäre ein stiller Fehler.
    ///
    /// Nur für `ElementKind::Path` belegt.
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// Kubische Bézier-Griffe je Stützpunkt: `[in_x, in_y, out_x, out_y]`.
    ///
    /// Die Werte sind **absolute Positionen im selben normalisierten Box-Raum
    /// wie `points`** — nicht Abstände zum Stützpunkt. Damit gilt für Griffe
    /// exakt dieselbe Abbildung auf die Box wie für die Stützpunkte selbst;
    /// Verschieben, Skalieren und Drehen brauchen keinen Sonderfall.
    ///
    /// Zwei Invarianten:
    /// * `handles` ist **leer** (= reiner Streckenzug) oder **genau so lang
    ///   wie `points`**. Ein halb gefüllter Vektor ist ein kaputter Pfad;
    ///   [`Element::path_handles_valid`] prüft das beim Laden.
    /// * Ein **Eckknoten** hat beide Griffe auf dem Stützpunkt liegen
    ///   (`in == out == point`). Genau so entsteht aus einem Kurvenzug wieder
    ///   eine Gerade.
    ///
    /// Das Feld fehlt in der Datei, solange es leer ist — ein Rechteck oder
    /// ein importierter Streckenzug bleibt in der `.boxdoc`-Datei damit exakt
    /// so kurz wie bisher.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handles: Vec<[f32; 4]>,
    /// Ist der Streckenzug geschlossen (Fläche) oder offen (Linienzug)?
    /// Ein offener Pfad wird nie gefüllt.
    #[serde(default)]
    pub path_closed: bool,
}

fn default_true() -> bool {
    true
}
// Die Standardwerte für Formen sind öffentlich, damit eingefügte Elemente
// überall gleich aussehen — sonst driften Rechteck, Ellipse und Pfad über
// kopierte Zahlenliterale langsam auseinander.
pub fn default_fill_color() -> [u8; 4] {
    [80, 140, 220, 60]
}
pub fn default_stroke_width() -> f32 {
    2.0
}
pub fn default_stroke_color() -> [u8; 4] {
    [40, 100, 180, 255]
}

impl Element {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn new_text(id: u64, x: f32, y: f32) -> Self {
        Element {
            id,
            kind: ElementKind::Text,
            x,
            y,
            w: 240.0,
            h: 60.0,
            rotation: 0.0,
            text: String::from("Text"),
            font_size: 14.0,
            font: default_font_key(),
            color: [20, 20, 20, 255],
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            valign: VAlign::default(),
            indent: 0.0,
            auto_height: true,
            crop: Crop::default(),
            image_w: 0,
            image_h: 0,
            fill_color: default_fill_color(),
            stroke_width: default_stroke_width(),
            stroke_color: default_stroke_color(),
            corner_radius: 0.0,
            points: Vec::new(),
            handles: Vec::new(),
            path_closed: false,
        }
    }

    pub fn new_image(id: u64, x: u32, y: u32, w: u32, h: u32) -> Self {
        let w = w.max(1) as f32;
        let h = h.max(1) as f32;
        let display = 200.0;
        let scale = (display / w).min(display / h).min(1.0);
        let (dw, dh) = (w * scale, h * scale);
        Element {
            id,
            kind: ElementKind::Image,
            x: x as f32,
            y: y as f32,
            w: dw,
            h: dh,
            rotation: 0.0,
            text: String::new(),
            font_size: 14.0,
            font: default_font_key(),
            color: [255, 255, 255, 255],
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            valign: VAlign::default(),
            indent: 0.0,
            auto_height: true,
            crop: Crop::default(),
            image_w: w as u32,
            image_h: h as u32,
            fill_color: default_fill_color(),
            stroke_width: default_stroke_width(),
            stroke_color: default_stroke_color(),
            corner_radius: 0.0,
            points: Vec::new(),
            handles: Vec::new(),
            path_closed: false,
        }
    }

    pub fn new_rectangle(id: u64, x: f32, y: f32) -> Self {
        Element {
            id,
            kind: ElementKind::Rectangle,
            x,
            y,
            w: 160.0,
            h: 100.0,
            rotation: 0.0,
            text: String::new(),
            font_size: 14.0,
            font: default_font_key(),
            color: [20, 20, 20, 255],
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            valign: VAlign::default(),
            indent: 0.0,
            auto_height: true,
            crop: Crop::default(),
            image_w: 0,
            image_h: 0,
            fill_color: default_fill_color(),
            stroke_width: default_stroke_width(),
            stroke_color: default_stroke_color(),
            corner_radius: 0.0,
            points: Vec::new(),
            handles: Vec::new(),
            path_closed: false,
        }
    }

    pub fn new_line(id: u64, x: f32, y: f32) -> Self {
        Element {
            id,
            kind: ElementKind::Line,
            x,
            y,
            w: 200.0,
            h: 0.0,
            rotation: 0.0,
            text: String::new(),
            font_size: 14.0,
            font: default_font_key(),
            color: [20, 20, 20, 255],
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            valign: VAlign::default(),
            indent: 0.0,
            auto_height: true,
            crop: Crop::default(),
            image_w: 0,
            image_h: 0,
            fill_color: [0, 0, 0, 0],
            stroke_width: 2.0,
            stroke_color: [40, 40, 40, 255],
            corner_radius: 0.0,
            points: Vec::new(),
            handles: Vec::new(),
            path_closed: false,
        }
    }

    pub fn new_ellipse(id: u64, x: f32, y: f32) -> Self {
        Element {
            id,
            kind: ElementKind::Ellipse,
            x,
            y,
            w: 160.0,
            h: 100.0,
            rotation: 0.0,
            text: String::new(),
            font_size: 14.0,
            font: default_font_key(),
            color: [20, 20, 20, 255],
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            align: TextAlign::Left,
            valign: VAlign::default(),
            indent: 0.0,
            auto_height: true,
            crop: Crop::default(),
            image_w: 0,
            image_h: 0,
            fill_color: default_fill_color(),
            stroke_width: default_stroke_width(),
            stroke_color: default_stroke_color(),
            corner_radius: 0.0,
            points: Vec::new(),
            handles: Vec::new(),
            path_closed: false,
        }
    }

    /// Baut ein Pfad-Element aus **absoluten Seitenkoordinaten** (pt).
    ///
    /// Die Box wird als Hüllbox der Punkte gesetzt, die Punkte selbst darauf
    /// normalisiert. Ein Pfad ohne Ausdehnung in einer Richtung (etwa ein
    /// senkrechter Linienzug) behält dort eine Mindestbreite, damit die
    /// Normalisierung nicht durch null teilt und der Pfad greifbar bleibt.
    pub fn new_path(id: u64, points: &[(f32, f32)], closed: bool) -> Self {
        /// Kleinste Boxkante in pt — darunter wäre der Pfad nicht mehr
        /// anklickbar und die Normalisierung numerisch instabil.
        const MIN_EXTENT: f32 = 0.5;

        let mut el = Element::new_rectangle(id, 0.0, 0.0);
        el.kind = ElementKind::Path;
        el.path_closed = closed;
        el.fill_color = [0, 0, 0, 0];
        el.stroke_color = [40, 40, 40, 255];
        el.stroke_width = 1.0;

        if points.is_empty() {
            el.w = MIN_EXTENT;
            el.h = MIN_EXTENT;
            return el;
        }

        let (mut min_x, mut min_y) = points[0];
        let (mut max_x, mut max_y) = points[0];
        for (x, y) in points {
            min_x = min_x.min(*x);
            min_y = min_y.min(*y);
            max_x = max_x.max(*x);
            max_y = max_y.max(*y);
        }
        let w = (max_x - min_x).max(MIN_EXTENT);
        let h = (max_y - min_y).max(MIN_EXTENT);

        el.x = min_x;
        el.y = min_y;
        el.w = w;
        el.h = h;
        el.points = points
            .iter()
            .map(|(x, y)| [(x - min_x) / w, (y - min_y) / h])
            .collect();
        el
    }

    /// Hat der Pfad überhaupt Kurven, oder ist er ein reiner Streckenzug?
    ///
    /// Gefragt wird nach der **Wirkung**, nicht nach dem Vorhandensein des
    /// Feldes: Ein Pfad, dessen Griffe alle auf ihren Stützpunkten liegen, ist
    /// eine Kette von Geraden und darf überall den billigeren Weg nehmen.
    pub fn path_is_curved(&self) -> bool {
        if self.handles.len() != self.points.len() {
            return false;
        }
        self.points
            .iter()
            .zip(self.handles.iter())
            .any(|(p, h)| {
                (h[0] - p[0]).abs() > 1e-6
                    || (h[1] - p[1]).abs() > 1e-6
                    || (h[2] - p[0]).abs() > 1e-6
                    || (h[3] - p[1]).abs() > 1e-6
            })
    }

    /// Stimmen Stützpunkte und Griffe überein?
    ///
    /// Die Griffe kommen aus der `.boxdoc`-Datei und damit potenziell von
    /// Hand oder von einer KI. Ein Vektor mit der falschen Länge würde sonst
    /// still zu einem Pfad führen, dessen zweite Hälfte gerade ist.
    pub fn path_handles_valid(&self) -> bool {
        self.handles.is_empty() || self.handles.len() == self.points.len()
    }

    /// Bringt kaputte Griff-Daten in einen brauchbaren Zustand: Zu kurze
    /// Vektoren werden mit Eckknoten aufgefüllt, zu lange abgeschnitten.
    ///
    /// Wird beim Laden aufgerufen, damit eine fehlerhaft bearbeitete Datei
    /// nicht die Bearbeitung blockiert, sondern nur den fehlenden Teil als
    /// Gerade zeigt.
    pub fn repair_path_handles(&mut self) {
        if self.path_handles_valid() {
            return;
        }
        self.handles.truncate(self.points.len());
        while self.handles.len() < self.points.len() {
            let p = self.points[self.handles.len()];
            self.handles.push([p[0], p[1], p[0], p[1]]);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    #[serde(deserialize_with = "deserialize_elements")]
    pub elements: Vec<Element>,
}

/// Liest die Elemente einer Seite und bringt sie in einen benutzbaren Zustand.
///
/// Der Haken sitzt bewusst **hier** und nicht in den einzelnen Ladefunktionen:
/// Ein Dokument kommt aus einer Datei, aus dem JSON-Editor, vom Web-Sync und
/// aus dem Konflikt-Merge — jeder dieser Wege ginge irgendwann vergessen.
/// An der Deserialisierung kommt keiner von ihnen vorbei.
fn deserialize_elements<'de, D>(d: D) -> Result<Vec<Element>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut elements = Vec::<Element>::deserialize(d)?;
    for el in &mut elements {
        // Von Hand oder von einer KI geschriebene Pfade dürfen die
        // Bearbeitung nicht blockieren, wenn die Griffe nicht passen.
        el.repair_path_handles();
    }
    Ok(elements)
}

impl Default for Page {
    fn default() -> Self {
        Page { elements: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub format: PaperFormat,
    pub orientation: Orientation,
    /// Vom Nutzer definierte Formate (Name + Maße). Sie werden in der Datei
    /// mitgespeichert, damit sie in diesem Dokument wieder wählbar bleiben,
    /// auch wenn gerade ein Standardformat aktiv ist. Das aktuell genutzte
    /// Custom-Format steckt zusätzlich in `format` (als
    /// `PaperFormat::Custom`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_formats: Vec<CustomFormat>,
    pub pages: Vec<Page>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            format: PaperFormat::A4,
            orientation: Orientation::Portrait,
            custom_formats: Vec::new(),
            pages: vec![Page::default()],
        }
    }
}

impl Document {
    pub fn current_page(&self, index: usize) -> Option<&Page> {
        self.pages.get(index)
    }
    pub fn current_page_mut(&mut self, index: usize) -> Option<&mut Page> {
        self.pages.get_mut(index)
    }

    /// (Breite, Höhe) der Seite in Punkten — inklusive Custom-Format.
    pub fn page_size_pt(&self) -> (f32, f32) {
        page_size_pt(&self.format, self.orientation)
    }

    /// Trägt ein Custom-Format in die Liste ein (gleicher Name ersetzt) und
    /// macht es zum aktiven Format.
    pub fn apply_custom_format(&mut self, fmt: CustomFormat) {
        match self.custom_formats.iter().position(|c| c.name == fmt.name) {
            Some(i) => self.custom_formats[i] = fmt.clone(),
            None => self.custom_formats.push(fmt.clone()),
        }
        self.format = PaperFormat::Custom(fmt);
    }

    /// Entfernt ein Custom-Format aus der Liste. Ist es gerade aktiv,
    /// fällt das Dokument auf A4 Hochformat zurück.
    pub fn remove_custom_format(&mut self, name: &str) {
        self.custom_formats.retain(|c| c.name != name);
        if let PaperFormat::Custom(c) = &self.format {
            if c.name == name {
                self.format = PaperFormat::A4;
            }
        }
    }
}
