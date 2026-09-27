//! Dokumentnamen der Web-Version („Slugs").
//!
//! Der Name eines Serverdokuments ist zugleich seine Adresse: `boxdoc.at/<slug>`.
//! Deshalb ist er eng geregelt — 4 bis 32 Zeichen, nur `a–z` und `0–9`, und
//! keiner der Namen, die im Webserver schon etwas anderes bedeuten.
//!
//! Diese Regel gilt an zwei Stellen: hier und in `valid_slug()` in
//! `web/index.php`. Sie muss an beiden Stellen **dieselbe** sein, denn sie
//! entscheidet über zwei verschiedene Dinge:
//!
//! * Was der Client zum Anlegen anbietet. Ist er lockerer als der Server,
//!   schickt er Namen los, die der Server mit 400 ablehnt.
//! * Was der Client als Dokument-URL wiedererkennt. Ist er strenger als der
//!   Server, öffnet er Dokumente nicht, die es sehr wohl gibt. Genau das ist
//!   passiert: Die frühere Fassung verlangte einen Buchstaben am Anfang,
//!   während der Server seine Namen als `bin2hex(random_bytes(5))` erzeugt —
//!   jeder zweite davon beginnt mit einer Ziffer.
//!
//! `tests/slug_parity.rs` liest die PHP-Datei und lässt den Test fehlschlagen,
//! sobald die beiden Fassungen auseinanderlaufen.
//!
//! Das Modul ist bewusst plattformneutral (kein `wasm32`-Zuschnitt): Nur so
//! lässt es sich nativ testen, und getestet gehört es.

/// Namen, die der Webserver selbst belegt. Ein Dokument, das so heißt, wäre
/// über seine Adresse nicht erreichbar — der Server lieferte etwas anderes aus.
pub const RESERVED: [&str; 8] = [
    "docs", "api", "index", "stream", "assets", "static", "dist", "web",
];

/// Länge eines Dokumentnamens, in Zeichen.
pub const MIN_LEN: usize = 4;
pub const MAX_LEN: usize = 32;

/// Ist das ein gültiger Dokumentname?
pub fn is_valid(s: &str) -> bool {
    s.len() >= MIN_LEN
        && s.len() <= MAX_LEN
        && s.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
        && !RESERVED.contains(&s)
}

/// Macht aus einer freien Eingabe einen Namensvorschlag, der die Regel erfüllt.
///
/// Umlaute werden ausgeschrieben („Lösung" wird zu „loesung"), alles übrige
/// Nicht-Passende fällt weg. Das Ergebnis darf leer oder zu kurz sein — dann
/// tippt der Nutzer selbst etwas ein. Stillschweigend aufzufüllen würde nur
/// einen Namen erfinden, den später niemand wiedererkennt.
///
/// Reservierte Namen werden hier **nicht** abgefangen: Der Vorschlag geht durch
/// `is_valid()`, bevor er abgeschickt wird, und dort fallen sie auf. Wer „Web"
/// eintippt, soll „web" im Feld stehen sehen und die Meldung dazu lesen — nicht
/// stumm ein anderes Wort vorgesetzt bekommen.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            'ä' | 'Ä' => out.push_str("ae"),
            'ö' | 'Ö' => out.push_str("oe"),
            'ü' | 'Ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c.to_ascii_lowercase()),
            _ => {}
        }
        if out.len() >= MAX_LEN {
            break;
        }
    }
    out.truncate(MAX_LEN);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gueltige_namen() {
        assert!(is_valid("lebenslauf"));
        assert!(is_valid("abcd"));
        assert!(is_valid("a1b2"));
        // Vom Server erzeugt: bin2hex(random_bytes(5)) — beginnt oft mit einer
        // Ziffer. Genau diese Namen wurden früher nicht erkannt.
        assert!(is_valid("4f2a9c1b7e"));
        assert!(is_valid(&"a".repeat(MAX_LEN)));
    }

    #[test]
    fn ungueltige_namen() {
        assert!(!is_valid("abc"), "zu kurz");
        assert!(!is_valid(&"a".repeat(MAX_LEN + 1)), "zu lang");
        assert!(!is_valid("Lebenslauf"), "Großbuchstaben");
        assert!(!is_valid("lebens-lauf"), "Bindestrich");
        assert!(!is_valid("lebens lauf"), "Leerzeichen");
        assert!(!is_valid("lebenslüge"), "Umlaut");
        assert!(!is_valid(""), "leer");
        for r in RESERVED {
            assert!(!is_valid(r), "reserviert: {r}");
        }
    }

    #[test]
    fn slugify_bereinigt() {
        assert_eq!(slugify("Mein Lebenslauf"), "meinlebenslauf");
        assert_eq!(slugify("Lösung 2024"), "loesung2024");
        assert_eq!(slugify("Größe/Maß"), "groessemass");
        assert_eq!(slugify("Über-Uns"), "ueberuns");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn slugify_haelt_die_laengengrenze_ein() {
        let lang = slugify(&"abcdefghij".repeat(10));
        assert_eq!(lang.len(), MAX_LEN);
        assert!(is_valid(&lang));
    }

    /// Ein Umlaut am Ende wird zu zwei Zeichen und kann die Grenze reißen.
    ///
    /// Dass `truncate` dabei nicht mitten in ein UTF-8-Zeichen schneidet, liegt
    /// allein daran, dass die Ausgabe reines ASCII ist. Der Test hält diese
    /// Eigenschaft fest: Kommt je eine Ersetzung dazu, die Nicht-ASCII
    /// ausgibt, panisiert `truncate` — und dieser Test schlägt vorher fehl.
    #[test]
    fn slugify_bleibt_ascii_und_in_der_grenze() {
        let s = slugify(&format!("{}ä", "a".repeat(MAX_LEN - 1)));
        assert!(s.len() <= MAX_LEN);
        assert!(s.is_ascii(), "Ausgabe muss ASCII bleiben: {s:?}");
    }
}
