fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Quartz Forge")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([980.0, 620.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Quartz Forge",
        native_options,
        Box::new(|_cc| Ok(Box::new(quartz_forge::app::QuartzForgeApp::default()))),
    )
}
