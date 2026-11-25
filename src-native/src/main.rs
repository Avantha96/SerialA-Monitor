#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use seriala_native::app::SerialAApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SerialA Monitor")
            .with_app_id("seriala-monitor")
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([880.0, 560.0]),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "SerialA Monitor",
        options,
        Box::new(|cc| Ok(Box::new(SerialAApp::new(cc)))),
    )
}
