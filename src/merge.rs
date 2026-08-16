//! Drei-Wege-Merge für gleichzeitiges Bearbeiten.
//!
//! # Warum nicht einfach das Dokument überschreiben?
//!
//! Die frühere Web-Synchronisierung war „Last-Write-Wins auf Dokumentebene":
//! Wer zuletzt speichert, gewinnt — und löscht damit alles, was andere in der
//! Zwischenzeit getan haben. Zwei Leute, die an verschiedenen Ecken derselben
//! Seite arbeiten, hätten sich gegenseitig die Arbeit vernichtet. Nicht als
//! Randfall, sondern als Regelverhalten.
//!
//! # Warum kein CRDT?
//!
//! Ein CRDT wäre die schwere Lösung. Das Datenmodell von BoxDoc braucht sie
//! nicht: Eine Seite ist ein `Vec<Element>`, jedes Element hat eine **stabile,
//! nie wiederverwendete `u64`-ID**. Damit lässt sich exakt bestimmen, was
//! hinzugefügt, gelöscht und geändert wurde — ein klassischer Drei-Wege-Merge
//! wie bei Versionsverwaltung, nur auf Feldebene statt auf Textzeilen.
//!
//! # Das Verfahren
//!
//! Gegeben sind drei Stände:
//!
//! - **base**   — der gemeinsame Ausgangspunkt (zuletzt erfolgreich synchronisiert)
//! - **local**  — der eigene, noch nicht hochgeladene Stand
//! - **remote** — der Stand, der inzwischen auf dem Server liegt
//!
//! Pro Element-ID wird entschieden:
//!
//! | in base | in local | in remote | Ergebnis |
//! |---|---|---|---|
//! | nein | ja | nein | lokal neu → übernehmen |
//! | nein | nein | ja | fremd neu → übernehmen |
//! | nein | ja | ja | beide neu, gleiche ID → lokal gewinnt (Konflikt) |
//! | ja | nein | ja (unverändert) | lokal gelöscht → löschen |
//! | ja | ja (unverändert) | nein | fremd gelöscht → löschen |
//! | ja | nein | ja (geändert) | Löschung vs. Änderung → **Änderung gewinnt** |
//! | ja | ja | ja | feldweise mergen |
//!
//! Beim feldweisen Merge gilt pro Feld: Wer es gegenüber `base` geändert hat,
//! setzt sich durch. Haben **beide** dasselbe Feld geändert, gewinnt der lokale
//! Wert — der Nutzer sieht seine eigene Eingabe nicht wegspringen — und der
//! Konflikt wird in [`MergeReport`] gemeldet, damit die Oberfläche ihn anzeigen
//! kann.
//!
//! „Löschung vs. Änderung → Änderung gewinnt" ist bewusst gewählt: Ein
//! versehentlich wiederauferstandenes Element ist mit einem Tastendruck
//! entfernt, verlorene Arbeit nicht.

use std::collections::{BTreeSet, HashMap};

use crate::model::{Document, Element, Page};

/// Was beim Mergen passiert ist — für Statusanzeige und Tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// IDs, die nur lokal neu hinzugekommen sind.
    pub added_local: Vec<u64>,
    /// IDs, die nur von der Gegenseite hinzugekommen sind.
    pub added_remote: Vec<u64>,
    /// IDs, die entfernt wurden (von einer der beiden Seiten).
    pub removed: Vec<u64>,
    /// IDs, bei denen beide Seiten dasselbe Feld geändert haben.
    /// Der lokale Wert wurde behalten.
    pub conflicts: Vec<u64>,
    /// IDs, die eine Seite löschen wollte, während die andere sie änderte.
    /// Das Element wurde behalten.
    pub revived: Vec<u64>,
}

impl MergeReport {
    /// Gab es überhaupt eine Abweichung zwischen den Ständen?
    pub fn is_empty(&self) -> bool {
        self.added_local.is_empty()
            && self.added_remote.is_empty()
            && self.removed.is_empty()
            && self.conflicts.is_empty()
            && self.revived.is_empty()
    }

    /// Kurze, für den Nutzer verständliche Zusammenfassung.
    pub fn summary(&self) -> String {
        if self.is_empty() {
            return String::from("Keine Änderungen.");
        }
        let mut parts = Vec::new();
        if !self.added_remote.is_empty() {
            parts.push(format!("{} neu von außen", self.added_remote.len()));
        }
        if !self.added_local.is_empty() {
            parts.push(format!("{} eigene neu", self.added_local.len()));
        }
        if !self.removed.is_empty() {
            parts.push(format!("{} entfernt", self.removed.len()));
        }
        if !self.conflicts.is_empty() {
            parts.push(format!(
                "{} Konflikt(e) — eigene Fassung behalten",
                self.conflicts.len()
            ));
        }
        if !self.revived.is_empty() {
            parts.push(format!("{} wiederhergestellt", self.revived.len()));
        }
        format!("Zusammengeführt: {}.", parts.join(", "))
    }
}

/// Führt `local` und `remote` auf Basis des gemeinsamen Standes `base` zusammen.
pub fn merge_documents(
    base: &Document,
    local: &Document,
    remote: &Document,
) -> (Document, MergeReport) {
    let mut report = MergeReport::default();

    // Seitenanzahl: das Maximum, damit von keiner Seite Seiten verschwinden.
    let page_count = local.pages.len().max(remote.pages.len());
    let mut pages = Vec::with_capacity(page_count);

    for i in 0..page_count {
        let empty = Page::default();
        let b = base.pages.get(i).unwrap_or(&empty);
        let l = local.pages.get(i).unwrap_or(&empty);
        let r = remote.pages.get(i).unwrap_or(&empty);
        pages.push(merge_page(b, l, r, &mut report));
    }

    // Dokumentweite Eigenschaften: Änderung gegenüber base setzt sich durch,
    // bei beidseitiger Änderung gewinnt lokal.
    let format = pick(base.format, local.format, remote.format);
    let orientation = pick(base.orientation, local.orientation, remote.orientation);

    (
        Document {
            format,
            orientation,
            pages,
        },
        report,
    )
}

/// Wählt zwischen zwei möglicherweise geänderten Werten.
/// Lokal hat Vorrang, wenn beide vom Basiswert abweichen.
fn pick<T: PartialEq + Copy>(base: T, local: T, remote: T) -> T {
    if local != base {
        local
    } else {
        remote
    }
}

fn merge_page(base: &Page, local: &Page, remote: &Page, report: &mut MergeReport) -> Page {
    let base_by_id: HashMap<u64, &Element> = base.elements.iter().map(|e| (e.id, e)).collect();
    let local_by_id: HashMap<u64, &Element> = local.elements.iter().map(|e| (e.id, e)).collect();
    let remote_by_id: HashMap<u64, &Element> = remote.elements.iter().map(|e| (e.id, e)).collect();

    // Ergebnis-Elemente nach ID.
    let mut merged: HashMap<u64, Element> = HashMap::new();

    let all_ids: BTreeSet<u64> = base_by_id
        .keys()
        .chain(local_by_id.keys())
        .chain(remote_by_id.keys())
        .copied()
        .collect();

    for id in all_ids {
        let b = base_by_id.get(&id).copied();
        let l = local_by_id.get(&id).copied();
        let r = remote_by_id.get(&id).copied();

        match (b, l, r) {
            // Neu, nur lokal.
            (None, Some(l), None) => {
                report.added_local.push(id);
                merged.insert(id, l.clone());
            }
            // Neu, nur entfernt.
            (None, None, Some(r)) => {
                report.added_remote.push(id);
                merged.insert(id, r.clone());
            }
            // Beide haben dieselbe ID neu vergeben. Sollte durch die
            // ID-Reservierung nicht vorkommen; falls doch, gewinnt lokal.
            (None, Some(l), Some(r)) => {
                if !elements_equal(l, r) {
                    report.conflicts.push(id);
                }
                merged.insert(id, l.clone());
            }
            // Auf beiden Seiten gelöscht — oder war nie da.
            (Some(_), None, None) => {
                report.removed.push(id);
            }
            // Lokal gelöscht.
            (Some(b), None, Some(r)) => {
                if elements_equal(b, r) {
                    // Gegenseite hat nichts geändert → Löschung übernehmen.
                    report.removed.push(id);
                } else {
                    // Gegenseite hat daran gearbeitet → Arbeit schlägt Löschung.
                    report.revived.push(id);
                    merged.insert(id, r.clone());
                }
            }
            // Von der Gegenseite gelöscht.
            (Some(b), Some(l), None) => {
                if elements_equal(b, l) {
                    report.removed.push(id);
                } else {
                    report.revived.push(id);
                    merged.insert(id, l.clone());
                }
            }
            // Auf beiden Seiten vorhanden → feldweise mergen.
            (Some(b), Some(l), Some(r)) => {
                let (el, conflicted) = merge_element(b, l, r);
                if conflicted {
                    report.conflicts.push(id);
                }
                merged.insert(id, el);
            }
            (None, None, None) => unreachable!("ID stammt aus einer der drei Mengen"),
        }
    }

    Page {
        elements: order_elements(local, remote, merged),
    }
}

/// Stellt die Z-Reihenfolge wieder her.
///
/// Die Reihenfolge im `Vec` ist die Zeichenreihenfolge und damit
/// bedeutungstragend. Grundlage ist die lokale Reihenfolge (der Nutzer sieht
/// sein eigenes Ergebnis stabil); fremd hinzugekommene Elemente werden an der
/// Position eingefügt, die sie auf der Gegenseite hatten, und ansonsten
/// angehängt.
fn order_elements(
    local: &Page,
    remote: &Page,
    mut merged: HashMap<u64, Element>,
) -> Vec<Element> {
    let mut out = Vec::with_capacity(merged.len());

    for el in &local.elements {
        if let Some(m) = merged.remove(&el.id) {
            out.push(m);
        }
    }
    // Was jetzt noch übrig ist, kam von der Gegenseite.
    for el in &remote.elements {
        if let Some(m) = merged.remove(&el.id) {
            out.push(m);
        }
    }
    // Sicherheitsnetz: nichts darf verloren gehen, auch wenn eine ID in
    // keiner der beiden Reihenfolgen auftaucht.
    let mut rest: Vec<Element> = merged.into_values().collect();
    rest.sort_by_key(|e| e.id);
    out.extend(rest);
    out
}

/// Vergleicht zwei Elemente inhaltlich.
///
/// `Element` leitet `PartialEq` nicht ab (es enthält `f32`), deshalb hier
/// explizit über alle Felder. Fließkommazahlen werden mit Toleranz verglichen,
/// damit ein Roundtrip durch JSON nicht als Änderung gilt.
pub fn elements_equal(a: &Element, b: &Element) -> bool {
    const EPS: f32 = 1e-4;
    let feq = |x: f32, y: f32| (x - y).abs() < EPS;

    a.id == b.id
        && a.kind == b.kind
        && feq(a.x, b.x)
        && feq(a.y, b.y)
        && feq(a.w, b.w)
        && feq(a.h, b.h)
        && feq(a.rotation, b.rotation)
        && a.text == b.text
        && feq(a.font_size, b.font_size)
        && a.font == b.font
        && a.color == b.color
        && a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.align == b.align
        && a.valign == b.valign
        && feq(a.indent, b.indent)
        && a.auto_height == b.auto_height
        && a.crop == b.crop
        && a.image_w == b.image_w
        && a.image_h == b.image_h
        && a.fill_color == b.fill_color
        && feq(a.stroke_width, b.stroke_width)
        && a.stroke_color == b.stroke_color
        && feq(a.corner_radius, b.corner_radius)
        && a.points.len() == b.points.len()
        && a.points
            .iter()
            .zip(b.points.iter())
            .all(|(p, q)| feq(p[0], q[0]) && feq(p[1], q[1]))
        && a.handles.len() == b.handles.len()
        && a.handles
            .iter()
            .zip(b.handles.iter())
            .all(|(p, q)| p.iter().zip(q.iter()).all(|(u, v)| feq(*u, *v)))
        && a.path_closed == b.path_closed
}

/// Feldweiser Merge eines Elements.
///
/// Rückgabe: das gemergte Element und ob mindestens ein Feld beidseitig
/// geändert wurde (echter Konflikt).
fn merge_element(base: &Element, local: &Element, remote: &Element) -> (Element, bool) {
    let mut out = local.clone();
    let mut conflict = false;

    // Für jedes Feld: Wenn nur die Gegenseite es geändert hat, übernehmen.
    // Wenn beide es geändert haben, lokal behalten und Konflikt melden.
    macro_rules! merge_field {
        ($field:ident) => {
            let local_changed = local.$field != base.$field;
            let remote_changed = remote.$field != base.$field;
            if remote_changed {
                if local_changed {
                    if local.$field != remote.$field {
                        conflict = true;
                    }
                } else {
                    out.$field = remote.$field.clone();
                }
            }
        };
    }

    // Fließkomma-Felder mit Toleranz, sonst meldet ein JSON-Roundtrip
    // Scheinänderungen.
    macro_rules! merge_float {
        ($field:ident) => {
            const EPS: f32 = 1e-4;
            let local_changed = (local.$field - base.$field).abs() >= EPS;
            let remote_changed = (remote.$field - base.$field).abs() >= EPS;
            if remote_changed {
                if local_changed {
                    if (local.$field - remote.$field).abs() >= EPS {
                        conflict = true;
                    }
                } else {
                    out.$field = remote.$field;
                }
            }
        };
    }

    // Geometrie
    {
        merge_float!(x);
    }
    {
        merge_float!(y);
    }
    {
        merge_float!(w);
    }
    {
        merge_float!(h);
    }
    {
        merge_float!(rotation);
    }
    {
        merge_float!(indent);
    }
    {
        merge_float!(font_size);
    }
    {
        merge_float!(stroke_width);
    }
    {
        merge_float!(corner_radius);
    }

    // Inhalt und Stil
    merge_field!(kind);
    merge_field!(text);
    merge_field!(font);
    merge_field!(color);
    merge_field!(bold);
    merge_field!(italic);
    merge_field!(underline);
    merge_field!(align);
    merge_field!(valign);
    merge_field!(auto_height);
    merge_field!(crop);
    merge_field!(image_w);
    merge_field!(image_h);
    merge_field!(fill_color);
    merge_field!(stroke_color);
    merge_field!(path_closed);

    // Stützpunkte und Griffe sind **ein** Feld.
    //
    // Getrennt gemergt könnten die Griffe der einen Seite an den Stützpunkten
    // der anderen landen — unterschiedlich lang, und damit ein Pfad, dessen
    // zweite Hälfte still ihre Rundung verliert. Die Knotenliste wird deshalb
    // immer als Ganzes übernommen oder als Ganzes als Konflikt gemeldet.
    {
        let local_changed = local.points != base.points || local.handles != base.handles;
        let remote_changed = remote.points != base.points || remote.handles != base.handles;
        if remote_changed {
            if local_changed {
                if local.points != remote.points || local.handles != remote.handles {
                    conflict = true;
                }
            } else {
                out.points = remote.points.clone();
                out.handles = remote.handles.clone();
            }
        }
    }

    (out, conflict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Element, ElementKind, Page, TextAlign};

    fn doc(elements: Vec<Element>) -> Document {
        Document {
            pages: vec![Page { elements }],
            ..Document::default()
        }
    }

    fn text(id: u64, s: &str) -> Element {
        let mut el = Element::new_text(id, 0.0, 0.0);
        el.text = s.to_string();
        el
    }

    fn ids(doc: &Document) -> Vec<u64> {
        doc.pages[0].elements.iter().map(|e| e.id).collect()
    }

    fn find<'a>(d: &'a Document, id: u64) -> &'a Element {
        d.pages[0]
            .elements
            .iter()
            .find(|e| e.id == id)
            .expect("Element muss vorhanden sein")
    }

    // --- Der Kernfall: zwei Leute, zwei verschiedene Ecken -------------------

    #[test]
    fn getrennte_aenderungen_bleiben_beide_erhalten() {
        let base = doc(vec![text(1, "A"), text(2, "B")]);

        let mut local = base.clone();
        local.pages[0].elements[0].text = "A lokal".into();

        let mut remote = base.clone();
        remote.pages[0].elements[1].text = "B fremd".into();

        let (merged, report) = merge_documents(&base, &local, &remote);

        assert_eq!(find(&merged, 1).text, "A lokal");
        assert_eq!(find(&merged, 2).text, "B fremd");
        assert!(
            report.conflicts.is_empty(),
            "getrennte Änderungen sind kein Konflikt"
        );
    }

    #[test]
    fn verschiedene_felder_desselben_elements_mergen() {
        // Einer verschiebt, der andere färbt — beides muss überleben.
        let base = doc(vec![text(1, "X")]);

        let mut local = base.clone();
        local.pages[0].elements[0].x = 100.0;

        let mut remote = base.clone();
        remote.pages[0].elements[0].color = [255, 0, 0, 255];

        let (merged, report) = merge_documents(&base, &local, &remote);
        let el = find(&merged, 1);

        assert_eq!(el.x, 100.0, "lokale Verschiebung fehlt");
        assert_eq!(el.color, [255, 0, 0, 255], "fremde Farbe fehlt");
        assert!(report.conflicts.is_empty());
    }

    // --- Hinzufügen ---------------------------------------------------------

    #[test]
    fn beidseitig_hinzugefuegte_elemente_bleiben_beide() {
        let base = doc(vec![text(1, "A")]);

        let mut local = base.clone();
        local.pages[0].elements.push(text(2, "lokal neu"));

        let mut remote = base.clone();
        remote.pages[0].elements.push(text(3, "fremd neu"));

        let (merged, report) = merge_documents(&base, &local, &remote);

        let got = ids(&merged);
        assert!(got.contains(&1) && got.contains(&2) && got.contains(&3), "{got:?}");
        assert_eq!(report.added_local, vec![2]);
        assert_eq!(report.added_remote, vec![3]);
    }

    // --- Löschen ------------------------------------------------------------

    #[test]
    fn loeschung_ohne_gegenaenderung_wird_uebernommen() {
        let base = doc(vec![text(1, "A"), text(2, "B")]);

        let mut local = base.clone();
        local.pages[0].elements.retain(|e| e.id != 2);

        let remote = base.clone();

        let (merged, report) = merge_documents(&base, &local, &remote);
        assert_eq!(ids(&merged), vec![1]);
        assert_eq!(report.removed, vec![2]);
    }

    #[test]
    fn aenderung_schlaegt_loeschung() {
        // Wer arbeitet, verliert seine Arbeit nicht, nur weil jemand anderes
        // das Element gelöscht hat.
        let base = doc(vec![text(1, "A"), text(2, "B")]);

        let mut local = base.clone();
        local.pages[0].elements.retain(|e| e.id != 2);

        let mut remote = base.clone();
        remote.pages[0].elements[1].text = "B wichtig".into();

        let (merged, report) = merge_documents(&base, &local, &remote);

        assert_eq!(find(&merged, 2).text, "B wichtig");
        assert_eq!(report.revived, vec![2]);
    }

    // --- Echte Konflikte ----------------------------------------------------

    #[test]
    fn gleicher_feldkonflikt_behaelt_lokal_und_meldet() {
        let base = doc(vec![text(1, "A")]);

        let mut local = base.clone();
        local.pages[0].elements[0].text = "lokale Fassung".into();

        let mut remote = base.clone();
        remote.pages[0].elements[0].text = "fremde Fassung".into();

        let (merged, report) = merge_documents(&base, &local, &remote);

        assert_eq!(find(&merged, 1).text, "lokale Fassung");
        assert_eq!(report.conflicts, vec![1]);
        assert!(report.summary().contains("Konflikt"));
    }

    #[test]
    fn identische_aenderung_ist_kein_konflikt() {
        let base = doc(vec![text(1, "A")]);
        let mut local = base.clone();
        local.pages[0].elements[0].text = "gleich".into();
        let remote = local.clone();

        let (merged, report) = merge_documents(&base, &local, &remote);
        assert_eq!(find(&merged, 1).text, "gleich");
        assert!(report.conflicts.is_empty());
    }

    // --- Stabilität ---------------------------------------------------------

    #[test]
    fn merge_ohne_aenderungen_ist_identitaet() {
        let base = doc(vec![text(1, "A"), text(2, "B")]);
        let (merged, report) = merge_documents(&base, &base, &base);
        assert_eq!(ids(&merged), vec![1, 2]);
        assert!(report.is_empty(), "{report:?}");
        assert_eq!(report.summary(), "Keine Änderungen.");
    }

    #[test]
    fn z_reihenfolge_folgt_lokal() {
        let base = doc(vec![text(1, "A"), text(2, "B"), text(3, "C")]);
        let mut local = base.clone();
        local.pages[0].elements.swap(0, 2); // C, B, A

        let (merged, _) = merge_documents(&base, &local, &base);
        assert_eq!(ids(&merged), vec![3, 2, 1]);
    }

    #[test]
    fn neue_seiten_gehen_nicht_verloren() {
        let base = doc(vec![text(1, "A")]);
        let mut remote = base.clone();
        remote.pages.push(Page {
            elements: vec![text(9, "Seite 2")],
        });

        let (merged, _) = merge_documents(&base, &base, &remote);
        assert_eq!(merged.pages.len(), 2);
        assert_eq!(merged.pages[1].elements[0].id, 9);
    }

    #[test]
    fn dokumenteigenschaften_mergen() {
        let base = Document::default();
        let mut local = base.clone();
        local.format = crate::model::PaperFormat::A3;
        let mut remote = base.clone();
        remote.orientation = crate::model::Orientation::Landscape;

        let (merged, _) = merge_documents(&base, &local, &remote);
        assert_eq!(merged.format, crate::model::PaperFormat::A3);
        assert_eq!(merged.orientation, crate::model::Orientation::Landscape);
    }

    #[test]
    fn float_rauschen_gilt_nicht_als_aenderung() {
        // JSON-Roundtrip kann f32 minimal verändern. Das darf keinen
        // Scheinkonflikt erzeugen.
        let base = doc(vec![text(1, "A")]);
        let mut local = base.clone();
        local.pages[0].elements[0].x = 0.000_01;
        let mut remote = base.clone();
        remote.pages[0].elements[0].text = "geändert".into();

        let (merged, report) = merge_documents(&base, &local, &remote);
        assert_eq!(find(&merged, 1).text, "geändert");
        assert!(report.conflicts.is_empty());
    }

    #[test]
    fn kind_und_stilfelder_werden_gemergt() {
        let base = doc(vec![text(1, "A")]);
        let mut local = base.clone();
        local.pages[0].elements[0].bold = true;
        let mut remote = base.clone();
        remote.pages[0].elements[0].align = TextAlign::Center;

        let (merged, _) = merge_documents(&base, &local, &remote);
        let el = find(&merged, 1);
        assert!(el.bold);
        assert_eq!(el.align, TextAlign::Center);
    }

    #[test]
    fn elements_equal_erkennt_unterschiede() {
        let a = text(1, "A");
        let mut b = a.clone();
        assert!(elements_equal(&a, &b));
        b.kind = ElementKind::Rectangle;
        assert!(!elements_equal(&a, &b));
    }
}
