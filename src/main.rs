// Release builds on Windows open without a console window behind the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod agent_terminal;
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
mod tool_call;
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
        if let Some(bytes) = read_font(font_dir.join("consola.ttf")) {
            fonts.font_data.insert("system_mono".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "system_mono".to_owned());
            loaded = true;
        }
        // Segoe UI Symbol for box drawing, arrows, checkmarks and unicode terminal symbols.
        if let Some(bytes) = read_font(font_dir.join("seguisym.ttf")) {
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
            if let Some(bytes) = read_font(path) {
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

/// Reads a font file, or None when it isn't there or doesn't look like a font.
fn read_font(path: impl AsRef<std::path::Path>) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    looks_like_a_font(&bytes).then_some(bytes)
}

/// Whether these bytes begin like a TrueType or OpenType file, with a table
/// directory that fits inside them.
///
/// This is checked here because epaint panics rather than erroring on a font it
/// can't parse, and it does so while building the first frame — after the window
/// is already up, and with no console in a release build. A truncated or stubbed
/// `consola.ttf`, from a bad update or a font manager, would take the app down
/// with nothing on screen to say why. A header this far wrong is the realistic
/// way that happens; a file that parses past it is left to epaint.
fn looks_like_a_font(bytes: &[u8]) -> bool {
    const SFNT: [&[u8]; 4] = [&[0x00, 0x01, 0x00, 0x00], b"true", b"ttcf", b"OTTO"];
    if !SFNT.iter().any(|tag| bytes.starts_with(tag)) {
        return false;
    }
    // A collection counts its fonts differently, so the tag is as far as we go.
    if bytes.starts_with(b"ttcf") {
        return true;
    }
    let Some(count) = bytes.get(4..6) else { return false };
    let tables = u16::from_be_bytes([count[0], count[1]]) as usize;
    tables > 0 && bytes.len() >= 12 + tables * 16
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_looking_fonts_reach_epaint() {
        // The shapes that actually turn up: a missing file, a zero-byte stub, a
        // text file with the wrong extension, and a font cut off mid-download.
        assert!(!looks_like_a_font(b""));
        assert!(!looks_like_a_font(b"not a font at all"));
        assert!(!looks_like_a_font(&[0x00, 0x01, 0x00, 0x00]), "a header with no table directory");
        let truncated: Vec<u8> = [&[0x00, 0x01, 0x00, 0x00, 0x00, 0x20][..], &[0; 40][..]].concat();
        assert!(!looks_like_a_font(&truncated), "32 tables promised, far fewer bytes present");

        // A well-formed single face and a collection are both let through.
        let mut face = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x02];
        face.extend_from_slice(&[0; 6 + 2 * 16]);
        assert!(looks_like_a_font(&face));
        assert!(looks_like_a_font(b"ttcf\x00\x01\x00\x00"), "a collection is taken on its tag");

        // And the fonts this computer actually has must not be turned away, or
        // the feature would silently do nothing.
        for name in ["consola.ttf", "seguisym.ttf", "arial.ttf"] {
            let path = std::path::Path::new(r"C:\Windows\Fonts").join(name);
            if let Ok(bytes) = std::fs::read(&path) {
                assert!(looks_like_a_font(&bytes), "{} should be accepted", path.display());
            }
        }
    }
}
