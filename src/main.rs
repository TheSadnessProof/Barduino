// Release builds on Windows open without a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod antigravity;
mod app;
mod browser;
mod changes;
mod chat;
mod claude;
mod codex;
mod git_diff;
mod icons;
mod models;
mod plan;
mod session;
mod settings;
mod sidebar;
mod terminal;
mod tools;
mod usage;

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
        Box::new(|cc| {
            set_text_sizes(&cc.egui_ctx);
            Ok(Box::new(app::BarduinoApp::new(cc)))
        }),
    )
}

/// egui's own text is smaller than a desktop app of this kind wants, so every
/// text style is set here rather than being nudged at each place it's used.
fn set_text_sizes(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(21.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(14.5, FontFamily::Proportional)),
            // Asides, not fine print: still readable at a glance.
            (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace)),
        ]
        .into();
    });
}
