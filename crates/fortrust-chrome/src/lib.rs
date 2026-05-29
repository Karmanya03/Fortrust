mod animation;
mod app;
mod download;
mod icons;
mod omnibox;
mod shield;
mod sidebar;
mod speed_dial;
mod backgrounds;
mod theme;

pub use app::FortrustApp;

pub fn run() -> eframe::Result<()> {
    let logo_bytes = include_bytes!("../../../assets/Fortrust-Logo.png");
    let icon = image::load_from_memory(logo_bytes)
        .ok()
        .map(|img| {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            std::sync::Arc::new(egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            })
        });

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([940.0, 620.0])
        .with_title("Fortrust");
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Fortrust",
        options,
        Box::new(|creation_context| {
            let app = FortrustApp::new(creation_context);
            Ok(Box::new(app))
        }),
    )
}
