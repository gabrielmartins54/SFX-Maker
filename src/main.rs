mod app;
mod geometry;
mod model;
mod render;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Manga SFX Maker")
            .with_inner_size([1200.0, 800.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Manga SFX Maker",
        options,
        Box::new(|_cc| Ok(Box::new(app::SfxApp::new()))),
    )
}
