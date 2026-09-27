//! Prüft, dass Client und Server dieselbe Regel für Dokumentnamen benutzen.
//!
//! Der Name eines Serverdokuments ist seine Adresse (`boxdoc.at/<slug>`), und
//! die Regel dafür steht zwangsläufig zweimal da: in `src/slug.rs` für die
//! Web-App und in `valid_slug()` in `web/index.php` für das Backend. PHP lässt
//! sich aus Rust nicht aufrufen, also lesen diese Tests die PHP-Datei und
//! vergleichen, was drinsteht.
//!
//! Warum das nötig ist, zeigt der Fall, der es ausgelöst hat: Der Client
//! verlangte einen Buchstaben am Anfang, der Server erzeugt seine Namen aber
//! als `bin2hex(random_bytes(5))`. Jeder zweite davon beginnt mit einer Ziffer
//! — und wurde von der App nicht als Dokument erkannt. Beide Seiten waren für
//! sich schlüssig; erst der Vergleich macht den Bruch sichtbar.
//!
//! Die Tests laufen nur nativ. Sie prüfen genau die Funktionen, die die
//! Web-Version aufruft.

#![cfg(not(target_arch = "wasm32"))]

use boxdoc::slug;

/// Der Pfad zum PHP-Backend, relativ zum Crate-Wurzelverzeichnis.
const PHP_PATH: &str = "web/index.php";

fn php_quelltext() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(PHP_PATH);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} nicht lesbar ({e}). Diese Datei ist das Backend der Web-Version \
             und gehört ins Repository.",
            path.display()
        )
    })
}

/// Schneidet den Rumpf von `valid_slug()` aus dem PHP-Quelltext.
fn valid_slug_rumpf(php: &str) -> String {
    let start = php
        .find("function valid_slug(")
        .expect("valid_slug() fehlt in web/index.php — wurde die Funktion umbenannt?");
    let rest = &php[start..];
    let ende = rest
        .find("\n}")
        .expect("Ende von valid_slug() nicht gefunden");
    rest[..ende].to_string()
}

/// Die Zeichen- und Längenregel muss wortgleich sein.
///
/// Statt den regulären Ausdruck nachzubauen, wird er als Text verglichen: Ein
/// abgewandeltes Muster fällt so garantiert auf, auch wenn es zufällig
/// dieselben Beispielnamen durchlässt.
#[test]
fn zeichen_und_laengenregel_stimmen_ueberein() {
    let rumpf = valid_slug_rumpf(&php_quelltext());
    let erwartet = format!(
        "/^[a-z0-9]{{{},{}}}$/",
        slug::MIN_LEN,
        slug::MAX_LEN
    );
    assert!(
        rumpf.contains(&erwartet),
        "web/index.php prüft nicht mehr mit {erwartet}. \
         Entweder dort oder in src/slug.rs (MIN_LEN/MAX_LEN, erlaubte Zeichen) \
         wurde die Regel geändert — beide müssen zusammenpassen.\n\
         Gefundener Rumpf:\n{rumpf}"
    );
}

/// Die Liste der reservierten Namen muss auf beiden Seiten dieselbe sein.
///
/// Fehlt einer in Rust, bietet die App einen Namen an, den der Server ablehnt.
/// Fehlt einer in PHP, entsteht ein Dokument, das über seine eigene Adresse
/// nicht erreichbar ist.
#[test]
fn reservierte_namen_stimmen_ueberein() {
    let rumpf = valid_slug_rumpf(&php_quelltext());

    for name in slug::RESERVED {
        assert!(
            rumpf.contains(&format!("'{name}'")),
            "'{name}' ist in src/slug.rs reserviert, in web/index.php aber nicht."
        );
    }

    // Und die Gegenrichtung: alles, was PHP in seiner Liste führt, muss auch
    // hier stehen. Die Namen stehen dort als einfach zitierte Zeichenketten
    // in `$reserved`.
    let zeile = rumpf
        .lines()
        .find(|l| l.contains("$reserved"))
        .expect("$reserved fehlt in valid_slug()");
    for stueck in zeile.split('\'').skip(1).step_by(2) {
        assert!(
            slug::RESERVED.contains(&stueck),
            "'{stueck}' ist in web/index.php reserviert, in src/slug.rs aber nicht."
        );
    }
}

/// Der Server erzeugt Namen als `bin2hex(random_bytes(5))`. Jeder davon muss
/// die Regel des Clients erfüllen — sonst öffnet die App Dokumente nicht, die
/// sie selbst hat anlegen lassen.
#[test]
fn servererzeugte_namen_sind_gueltig() {
    let php = php_quelltext();
    assert!(
        php.contains("bin2hex(random_bytes(5))"),
        "gen_slug() erzeugt nicht mehr 5 Zufallsbytes als Hex — \
         dieser Test prüft dann das Falsche."
    );

    // 10 Hex-Zeichen, alle Kombinationen von Anfangszeichen abgedeckt.
    for c in "0123456789abcdef".chars() {
        let name = format!("{c}{}", "0123456789".chars().take(9).collect::<String>());
        assert_eq!(name.len(), 10);
        assert!(
            slug::is_valid(&name),
            "vom Server erzeugbarer Name {name:?} gilt dem Client als ungültig"
        );
    }
}

/// Was `slugify()` vorschlägt, muss der Server auch annehmen — sonst führt der
/// Dialog den Nutzer in eine Ablehnung.
#[test]
fn slugify_liefert_annehmbare_namen() {
    let lang = "a".repeat(200);
    let eingaben = [
        "Mein Lebenslauf",
        "Angebot 2026 – Küche & Bad",
        "Größe/Maß",
        "ÄÖÜ ßß",
        lang.as_str(),
    ];
    for e in eingaben {
        let s = slug::slugify(e);
        if s.len() < slug::MIN_LEN {
            // Zu kurz ist erlaubt — der Dialog verlangt dann eine Eingabe.
            continue;
        }
        assert!(
            slug::is_valid(&s),
            "slugify({e:?}) = {s:?} wäre vom Server abgelehnt worden"
        );
    }
}
