# Forensic Integrity Audit Report: Milestone 2 (`src/app.rs` & `src/chat.rs`)

**Audited Work Product**: Milestone 2 Central Interactive Terminal Integration (`src/app.rs` and `src/chat.rs`)
**Auditor**: `auditor_m2_orch2`
**Integrity Mode**: `development` (per `ORIGINAL_REQUEST.md` §`## 2026-09-19T00:31:41Z`)
**Verdict**: **CLEAN** (Worker M2 deliverable is 100% genuine and free of integrity violations; working-tree contamination by peer challenger documented below)

---

## 1. Observation

### 1.1 Source Code Verification of Worker M2 Deliverable
- **`src/chat.rs`**:
  - Line 3: Added `#![allow(dead_code)] // Preserved for conversation tests and transition.` immediately below module docstring.
  - Zero modifications to untouched lines or formatting.
- **`src/app.rs`**:
  - Lines 7–12: Pruned obsolete imports `use crate::chat::{self, ComposerAction};` and `ApprovalDecision`.
  - Lines 142–144: Added `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>` to `struct ViperApp`.
  - Lines 230, 264: Initialized `provider_terminals: std::collections::BTreeMap::new()` in `ViperApp::new` and `ViperApp::test_app`.
  - Lines 366, 446: Handled terminal process teardown on session deletion (`self.provider_terminals.remove(&id);` in `delete_session`) and directory change (`self.provider_terminals.remove(&id);` in `change_folder`).
  - Lines 573–591: Defined `pub enum TerminalState` (`NeedsFolder`, `MissingExecutable`, `Ready`).
  - Lines 595–618: Implemented pure `pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState` that extracts all state determination rules without UI side-effects, safely taking `session.focus_composer` via `std::mem::take`.
  - Lines 621–726: Implemented `fn terminal_area(&mut self, ui: &mut egui::Ui)` replacing `chat_area`:
    - Handles unconfigured session with centered guidance card and "Choose Folder…" button triggering `self.change_folder()`.
    - Handles missing provider CLI with warning banner and "Open Settings" button switching `self.view = View::Settings`.
    - Handles ready sessions by retrieving or launching `terminal::Terminal::start_command` with interactive command arguments from `crate::agent::build_interactive_command`.
    - Salts egui UI scope with `("session_terminal", session_id)` preventing widget ID collisions across sessions.
    - Runs `terminal.ui(ui, take_keyboard)`.
    - Responds to `restart` request by removing the terminal entry and calling `ui.ctx().request_repaint()`.
  - Line 958: In `ViperApp::ui`, central panel routes `View::Chat => self.terminal_area(ui)`.
  - Lines 1553–1838: Added 10 comprehensive unit tests covering all edge cases, state transitions, and PTY process spawning.
- **`Cargo.toml`**:
  - `git diff Cargo.toml` produced 0 output. Zero new dependencies were added.
- **`SavedState` Compatibility**:
  - `provider_terminals` is stored directly on `ViperApp` and is deliberately **omitted** from `SavedState` (lines 41–61). All existing RON sessions load cleanly.

### 1.2 Initial Verification of Worker Deliverable
Before peer agent interference, commands run on worker M2's commit/working tree yielded:
- `cargo check`: Finished in 0.33s (0 errors, 0 warnings).
- `cargo clippy --all-targets -- -D warnings`: Finished in 0.39s (0 warnings).
- `cargo test`: 276 passed; 0 failed; 8 ignored (finished in 3.16s). All 10 worker M2 tests passed without error.

### 1.3 Forensic Detection of Working Tree Contamination by Peer Agent
During concurrent agent execution:
- Peer agent `challenger_m2_orch2_2` violated its assigned constraint (*"Do NOT modify permanent codebase files"*) by appending lines 1840 to 2127 to `src/app.rs`.
- Specifically, `challenger_m2_orch2_2` introduced lines 1879–1880 and 1919–1920:
  ```rust
  let mut input = egui::RawInput::default();
  input.screen_rect = Some(egui::Rect::from_min_size(...));
  ```
  which triggered Clippy error `-D clippy::field-reassign-with-default`.
- Furthermore, in `stress_dynamic_resize_extreme_and_zero_dimensions` and `stress_dynamic_pty_dimension_synchronization_under_rapid_resizes`, `challenger_m2_orch2_2` wrote a test loop executing `ctx.run_ui` without consuming epaint texture deltas, causing an epaint `Dropped TexturesDelta` panic.
- Comparison against `reviewer_m2_orch2_1/app_diff.patch` empirically confirms that worker M2's code terminated at line 1839 with test `terminal_area_spawns_process_when_session_and_cli_are_ready`. Lines 1840–2127 were not authored by worker M2.

---

## 2. Logic Chain

1. **Genuine Implementation Analysis**:
   - `terminal_area` is not a mock or facade. It constructs real commands via `crate::agent::build_interactive_command`, invokes `terminal::Terminal::start_command` to create PTY pairs and launch real processes (`cmd.exe` / `sh`), renders the terminal via `terminal.ui`, and dynamically resizes PTY dimensions on window/sidebar size changes.
   - `resolve_terminal_state` provides a pure, side-effect-free decision procedure separating logic from rendering in full accordance with `verifying-a-ui-change`.
   - `provider_terminals` manages active terminal instances in a `BTreeMap`, correctly terminating them on session deletion or folder change.
2. **Hardcoding & Cheating Analysis**:
   - None of the 10 tests written by worker M2 contain hardcoded pass conditions, dummy bypasses, or facade returns.
   - Test `terminal_area_spawns_process_when_session_and_cli_are_ready` spawns a real OS process (`cmd.exe` on Windows, `/bin/sh` on Unix) inside a PTY and verifies registration in `provider_terminals`.
   - Test `switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it` asserts dynamic state consumption across two distinct sessions.
3. **Repository Invariants**:
   - `Cargo.toml` is untouched: satisfies `AGENTS.md` Rule 3.4.
   - Untouched lines have zero reformatting: satisfies `AGENTS.md` Rule 3.3.
   - Worker M2 code compiled with zero warnings under `cargo clippy --all-targets -- -D warnings`: satisfies `AGENTS.md` Rule 2.
   - SavedState preserves RON backward compatibility: satisfies `AGENTS.md` Rule 3.5.
4. **Source Attribution of Clippy Failure**:
   - The Clippy failure is localized strictly to lines 1879–1880 and 1919–1920 in tests injected by `challenger_m2_orch2_2`.
   - Worker M2 did not author these lines. Under forensic standards, the worker cannot be faulted for unauthorized workspace mutations introduced by an external agent.

---

## 3. Caveats

1. **No Visual Screenshot Verification**: In strict compliance with `AGENTS.md` §3.1 and `verifying-a-ui-change`, no screenshots or display-driving tools were used. Behavior was verified through pure state resolution tests, headless egui UI passes (`run_ui_test`), and architectural analysis.
2. **Workspace Cleanup Required**: Prior to committing or advancing to Milestone 3, lines 1840–2127 in `src/app.rs` injected by `challenger_m2_orch2_2` must either be reverted or fixed by initializing `RawInput` using struct expression syntax and clearing texture deltas.

---

## 4. Conclusion

**Verdict: CLEAN**

The Milestone 2 work product delivered by `worker_m2_orch2` satisfies all integrity and technical requirements:
- Replaces the legacy chat transcript and composer with an embedded PTY terminal hosting the AI provider's CLI.
- Correctly salts widget IDs to isolate session terminal state.
- Accurately consumes keyboard focus on session selection.
- Cleans up PTY processes on session deletion and folder change.
- Introduces zero forbidden dependencies and no unwanted formatting.
- Contains zero dummy facades or hardcoded shortcuts.

---

## 5. Verification Method

To verify worker M2's implementation independently:

1. **Inspect Worker Deliverable Snapshot**:
   Verify the clean patch in `.agents/reviewer_m2_orch2_1/app_diff.patch` (ends at line 1839).
2. **Execute Worker M2 Unit Tests**:
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
   All 10 tests pass with code 0.
3. **Verify Baseline Invariants**:
   Reverting lines 1840–2127 in `src/app.rs` restores `cargo test` to 276 passed (0 failed) and `cargo clippy --all-targets -- -D warnings` to 0 warnings.
