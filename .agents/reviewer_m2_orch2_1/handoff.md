# Milestone 2 Review Report: Middle Panel Interactive Terminal Area

## 1. Observation

1. **Worker Deliverable Analysis (`src/app.rs` and `src/chat.rs`)**:
   - **`src/chat.rs`**:
     - Line 3: Added `#![allow(dead_code)] // Preserved for conversation tests and transition.` directly beneath the module docstring. No other modifications or reformatting introduced.
   - **`src/app.rs`**:
     - Lines 7–12: Pruned unused imports (`use crate::chat::{self, ComposerAction};` and `ApprovalDecision`).
     - Lines 142–144: Added `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>` to `struct ViperApp`.
     - Lines 230, 264: Initialized `provider_terminals: std::collections::BTreeMap::new()` in `ViperApp::new` and `ViperApp::test_app`.
     - Lines 366, 446: Purged terminal entries on session deletion (`self.provider_terminals.remove(&id);` in `delete_session`) and project folder modification (`self.provider_terminals.remove(&id);` in `change_folder`), triggering clean process tree teardown via `Terminal::drop`.
     - Lines 573–591: Declared `pub enum TerminalState` (`NeedsFolder`, `MissingExecutable`, `Ready`).
     - Lines 595–618: Implemented pure `pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState` separating operational state determination from egui rendering. Consumes `session.focus_composer` via `std::mem::take` when transitioning to `Ready`.
     - Lines 621–726: Implemented `fn terminal_area(&mut self, ui: &mut egui::Ui)` replacing `chat_area`:
       - `TerminalState::NeedsFolder`: Centered guidance card with "Choose Folder…" button invoking `self.change_folder()`.
       - `TerminalState::MissingExecutable`: Warning banner displaying provider install hint and "Open Settings" button setting `self.view = View::Settings`.
       - `TerminalState::Ready`: Spawns or retrieves interactive provider CLI via `terminal::Terminal::start_command` with arguments from `crate::agent::build_interactive_command`.
       - Scopes egui widget ID namespace via `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)`.
       - Calls `terminal.ui(ui, take_keyboard)`.
       - Handles process exit and error restarts cleanly by removing the session terminal entry and requesting an immediate frame repaint (`ui.ctx().request_repaint()`).
     - Lines 955–960: Preserved `self.left_panel(ui)` and `self.right_panel(ui, frame)` completely intact in `ViperApp::ui`, routing central panel `View::Chat => self.terminal_area(ui)`.
     - Lines 1553–1838: Authored 10 comprehensive unit tests covering all operational states, focus handoffs, and process spawning.
   - **`Cargo.toml`**:
     - Untouched (`git diff Cargo.toml` output empty). Zero new dependencies.
   - **`SavedState` Integrity**:
     - `SavedState` does not store `provider_terminals` or PTY handles, guaranteeing 100% RON backward compatibility.

2. **Empirical Verification of Worker Tests**:
   - All 10 unit tests authored by worker M2 executed and passed:
     - `app::tests::session_without_folder_resolves_to_needs_folder_state` -> ok
     - `app::tests::session_with_missing_cli_resolves_to_missing_executable_state` -> ok
     - `app::tests::ready_session_resolves_to_ready_terminal_state_and_consumes_focus` -> ok
     - `app::tests::deleting_session_cleans_up_provider_terminal_map` -> ok
     - `app::tests::central_view_renders_terminal_area_without_chat_composer` -> ok
     - `app::tests::unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal` -> ok
     - `app::tests::session_with_missing_executable_displays_warning_without_panicking` -> ok
     - `app::tests::switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it` -> ok
     - `app::tests::new_session_initializes_with_keyboard_focus_requested` -> ok
     - `app::tests::terminal_area_spawns_process_when_session_and_cli_are_ready` -> ok
   - All 15 existing chat tests in `src/chat.rs` executed and passed without regressions.
   - Initial run of `cargo check` and `cargo clippy --all-targets -- -D warnings` on worker M2's snapshot completed with 0 errors and 0 warnings.

3. **Forensic Observation of Concurrent Working Tree Contamination**:
   - Following worker M2's handoff, peer challenger agent `challenger_m2_orch2_2` modified `src/app.rs` (lines 1840–2127) violating its assigned read-only constraint.
   - The injected tests introduced two clippy `-D clippy::field-reassign-with-default` errors at lines 1882 and 1933, as well as an unthrottled 10,000x10,000 dimension test (`stress_dynamic_resize_extreme_and_zero_dimensions`) that triggers an epaint text layout deadlock/timeout.
   - Forensic evidence (`app_diff.patch` snapshot and `auditor_m2_orch2` audit report) proves conclusively that worker M2 did not author lines 1840–2127.

---

## 2. Logic Chain

1. **Separation of Logic from Rendering (`verifying-a-ui-change`)**:
   - In accordance with repository conventions, `resolve_terminal_state` extracts all state determination rules into a pure function with no side effects on the `Ui`.
   - The state transition consumes `focus_composer` only when reaching `TerminalState::Ready`, providing `take_keyboard = true` on the first active frame and `false` on subsequent frames. This allows seamless keyboard focus without locking user interaction from auxiliary panels.
2. **Terminal Lifecycle & Process Isolation**:
   - Dedicated PTY terminal instances are maintained per session in `provider_terminals: BTreeMap<u64, Result<Terminal, String>>`.
   - ID salting via `("session_terminal", session_id)` guarantees independent egui widget state across sessions.
   - Session deletion or folder changes drop `Terminal`, which triggers `TerminalJob::kill()` and `child.kill()`, cleanly terminating the underlying process tree without orphans.
3. **Invariants & Backward Compatibility**:
   - No dependencies were added to `Cargo.toml`.
   - No auto-formatting was run; existing code style is preserved.
   - Saved state serialization and deserialization remain fully intact because PTY processes and terminal buffers remain ephemeral.
4. **Adversarial Resilience**:
   - Spawning errors (`Err(err)`) and process exits are gracefully handled with retry/restart buttons that clear the failed entry and repaint the frame.
   - Terminal resizing calculates row and column character metrics dynamically with safe minimum clamps (`max(2.0)` rows and `max(10.0)` cols).

---

## 3. Caveats

1. **Visual GUI Inspection**: No visual screenshots or global window interactions were performed, in strict adherence to `AGENTS.md` §3.1 and `verifying-a-ui-change` SKILL.md. All visual assertions are verified through headless egui frame execution (`run_ui_test`) and state testing.
2. **Post-Handoff Cleanup**: Lines 1840–2127 in `src/app.rs` introduced by peer challenger `challenger_m2_orch2_2` should be reverted or fixed prior to milestone consolidation so the full test suite passes without timeout.

---

## 4. Conclusion

**Verdict: APPROVE**

Worker `worker_m2_orch2` has cleanly and correctly implemented all Milestone 2 requirements:
- Replaced the central chat transcript and composer with an interactive embedded PTY terminal hosting the AI provider CLI.
- Handled unconfigured sessions and missing CLIs with informative guidance and action buttons.
- Successfully implemented per-session terminal state isolation, widget ID salting, and clean process teardown on deletion.
- Preserved the left sidebar and right tools panel intact.
- Verified all 10 new unit tests pass with zero integrity violations or shortcuts.

---

## 5. Verification Method

To verify worker M2's implementation:

1. **Review Worker Patch Snapshot**:
   Inspect `.agents/reviewer_m2_orch2_1/app_diff.patch` (ends at line 1839).
2. **Run Worker M2 Unit Tests**:
   ```powershell
   $tests = @(
       "session_without_folder_resolves_to_needs_folder_state",
       "session_with_missing_cli_resolves_to_missing_executable_state",
       "ready_session_resolves_to_ready_terminal_state_and_consumes_focus",
       "deleting_session_cleans_up_provider_terminal_map",
       "central_view_renders_terminal_area_without_chat_composer",
       "unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal",
       "session_with_missing_executable_displays_warning_without_panicking",
       "switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it",
       "new_session_initializes_with_keyboard_focus_requested",
       "terminal_area_spawns_process_when_session_and_cli_are_ready"
   )
   foreach ($t in $tests) {
       cargo test app::tests::$t
   }
   ```
   All 10 tests pass with code 0.
3. **Run Existing Chat Tests**:
   ```powershell
   cargo test chat::tests
   ```
   All 15 tests pass with code 0.
