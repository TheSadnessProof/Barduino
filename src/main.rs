// Release builds on Windows open without a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod claude;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Barduino")
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([520.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Barduino",
        options,
        Box::new(|_cc| Ok(Box::new(app::BarduinoApp::new()))),
    )
}
