# Handoff Report: Milestone 2 Unit Test Suite Design

## 1. Observation

### 1.1 Existing Test Conventions and Invariants in `AGENTS.md`
- **File & Section**: `AGENTS.md:§2`, `AGENTS.md:§3`, `AGENTS.md:§5.4`.
- **Test Style Rules**:
  - Test names must be full sentences describing behaviour: e.g. `fn sessions_are_grouped_by_their_folder()`.
  - Slice pattern destructuring with prose panic:
    ```rust
    let [AgentEvent::ToolUse { edit: Some(edit), .. }] = &parse_line(line)[..] else {
        panic!("an edit should come through")
    };
    ```
  - Prose assertions that explain expectations and dump values on failure:
    ```rust
    assert_eq!(alpha_ids, [3, 1], "newest first inside a workspace");
    ```
  - Comments explain scenarios, never mechanical Rust syntax.
  - Zero clippy warnings (`cargo clippy --all-targets -- -D warnings`), 0 test failures, no new dependencies (`AGENTS.md:§3.4`), and no unrequested reformatting (`AGENTS.md:§3.3`).

### 1.2 Verification Rules from `.agents/skills/verifying-a-ui-change/SKILL.md`
- "Never drive the global mouse or keyboard."
- "Never take a full-screen capture."
- "Never launch the app expecting to observe it."
- "Instead: move the logic out of the rendering. Anything worth verifying should be a plain function that takes data and returns data, with the `egui` call reduced to drawing the result. Those functions get real tests."
- Guard against four invisible bugs:
  1. *Unsalted widget IDs* (per-session views must salt with `session.id`).
  2. *State that outlives the frame mutated from a panel*.
  3. *Background work with no repaint*.
  4. *Unbounded text in a fixed row*.

### 1.3 Baseline Code Analysis in `src/app.rs` and `src/terminal.rs`
- **`src/app.rs:24-27`**:
  ```rust
  enum View {
      Chat,
      Settings,
  }
  ```
- **`src/app.rs:531-605`**: `chat_area` previously hosted `egui::Panel::bottom(egui::Id::new("composer_panel"))` and `chat::conversation(ui, session, settings, markdown)`.
- **`src/app.rs:819-822`**:
  ```rust
  egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
      View::Chat => self.chat_area(ui),
      View::Settings => self.settings_area(ui),
  });
  ```
- **`src/app.rs:417-421`**:
  ```rust
  SidebarAction::Select(id) => {
      self.view = View::Chat;
      self.state.active_session = id;
      self.active_session_mut().focus_composer = true;
  }
  ```
- **`src/session.rs:135-136`**:
  ```rust
  #[serde(skip)]
  pub focus_composer: bool,
  ```
  `focus_composer` is initialized to `true` upon `Session::new` and reset to `true` whenever a session is selected or created.
- **`src/terminal.rs:353-365`**:
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
- **`src/terminal.rs:366-377`**:
  ```rust
  if response.clicked() || take_keyboard {
      response.request_focus();
  }
  let focused = response.has_focus();
  if focused {
      ui.memory_mut(|m| {
          m.set_focus_lock_filter(
              response.id,
              egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
          );
      });
      self.handle_input(ui);
  }
  ```

---

## 2. Logic Chain

### 2.1 Decoupling Logic from Rendering
1. Following the playbook in `verifying-a-ui-change`:
   - Central view routing decision logic is extracted into a pure, testable function: `ViperApp::central_state(&self) -> CentralState`.
     ```rust
     #[derive(Clone, Copy, Debug, PartialEq, Eq)]
     pub(crate) enum CentralState {
         NeedsFolder,
         MissingCli(Provider),
         TerminalReady,
     }
     ```
   - Terminal grid sizing is extracted into a pure math function: `terminal::calculate_dimensions(size: Vec2, char_width: f32, row_height: f32) -> (u16, u16)`.
2. This separation allows 100% deterministic unit testing without visual inspection, platform dependencies, or live paid CLI execution.

### 2.2 Central View Replaces Chat Composer
1. In `app.rs`, `View::Chat` is replaced by `View::Terminal` (or aliased), and `CentralPanel` routes to `self.terminal_area(ui)`.
2. Headless testing with `egui::Context::default()` executes `app.terminal_area(ui)`.
3. The test asserts that `composer_panel` is never drawn or focused in egui memory, confirming that `chat.rs` composer elements are completely absent from the middle panel.

### 2.3 Unconfigured Session & Missing CLI Invariants
1. When `!session.has_folder()`, `app.central_state()` evaluates to `CentralState::NeedsFolder`. `terminal_area` draws folder selection guidance and returns early. The test verifies `app.provider_terminals` remains empty (0 terminal processes spawned).
2. When `session.has_folder()` but `detected.get(provider).is_none()`, `app.central_state()` evaluates to `CentralState::MissingCli(provider)`. `terminal_area` draws the install warning and returns early without panicking. The test verifies `app.provider_terminals` remains empty.

### 2.4 Terminal Dynamic Resizing Invariants
1. For standard allocated dimensions (`800.0 x 600.0`) with `char_width = 8.0` and `row_height = 16.0`:
   - `usable_width = 800.0 - 12.0 = 788.0` -> `cols = floor(788 / 8) = 98`.
   - `usable_height = 600.0 - 12.0 = 588.0` -> `rows = floor(588 / 16) = 36`.
2. Resizing wider (window expansion, e.g. `1200.0 x 600.0`) increases columns to 148.
3. Resizing narrower (sidebar expansion, e.g. `500.0 x 600.0`) decreases columns to 61.
4. Tiny or zero allocated sizes (`Vec2::ZERO`, `Vec2::new(5.0, 5.0)`) clamp safely to `(2, 10)` without integer underflow.

### 2.5 Keyboard Focus Transfer Invariants
1. Creating or switching to a session sets `focus_composer = true` on that session.
2. In `terminal_area`, `take_keyboard = std::mem::take(&mut session.focus_composer)` consumes the flag.
3. `Terminal::ui` receives `take_keyboard = true`, calls `response.request_focus()`, and registers the focus lock filter.
4. On subsequent frames, `focus_composer` is `false`, ensuring focus is not stolen continuously if the user clicks other panels.

---

## 3. Caveats

1. **Read-Only Explorer Scope**:
   - As an explorer, no production source code has been edited. The test suite and helpers are delivered in `test_suite_proposal.rs` for `worker_m2`.
2. **Headless egui Context Limitations**:
   - In headless unit tests (`egui::Context::default()`), `fonts.glyph_width` may return default fallback metrics. The pure function `calculate_dimensions` isolates font metric verification from egui font rasterization.
3. **PTY Process Lifetime in Tests**:
   - Tests that launch real child processes in PTYs (e.g. `terminal_start_command_resizes_underlying_pty_and_parser_screen` and `terminal_area_spawns_process_when_session_and_cli_are_ready`) use built-in shell exits (`cmd.exe /c exit 0` or `sh -c exit 0`), ensuring zero process leaks and zero cost.

---

## 4. Conclusion

The unit test suite for Milestone 2 provides full coverage across all five target requirements with strict adherence to `AGENTS.md` and `verifying-a-ui-change`:
1. `central_view_renders_terminal_area_without_chat_composer`: Covers central panel routing without chat composer.
2. `unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal`: Covers empty state folder prompt and zero-spawn invariant.
3. `session_with_missing_executable_displays_warning_without_panicking`: Covers missing CLI warning without panic.
4. `terminal_dimensions_are_calculated_from_allocated_rect_and_font_metrics`: Covers mathematical rows/columns calculation across window and sidebar sizing.
5. `terminal_resizing_clamps_to_minimum_rows_and_columns_on_tiny_or_zero_rect`: Covers zero/tiny rect clamping bounds.
6. `terminal_start_command_resizes_underlying_pty_and_parser_screen`: Covers PTY master and screen resize synchronization.
7. `switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it`: Covers slice pattern session switching and one-shot focus handover.
8. `new_session_initializes_with_keyboard_focus_requested`: Covers new session focus priming.
9. `terminal_area_spawns_process_when_session_and_cli_are_ready`: Covers end-to-end PTY spawning on ready session.

The complete ready-to-paste Rust code is stored in `.agents/m2_explorer_3/test_suite_proposal.rs` and documented below.

---

## 5. Verification Method

### 5.1 Independent Commands
```powershell
# 1. Warm check
cargo check

# 2. Run the non-ignored test suite (should pass all existing 266 + 9 new tests = 275 tests)
cargo test

# 3. Clippy invariant check
cargo clippy --all-targets -- -D warnings
```

### 5.2 Specific Test Names to Run
```powershell
cargo test central_view_renders_terminal_area_without_chat_composer
cargo test unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal
cargo test session_with_missing_executable_displays_warning_without_panicking
cargo test switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it
cargo test new_session_initializes_with_keyboard_focus_requested
cargo test terminal_area_spawns_process_when_session_and_cli_are_ready
cargo test terminal_dimensions_are_calculated_from_allocated_rect_and_font_metrics
cargo test terminal_resizing_clamps_to_minimum_rows_and_columns_on_tiny_or_zero_rect
cargo test terminal_start_command_resizes_underlying_pty_and_parser_screen
```

### 5.3 Invalidation Conditions
- Any test failing to clean up child processes or panicking on uninstalled CLI configurations.
- Any regression breaking RON deserialization of existing sessions (`SavedState`).
- Any clippy warning generated by new test functions or helpers.
