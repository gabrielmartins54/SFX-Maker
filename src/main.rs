mod app;
mod export;
mod fonts;
mod geometry;
mod history;
mod interact;
mod library;
mod model;
mod random;
mod render;
mod settings;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Manga SFX Maker")
            .with_inner_size([1200.0, 800.0])
            .with_drag_and_drop(true),
        // MSAA gives the GPU canvas smooth glyph edges.
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        "Manga SFX Maker",
        options,
        Box::new(|_cc| Ok(Box::new(app::SfxApp::new()))),
    )
}
