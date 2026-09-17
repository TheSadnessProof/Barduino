//! A terminal panel: a real shell running in a pseudo-terminal, drawn with egui.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use eframe::egui::{self, Color32, FontId, Key, Modifiers, Sense, Vec2, text::LayoutJob};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};

use crate::agent;

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
    pub fn start(cwd: &Path, shell: &Path, ctx: egui::Context) -> Result<Self, String> {
        let size = (24, 80);
        let pair = portable_pty::native_pty_system()
            .openpty(pty_size(size))
            .map_err(|err| format!("Couldn't open a terminal: {err}"))?;

        let mut cmd = CommandBuilder::new(shell);
        configure(&mut cmd, shell);
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

    /// Draws the terminal. `take_keyboard` gives it the keyboard without waiting
    /// for a click. Returns true if the user asked to restart the shell.
    pub fn ui(&mut self, ui: &mut egui::Ui, take_keyboard: bool) -> bool {
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

        if response.clicked() || take_keyboard {
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

/// Shows the terminal for a session, starting it the first time. `typed` is
/// text to send to the shell as if the user had typed it once it is running, and
/// `take_keyboard`
/// puts the cursor in it straight away. Returns true if the shell was restarted,
/// since the replacement is a new terminal that wants the cursor too.
pub fn show(
    ui: &mut egui::Ui,
    slot: &mut Option<Result<Terminal, String>>,
    cwd: &Path,
    shell: &Path,
    typed: &mut Option<String>,
    take_keyboard: bool,
) -> bool {
    let terminal = slot.get_or_insert_with(|| Terminal::start(cwd, shell, ui.ctx().clone()));
    let restart = match terminal {
        Ok(terminal) => {
            // Taken here rather than by the caller, so a shell that failed to start
            // doesn't swallow the command it was opened to run.
            if let Some(text) = typed.take() {
                terminal.parser().screen_mut().set_scrollback(0);
                write_all(&terminal.writer, text.as_bytes());
            }
            terminal.ui(ui, take_keyboard)
        }
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error.as_str());
            ui.button("Try again").clicked()
        }
    };
    if restart {
        *slot = None;
    }
    restart
}

fn write_all(writer: &Writer, bytes: &[u8]) {
    let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
    let _ = writer.write_all(bytes);
    let _ = writer.flush();
}

fn pty_size((rows, cols): (u16, u16)) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

/// A shell Barduino can start a terminal with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// What Settings calls it.
    pub name: &'static str,
    pub path: PathBuf,
}

/// The shells on this computer, the most preferred first. Only file checks, so
/// it costs little enough to ask again whenever Settings is opened.
pub fn available_shells() -> Vec<Shell> {
    let mut found: Vec<Shell> = Vec::new();
    let mut add = |name: &'static str, path: Option<PathBuf>| {
        if let Some(path) = path.filter(|path| path.is_file())
            && !found.iter().any(|shell| shell.path == path)
        {
            found.push(Shell { name, path });
        }
    };

    if cfg!(windows) {
        add("PowerShell 7", agent::find_on_path(&["pwsh.exe"]));
        add("Windows PowerShell", agent::find_on_path(&["powershell.exe"]));
        add("Command Prompt", agent::find_on_path(&["cmd.exe"]));
        add("Git Bash", agent::find_on_path(&["bash.exe"]).or_else(git_bash));
    } else {
        // The login shell comes first, since it's the one the user chose already.
        add("Login shell", std::env::var_os("SHELL").map(PathBuf::from));
        for (name, path) in
            [("Bash", "/bin/bash"), ("Zsh", "/bin/zsh"), ("Fish", "/usr/bin/fish"), ("sh", "/bin/sh")]
        {
            add(name, Some(PathBuf::from(path)));
        }
    }
    found
}

/// Git for Windows ships bash but doesn't always leave it on PATH. Returns None
/// anywhere there is no `ProgramFiles`, which is every other platform.
fn git_bash() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles")?;
    Some(PathBuf::from(program_files).join("Git").join("bin").join("bash.exe"))
}

/// The shell a new terminal uses while the user hasn't chosen one in Settings.
pub fn default_shell() -> PathBuf {
    match available_shells().into_iter().next() {
        Some(shell) => shell.path,
        // Worth trying even when the search came up empty.
        None => PathBuf::from(if cfg!(windows) { "powershell.exe" } else { "/bin/sh" }),
    }
}

/// The flags and environment one particular shell wants. `-NoLogo` is
/// PowerShell's alone — cmd would take it as a stray argument and bash would
/// refuse it — so this asks which shell it is rather than which platform.
fn configure(cmd: &mut CommandBuilder, shell: &Path) {
    let name = shell.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
    match name.as_str() {
        "pwsh" | "powershell" => cmd.arg("-NoLogo"),
        "cmd" => {}
        // Everything else is happier being told what the terminal can do.
        _ => cmd.env("TERM", "xterm-256color"),
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

    /// Process ids of running processes whose command line contains `marker`.
    #[cfg(windows)]
    fn processes_with(marker: &str) -> Vec<String> {
        let script = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{marker}*' -and $_.Name -ne 'powershell.exe' }} | ForEach-Object {{ \"$($_.ProcessId) $($_.Name)\" }}"
        );
        let output = crate::agent::hidden_command("powershell.exe").args(["-NoProfile", "-Command", &script]).output().unwrap();
        String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).filter(|l| !l.is_empty()).collect()
    }

    /// Lists the shells this computer offers. Free, but it depends on what is
    /// installed here, so it only runs when asked for:
    /// `cargo test -- --ignored shells --nocapture`
    #[test]
    #[ignore]
    fn finds_the_shells_on_this_computer() {
        let shells = available_shells();
        for shell in &shells {
            println!("{:20} {}", shell.name, shell.path.display());
        }
        println!("default: {}", default_shell().display());
        assert!(!shells.is_empty(), "anything running this has a shell");
        assert!(shells.iter().all(|shell| shell.path.is_file()), "and each one is really there");
        let paths: Vec<&PathBuf> = shells.iter().map(|shell| &shell.path).collect();
        let mut unique = paths.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(paths.len(), unique.len(), "with no shell listed twice");
    }

    /// Starts a long-running program inside a terminal, closes the terminal, and
    /// checks that the program was stopped too. Only runs when asked for:
    /// `cargo test -- --ignored closing_a_terminal --nocapture`
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn closing_a_terminal_stops_programs_started_in_it() {
        // An unusual ping count makes the process easy to find.
        const MARKER: &str = "-n 4242";
        let terminal =
            Terminal::start(&std::env::temp_dir(), &default_shell(), egui::Context::default()).unwrap();
        write_all(&terminal.writer, format!("ping {MARKER} 127.0.0.1\r").as_bytes());

        let mut running = Vec::new();
        for _ in 0..40 {
            running = processes_with(MARKER);
            if !running.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        println!("before closing: {running:?}");
        assert!(!running.is_empty(), "ping should have started inside the terminal");

        drop(terminal);
        std::thread::sleep(std::time::Duration::from_secs(2));
        let left = processes_with(MARKER);
        println!("after closing: {left:?}");
        assert!(left.is_empty(), "still running after the terminal closed: {left:?}");
    }

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
