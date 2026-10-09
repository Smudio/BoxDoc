//! BoxDoc – Einstiegspunkt.

mod app;
mod canvas;
mod file_watch;
mod fonts;
mod geometry;
mod history;
mod io;
mod merge;
mod model;
mod odt;
#[cfg(not(target_arch = "wasm32"))]
mod pdf_import;
mod printing;
mod settings_io;
mod slug;
mod store;
mod svg;
mod svg_import;
mod text_layout;
mod themes;
#[cfg(target_arch = "wasm32")]
mod web_sync;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BoxDoc")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([640.0, 420.0]),
        // Der Close-Wunsch wird nicht sofort ausgeführt, sondern erst an die App
        // gemeldet. So kann `EditorApp` bei ungespeicherten Änderungen
        // nachfragen, statt die Arbeit kommentarlos zu verwerfen.
        // Siehe `app.rs` → Close-Guard in `ui()`.
        ..Default::default()
    };
    eframe::run_native(
        "BoxDoc",
        options,
        Box::new(|cc| {
            fonts::install(&cc.egui_ctx);
            let saved = settings_io::load_or_detect();
            themes::apply(&cc.egui_ctx, saved.theme);
            io::install_clipboard_paste_listener();
            let mut app = app::EditorApp::default();
            // Optional: Datei als Startargument (z. B. für KI-Agenten-Workflows).
            if let Some(arg) = std::env::args().nth(1) {
                app.open_path(std::path::PathBuf::from(arg));
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    use web_sys::HtmlCanvasElement;

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().unwrap().document().unwrap();
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .unwrap()
            .dyn_into::<HtmlCanvasElement>()
            .unwrap()
            .clone();

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    fonts::install(&cc.egui_ctx);
                    let saved = settings_io::load_or_detect();
                    themes::apply(&cc.egui_ctx, saved.theme);
                    io::install_clipboard_paste_listener();
                    Ok(Box::new(app::EditorApp::default()))
                }),
            )
            .await
            .expect("failed to start eframe");
    });
}
