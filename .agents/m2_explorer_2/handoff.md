# Investigation Report: Dynamic Resizing, Keystroke Routing, and Focus Locking for Middle Terminal

## 1. Observation

### 1.1 Font Metrics, Available Size, and Dynamic PTY Resizing in `src/terminal.rs`
- **File & Line Numbers**: `src/terminal.rs:15-16`, `src/terminal.rs:353-365`, `src/terminal.rs:616-618`.
- **Constants**:
  ```rust
  const FONT_SIZE: f32 = 13.0;
  const PADDING: f32 = 6.0;
  const BACKGROUND: Color32 = Color32::from_rgb(24, 24, 24);
  const FOREGROUND: Color32 = Color32::from_rgb(204, 204, 204);
  ```
- **Dimension Calculation & Resize Trigger** (`src/terminal.rs:353-365`):
  ```rust
  let font_id = FontId::monospace(FONT_SIZE);
  let (char_width, row_height) =
      ui.ctx().fonts_mut(|fonts| (fonts.glyph_width(&font_id, 'M'), fonts.row_height(&font_id)));

  let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
  let rows = ((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
  let cols = ((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
  if (rows, cols) != self.size {
      self.size = (rows, cols);
      let _ = self.master.resize(pty_size(self.size));
      self.parser().screen_mut().set_size(rows, cols);
  }
  ```
- **PTY Size Struct** (`src/terminal.rs:616-618`):
  ```rust
  fn pty_size((rows, cols): (u16, u16)) -> PtySize {
      PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
  }
  ```

### 1.2 Focus Locking and Input Routing in `src/terminal.rs`
- **Focus Acquisition & Lock Filter** (`src/terminal.rs:366-379`):
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
- **Cursor Rendering Based on Focus** (`src/terminal.rs:424-435`):
  ```rust
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
  ```
- **Input Handling Loop** (`src/terminal.rs:451-504`):
  - In `handle_input(&mut self, ui: &egui::Ui)`:
    - Queries terminal parser state: `(app_cursor, bracketed_paste)`.
    - Handles `egui::Event::Text(text)`: passes UTF-8 bytes verbatim via `bytes.extend_from_slice(text.as_bytes())`.
    - Handles `egui::Event::Paste(text)`: translates `\n` to `\r`; if `bracketed_paste` is active, surrounds text with `\x1b[200~` and `\x1b[201~`.
    - Handles `egui::Event::Copy`: if text is selected, copies text to clipboard; if no selection, emits `0x03` (`Ctrl+C`, interrupt).
    - Handles `egui::Event::Cut`: emits `0x18` (`Ctrl+X`).
    - Handles `egui::Event::Key { key, pressed: true, modifiers, .. }`: evaluates `key_sequence(*key, *modifiers, app_cursor)`.
    - Automatically resets scrollback to 0 and writes all accumulated bytes to `self.writer` (PTY stdin).

- **Keystroke Sequence Translation** (`src/terminal.rs:784-822`):
  ```rust
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
              let [letter] = key.name().as_bytes() else { return None };
              if !letter.is_ascii_alphabetic() {
                  return None;
              }
              vec![letter.to_ascii_uppercase() & 0x1f]
          }
          _ => return None,
      })
  }
  ```

### 1.3 Process Exit Notice and Restart Mechanism in `src/terminal.rs`
- **Exit Detection** (`src/terminal.rs:331-334`):
  ```rust
  pub fn has_exited(&mut self) -> bool {
      self.exited.load(Ordering::Relaxed) || matches!(self.child.try_wait(), Ok(Some(_)))
  }
  ```
- **Exit Notice & Restart UI** (`src/terminal.rs:344-351`):
  ```rust
  let mut restart = false;
  if self.has_exited() {
      ui.horizontal(|ui| {
          ui.label(egui::RichText::new("The process has exited.").weak());
          restart = ui.button("Restart").clicked();
      });
  }
  ```
- **Process Cleanup on Drop** (`src/terminal.rs:521-526`):
  ```rust
  impl Drop for Terminal {
      fn drop(&mut self) {
          self.job.kill();
          let _ = self.child.kill();
      }
  }
  ```

### 1.4 Application Layout and Panel Hierarchy in `src/app.rs`
- **Panel Layout Order** (`src/app.rs:814-823`):
  1. `self.notice_banner(ui)`: Top panel (`egui::Panel::top(egui::Id::new("notice_banner"))`).
  2. `self.left_panel(ui)`: Left panel (collapsed rail width `44.0`, expanded panel resizable `180.0..=420.0`, default `260.0`).
  3. `self.right_panel(ui, frame)`: Right panel (collapsed rail width `44.0`, expanded panel resizable `240.0..=1600.0`, default 20% width).
  4. `egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, ...)`: Fills remainder of screen area between top, left, and right panels.
- **Shortcut Handling in `App::logic`** (`src/app.rs:361-381`, `src/app.rs:807`):
  `handle_shortcuts` consumes `Ctrl+\`` (Terminal) and `Ctrl+Shift+B` (Browser) via `ctx.input_mut(|i| i.consume_shortcut(...))` during `logic()` *before* rendering panels.

---

## 2. Logic Chain

### 2.1 Dynamic Resizing Propagation
1. From Observation §1.4, `CentralPanel` computes its available space dynamically on every frame as:
   `available_size = window_viewport_size - left_panel_width - right_panel_width - top_banner_height`.
2. From Observation §1.1, `Terminal::ui` allocates `ui.available_size()` with `ui.allocate_exact_size`.
3. The row and column counts are derived deterministically:
   - `rows = ((rect.height() - 12.0) / row_height).floor().max(2.0) as u16`
   - `cols = ((rect.width() - 12.0) / char_width).floor().max(10.0) as u16`
4. Whenever:
   - The user resizes the main application window;
   - The left sidebar is expanded (`44.0 -> 260.0`) or collapsed (`260.0 -> 44.0`);
   - The left sidebar width is dragged along its resize handle (`180.0..=420.0`);
   - The right tools panel is opened, closed, or dragged (`240.0..=1600.0`);
   - A notice banner is displayed or dismissed at the top;
   the change immediately alters `rect.width()` and `rect.height()` on the subsequent frame.
5. When `(rows, cols) != self.size`, two synchronized resize calls occur:
   - `self.master.resize(pty_size(self.size))`: On Windows, invokes Win32 `ResizePseudoConsole` via `portable-pty`; on Unix, invokes `ioctl(TIOCSWINSZ)`. This causes the child process (Claude Code, Codex, Antigravity, or a shell) to receive a terminal resize event and recalculate its TUI line wrapping, prompt layout, and viewport bounds.
   - `self.parser().screen_mut().set_size(rows, cols)`: Updates the `vt100` virtual terminal emulator buffer dimensions, ensuring characters drawn by egui correspond 1:1 with the PTY screen grid.
6. The painter uses `PADDING = 6.0` and clips to `rect`. Because `floor()` is used, any fractional pixel space at the right and bottom boundaries is safely filled by `painter.rect_filled(rect, 0.0, BACKGROUND)` (RGB 24, 24, 24), preventing visual artifacts or text cutoff.

### 2.2 Keystroke Routing, Escape Sequences, and Focus Isolation
1. From Observation §1.2, egui's default behavior intercepts `Tab` to shift focus to the next widget, and can intercept arrows or `Escape`.
2. By invoking `ui.memory_mut(|m| m.set_focus_lock_filter(response.id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }))` when the terminal has focus, egui suppresses widget focus cycling.
3. Consequently, all critical TUI keys are delivered to `handle_input`:
   - `Enter` -> `b"\r"` (standard terminal carriage return).
   - `Backspace` -> `0x7f` (standard ASCII DEL).
   - `Tab` -> `b"\t"` (tab completion / option cycling) or `b"\x1b[Z"` (Backtab via Shift+Tab).
   - `Arrow keys` -> `\x1b[A-D]` (normal CSI), `\x1bOA-D` (application cursor mode DECCKM when active in curses/vim/TUI menus), or `\x1b[1;5A-D]` (Ctrl+Arrow word navigation).
   - `Ctrl+[A-Z]` -> `letter & 0x1f` (e.g. `Ctrl+C` sends `0x03` if no text is selected, `Ctrl+D` sends `0x04` for EOF, `Ctrl+L` sends `0x0c` to clear screen).
   - `Bracketed paste` -> wraps multi-line pastes in `\x1b[200~` and `\x1b[201~` when requested by the CLI, avoiding accidental early execution of pasted commands.
4. App-level global shortcuts (`Ctrl+\`` and `Ctrl+Shift+B` in Observation §1.4) are consumed in `App::logic` via `ctx.input_mut(|i| i.consume_shortcut(...))` prior to `ui()`, meaning the middle terminal never swallows app-level panel toggles even while focus-locked.

### 2.3 Process Exit Notice and Clean Restart Lifecycle
1. When a provider process terminates or exits (e.g., user runs `/exit`, types `exit`, or presses `Ctrl+D`), `Terminal::has_exited()` evaluates to true via either atomic flag notification or `child.try_wait()`.
2. As observed in §1.3, `Terminal::ui` inserts `ui.horizontal` rendering `The process has exited. [Restart]` at the top of the terminal area.
3. The notice consumes vertical layout space (~24px), smoothly reducing `rows` for the remaining terminal grid via standard layout flow.
4. Clicking `Restart` sets `restart = true` as the return value of `Terminal::ui`.
5. When `restart` is true:
   - Dropping the terminal instance triggers `Terminal::drop`, which calls `self.job.kill()` (terminating Windows Job Object process trees, including grandchildren dev servers or spawned tools) and `self.child.kill()`.
   - The session or app replaces the slot with a fresh `Terminal::start_command` with `take_keyboard = true`, seamlessly relaunching the interactive CLI in the session working directory with full keyboard focus.

---

## 3. Caveats

1. **Unsalted ID Collision Risk**:
   - `Terminal::ui` allocates `allocate_exact_size` based on the active egui ID stack.
   - If the central panel does not wrap the call in `ui.push_id(("session_terminal", session.id), ...)`, its widget ID could clash with secondary terminal tabs in `tools.rs` or between session switches, corrupting focus state or focus lock filters.
2. **Persistent vs One-Shot Focus**:
   - Passing `take_keyboard = true` on every frame in `CentralPanel` would cause the middle terminal to steal keyboard focus unconditionally, preventing the user from typing in the right tools panel (secondary terminal, browser search bar), renaming sessions, or modifying settings.
   - `take_keyboard` must be one-shot (e.g., `std::mem::take(&mut session.focus_terminal)` or on explicit session selection / settings close).
3. **Session Unconfigured / Missing Folder Edge Case**:
   - When a session has `!session.has_folder()`, `session.working_dir()` defaults to an empty path or root. `Terminal::start_command` rejects non-directory paths (`"directory ... is not a directory"`).
   - The central panel must guard against this by rendering a folder selection card rather than attempting to launch a PTY in an unconfigured directory.
4. **Missing CLI Executable Edge Case**:
   - If the provider CLI is not installed (`self.detected.get(session.provider).is_none()`), attempting to spawn the terminal will fail with an error label. The central panel should detect this upfront and provide a helpful install hint and a direct button to Open Settings.
5. **No Visual Screen Inspection**:
   - Per `AGENTS.md` §3.1 and `verifying-a-ui-change` skill, the GUI cannot be directly observed via screenshot. All UI layout and sizing verification relies on layout formulas, mathematical bounds, headless egui tests, and code analysis.

---

## 4. Conclusion

The existing `Terminal` implementation in `src/terminal.rs` is fully equipped for hosting in `src/app.rs`'s `CentralPanel`:
1. **Dynamic Resizing**: Seamlessly driven by `ui.available_size()`, `fonts.glyph_width`, `fonts.row_height`, and PTY/screen resizing (`master.resize` and `screen_mut().set_size`). It automatically accommodates main window resizing, sidebar collapse/expand, and tools panel width adjustments.
2. **Keystroke & Focus Routing**: Fully handles ANSI/VT100 escape sequences, Enter (`\r`), Backspace (`0x7f`), Tab / Shift+Tab (`\x1b[Z`), arrow keys with application cursor and Ctrl modifiers, Ctrl+[A-Z], bracketed paste, and focus locking (`set_focus_lock_filter`).
3. **Exit & Restart**: Detects process termination and renders `The process has exited. [Restart]`. Clicking restart returns `true` and cleanly teardowns processes via `TerminalJob` before reinitializing.
4. **Integration Blueprint for `terminal_area` in `src/app.rs`**:
   - Salt the UI ID with `("session_terminal", session.id)`.
   - Consume `take_keyboard` as a one-shot flag upon session switch / selection.
   - Render folder prompt if `!session.has_folder()`.
   - Render install guidance if provider CLI is missing.
   - Call `terminal.ui(ui, take_keyboard)` and handle `restart` by resetting the session terminal slot.

---

## 5. Verification Method

### 5.1 Independent Commands
Run the following standard repo verification commands:
```powershell
# 1. Warm type check
cargo check

# 2. Complete non-ignored test suite
cargo test

# 3. Clippy with zero warnings invariant
cargo clippy --all-targets -- -D warnings
```

### 5.2 Specific Unit Tests Covering Resizing & Keystroke Logic
- `terminal::tests::special_keys_map_to_escape_sequences`: Validates Enter, ArrowUp/Down/Left/Right, application cursor mode, Ctrl+Arrow, Shift+Tab Backtab, and Ctrl+D mapping.
- `terminal::tests::answers_cursor_position_and_status_requests`: Validates VT100 replies callbacks (e.g. cursor position report `\x1b[6n` responding `\x1b[row;colR`).
- `terminal::tests::rows_group_cells_with_the_same_style`: Validates layout job formatting and text styling across rows.
- `terminal::tests::terminal_start_command_initializes_dimensions_and_cleans_up`: Validates PTY startup dimensions (24x80) and drop cleanup.
- `terminal::tests::stress_terminal_cleanly_kills_child_and_grandchild_process_tree`: Validates `TerminalJob` process tree termination.

### 5.3 Invalidation Conditions
- Any changes to `FONT_SIZE` or `PADDING` without recalculating row/column boundaries.
- Failing to salt widget IDs in `terminal_area`, which causes focus lock filter collision between concurrent terminal widgets.
- Passing `take_keyboard = true` on every frame, which invalidates keyboard input in secondary panels.
