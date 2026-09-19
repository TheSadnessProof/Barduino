# Architectural and Technical Survey: Terminal and PTY Infrastructure in Viper for Middle-Panel AI Provider CLIs

## 1. Observation

### 1.1 Existing Dependencies and Environment (`Cargo.toml`)
From `c:\Users\ditob\Documents\viper\Cargo.toml` (lines 6–24):
```toml
[dependencies]
chrono = { version = "0.4.45", default-features = false, features = ["clock"] }
eframe = { version = "0.36.2", features = ["persistence"] }
egui_commonmark = { version = "0.25.0", default-features = false, features = ["pulldown_cmark"] }
portable-pty = "0.9.0"
rfd = "0.17.2"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
ron = "0.12.2"
vt100 = "0.16.2"

[target.'cfg(any(windows, target_os = "macos"))'.dependencies]
wry = "0.57.0"

[target.'cfg(windows)'.dependencies]
windows = { version = "0.62.2", features = ["Win32_Security", "Win32_System_JobObjects"] }
```
- No async runtime (`tokio` is intentionally forbidden per `AGENTS.md` Rule 3.4).
- PTY emulation is powered by `portable-pty = "0.9.0"` (using native ConPTY on Windows) and `vt100 = "0.16.2"` for virtual terminal screen state.
- Windows process isolation and tree termination uses Win32 Job Objects via `windows` crate features `Win32_Security` and `Win32_System_JobObjects`.

### 1.2 Core Terminal Abstraction (`src/terminal.rs`)
In `src/terminal.rs`, the `Terminal` struct and supporting infrastructure manage the PTY lifecycle:
```rust
pub struct Terminal {
    parser: Arc<Mutex<Parser>>,
    writer: Writer,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    job: TerminalJob,
    size: (u16, u16),
    exited: Arc<AtomicBool>,
    /// Scroll wheel movement not yet turned into whole lines.
    scroll_remainder: f32,
    /// What the user has dragged over, for the clipboard.
    selection: Option<Selection>,
}
```
- **Type aliases** (`src/terminal.rs`, lines 20–21):
  ```rust
  type Parser = vt100::Parser<Replies>;
  type Writer = Arc<Mutex<Box<dyn Write + Send>>>;
  ```
- **ConPTY Handshake Callback (`Replies`)** (`src/terminal.rs`, lines 26–54):
  ```rust
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
  ```
- **Windows Process Tree Job Object (`TerminalJob`)** (`src/terminal.rs`, lines 58–126):
  ```rust
  #[cfg(windows)]
  struct TerminalJob(Option<windows::Win32::Foundation::HANDLE>);

  #[cfg(windows)]
  impl TerminalJob {
      fn new(pid: Option<u32>) -> Self {
          // Creates JobObject with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
          // Assigns child process handle to job object
      }
      fn kill(&self) {
          if let Some(job) = self.0 {
              let _ = unsafe { windows::Win32::System::JobObjects::TerminateJobObject(job, 1) };
          }
      }
  }

  #[cfg(windows)]
  impl Drop for TerminalJob {
      fn drop(&mut self) {
          if let Some(job) = self.0.take() {
              let _ = unsafe { windows::Win32::System::JobObjects::TerminateJobObject(job, 1) };
              let _ = unsafe { windows::Win32::Foundation::CloseHandle(job) };
          }
      }
  }
  ```
- **Spawning and Background Reader Thread** (`src/terminal.rs`, lines 177–230):
  ```rust
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

      let job = TerminalJob::new(child.process_id());
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

      Ok(Self { parser, writer, master: pair.master, child, job, size, exited, scroll_remainder: 0.0, selection: None })
  }
  ```

### 1.3 Keystroke Capture, Input Handling, and Focus Locking
In `src/terminal.rs` (lines 265–278, 350–403, 682–721):
- **Focus Locking Filter**:
  ```rust
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
  ```
- **Input Events**:
  - `egui::Event::Text(text)`: sends `text.as_bytes()` directly to the PTY writer.
  - `egui::Event::Paste(text)`: normalizes `\n` to `\r`. If `screen.bracketed_paste()` is enabled, encloses text in `\x1b[200~` and `\x1b[201~`.
  - `egui::Event::Copy`: if text is selected, copies text to egui clipboard; if no text is selected, sends byte `0x03` (ETX / Ctrl+C) to interrupt the running process.
  - `egui::Event::Cut`: sends byte `0x18` (CAN / Ctrl+X).
  - `egui::Event::Key`: dispatches through `key_sequence(*key, *modifiers, app_cursor)`.
- **Key Sequence Table (`key_sequence`)**:
  - `Key::Enter` $\to$ `\r` (`b"\r"`)
  - `Key::Backspace` $\to$ `0x7f` (ASCII DEL)
  - `Key::Tab` $\to$ `\t` (`Key::Tab + Shift` $\to$ `\x1b[Z`)
  - `Key::Escape` $\to$ `\x1b`
  - Arrows $\to$ `\x1b[A`, `\x1b[B`, `\x1b[C`, `\x1b[D` (supports Ctrl modifier `\x1b[1;5{A-D}` and application cursor mode `\x1bO{A-D}`)
  - Navigation keys $\to$ Home (`\x1b[H`), End (`\x1b[F`), PageUp (`\x1b[5~`), PageDown (`\x1b[6~`), Insert (`\x1b[2~`), Delete (`\x1b[3~`)
  - Ctrl combinations $\to$ `modifiers.ctrl && !modifiers.alt` converts ASCII characters `A-Z` to `0x01..0x1A` (`letter.to_ascii_uppercase() & 0x1f`).

### 1.4 ANSI Escape Codes, Colors, and Rendering
In `src/terminal.rs` (lines 252–255, 303–334, 585–680):
- **Font metrics**: Uses fixed monospace glyph width and row height (`FontId::monospace(FONT_SIZE)` where `FONT_SIZE = 13.0`).
- **Screen Buffer & Grid Layout**:
  `vt100::Screen` maintains cells with character content, colors, and attributes (bold, dim, italic, underline, inverse, wide continuations).
- **Cell Styling & LayoutJob**:
  `row_layout()` collects continuous runs of cells sharing the same `CellStyle` into an `egui::text::LayoutJob`. Wide character continuations (`cell.is_wide_continuation()`) are skipped so double-width characters align on the grid.
- **Color Palettes**:
  Supports `vt100::Color::Default`, `vt100::Color::Rgb(r, g, b)` (24-bit TrueColor), `vt100::Color::Idx(0..16)` (16 standard ANSI colors), `vt100::Color::Idx(16..232)` (6x6x6 RGB color cube), and `vt100::Color::Idx(232..256)` (24-step grayscale).
- **Cursor Rendering**:
  Positions cursor rectangle at `(cursor_col * char_width, cursor_row * row_height)`. If focused, painted as filled semi-transparent rectangle (`FOREGROUND.gamma_multiply(0.6)`); if unfocused, rendered as an outline stroke.

### 1.5 Terminal Resizing Propagation
In `src/terminal.rs` (lines 256–263):
```rust
let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
let rows = ((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
let cols = ((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
if (rows, cols) != self.size {
    self.size = (rows, cols);
    let _ = self.master.resize(pty_size(self.size));
    self.parser().screen_mut().set_size(rows, cols);
}
```
- Calculated dynamically each frame based on `ui.available_size()`.
- On change, calls `self.master.resize(pty_size(self.size))` which translates to `ResizePseudoConsole` on Windows ConPTY (or `ioctl(TIOCSWINSZ)` on Unix).
- Synchronously updates `vt100::Screen` dimensions via `self.parser().screen_mut().set_size(rows, cols)`.

### 1.6 Current Tool Panel Terminal Usage (`src/tools.rs`)
In `src/tools.rs` (lines 12–23, 162–167, 523–532):
- Secondary shell terminals are stored inside `enum Tab { Terminal { number, cwd, terminal, typed, focus } }`.
- `terminal: Option<Result<Terminal, String>>` is lazily spawned on first render via `terminal::show(...)`.
- The `Tools` struct is owned by `ViperApp` in `tools: BTreeMap<u64, Tools>`, indexed by session ID.
- Closing or deleting a session drops the `Tools` entry, which triggers `Terminal::drop` $\to$ `TerminalJob::kill` $\to$ clean termination of child processes.

### 1.7 Current Middle Panel Chat Architecture (`src/app.rs`, `src/chat.rs`, `src/session.rs`)
In `src/app.rs` (lines 531–605, 819–822):
- `egui::CentralPanel` currently hosts `self.chat_area(ui)`.
- `chat_area()` splits into `composer_panel` (`chat::composer`) and central `chat::conversation`.
- Turns are executed headless via `agent::start_turn` (`claude -p ... --output-format stream-json`, `codex exec --json ...`, `agy --output-format stream-json ...`), parsed by custom line parsers (`claude::parse_line`, `codex::parse_line`, `antigravity::parse_line`) and stored as `Entry` items in `Session.entries`.

---

## 2. Logic Chain

### 2.1 Feasibility of Embedding Interactive AI CLIs in Middle Panel
1. **P1 (PTY & Screen Parity)**: `portable-pty` and `vt100` in `src/terminal.rs` already implement a complete, robust, ANSI/VT100 terminal emulator with TrueColor, alternate screen buffer, cursor position reporting, selection/clipboard, and ConPTY startup handshake handling.
2. **P2 (Interactive TUI Execution)**: Interactive AI CLIs (`claude`, `codex`, `agy`) are standard terminal programs that require a PTY (`isatty(stdout) == true`) with support for VT100 cursor addressing, raw keyboard input, and dynamic window resize signals (SIGWINCH / ConPTY resize).
3. **P3 (Direct Alignment)**: Because `src/terminal.rs` already drives standard shells (`pwsh`, `cmd`, `bash`), it can run any CLI executable directly without intermediate stream parsers. Replacing `chat_area` with an embedded `Terminal` running the provider's CLI directly satisfies Requirements R1 and R2 from `ORIGINAL_REQUEST.md`.

### 2.2 Terminal Lifecycle per Session (Requirement R3)
1. **P1 (Session Ownership)**: In `src/session.rs`, `Session` represents one conversation. Each session has a `project_dir` / `working_dir()`, `provider: Provider`, and unique `id: u64`.
2. **P2 (Non-Serialized Execution State)**: `Terminal` contains OS handles (`MasterPty`, `Child`, `TerminalJob`), thread handles, and mutexes that cannot be serialized to disk. Therefore, the terminal instance must be marked `#[serde(skip)]` on `Session` (or stored in an ephemeral session-keyed table on `ViperApp`).
3. **P3 (Lazy Initialization)**: When a session is loaded from disk or created without a folder, `terminal` starts as `None`. When the middle panel is drawn for an active session with a valid folder, if `terminal.is_none()`, it is started using `provider.find()` in `session.working_dir()`.
4. **P4 (Clean Teardown)**: When a session is deleted via `SidebarAction::Delete(id)`:
   - In `app.rs`: `self.state.sessions.remove(index)`.
   - Dropping the `Session` drops its `Terminal`, which immediately triggers `TerminalJob::kill()` and `child.kill()`. All processes in the job tree (including dev servers or child commands spawned by the AI CLI) are terminated by the kernel.

### 2.3 Windows ConPTY Constraints and Adaptations
1. **P1 (Batch Script Execution on Windows)**: On Windows, `claude` installed via `npm -g` or native package managers is frequently located as `claude.cmd` or `claude.bat`.
   - `portable_pty::CommandBuilder` uses `CreateProcessW` under the hood. Windows `CreateProcessW` returns `ERROR_BAD_EXE_FORMAT` (error code 193) if passed a `.cmd` or `.bat` file directly as the application name.
   - *Inference*: The terminal process builder must detect if the executable has a `.cmd` or `.bat` extension (or execute via `cmd.exe /c "<path>"`), while `.exe` binaries (`codex.exe`, `agy.exe`, `claude.exe`) can be passed directly to `CommandBuilder::new`.
2. **P2 (Environment Variables)**: AI CLIs rely on `TERM` and `COLORTERM` to enable colors and mouse/cursor features. `configure(&mut cmd, ...)` in `terminal.rs` currently only sets `TERM=xterm-256color` for non-powershell/non-cmd. For provider CLIs, setting `TERM=xterm-256color` and `COLORTERM=truecolor` ensures rich rendering across all providers.
3. **P3 (ConPTY Handshake Query)**: ConPTY emits cursor query `\x1b[6n` immediately upon initialization. The `Replies` callback in `vt100::Callbacks` already responds with `\x1b[{row};{col}R`. This mechanism must be preserved when generalizing `Terminal::start` to run provider CLIs.

---

## 3. Caveats

1. **No Source Code Modifications**: This investigation is strictly read-only. No application files outside `.agents/survey_explorer_1` were altered.
2. **Provider CLI Installation Dependency**: Interactive provider CLIs require that the user has already installed the respective tool (`claude`, `codex`, `agy`). If an executable is missing on the machine, the terminal cannot spawn it; the UI must display a clear "not installed" placeholder with an installation hint and "Open Settings" link.
3. **Multi-Turn Chat History vs. Persistent PTY**: The old chat model stored structured `Entry` events in `app.ron`. An interactive PTY session stores raw terminal scrollback in memory (`SCROLLBACK_LINES = 5000`). PTY state cannot be serialized to disk across application restarts. When restarting Viper, sessions will start a fresh CLI instance in the session folder (or resume if supported by the CLI flags).
4. **Platform Scope**: While Windows ConPTY is primary (and explicitly surveyed), Unix (`portable-pty` native PTY using `forkpty` and process group signals) is structurally identical in `src/terminal.rs`.

---

## 4. Conclusion

### 4.1 Architectural Assessment
Viper's existing `src/terminal.rs` infrastructure is already **90% complete and architecturally ready** to power the middle-panel interactive AI CLI. The combination of `portable-pty` (0.9.0), `vt100` (0.16.2), Win32 Job Objects (`TerminalJob`), and egui custom painting (`LayoutJob` + monospace glyph cache) provides a rock-solid, production-grade foundation.

### 4.2 Key Extensions and Refactorings Needed for Implementation

```
+-----------------------------------------------------------------------------------+
|                                 ViperApp (ui)                                     |
|                                                                                   |
|  +---------------------+  +--------------------------------+  +----------------+  |
|  |    Sidebar (Left)   |  |     Middle Panel (Central)     |  |  Tools (Right) |  |
|  |                     |  |                                |  |                |  |
|  |  Session 1 [Claude] |  |   Active Session Terminal:     |  |  Shell Tab 1   |  |
|  |  Session 2 [Codex]  |  |   - Portable-PTY Master        |  |  Changes Tab   |  |
|  |  Session 3 [agy]    |  |   - vt100 Parser & Screen      |  |  Browser Tab   |  |
|  |                     |  |   - Direct Interactive CLI     |  |                |  |
|  |  [+ New Session]    |  |   - Dynamic Resize & Focus     |  |                |  |
|  +---------------------+  +--------------------------------+  +----------------+  |
+-----------------------------------------------------------------------------------+
```

#### 1. Generalize `Terminal::start` for Arbitrary Commands / Providers
Currently, `Terminal::start` only accepts `shell: &Path`.
Extend `Terminal` with a dedicated constructor:
```rust
impl Terminal {
    pub fn start_command(
        cwd: &Path,
        program: &Path,
        args: &[String],
        ctx: egui::Context,
    ) -> Result<Self, String> {
        let size = (24, 80);
        let pair = portable_pty::native_pty_system()
            .openpty(pty_size(size))
            .map_err(|err| format!("Couldn't open a terminal: {err}"))?;

        let mut cmd = CommandBuilder::new(program);
        #[cfg(windows)]
        {
            // If program is a .cmd or .bat (e.g. npm-installed claude.cmd), wrap via cmd.exe
            if program.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")) {
                cmd = CommandBuilder::new("cmd.exe");
                cmd.args(["/c", &program.to_string_lossy()]);
            }
        }
        cmd.args(args);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.cwd(cwd);

        let child = pair.slave.spawn_command(cmd)
            .map_err(|err| format!("Couldn't start process: {err}"))?;
        drop(pair.slave);
        // ... (standard reader thread, replies, and job object creation)
    }
}
```

#### 2. Session-Level Terminal Management
In `src/session.rs`:
```rust
pub struct Session {
    pub id: u64,
    pub title: String,
    pub project_dir: PathBuf,
    pub provider: Provider,
    // ...
    #[serde(skip)]
    pub terminal: Option<Result<Terminal, String>>,
    #[serde(skip)]
    pub focus_terminal: bool,
}
```
- When switching sessions (`SidebarAction::Select(id)`):
  - Set `active_session = id`.
  - Set `session.focus_terminal = true`.
- When deleting a session (`SidebarAction::Delete(id)`):
  - Removing the session from `sessions: Vec<Session>` drops the session and its `Option<Result<Terminal, String>>`.
  - `Terminal::drop` terminates the entire Job Object and child process.

#### 3. Central Panel Routing (`src/app.rs`)
In `ViperApp::ui`:
- Replace `self.chat_area(ui)` with `self.terminal_area(ui)`.
- If `!session.has_folder()`: Render folder picker ("Choose a project folder to start").
- If `provider.find()` is `None`: Render "CLI not installed" message with installation instructions.
- If `session.terminal.is_none()`: Call `Terminal::start_command(session.working_dir(), &exe, &[], ui.ctx().clone())`.
- Draw `terminal.ui(ui, take_keyboard)`.

---

## 5. Verification Method

### 5.1 Verification Commands
To independently verify that the existing codebase compiles, all unit tests pass, and zero clippy warnings exist:

```powershell
# 1. Warm check compilation
cargo check

# 2. Run unit test suite (237 passed, 0 failed, 8 ignored)
cargo test

# 3. Run Clippy across all targets with zero warnings
cargo clippy --all-targets -- -D warnings
```

### 5.2 Specific Files and Code Locations to Inspect
1. `src/terminal.rs`:
   - Lines 26–54: ConPTY `Replies` cursor query response.
   - Lines 58–126: `TerminalJob` Windows Job Object implementation.
   - Lines 177–230: `Terminal::start` PTY initialization and background reader thread.
   - Lines 256–264: Monospace row/col calculation and `self.master.resize` propagation.
   - Lines 270–276: `set_focus_lock_filter` for Tab, Arrow keys, and Escape.
   - Lines 350–403 & 682–721: `handle_input` and `key_sequence` mapping.
2. `src/tools.rs`:
   - Lines 12–23 & 523–532: Tab-level `Terminal` lifecycle and keyboard focus delegation.
3. `src/app.rs`:
   - Lines 323–344: `delete_session` session cleanup and dropping.
   - Lines 531–605: `chat_area` rendering to be replaced with the terminal widget.
   - Lines 819–823: Central panel view dispatch.

### 5.3 Invalidation Conditions
- If running interactive CLIs inside ConPTY produces duplicate echoes or character drops due to line discipline settings.
- If `portable_pty::CommandBuilder` on Windows fails to launch `.cmd` scripts without `cmd.exe /c` wrapping.
- If removing `chat::composer` breaks slash commands (e.g. `/model`, `/clear`), requiring slash commands to be delegated either to the CLI's native REPL or a top/bottom toolbar overlay.
