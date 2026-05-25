mod app;

pub use app::FortrustApp;

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([940.0, 620.0])
            .with_title("Fortrust"),
        ..Default::default()
    };

    eframe::run_native(
        "Fortrust",
        options,
        Box::new(|creation_context| Ok(Box::new(FortrustApp::new(creation_context)))),
    )
}
