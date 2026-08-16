//! Tests für Undo/Redo.
//!
//! Undo ist die letzte Verteidigungslinie gegen Datenverlust. Wenn es
//! Sonderfälle gibt, in denen es stillschweigend nicht greift, ist der Nutzer
//! ungeschützt — deshalb sind hier auch die Randfälle abgedeckt.

use boxdoc::history::{History, Snapshot};
use boxdoc::model::{Document, Element, Page};

fn snap(marker: &str) -> Snapshot {
    let mut el = Element::new_text(1, 0.0, 0.0);
    el.text = marker.to_string();
    Snapshot {
        doc: Document {
            pages: vec![Page { elements: vec![el] }],
            ..Document::default()
        },
        selection: vec![1],
        page_index: 0,
    }
}

fn text_of(s: &Snapshot) -> &str {
    &s.doc.pages[0].elements[0].text
}

#[test]
fn undo_und_redo_laufen_den_verlauf_ab() {
    let mut h = History::default();
    h.init(snap("A"));
    h.push(snap("B"));
    h.push(snap("C"));

    assert_eq!(text_of(h.undo().unwrap()), "B");
    assert_eq!(text_of(h.undo().unwrap()), "A");
    assert!(h.undo().is_none(), "am Anfang gibt es nichts mehr");

    assert_eq!(text_of(h.redo().unwrap()), "B");
    assert_eq!(text_of(h.redo().unwrap()), "C");
    assert!(h.redo().is_none(), "am Ende gibt es nichts mehr");
}

#[test]
fn neue_aktion_verwirft_den_redo_stack() {
    let mut h = History::default();
    h.init(snap("A"));
    h.push(snap("B"));
    h.push(snap("C"));

    h.undo(); // -> B
    assert!(h.can_redo());

    h.push(snap("D")); // neue Aktion nach Undo
    assert!(!h.can_redo(), "Redo muss nach neuer Aktion leer sein");
    assert_eq!(text_of(h.undo().unwrap()), "B");
}

#[test]
fn history_ist_begrenzt_und_cursor_bleibt_gueltig() {
    let mut h = History::default();
    h.init(snap("start"));
    // Deutlich mehr als MAX_HISTORY (200) einfügen.
    for i in 0..500 {
        h.push(snap(&format!("s{i}")));
    }
    assert!(h.len() <= 200, "History wurde nicht begrenzt: {}", h.len());

    // Nach der Begrenzung muss Undo weiterhin funktionieren und den
    // unmittelbar vorherigen Stand liefern.
    assert_eq!(text_of(h.undo().unwrap()), "s498");
    assert_eq!(text_of(h.undo().unwrap()), "s497");
}

#[test]
fn undo_bis_zum_anschlag_stuerzt_nicht_ab() {
    let mut h = History::default();
    h.init(snap("A"));
    h.push(snap("B"));
    for _ in 0..50 {
        h.undo();
    }
    assert!(!h.can_undo());
    // Danach muss Redo wieder bis ans Ende laufen.
    for _ in 0..50 {
        h.redo();
    }
    assert!(!h.can_redo());
}

#[test]
fn frische_history_kann_nichts_rueckgaengig_machen() {
    let mut h = History::default();
    h.init(snap("A"));
    assert!(!h.can_undo());
    assert!(!h.can_redo());
    assert_eq!(h.len(), 1);
}
