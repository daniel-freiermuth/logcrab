mod parser;
mod anomaly;
mod ui;
mod app;

use app::LogOwlApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(&[])
                    .unwrap_or_default()
            ),
        ..Default::default()
    };
    
    eframe::run_native(
        "LogOwl - Log Anomaly Explorer",
        native_options,
        Box::new(|cc| Ok(Box::new(LogOwlApp::new(cc)))),
    )
}
