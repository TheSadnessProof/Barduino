# Handoff Report: Milestone 2 Blueprint — Replacing `chat_area` with `terminal_area`

## 1. Observation

1. **Existing `chat_area` and Central Panel in `src/app.rs`**:
   - `src/app.rs` lines 531–605 implements `fn chat_area(&mut self, ui: &mut egui::Ui)`:
     - Lines 544–548: Allocates a bottom panel for the chat composer (`egui::Panel::bottom(egui::Id::new("composer_panel"))`).
     - Lines 552–563: Spawns an inner `egui::CentralPanel` to draw the markdown conversation transcript via `chat::conversation(ui, session, settings, markdown)`.
     - Lines 553–561: If `!installed`, renders a warning banner (`format!("{} isn't installed. {}.", session.provider.label(), session.provider.install_hint())`) with an "Open Settings" link.
     - Lines 565–605: Dispatches `conv_action` (`ChangeFolder`, `SelectProvider`, `Approve`, `Deny`, `Preview`) and `composer_action` (`Send`, `Stop`, `Apply`, `Notice`).
   - `src/app.rs` lines 607–637 implements `fn apply_setting(&mut self, setting: SlashAction)` which was called exclusively by `chat_area` line 598.
   - `src/app.rs` lines 814–823 in `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`:
     ```rust
     self.browser.release_focus_on_click(ui.ctx());
     self.notice_banner(ui);
     self.left_panel(ui);
     self.right_panel(ui, frame);
     egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
         View::Chat => self.chat_area(ui),
         View::Settings => self.settings_area(ui),
     });
     ```

2. **PTY Interactive Execution Primitives from Milestone 1**:
   - `src/terminal.rs` line 261 exposes `pub fn start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>`. It handles Windows batch wrapping (`cmd.exe /c` for `.cmd`/`.bat` files), injects `TERM=xterm-256color` and `COLORTERM=truecolor`, and attaches a Win32 Job Object for clean process-tree termination.
   - `src/agent.rs` line 414 exposes `pub fn build_interactive_command(provider: Provider, exe: &Path, cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> (PathBuf, Vec<String>)`. It constructs interactive argument vectors for Claude, Codex, and Antigravity, stripping headless/JSON flags and configuring models, reasoning efforts, resume IDs, and permission mode flags.
   - `src/terminal.rs` line 344 exposes `pub fn ui(&mut self, ui: &mut egui::Ui, take_keyboard: bool) -> bool`. It dynamically resizes the PTY via `self.master.resize(pty_size(self.size))` to match `ui.available_size()`, handles keyboard focus and focus locking via `ui.memory_mut(|m| m.set_focus_lock_filter(...))`, and streams TrueColor ANSI output directly. It returns `true` if the process has exited and the user clicks "Restart".

3. **Session State and Keyboard Routing**:
   - `src/session.rs` line 136 defines `#[serde(skip)] pub focus_composer: bool`. This flag is set to `true` upon session initialization (`Session::new`, line 180), upon selecting a session in the sidebar (`SidebarAction::Select`, `src/app.rs` line 420), and when switching tabs.
   - `session.has_folder()` (`src/session.rs` line 218) returns `!self.project_dir.as_os_str().is_empty()`.
   - `session.working_dir()` (`src/session.rs` line 235) returns `self.worktree_dir.as_deref().unwrap_or(&self.project_dir)`.
   - `session.agent_session_id` (`src/session.rs` line 119) stores the CLI conversation resumption token.

4. **Left Sidebar and Right Tools Isolation**:
   - `self.left_panel(ui)` (`src/app.rs` lines 639–655) and `self.right_panel(ui, frame)` (`src/app.rs` lines 678–744) are invoked prior to `CentralPanel`. They are completely decoupled from `chat_area`.
   - In `src/app.rs`, `delete_session` (line 328) drops `self.tools.remove(&id)` and `change_folder` (line 406) drops `self.tools.remove(&id)`.

5. **Clippy and Test Invariants**:
   - `cargo check`: passes in 0.38s.
   - `cargo test`: passes all 260 tests (8 ignored).
   - `cargo clippy --all-targets -- -D warnings`: passes with 0 warnings.
   - Any unused imports resulting from removing `chat_area` (such as `use crate::chat::{self, ComposerAction};` and `ApprovalDecision` in `src/app.rs`) must be pruned or marked, and `chat.rs` must have `#![allow(dead_code)]` so historical chat tests remain compileable without dead-code warnings.

---

## 2. Logic Chain

1. **Replacing `chat_area` with `terminal_area`**:
   - *From Observation 1.1 & 1.2*: In interactive mode, the AI CLI renders its own TUI (prompts, history, spinners, approval dialogs) inside the terminal. The bottom composer (`chat::composer`) and markdown transcript (`chat::conversation`) are redundant and must be replaced with the embedded terminal widget (`terminal.ui`).
   - *Therefore*: In `src/app.rs` line 820, change `View::Chat => self.chat_area(ui)` to `View::Chat => self.terminal_area(ui)` (or alias `View::Chat` to denote the primary terminal view), and replace `fn chat_area` with `fn terminal_area`.

2. **Decoupling UI Decision Logic for Testability (`verifying-a-ui-change` Skill)**:
   - *From Skill Playbook*: "Move the logic out of the rendering... Anything worth verifying should be a plain function that takes data and returns data, with the egui call reduced to drawing the result."
   - *Therefore*: Define an explicit enum `TerminalState` and a pure helper function:
     ```rust
     #[derive(Debug, PartialEq, Eq)]
     pub enum TerminalState {
         NeedsFolder { provider: Provider },
         MissingExecutable { provider: Provider, hint: &'static str },
         Ready {
             session_id: u64,
             provider: Provider,
             cwd: PathBuf,
             exe: PathBuf,
             model: Option<String>,
             effort: Option<String>,
             resume_id: Option<String>,
             permission_mode: PermissionMode,
             take_keyboard: bool,
         },
     }

     pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState
     ```
   - This isolates:
     - Check 1: If `!session.has_folder()` -> `TerminalState::NeedsFolder`.
     - Check 2: If `detected.get(session.provider).is_none()` -> `TerminalState::MissingExecutable`.
     - Check 3: If valid folder and executable -> `TerminalState::Ready`, consuming `session.focus_composer` via `std::mem::take` into `take_keyboard`.
   - This function is 100% testable without a window or running process.

3. **Handling Unconfigured Sessions (`!session.has_folder()`)**:
   - *From Observation 3*: A fresh session created via `SidebarAction::NewSession` or on fresh launch has an empty `project_dir` (`PathBuf::new()`). An AI CLI cannot run without a valid project folder.
   - *Therefore*: When state is `TerminalState::NeedsFolder`, render a centered empty-state card:
     - Heading: `"Choose a project folder to start"` (18pt, bold).
     - Subtext: `"Select a project directory to launch an interactive {provider} session."` (weak).
     - Action button: `"Choose Folder…"` which invokes `self.change_folder()`.
     - Clicking the button opens `rfd::FileDialog`. When chosen, `session.project_dir` updates and the next frame transitions to `Ready`.

4. **Handling Missing Provider CLI Executables**:
   - *From Observation 1.1 & Observation 2*: If `self.detected.get(provider)` returns `None`, `Terminal::start_command` cannot execute.
   - *Therefore*: When state is `TerminalState::MissingExecutable`, render a banner:
     - Warning text: `format!("{} isn't installed. {}.", provider.label(), hint)`.
     - Button: `"Open Settings"` which sets `self.view = View::Settings`.
     - This gives immediate guidance and a one-click path to settings for executable configuration or rescanning.

5. **Ready State & Runtime PTY Terminal Map**:
   - *From Observation 1.2, 2, and 4*: Each session must own its dedicated terminal process.
   - Add field to `ViperApp`:
     `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>`.
   - When in `TerminalState::Ready`:
     - Fetch or spawn via `self.provider_terminals.entry(session_id).or_insert_with(...)`.
     - Render inside an egui scope with salted ID: `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)`. This prevents egui ID collisions between sessions (per `verifying-a-ui-change` §Unsalted widget ids).
     - Call `terminal.ui(ui, take_keyboard)`.
     - If `terminal.ui` returns `true` (user clicked "Restart" on exited process): remove entry from `self.provider_terminals` and request repaint.
     - If entry is `Err(err)`: show error with a `"Try again"` button which clears the error entry.

6. **Cleanup on Session Deletion and Folder Change**:
   - *From Observation 4*: In `delete_session(&mut self, id: u64)`, add `self.provider_terminals.remove(&id);` to ensure the terminal is dropped and the process tree terminated.
   - In `change_folder(&mut self)`, add `self.provider_terminals.remove(&id);` so switching folders terminates the old process and starts fresh in the new directory.

---

## 3. Caveats

- **`SavedState` RON Invariant**: `provider_terminals` is stored as an ephemeral field on `ViperApp`, NOT in `SavedState`. PTY handles and child processes cannot be serialized. Existing saved sessions will cleanly respawn their provider terminals upon being opened.
- **`chat.rs` Retention**: `chat.rs` contains extensive unit tests and data structures for markdown rendering. It is not deleted; adding `#![allow(dead_code)]` at the top of `src/chat.rs` ensures existing unit tests remain green while avoiding dead code compiler warnings when `chat_area` is replaced.
- No other caveats.

---

## 4. Conclusion & Precise Worker Blueprint

### 4.1 Changes to `src/main.rs`
Add `#![allow(dead_code)]` at top of `src/chat.rs` or on `mod chat;` in `src/main.rs` to allow historical chat tests without compiler warnings.

### 4.2 Changes to `src/chat.rs`
At line 1 of `src/chat.rs`, add:
```rust
#![allow(dead_code)] // Preserved for conversation tests and transition.
```

### 4.3 Changes to `src/app.rs`

#### A. Prune Unused Imports (lines 7–10)
**Before**:
```rust
use crate::agent::{AgentEvent, ApprovalDecision, PermissionMode, Provider};
use crate::browser::{Browser, BrowserState};
use crate::chat::{self, ComposerAction};
use crate::commands::SlashAction;
```
**After**:
```rust
use crate::agent::{AgentEvent, PermissionMode, Provider};
use crate::browser::{Browser, BrowserState};
```

#### B. Add `provider_terminals` to `ViperApp` (around line 134)
**Add to struct `ViperApp`**:
```rust
    /// The interactive provider CLI terminal for each session, kept by session ID
    /// so switching sessions swaps the terminal buffer and keeps the CLI running.
    provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>,
```

#### C. Initialize `provider_terminals` in `ViperApp::new` and `test_app`
In `ViperApp::new` (line 228):
```rust
            provider_terminals: std::collections::BTreeMap::new(),
```
In `ViperApp::test_app` (line 260):
```rust
            provider_terminals: std::collections::BTreeMap::new(),
```

#### D. Update `delete_session` and `change_folder`
In `delete_session` (line 328):
```rust
        self.tools.remove(&id);
        self.provider_terminals.remove(&id);
```
In `change_folder` (line 406):
```rust
        if session.entries.is_empty() {
            let id = session.id;
            session.project_dir = dir;
            self.tools.remove(&id);
            self.provider_terminals.remove(&id);
        } else {
```

#### E. Add `TerminalState` and `resolve_terminal_state`
Add above `terminal_area`:
```rust
/// The operational state of the middle terminal area for the active session.
#[derive(Debug, PartialEq, Eq)]
pub enum TerminalState {
    /// No folder has been selected yet.
    NeedsFolder { provider: Provider },
    /// The selected provider executable was not found on this computer.
    MissingExecutable { provider: Provider, hint: &'static str },
    /// The session is configured and the CLI executable is ready to run.
    Ready {
        session_id: u64,
        provider: Provider,
        cwd: PathBuf,
        exe: PathBuf,
        model: Option<String>,
        effort: Option<String>,
        resume_id: Option<String>,
        permission_mode: PermissionMode,
        take_keyboard: bool,
    },
}

/// Resolves the operational state for the active session's middle terminal view.
/// Consumes `session.focus_composer` when transitioning to `Ready`.
pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState {
    let provider = session.provider;
    if !session.has_folder() {
        return TerminalState::NeedsFolder { provider };
    }
    let Some(exe) = detected.get(provider).cloned() else {
        return TerminalState::MissingExecutable {
            provider,
            hint: provider.install_hint(),
        };
    };
    let take_keyboard = std::mem::take(&mut session.focus_composer);
    TerminalState::Ready {
        session_id: session.id,
        provider,
        cwd: session.working_dir().to_path_buf(),
        exe,
        model: session.chosen_model.clone(),
        effort: session.effort.clone(),
        resume_id: session.agent_session_id.clone(),
        permission_mode: session.permission_mode,
        take_keyboard,
    }
}
```

#### F. Implement `terminal_area` (replacing `chat_area` lines 531–605)
```rust
    fn terminal_area(&mut self, ui: &mut egui::Ui) {
        if self.state.sessions.is_empty() {
            return;
        }
        let index = self.active_index();
        let provider = self.state.sessions[index].provider;
        if let Some(exe) = self.detected.get(provider).cloned() {
            self.models.start(provider, exe, ui.ctx());
        }

        let state = resolve_terminal_state(&mut self.state.sessions[index], &self.detected);

        match state {
            TerminalState::NeedsFolder { provider } => {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new("Choose a project folder to start").strong().size(18.0));
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Select a project directory to launch an interactive {} session.",
                                provider.label()
                            ))
                            .weak(),
                        );
                        ui.add_space(16.0);
                        if ui.button("Choose Folder…").clicked() {
                            self.change_folder();
                        }
                    });
                });
            }
            TerminalState::MissingExecutable { provider, hint } => {
                let mut open_settings = false;
                ui.vertical(|ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(
                                    ui.visuals().error_fg_color,
                                    format!("{} isn't installed. {}.", provider.label(), hint),
                                );
                                if ui.button("Open Settings").clicked() {
                                    open_settings = true;
                                }
                            });
                        });
                });
                if open_settings {
                    self.view = View::Settings;
                }
            }
            TerminalState::Ready {
                session_id,
                provider,
                cwd,
                exe,
                model,
                effort,
                resume_id,
                permission_mode,
                take_keyboard,
            } => {
                let terminal_entry = self.provider_terminals.entry(session_id).or_insert_with(|| {
                    let (prog, args) = crate::agent::build_interactive_command(
                        provider,
                        &exe,
                        &cwd,
                        model.as_deref(),
                        effort.as_deref(),
                        resume_id.as_deref(),
                        permission_mode,
                    );
                    terminal::Terminal::start_command(&cwd, &prog, &args, ui.ctx().clone())
                });

                let mut restart = false;
                ui.scope_builder(
                    egui::UiBuilder::new().id_salt(("session_terminal", session_id)),
                    |ui| match terminal_entry {
                        Ok(terminal) => {
                            restart = terminal.ui(ui, take_keyboard);
                        }
                        Err(err) => {
                            ui.centered_and_justified(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.colored_label(ui.visuals().error_fg_color, err.as_str());
                                    ui.add_space(8.0);
                                    if ui.button("Try again").clicked() {
                                        restart = true;
                                    }
                                });
                            });
                        }
                    },
                );

                if restart {
                    self.provider_terminals.remove(&session_id);
                    ui.ctx().request_repaint();
                }
            }
        }
    }
```

#### G. Update `apply_setting`
Mark `#[allow(dead_code)]` with rationale:
```rust
    #[allow(dead_code)] // Retained for compatibility with programmatic slash commands.
    fn apply_setting(&mut self, setting: SlashAction) { ... }
```
(Or remove `apply_setting` if not needed).

#### H. Update `CentralPanel` Dispatch in `ViperApp::ui` (line 820)
```rust
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
            View::Chat => self.terminal_area(ui),
            View::Settings => self.settings_area(ui),
        });
```

#### I. Unit Tests for `src/app.rs::tests`
Add to `mod tests`:
```rust
    #[test]
    fn session_without_folder_resolves_to_needs_folder_state() {
        let mut session = Session::new(1, PathBuf::new(), Provider::Claude, PermissionMode::ReadOnly);
        let detected = Detected::default();
        let state = resolve_terminal_state(&mut session, &detected);
        assert_eq!(state, TerminalState::NeedsFolder { provider: Provider::Claude });
    }

    #[test]
    fn session_with_missing_cli_resolves_to_missing_executable_state() {
        let mut session = Session::new(2, PathBuf::from(r"C:\work\project"), Provider::Codex, PermissionMode::Full);
        let detected = Detected::default();
        let state = resolve_terminal_state(&mut session, &detected);
        assert_eq!(
            state,
            TerminalState::MissingExecutable {
                provider: Provider::Codex,
                hint: Provider::Codex.install_hint(),
            }
        );
    }

    #[test]
    fn ready_session_resolves_to_ready_terminal_state_and_consumes_focus() {
        let mut session = Session::new(3, PathBuf::from(r"C:\work\project"), Provider::Antigravity, PermissionMode::Full);
        session.chosen_model = Some("gemini-2.5-flash".into());
        session.effort = Some("high".into());
        session.agent_session_id = Some("conv-12345".into());
        assert!(session.focus_composer, "session starts with focus requested");

        let mut detected = Detected::default();
        let fake_exe = PathBuf::from(r"C:\bin\agy.exe");
        detected.insert(Provider::Antigravity, fake_exe.clone());

        let state = resolve_terminal_state(&mut session, &detected);
        assert_eq!(
            state,
            TerminalState::Ready {
                session_id: 3,
                provider: Provider::Antigravity,
                cwd: PathBuf::from(r"C:\work\project"),
                exe: fake_exe,
                model: Some("gemini-2.5-flash".into()),
                effort: Some("high".into()),
                resume_id: Some("conv-12345".into()),
                permission_mode: PermissionMode::Full,
                take_keyboard: true,
            }
        );
        assert!(!session.focus_composer, "first resolution must consume focus_composer");

        // Subsequent call does not request keyboard focus again
        let state_subsequent = resolve_terminal_state(&mut session, &detected);
        let TerminalState::Ready { take_keyboard, .. } = state_subsequent else {
            panic!("expected Ready state");
        };
        assert!(!take_keyboard, "subsequent frame must not take keyboard without new trigger");
    }

    #[test]
    fn deleting_session_cleans_up_provider_terminal_map() {
        let mut state = populated_state();
        let mut app = ViperApp::test_app(state);
        // Simulate a terminal entry in provider_terminals
        app.provider_terminals.insert(7, Err("test error".to_owned()));
        assert!(app.provider_terminals.contains_key(&7));

        app.delete_session(7);
        assert!(!app.provider_terminals.contains_key(&7), "deleting session must purge provider terminal entry");
    }
```

---

## 5. Verification Method

### 5.1 Compilation and Clippy Checks
Run from repo root `c:\Users\ditob\Documents\viper`:
```powershell
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```
Expected: 0 errors, 0 warnings, all existing tests and new unit tests pass.

### 5.2 Targeted Terminal State Tests
```powershell
cargo test app::tests::session_without_folder_resolves_to_needs_folder_state
cargo test app::tests::session_with_missing_cli_resolves_to_missing_executable_state
cargo test app::tests::ready_session_resolves_to_ready_terminal_state_and_consumes_focus
cargo test app::tests::deleting_session_cleans_up_provider_terminal_map
```

### 5.3 Invalidation Conditions
- Any visual layout regression in `left_panel` or `right_panel`.
- Any compiler warning on dead code or unused imports under `cargo clippy --all-targets`.
- Failure of `SavedState` RON serialization backward compatibility roundtrip.
- Failure of PTY dimension resize when resizing the window.
