# Handoff Report: Milestone 4 Adversarial Integration Review

## Review Summary
- **Reviewer**: `reviewer_m4_orch2_2`
- **Milestone**: Milestone 4 (Interactive Provider Terminal Integration)
- **Verdict**: **APPROVE**
- **Integrity Audit**: PASSED (0 integrity violations, 0 hardcoded facades, 0 unauthorized shortcuts)

---

## 1. Observation

### 1.1 Definition of Done Commands & Output
1. **Compilation Check**:
   - Command: `cargo check`
   - Result: Exit code 0 in 0.35s:
     ```
     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.35s
     ```
2. **Full Unit Test Suite**:
   - Command: `cargo test`
   - Result: Exit code 0 in 3.56s:
     ```
     test result: ok. 284 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 3.56s
     ```
   - Ignored test check: 8 tests are marked `#[ignore]`. Each was verified against `AGENTS.md` Rule 3.2 (`runs_the_real_antigravity_cli`, `runs_the_real_codex_cli`, `full_access_really_runs_commands`, `real_models_come_from_the_clis`, `checks_real_plan_limits`, `reads_codex_limits_from_this_computer`, `finds_the_shells_on_this_computer`, `closing_a_terminal_stops_programs_started_in_it`). Exactly **zero newly ignored tests**.
3. **Clippy Linter Verification**:
   - Command: `cargo clippy --all-targets -- -D warnings`
   - Result: Exit code 0 in 0.33s:
     ```
     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.33s
     ```
   - Clippy reports **zero warnings**.
4. **Dependency Check**:
   - Command: `git diff Cargo.toml`
   - Result: Output is completely empty. Exactly **zero dependencies added** to `Cargo.toml`.
5. **Formatting and Commit Policy**:
   - Command: `git diff -w --stat src/` vs `git diff --stat src/`:
     `git diff --stat`: 2031 insertions, 79 deletions across 7 files.
     `git diff -w --stat`: 2021 insertions, 69 deletions across 7 files.
     Untouched lines were not modified or reformatted; `cargo fmt` was **not** run.
   - Command: `git log -n 1 --oneline` shows latest commit `e821082`. Exactly **zero automatic commits** were executed.

### 1.2 Architecture & Error Recovery Observations
1. **CLI Process Exit & Crash Recovery**:
   - In `src/terminal.rs` (lines 346–351):
     ```rust
     if self.has_exited() {
         ui.horizontal(|ui| {
             ui.label(egui::RichText::new("The process has exited.").weak());
             restart = ui.button("Restart").clicked();
         });
     }
     ```
   - In `src/app.rs` (lines 721–724):
     ```rust
     if restart {
         self.provider_terminals.remove(&session_id);
         ui.ctx().request_repaint();
     }
     ```
   - Dropping the terminal invokes `TerminalJob::drop`, which cleanly terminates any residual processes via Windows Job Objects (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` + `TerminateJobObject`) or Unix process group kill (`kill(-pgid, 15)`).
   - On spawn error (`Err(err)` in `provider_terminals`), `app.rs` renders the error with a "Try again" button that clears the error entry upon click and triggers a respawn. Tested in `failed_terminal_spawn_retry_action_clears_error_and_allows_respawn`.

2. **Unconfigured Session Without Folder**:
   - In `src/app.rs` (lines 597–599, 634–652):
     `resolve_terminal_state` checks `if !session.has_folder() { return TerminalState::NeedsFolder { provider }; }`.
     `terminal_area` draws a centered guidance card:
     - Heading: "Choose a project folder to start"
     - Description: "Select a project directory to launch an interactive <Provider> session."
     - Button: "Choose Folder…" which triggers `change_folder()`.
     No terminal process is spawned while the session lacks a folder. Tested in `unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal`.

3. **Missing Provider CLI Executable**:
   - In `src/app.rs` (lines 600–605, 654–675):
     `resolve_terminal_state` returns `TerminalState::MissingExecutable { provider, hint: provider.install_hint() }`.
     `terminal_area` displays a styled frame with:
     - Label: "<Provider> isn't installed. <Install hint>."
     - Button: "Open Settings" which sets `self.view = View::Settings`.
     Prevents attempting to spawn non-existent executables. Tested in `session_with_missing_executable_displays_warning_without_panicking`.

4. **Application Restart & State Restoration**:
   - In `src/app.rs` (lines 41–61, 144):
     `provider_terminals` is an ephemeral member of `ViperApp` (`BTreeMap<u64, Result<terminal::Terminal, String>>`) and is deliberately omitted from `SavedState`.
     On restart, `SavedState` deserializes from RON without PTY handles.
     On the initial frame of `terminal_area`, `provider_terminals.entry(session_id).or_insert_with(...)` lazily starts the interactive CLI inside `session.working_dir()`. Inactive sessions remain unspawned until switched to.
     Tested in `saved_state_ron_serialization_omits_provider_terminals`, `comprehensive_legacy_ron_state_loads_and_carries_defaults`, and `restart_restores_session_and_spawns_cli_lazily_in_session_folder`.

5. **Multi-Session Switching & Keyboard Focus**:
   - In `src/app.rs` (lines 457–461):
     `SidebarAction::Select(id)` sets `self.active_session = id; self.active_session_mut().focus_composer = true;`.
   - In `src/app.rs` (lines 606–617, 685, 705):
     `resolve_terminal_state` consumes the flag (`let take_keyboard = std::mem::take(&mut session.focus_composer);`) and passes it to `terminal.ui(ui, take_keyboard)`.
   - In `src/terminal.rs` (lines 366–377):
     `if response.clicked() || take_keyboard { response.request_focus(); }` acquires focus immediately and locks Tab, arrows, and Escape via `set_focus_lock_filter`.
   - Tested in `switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it` and `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`.

6. **Session Deletion Teardown**:
   - In `src/app.rs` (lines 360–382):
     `delete_session(id)` removes `self.provider_terminals.remove(&id);`.
     Dropping the terminal kills the process tree.
     If the active session was deleted, the neighbor session is selected and `next.focus_composer = true;` ensures keyboard focus is immediately transferred.
     Tested in `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`.

---

## 2. Logic Chain

1. **Premise 1**: All requirements defined in `ORIGINAL_REQUEST.md` (§R1–§R4) specify hosting the active AI provider's CLI directly in an embedded PTY terminal in the middle panel, dynamic resize handling, per-session lifecycle management, and preservation of the sidebar and tools panel.
2. **Observation 1.1**: `terminal_area` replaces `chat_area` in `src/app.rs:958`, hosting `Terminal::start_command` for Claude, Codex, and Antigravity.
3. **Observation 1.2**: `Terminal::ui` dynamically updates `(rows, cols)` and invokes `self.master.resize(pty_size(self.size))` and `self.parser().screen_mut().set_size(rows, cols)` whenever `ui.available_size()` changes.
4. **Observation 1.3**: `ViperApp.provider_terminals` keeps terminal instances indexed by `session.id`, ensuring background sessions remain running and switching sessions swaps the active display buffer without re-spawning.
5. **Observation 1.4**: `SavedState` does not serialize PTY handles, preserving 100% backward compatibility with existing RON save files.
6. **Observation 1.5**: All 284 unit tests pass, zero clippy warnings exist under `-D warnings`, zero dependencies were added to `Cargo.toml`, and all 8 ignored tests are the pre-existing ones authorized by `AGENTS.md`.
7. **Deduction**: The Milestone 4 integration satisfies all architectural and functional acceptance criteria, respects all codebase invariants, and introduces no regressions.

---

## 3. Caveats

1. **Interactive Visual Confirmation**:
   In strict accordance with `AGENTS.md` Rule 3.1 and the `verifying-a-ui-change` skill, automated headless frame tests (`run_ui_test`) were executed, but the GUI was not visually observed on a real physical monitor. A human user must look at the window to verify aesthetic rendering.
2. **Paid CLI Invocations**:
   Per `AGENTS.md` Rule 3.2, live CLI tests that consume paid tokens or drive external accounts (`runs_the_real_antigravity_cli`, etc.) remain `#[ignore]`d and were not executed during this review.

---

## 4. Conclusion

**Verdict: APPROVE**

The interactive provider terminal integration is robust, cleanly structured, and strictly compliant with all requirements and rules:
- Integrity forensics confirmed real PTY execution via `portable-pty` and `vt100` with no facade or fake implementations.
- Error recovery covers process exits, invalid directories, missing binaries, missing folder configuration, and application restarts.
- Focus routing seamlessly hands keyboard control over upon session creation, session switching, and session deletion.
- Code style, error messages, typographic punctuation, and test naming adhere to `AGENTS.md`.

---

## 5. Verification Method

To independently verify this evaluation:

1. **Compilation**:
   ```bash
   cargo check
   ```
   *Expected*: Passes with 0 errors in under 1 second.
2. **Clippy Linters**:
   ```bash
   cargo clippy --all-targets -- -D warnings
   ```
   *Expected*: Zero warnings across all targets.
3. **Unit Tests**:
   ```bash
   cargo test
   ```
   *Expected*: 284 tests pass, 0 fail, 8 pre-existing ignored.
4. **Dependency Invariant**:
   ```bash
   git diff Cargo.toml
   ```
   *Expected*: Empty output.

---

## 6. What the User Will See (UI Description per `verifying-a-ui-change`)

> **Note**: This description is provided without driving the user's mouse or capturing their screen. Automated tests and clippy pass. Please review the application visually to confirm:

1. **Middle Panel (Interactive AI Terminal)**:
   - **Configured & Ready Session**:
     The former markdown chat bubbles and bottom composer text input box are replaced by a dedicated full-height monospace terminal widget (dark `#181818` background with light gray text).
     The terminal runs the chosen AI provider's CLI (`claude`, `codex`, or `agy`) in interactive mode directly inside the session's folder.
     Keystrokes (including Enter, Backspace, arrow keys, Tab, and Ctrl combinations) go straight to the running CLI.
     Dragging over terminal text highlights it for copying (Ctrl+C copies selected text; without a selection, Ctrl+C sends SIGINT to the running CLI).
     Resizing the Viper window or expanding/collapsing side panels dynamically adjusts the terminal's columns and rows.
   - **Process Exit / Crash**:
     If the CLI process exits, a status bar appears at the top of the terminal reading: *"The process has exited."* alongside a **"Restart"** button. Clicking "Restart" drops the previous process tree and immediately launches a fresh CLI instance.
   - **New Session (Unconfigured Folder)**:
     If a session has no folder chosen yet, the middle panel displays a centered guidance card:
     **"Choose a project folder to start"**
     *"Select a project directory to launch an interactive <Provider> session."*
     with a **"Choose Folder…"** button. Clicking the button opens the folder picker. Once chosen, the interactive terminal launches automatically.
   - **Missing CLI Executable**:
     If the provider's CLI is not installed on the system, the middle area shows an alert frame at the top:
     *"<Provider> isn't installed. <Install hint>."* with an **"Open Settings"** button that navigates directly to Viper's Settings view.
2. **Left Sidebar**:
   - The project list, session cards, "+ New Session", rename, and delete actions continue to function as before.
   - Clicking a session card switches the middle terminal view to that session's dedicated buffer and transfers keyboard focus immediately without requiring an extra mouse click.
   - Deleting a session stops its underlying process tree and switches the view/focus to the neighboring session.
3. **Right Tools Panel**:
   - Secondary shell terminals, git branch/working tree diffs, and live preview browser tabs remain available on the right side.
   - Opening or closing the tools panel dynamically reflows the middle terminal without interrupting the CLI.
