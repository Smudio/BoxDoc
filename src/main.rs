//! BoxDoc – Einstiegspunkt.

mod app;
mod canvas;
mod fonts;
mod geometry;
mod io;
mod model;
mod odt;
mod printing;
mod store;
mod themes;

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
            // Schriften beim Start registrieren.
            fonts::install(&cc.egui_ctx);
            // Standard-Thema anwenden.
            themes::apply(&cc.egui_ctx, model::Theme::default());
            Ok(Box::new(app::EditorApp::default()))
        }),
    )
}
