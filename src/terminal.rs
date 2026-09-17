//! A terminal panel: a real shell running in a pseudo-terminal, drawn with egui.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use eframe::egui::{self, Color32, FontId, Key, Modifiers, Sense, Vec2, text::LayoutJob};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};

const SCROLLBACK_LINES: usize = 5000;
const FONT_SIZE: f32 = 13.0;
const PADDING: f32 = 6.0;
const BACKGROUND: Color32 = Color32::from_rgb(24, 24, 24);
const FOREGROUND: Color32 = Color32::from_rgb(204, 204, 204);

type Parser = vt100::Parser<Replies>;
type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// Answers the questions programs ask the terminal, such as where the cursor is.
/// Windows' pseudo-console asks for the cursor position at startup and prints
/// nothing until it gets an answer.
#[derive(Default)]
struct Replies {
    pending: Vec<u8>,
}

impl vt100::Callbacks for Replies {
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first_param = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
        match (i1, c, first_param) {
            // Cursor position report.
            (None, 'n', 6) => {
                let (row, col) = screen.cursor_position();
                self.pending.extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            // Status report: "OK".
            (None, 'n', 5) => self.pending.extend_from_slice(b"\x1b[0n"),
            // Device attributes: a VT100 with advanced video.
            (None, 'c', 0) => self.pending.extend_from_slice(b"\x1b[?1;2c"),
            _ => {}
        }
    }
}

pub struct Terminal {
    parser: Arc<Mutex<Parser>>,
    writer: Writer,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    size: (u16, u16),
    exited: Arc<AtomicBool>,
    /// Scroll wheel movement not yet turned into whole lines.
    scroll_remainder: f32,
}

impl Terminal {
    pub fn start(cwd: &Path, ctx: egui::Context) -> Result<Self, String> {
        let size = (24, 80);
        let pair = portable_pty::native_pty_system()
            .openpty(pty_size(size))
            .map_err(|err| format!("Couldn't open a terminal: {err}"))?;

        let mut cmd = CommandBuilder::new(default_shell());
        if cfg!(windows) {
            cmd.arg("-NoLogo");
        } else {
            cmd.env("TERM", "xterm-256color");
        }
        cmd.cwd(cwd);
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|err| format!("Couldn't start the shell: {err}"))?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|err| err.to_string())?;
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(|err| err.to_string())?));
        let parser = Arc::new(Mutex::new(Parser::new_with_callbacks(
            size.0,
            size.1,
            SCROLLBACK_LINES,
            Replies::default(),
        )));
        let exited = Arc::new(AtomicBool::new(false));

        let output = Arc::clone(&parser);
        let reply_writer = Arc::clone(&writer);
        let done = Arc::clone(&exited);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let replies = {
                            let mut parser = output.lock().unwrap_or_else(PoisonError::into_inner);
                            parser.process(&buf[..n]);
                            std::mem::take(&mut parser.callbacks_mut().pending)
                        };
                        if !replies.is_empty() {
                            write_all(&reply_writer, &replies);
                        }
                        ctx.request_repaint();
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });

        Ok(Self { parser, writer, master: pair.master, child, size, exited, scroll_remainder: 0.0 })
    }

    fn parser(&self) -> MutexGuard<'_, Parser> {
        self.parser.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn has_exited(&mut self) -> bool {
        // On Windows the output pipe can stay open after the shell exits, so also ask the process.
        self.exited.load(Ordering::Relaxed) || matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Draws the terminal. Returns true if the user asked to restart the shell.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut restart = false;
        if self.has_exited() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("The shell has exited.").weak());
                restart = ui.button("Restart").clicked();
            });
        }

        let font_id = FontId::monospace(FONT_SIZE);
        let (char_width, row_height) =
            ui.ctx().fonts_mut(|fonts| (fonts.glyph_width(&font_id, 'M'), fonts.row_height(&font_id)));

        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        let rows = ((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
        let cols = ((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
        if (rows, cols) != self.size {
            self.size = (rows, cols);
            let _ = self.master.resize(pty_size(self.size));
            self.parser().screen_mut().set_size(rows, cols);
        }

        if response.clicked() {
            response.request_focus();
        }
        let focused = response.has_focus();
        if focused {
            // Keep Tab, arrows and Escape in the terminal instead of moving focus around the app.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
            self.handle_input(ui);
        }
        if response.hovered() {
            self.handle_scroll(ui, row_height);
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);
        let origin = rect.min + Vec2::splat(PADDING);

        let parser = self.parser();
        let screen = parser.screen();
        for row in 0..rows {
            let job = row_layout(screen, row, cols, &font_id);
            let galley = painter.layout_job(job);
            painter.galley(origin + Vec2::new(0.0, row as f32 * row_height), galley, FOREGROUND);
        }

        if !screen.hide_cursor() && screen.scrollback() == 0 {
            let (cursor_row, cursor_col) = screen.cursor_position();
            let cursor = egui::Rect::from_min_size(
                origin + Vec2::new(cursor_col as f32 * char_width, cursor_row as f32 * row_height),
                Vec2::new(char_width, row_height),
            );
            if focused {
                painter.rect_filled(cursor, 0.0, FOREGROUND.gamma_multiply(0.6));
            } else {
                painter.rect_stroke(cursor, 0.0, (1.0, FOREGROUND.gamma_multiply(0.6)), egui::StrokeKind::Inside);
            }
        }
        restart
    }

    fn handle_input(&mut self, ui: &egui::Ui) {
        let (app_cursor, bracketed_paste) = {
            let parser = self.parser();
            (parser.screen().application_cursor(), parser.screen().bracketed_paste())
        };

        let mut bytes = Vec::new();
        ui.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Text(text) => bytes.extend_from_slice(text.as_bytes()),
                    egui::Event::Paste(text) => {
                        let text = text.replace('\n', "\r");
                        if bracketed_paste {
                            bytes.extend_from_slice(b"\x1b[200~");
                            bytes.extend_from_slice(text.as_bytes());
                            bytes.extend_from_slice(b"\x1b[201~");
                        } else {
                            bytes.extend_from_slice(text.as_bytes());
                        }
                    }
                    // The app turns Ctrl+C and Ctrl+X into copy/cut; in a terminal they are control keys.
                    egui::Event::Copy => bytes.push(0x03),
                    egui::Event::Cut => bytes.push(0x18),
                    egui::Event::Key { key, pressed: true, modifiers, .. } => {
                        if let Some(sequence) = key_sequence(*key, *modifiers, app_cursor) {
                            bytes.extend_from_slice(&sequence);
                        }
                    }
                    _ => {}
                }
            }
        });

        if !bytes.is_empty() {
            self.parser().screen_mut().set_scrollback(0);
            write_all(&self.writer, &bytes);
        }
    }

    fn handle_scroll(&mut self, ui: &egui::Ui, row_height: f32) {
        self.scroll_remainder += ui.input(|i| i.smooth_scroll_delta.y);
        let lines = (self.scroll_remainder / row_height).trunc();
        if lines != 0.0 {
            self.scroll_remainder -= lines * row_height;
            let mut parser = self.parser();
            let offset = parser.screen().scrollback() as i64 + lines as i64;
            parser.screen_mut().set_scrollback(offset.max(0) as usize);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Shows the terminal for a session, starting it the first time.
pub fn show(ui: &mut egui::Ui, slot: &mut Option<Result<Terminal, String>>, cwd: &Path) {
    let terminal = slot.get_or_insert_with(|| Terminal::start(cwd, ui.ctx().clone()));
    let restart = match terminal {
        Ok(terminal) => terminal.ui(ui),
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error.as_str());
            ui.button("Try again").clicked()
        }
    };
    if restart {
        *slot = None;
    }
}

fn write_all(writer: &Writer, bytes: &[u8]) {
    let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = writer.write_all(bytes);
    let _ = writer.flush();
}

fn pty_size((rows, cols): (u16, u16)) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

fn default_shell() -> PathBuf {
    if cfg!(windows) {
        // Prefer PowerShell 7 when it's installed.
        let pwsh = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths).map(|dir| dir.join("pwsh.exe")).find(|path| path.is_file())
        });
        pwsh.unwrap_or_else(|| PathBuf::from("powershell.exe"))
    } else {
        std::env::var_os("SHELL").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/bin/sh"))
    }
}

/// The styling that can differ between neighboring cells.
#[derive(Clone, Copy, PartialEq)]
struct CellStyle {
    fg: Color32,
    bg: Color32,
    italic: bool,
    underline: bool,
}

fn row_layout(screen: &vt100::Screen, row: u16, cols: u16, font_id: &FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut run = String::new();
    let mut run_style: Option<CellStyle> = None;

    let mut flush = |run: &mut String, style: CellStyle| {
        job.append(
            run,
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color: style.fg,
                background: style.bg,
                italics: style.italic,
                underline: if style.underline { egui::Stroke::new(1.0, style.fg) } else { egui::Stroke::NONE },
                ..Default::default()
            },
        );
        run.clear();
    };

    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else { continue };
        if cell.is_wide_continuation() {
            continue;
        }
        let (mut fg, mut bg) = (color(cell.fgcolor(), FOREGROUND), color(cell.bgcolor(), Color32::TRANSPARENT));
        if cell.inverse() {
            (fg, bg) = (if bg == Color32::TRANSPARENT { BACKGROUND } else { bg }, fg);
        }
        if cell.dim() {
            fg = fg.gamma_multiply(0.7);
        }
        let style = CellStyle { fg, bg, italic: cell.italic(), underline: cell.underline() };

        if let Some(previous) = run_style
            && previous != style
        {
            flush(&mut run, previous);
        }
        run_style = Some(style);
        run.push_str(if cell.has_contents() { cell.contents() } else { " " });
    }
    if let Some(style) = run_style {
        flush(&mut run, style);
    }
    job
}

fn color(color: vt100::Color, default: Color32) -> Color32 {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(i @ 0..16) => {
            let (r, g, b) = ANSI[i as usize];
            Color32::from_rgb(r, g, b)
        }
        vt100::Color::Idx(i @ 16..232) => {
            // The 6x6x6 color cube.
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            let i = i - 16;
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        vt100::Color::Idx(i) => {
            let gray = 8 + (i - 232) * 10;
            Color32::from_rgb(gray, gray, gray)
        }
    }
}

/// The bytes a terminal expects for a special key, or None for keys that arrive as text.
fn key_sequence(key: Key, modifiers: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let csi = |s: &str| [b"\x1b[".as_slice(), s.as_bytes()].concat();
    let arrow = |letter: &str| {
        if modifiers.ctrl {
            csi(&format!("1;5{letter}"))
        } else if app_cursor {
            [b"\x1bO".as_slice(), letter.as_bytes()].concat()
        } else {
            csi(letter)
        }
    };

    Some(match key {
        Key::Enter => b"\r".to_vec(),
        Key::Backspace => vec![0x7f],
        Key::Tab if modifiers.shift => csi("Z"),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => vec![0x1b],
        Key::ArrowUp => arrow("A"),
        Key::ArrowDown => arrow("B"),
        Key::ArrowRight => arrow("C"),
        Key::ArrowLeft => arrow("D"),
        Key::Home => csi("H"),
        Key::End => csi("F"),
        Key::PageUp => csi("5~"),
        Key::PageDown => csi("6~"),
        Key::Insert => csi("2~"),
        Key::Delete => csi("3~"),
        _ if modifiers.ctrl && !modifiers.alt => {
            // Ctrl+letter sends the matching control character, e.g. Ctrl+D is 0x04.
            let [letter] = key.name().as_bytes() else { return None };
            if !letter.is_ascii_alphabetic() {
                return None;
            }
            vec![letter.to_ascii_uppercase() & 0x1f]
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_keys_map_to_escape_sequences() {
        let none = Modifiers::NONE;
        assert_eq!(key_sequence(Key::Enter, none, false), Some(b"\r".to_vec()));
        assert_eq!(key_sequence(Key::ArrowUp, none, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_sequence(Key::ArrowUp, none, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_sequence(Key::ArrowLeft, Modifiers::CTRL, false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key_sequence(Key::Tab, Modifiers::SHIFT, false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key_sequence(Key::D, Modifiers::CTRL, false), Some(vec![0x04]));
        assert_eq!(key_sequence(Key::A, none, false), None);
        assert_eq!(key_sequence(Key::Num1, Modifiers::CTRL, false), None);
    }

    #[test]
    fn indexed_colors_cover_the_palette() {
        assert_eq!(color(vt100::Color::Idx(1), FOREGROUND), Color32::from_rgb(205, 49, 49));
        assert_eq!(color(vt100::Color::Idx(16), FOREGROUND), Color32::from_rgb(0, 0, 0));
        assert_eq!(color(vt100::Color::Idx(231), FOREGROUND), Color32::from_rgb(255, 255, 255));
        assert_eq!(color(vt100::Color::Idx(255), FOREGROUND), Color32::from_rgb(238, 238, 238));
        assert_eq!(color(vt100::Color::Default, FOREGROUND), FOREGROUND);
    }

    #[test]
    fn answers_cursor_position_and_status_requests() {
        let mut parser = Parser::new_with_callbacks(5, 20, 0, Replies::default());
        parser.process(b"abc\x1b[6n\x1b[5n\x1b[c\x1b[>c");
        assert_eq!(parser.callbacks().pending, b"\x1b[1;4R\x1b[0n\x1b[?1;2c");
    }

    #[test]
    fn rows_group_cells_with_the_same_style() {
        let mut parser = vt100::Parser::new(2, 10, 0);
        parser.process(b"ab\x1b[31mcd\x1b[0m");
        let job = row_layout(parser.screen(), 0, 10, &FontId::monospace(FONT_SIZE));
        assert_eq!(job.text, "abcd      ");
        assert_eq!(job.sections.len(), 3);
    }
}
