# Milestone 2 Implementation Handoff: Middle Panel Interactive Terminal Area

## 1. Observation
1. **Target Files & Ownership**:
   - `src/chat.rs`: Added `#![allow(dead_code)] // Preserved for conversation tests and transition.` at line 3 immediately after module docstring. This allows historic conversation tests and data models to compile cleanly without dead-code warnings.
   - `src/app.rs`:
     - Pruned unused imports (`use crate::chat::{self, ComposerAction};` and `ApprovalDecision`).
     - Added `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>` to `struct ViperApp` (lines 141–143).
     - Initialized `provider_terminals: std::collections::BTreeMap::new()` in `ViperApp::new` (line 229) and `ViperApp::test_app` (line 261).
     - Wired terminal process teardown in `delete_session` (line 345: `self.provider_terminals.remove(&id);`) and `change_folder` (line 425: `self.provider_terminals.remove(&id);`).
     - Defined `TerminalState` enum (lines 589–607) and pure state resolution function `resolve_terminal_state` (lines 610–634) which consumes `session.focus_composer` via `std::mem::take` when transitioning to `Ready`.
     - Implemented `terminal_area` (lines 637–744) replacing `chat_area`:
       - `TerminalState::NeedsFolder`: Renders centered empty-state card ("Choose a project folder to start") with action button calling `self.change_folder()`.
       - `TerminalState::MissingExecutable`: Renders install guidance banner with button setting `self.view = View::Settings`.
       - `TerminalState::Ready`: Retrieves or starts `Terminal::start_command` in `self.provider_terminals`, salts egui ID with `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)`, executes `terminal.ui(ui, take_keyboard)`, and purges exited terminals when user clicks Restart.
     - Preserved `self.left_panel(ui)` and `self.right_panel(ui, frame)` completely intact in `ViperApp::ui` (lines 948–955), dispatching `View::Chat => self.terminal_area(ui)`.
     - Preserved transitional agent hooks for interactive turn approvals and preview loading without modifying external modules.
     - Added 10 unit tests in `src/app.rs::tests` covering state resolution, focus consumption, empty-folder prompts, missing CLI warnings, PTY process spawning on ready sessions, and session deletion terminal cleanup.
2. **Commands Executed & Verified Output**:
   - `cargo check`: Exited with code 0 (0 errors, 0 warnings).
   - `cargo test`: Exited with code 0 (276 passed; 0 failed; 8 ignored; finished in 3.31s).
   - `cargo clippy --all-targets -- -D warnings`: Exited with code 0 (0 warnings, zero additions to Cargo.toml).

## 2. Logic Chain
1. **Separation of Terminal State Logic from Rendering**:
   - Following `verifying-a-ui-change`, `resolve_terminal_state` extracts all state determination rules into a pure, side-effect-free function:
     - If `!session.has_folder()`, evaluates to `TerminalState::NeedsFolder`.
     - Else if `detected.get(session.provider).is_none()`, evaluates to `TerminalState::MissingExecutable`.
     - Else, transfers `session.focus_composer` into `take_keyboard` and evaluates to `TerminalState::Ready`.
   - This makes terminal activation 100% testable in headless unit tests without needing window handles or live CLI tools.
2. **Interactive TUI Execution & Dynamic Resizing**:
   - In `TerminalState::Ready`, `Terminal::start_command` runs the interactive provider command built by `crate::agent::build_interactive_command` inside a PTY with `TERM=xterm-256color` and `COLORTERM=truecolor`.
   - `terminal.ui(ui, take_keyboard)` allocates `ui.available_size()`, computes row and column character metrics, and propagates dimension changes via `master.resize(pty_size(self.size))` and `parser.screen_mut().set_size(rows, cols)` on main window resize or sidebar collapse/expansion.
   - When focused, it sets `set_focus_lock_filter` to retain Tab, arrows, and Escape within the terminal emulator.
3. **Session Switching and Lifecycle Cleanup**:
   - Sidebar session switching sets `focus_composer = true` on the selected session. When `terminal_area` draws that session, `resolve_terminal_state` consumes the flag via `std::mem::take`, delivering `take_keyboard = true` on the very first frame to instantly focus the terminal emulator. Subsequent frames pass `take_keyboard = false`, allowing focus to move to the tools panel or sidebar if clicked.
   - Deleting a session or changing folders removes the session's entry from `self.provider_terminals`, dropping `Terminal` which invokes `job.kill()` and `child.kill()`, cleanly terminating the underlying CLI process group without orphan processes.

## 3. Caveats
- **Visual Inspection**: In accordance with `AGENTS.md` §3.1 and `verifying-a-ui-change` SKILL.md, no screenshots or visual GUI inspections were performed. Visual behavior is verified through pure logic state testing, headless egui UI passes (`run_ui_test`), and architectural analysis.
- **Process Spawning in Tests**: Tests that spawn child processes use system utilities (`cmd.exe` on Windows, `/bin/sh` on Unix) to verify PTY startup and shutdown without incurring paid API costs or reaching outside local resources.
- No other caveats.

## 4. Conclusion
Milestone 2 is fully implemented and passes all verification criteria:
- The middle panel replaces the chat bubbles and composer with an embedded PTY terminal running the selected AI CLI.
- Resizing sidebars or windows dynamically adjusts terminal columns and rows.
- Switching sessions retains distinct terminal buffers and immediately hands over keyboard focus.
- Session deletion tears down the associated terminal process.
- Zero clippy warnings, zero compilation errors, and all 276 tests pass.

### User-Facing UI Verification Description
> The middle area of the application now renders an embedded terminal hosting the active AI CLI instead of the markdown chat transcript and bottom message box.
> - When a new session has no folder chosen yet, you will see a centered prompt: **"Choose a project folder to start"** with a **"Choose Folder…"** button.
> - If the selected provider CLI (Claude, Codex, or Antigravity) is not installed on your machine, you will see a banner across the top indicating it isn't installed with an **"Open Settings"** button.
> - Once configured, the middle panel launches the provider CLI directly in an embedded terminal with full ANSI colors, cursor positioning, and keyboard focus immediately active. Resizing the window or toggling the left/right panels smoothly wraps and resizes the terminal text.
> *(I did not visually observe this window directly; all verification was completed via headless egui passes, 276 passed unit tests, and cargo clippy at zero warnings. Please confirm the terminal layout and interactive prompt appear as expected on your screen.)*

## 5. Verification Method
Execute the following commands from the repository root `c:\Users\ditob\Documents\viper`:
```powershell
# 1. Warm check (must produce 0 errors, 0 warnings)
cargo check

# 2. Run the test suite (all 276 tests pass, 8 ignored)
cargo test

# 3. Clippy invariant (must produce 0 warnings)
cargo clippy --all-targets -- -D warnings
```
To run the newly added Milestone 2 tests specifically:
```powershell
cargo test app::tests::session_without_folder_resolves_to_needs_folder_state
cargo test app::tests::session_with_missing_cli_resolves_to_missing_executable_state
cargo test app::tests::ready_session_resolves_to_ready_terminal_state_and_consumes_focus
cargo test app::tests::deleting_session_cleans_up_provider_terminal_map
cargo test app::tests::central_view_renders_terminal_area_without_chat_composer
cargo test app::tests::unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal
cargo test app::tests::session_with_missing_executable_displays_warning_without_panicking
cargo test app::tests::switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it
cargo test app::tests::new_session_initializes_with_keyboard_focus_requested
cargo test app::tests::terminal_area_spawns_process_when_session_and_cli_are_ready
```
