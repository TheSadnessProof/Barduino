// Release builds on Windows open without a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod app;
mod browser;
mod chat;
mod claude;
mod gemini;
mod session;
mod settings;
mod terminal;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Barduino")
            .with_inner_size([1400.0, 850.0])
            .with_min_inner_size([900.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Barduino",
        options,
        Box::new(|cc| Ok(Box::new(app::BarduinoApp::new(cc)))),
    )
}
