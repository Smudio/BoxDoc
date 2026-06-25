//! Kern-Datenmodell des Dokuments.
//!
//! Alles arbeitet in "Punkten" (1/72 Zoll), damit die Darstellung auf dem
//! Bildschirm, beim Drucken und im PDF identisch ist.

use serde::{Deserialize, Serialize};

/// Papierformate. Die Größe wird in Millimetern angegeben (Hochformat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaperFormat {
    A3,
    A4,
    A5,
    Letter,
    Legal,
}

impl PaperFormat {
    pub fn label(self) -> &'static str {
        match self {
            PaperFormat::A3 => "A3",
            PaperFormat::A4 => "A4",
            PaperFormat::A5 => "A5",
            PaperFormat::Letter => "Letter",
            PaperFormat::Legal => "Legal",
        }
    }

    pub fn all() -> [PaperFormat; 5] {
        [PaperFormat::A4, PaperFormat::A3, PaperFormat::A5, PaperFormat::Letter, PaperFormat::Legal]
    }

    /// (Breite, Höhe) in Millimeter, Hochformat.
    pub fn size_mm(self) -> (f32, f32) {
        match self {
            PaperFormat::A3 => (297.0, 420.0),
            PaperFormat::A4 => (210.0, 297.0),
            PaperFormat::A5 => (148.0, 210.0),
            PaperFormat::Letter => (215.9, 279.4),
            PaperFormat::Legal => (215.9, 355.6),
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

/// (Breite, Höhe) der Seite in Punkten.
pub fn page_size_pt(format: PaperFormat, orientation: Orientation) -> (f32, f32) {
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

/// Globale Anwendungseinstellungen.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub units: Units,
    pub page_align: PageAlign,
    pub scroll_mode: ScrollMode,
}

// ===========================================================================
// Schriften
// ===========================================================================

/// Default-Schrift-Schlüssel (für `#[serde(default)]`).
pub fn default_font_key() -> String {
    String::from("default")
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
}

/// Kuratierte Auswahl. Index 0 ist die Standard-Schrift.
pub const FONT_CHOICES: &[FontDef] = &[
    FontDef {
        key: "default",
        display: "Standard",
        paths: &[],
    },
    FontDef {
        key: "arial",
        display: "Arial",
        paths: &[
            "C:\\Windows\\Fonts\\arial.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ],
    },
    FontDef {
        key: "calibri",
        display: "Calibri",
        paths: &[
            "C:\\Windows\\Fonts\\calibri.ttf",
            "/usr/share/fonts/truetype/calibri/Calibri-Regular.ttf",
        ],
    },
    FontDef {
        key: "cambria",
        display: "Cambria",
        paths: &[
            "C:\\Windows\\Fonts\\cambria.ttc",
            "/usr/share/fonts/truetype/cambria/Cambria.ttf",
        ],
    },
    FontDef {
        key: "georgia",
        display: "Georgia",
        paths: &[
            "C:\\Windows\\Fonts\\georgia.ttf",
            "/usr/share/fonts/truetype/georgia/Georgia.ttf",
        ],
    },
    FontDef {
        key: "verdana",
        display: "Verdana",
        paths: &["C:\\Windows\\Fonts\\verdana.ttf"],
    },
    FontDef {
        key: "tahoma",
        display: "Tahoma",
        paths: &["C:\\Windows\\Fonts\\tahoma.ttf"],
    },
    FontDef {
        key: "trebuc",
        display: "Trebuchet MS",
        paths: &["C:\\Windows\\Fonts\\trebuc.ttf"],
    },
    FontDef {
        key: "palatino",
        display: "Palatino Linotype",
        paths: &["C:\\Windows\\Fonts\\pala.ttf"],
    },
    FontDef {
        key: "segoeui",
        display: "Segoe UI",
        paths: &["C:\\Windows\\Fonts\\segoeui.ttf"],
    },
    FontDef {
        key: "consolas",
        display: "Consolas",
        paths: &[
            "C:\\Windows\\Fonts\\consola.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        ],
    },
    FontDef {
        key: "gabriola",
        display: "Gabriola",
        paths: &["C:\\Windows\\Fonts\\Gabriola.ttf"],
    },
    FontDef {
        key: "inkfree",
        display: "Ink Free",
        paths: &["C:\\Windows\\Fonts\\Inkfree.ttf"],
    },
    FontDef {
        key: "comic",
        display: "Comic Sans MS",
        paths: &["C:\\Windows\\Fonts\\comic.ttf"],
    },
    FontDef {
        key: "impact",
        display: "Impact",
        paths: &["C:\\Windows\\Fonts\\impact.ttf"],
    },
    FontDef {
        key: "candara",
        display: "Candara",
        paths: &["C:\\Windows\\Fonts\\Candara.ttf"],
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
    pub align: TextAlign,
    /// Einzug jeder Zeile in Punkten.
    pub indent: f32,

    // --- Bild ---
    pub crop: Crop,
    /// Originale Pixelgröße des geladenen Bilds.
    pub image_w: u32,
    pub image_h: u32,
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
            align: TextAlign::Left,
            indent: 0.0,
            crop: Crop::default(),
            image_w: 0,
            image_h: 0,
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
            align: TextAlign::Left,
            indent: 0.0,
            crop: Crop::default(),
            image_w: w as u32,
            image_h: h as u32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub elements: Vec<Element>,
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
    pub pages: Vec<Page>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            format: PaperFormat::A4,
            orientation: Orientation::Portrait,
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
}
