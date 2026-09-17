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
mod commands;
mod git_diff;
mod icons;
mod line_diff;
mod logo;
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
            .with_icon(logo::window_icon(64))
            .with_inner_size([1400.0, 850.0])
            .with_min_inner_size([900.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Barduino",
        options,
        Box::new(|cc| {
            load_system_fonts(&cc.egui_ctx);
            set_text_sizes(&cc.egui_ctx);
            Ok(Box::new(app::BarduinoApp::new(cc)))
        }),
    )
}

/// Loads clean system monospace and symbol fonts if available, so box-drawing
/// characters, arrows and terminal symbols render sharply without fallback boxes.
fn load_system_fonts(ctx: &egui::Context) {
    use std::sync::Arc;
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded = false;

    #[cfg(windows)]
    {
        let font_dir = std::path::Path::new("C:\\Windows\\Fonts");
        // Consolas for clean monospace text.
        if let Ok(bytes) = std::fs::read(font_dir.join("consola.ttf")) {
            fonts.font_data.insert("system_mono".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "system_mono".to_owned());
            loaded = true;
        }
        // Segoe UI Symbol for box drawing, arrows, checkmarks and unicode terminal symbols.
        if let Ok(bytes) = std::fs::read(font_dir.join("seguisym.ttf")) {
            fonts.font_data.insert("system_symbols".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(egui::FontFamily::Monospace).or_default().push("system_symbols".to_owned());
            fonts.families.entry(egui::FontFamily::Proportional).or_default().push("system_symbols".to_owned());
            loaded = true;
        }
    }

    #[cfg(target_os = "macos")]
    {
        let paths = ["/System/Library/Fonts/SFMono-Regular.otf", "/System/Library/Fonts/Menlo.ttc"];
        for path in paths {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert("system_mono".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
                fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "system_mono".to_owned());
                loaded = true;
                break;
            }
        }
    }

    if loaded {
        ctx.set_fonts(fonts);
    }
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

