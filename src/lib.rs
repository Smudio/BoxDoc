//! BoxDoc — Library-Target.
//!
//! Diese Crate ist primär eine Binary (Dokumenten-Editor), aber sie stellt
//! auch eine Library-Oberfläche bereit, damit Build-Tools (z. B.
//! `gen_test_pdfs`) und zukünftige Integration Tests auf die Module zugreifen
//! können. Die Binary (`src/main.rs`) verwendet ihre eigenen `mod`-Deklarationen
//! und ist von diesem Lib-Target unabhängig.
//!
//! Wir exposen hier nur Backend-Module ohne UI-Abhängigkeiten. `printing`
//! und `odt` referenzieren via `crate::app` die Binary-internen UI-Module und
//! bleiben daher Binary-exklusiv.

pub mod fonts;
pub mod geometry;
pub mod history;
pub mod merge;
pub mod model;
#[cfg(not(target_arch = "wasm32"))]
pub mod pdf_import;
#[cfg(not(target_arch = "wasm32"))]
pub mod printing;
pub mod store;
pub mod text_layout;
