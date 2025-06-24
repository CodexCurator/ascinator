#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod ascii_converter;
mod app_ui; // Our new module for UI and app state

fn main() -> Result<(), eframe::Error> {
    // Setup logging (optional, but good for development)
    tracing_subscriber::fmt::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 650.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Ascii Video Converter & Player (Rust)",
        options,
        Box::new(|cc| Box::new(app_ui::AsciiApp::new(cc))),
    )
}
