//! BoxDoc – Einstiegspunkt.

mod app;
mod canvas;
mod fonts;
mod geometry;
mod history;
mod io;
mod model;
#[cfg(not(target_arch = "wasm32"))]
mod odt;
#[cfg(not(target_arch = "wasm32"))]
mod printing;
mod settings_io;
mod store;
mod themes;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("BoxDoc")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "BoxDoc",
        options,
        Box::new(|cc| {
            fonts::install(&cc.egui_ctx);
            themes::apply(&cc.egui_ctx, model::Theme::default());
            io::install_clipboard_paste_listener();
            Ok(Box::new(app::EditorApp::default()))
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
                    themes::apply(&cc.egui_ctx, model::Theme::default());
                    io::install_clipboard_paste_listener();
                    Ok(Box::new(app::EditorApp::default()))
                }),
            )
            .await
            .expect("failed to start eframe");
    });
}
