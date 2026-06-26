//! Undo/Redo-System — Snapshot-basiert.
//!
//! Speichert den kompletten Dokumentzustand bei jeder Aktion.
//! Bilder (ImageStore) werden nicht gesnapshottet — sie bleiben erhalten,
//! auch wenn Elemente gelöscht werden, sodass Undo wiederhergestellt werden
//! kann. Orphaned Images sind ein akzeptabler, minimaler Speicher-Overhead.

use crate::model::Document;

/// Maximal gespeicherte Snapshots (History-Tiefe).
const MAX_HISTORY: usize = 200;

#[derive(Clone)]
pub struct Snapshot {
    pub doc: Document,
    pub selection: Vec<u64>,
    pub page_index: usize,
}

pub struct History {
    snapshots: Vec<Snapshot>,
    /// Index des aktuellen Zustands.
    cursor: usize,
}

impl Default for History {
    fn default() -> Self {
        History {
            snapshots: Vec::new(),
            cursor: 0,
        }
    }
}

impl History {
    /// Initialisiert die History mit dem Startzustand.
    pub fn init(&mut self, snap: Snapshot) {
        self.snapshots.clear();
        self.snapshots.push(snap);
        self.cursor = 0;
    }

    /// Nimmt einen neuen Snapshot auf. Verwirft alle Redo-Zustände.
    pub fn push(&mut self, snap: Snapshot) {
        // Alles nach dem Cursor abschneiden (Redo-Stack leeren).
        if self.cursor + 1 < self.snapshots.len() {
            self.snapshots.truncate(self.cursor + 1);
        }
        self.snapshots.push(snap);
        self.cursor = self.snapshots.len() - 1;

        // History begrenzen — älteste Snapshots verwerfen.
        if self.snapshots.len() > MAX_HISTORY {
            let excess = self.snapshots.len() - MAX_HISTORY;
            self.snapshots.drain(0..excess);
            self.cursor -= excess;
        }
    }

    /// Einen Schritt zurück. Gibt den wiederherzustellenden Snapshot zurück.
    pub fn undo(&mut self) -> Option<&Snapshot> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.snapshots.get(self.cursor)
    }

    /// Einen Schritt vor. Gibt den wiederherzustellenden Snapshot zurück.
    pub fn redo(&mut self) -> Option<&Snapshot> {
        if self.cursor + 1 >= self.snapshots.len() {
            return None;
        }
        self.cursor += 1;
        self.snapshots.get(self.cursor)
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor + 1 < self.snapshots.len()
    }

    pub fn len(&self) -> usize {
        self.snapshots.len()
    }
}
