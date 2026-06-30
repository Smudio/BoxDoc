//! Beobachtet eine Datei auf externe Änderungen (z. B. durch opencode / KI-Agent).
//!
//! Native: läuft in einem Hintergrund-Thread und meldet gültige, entprellte
//! Änderungen über einen Kanal.
//! WASM:   nicht verfügbar (Browser kann keine Dateien überwachen) – Stub.

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

/// Handle auf den laufenden Watcher. Wird gedroppt → Watching stoppt.
pub struct FileWatcher {
    #[cfg(not(target_arch = "wasm32"))]
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl FileWatcher {
    /// Startet das Beobachten von `path` (eine einzelne Datei, nicht-rekursiv).
    ///
    /// Liefert den Watcher und einen Empfänger, der bei jeder erkannten
    /// Änderung den veränderten Pfad zustellt. Entprellt mit 300 ms, damit
    /// teilgeschriebene Dateien nicht zu Fehl-Reloads führen.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(path: PathBuf) -> Result<(Self, Receiver<PathBuf>), String> {
        use notify_debouncer_mini::{new_debouncer, DebounceEventResult};

        // notify-debouncer-mini sendet DebounceEventResult (= Result<Vec<_>, Error>).
        let (tx, rx) = mpsc::channel::<DebounceEventResult>();

        let mut debouncer = new_debouncer(Duration::from_millis(300), tx)
            .map_err(|e| format!("Watcher-Initialisierung fehlgeschlagen: {e}"))?;

        debouncer
            .watcher()
            .watch(path.as_path(), notify::RecursiveMode::NonRecursive)
            .map_err(|e| format!("Watch fehlgeschlagen: {e}"))?;

        // Ursprünglicher Kanal liefert DebounceEventResult; auf Einzel-Pfade
        // mappen, damit der Verbraucher nichts validieren muss.
        let (path_tx, path_rx) = mpsc::channel::<PathBuf>();
        std::thread::spawn(move || {
            for result in rx.iter() {
                if let Ok(events) = result {
                    for ev in events {
                        let _ = path_tx.send(ev.path);
                    }
                }
            }
        });

        Ok((
            FileWatcher {
                _debouncer: debouncer,
            },
            path_rx,
        ))
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start(_path: PathBuf) -> Result<(Self, Receiver<PathBuf>), String> {
        Err(String::from("File-Watch auf Web nicht verfügbar."))
    }
}
