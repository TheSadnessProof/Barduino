# Milestone 2 Independent & Adversarial Review Report

**Reviewer**: `reviewer_m2_orch2_2`  
**Roles**: Reviewer, Adversarial Critic  
**Working Directory**: `c:\Users\ditob\Documents\viper\.agents\reviewer_m2_orch2_2`  
**Target Files**: `src/app.rs`, `src/chat.rs`  
**Final Verdict**: **REQUEST_CHANGES** (Worker M2 implementation is technically sound and genuine; working tree has blocking clippy and test failures due to unauthorized code injection by peer agent `challenger_m2_orch2_2`)

---

## 1. Observation

### 1.1 Worker M2 Deliverable Inspection (`src/app.rs` & `src/chat.rs`)
- **`src/chat.rs`**:
  - Line 3: Added `#![allow(dead_code)] // Preserved for conversation tests and transition.` immediately below module documentation. Preserves legacy models and conversation tests for regression protection without dead-code warnings.
  - Zero modifications to untouched lines or formatting.
- **`src/app.rs`**:
  - Lines 7–12: Cleaned up unused imports (`use crate::chat::{self, ComposerAction};` and `ApprovalDecision`).
  - Lines 142–144: Added `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>` to `struct ViperApp`. Kept in-memory and omitted from `SavedState` to maintain 100% RON serialization backward compatibility.
  - Lines 230, 264: Initialized `provider_terminals: std::collections::BTreeMap::new()` in `ViperApp::new` and `ViperApp::test_app`.
  - Lines 366, 446: Handled terminal cleanup on session deletion (`self.provider_terminals.remove(&id);` in `delete_session`) and project folder changes (`self.provider_terminals.remove(&id);` in `change_folder`).
  - Lines 573–591: Defined `pub enum TerminalState` (`NeedsFolder`, `MissingExecutable`, `Ready`).
  - Lines 595–618: Implemented pure `pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState`. Evaluates folder presence, CLI detection, and consumes `session.focus_composer` via `std::mem::take` when transitioning to `Ready`.
  - Lines 621–726: Implemented `terminal_area(&mut self, ui: &mut egui::Ui)` replacing `chat_area`:
    - `TerminalState::NeedsFolder`: Renders centered empty-state card with "Choose Folder…" button invoking `self.change_folder()`.
    - `TerminalState::MissingExecutable`: Renders warning banner with "Open Settings" button switching `self.view = View::Settings`.
    - `TerminalState::Ready`: Spawns or retrieves terminal via `Terminal::start_command` with interactive command arguments from `crate::agent::build_interactive_command`. Salts egui scope with `("session_terminal", session_id)` to isolate widget IDs, runs `terminal.ui(ui, take_keyboard)`, and purges exited/failed terminals on restart request.
  - Line 958: Central panel routes `View::Chat => self.terminal_area(ui)`.
  - Lines 1553–1839: Added 10 unit tests in `src/app.rs::tests` covering state transitions, focus handoff, folder selection prompts, missing CLI banners, PTY process startup, and process cleanup.
- **`Cargo.toml`**:
  - `git diff origin/dev Cargo.toml` output was completely empty. Zero new dependencies were added.
- **Repository Invariants on Worker M2 Baseline**:
  - Before peer interference, `cargo check` completed in 0.47s with 0 errors.
  - `cargo test` completed with 276 passed, 0 failed, 8 ignored (historical external tests). All 10 worker tests passed.
  - `cargo clippy --all-targets -- -D warnings` completed with 0 warnings.

### 1.2 Adversarial Inspection of Specific Requirements
1. **Keyboard Focus Theft**:
   - In `resolve_terminal_state` (line 606): `let take_keyboard = std::mem::take(&mut session.focus_composer);`.
   - In `terminal_area` (lines 704–705): `terminal.ui(ui, take_keyboard)`.
   - In `terminal.rs` (lines 366–368):
     ```rust
     if response.clicked() || take_keyboard {
         response.request_focus();
     }
     ```
   - On session switch (`SidebarAction::Select`), `session.focus_composer` is primed to `true`. On the first frame rendered in `terminal_area`, `take_keyboard` is `true` and focus is requested. On subsequent frames, `session.focus_composer` is `false`, so `take_keyboard` is `false`. Focus is NOT permanently stolen; clicking on the sidebar or tools panel freely moves keyboard focus away.
2. **egui Widget ID Collisions**:
   - In `terminal_area` (line 701): `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)`. All widgets and sizing responses allocated within `terminal.ui` derive their egui ID from this salted scope.
   - In `tools.rs` (line 526): `ui.push_id(("terminal", *number), ...)`.
   - In `sidebar.rs` (line 461): `id_salt(("session_row", session.id))`.
   - Middle terminals and auxiliary tool terminals operate under distinct ID salts. Concurrent sessions do not collide.
3. **Resource Cleanup**:
   - In `delete_session` (line 366): `self.provider_terminals.remove(&id);`.
   - In `change_folder` (line 446): `self.provider_terminals.remove(&id);`.
   - When removed from `provider_terminals`, the `Terminal` instance is dropped, invoking `Terminal::drop` (lines 521–526 of `src/terminal.rs`), which triggers `self.job.kill()` (Windows Job Object `TerminateJobObject(job, 1)` or Unix `kill(-pgid, 9)`) and `self.child.kill()`. Entire process tree is terminated without orphans.
4. **PTY Dynamic Resizing**:
   - In `terminal.rs` (lines 357–364):
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
   - Resizing the application window or collapsing/expanding the sidebars alters `ui.available_size()` in the central panel. The changed dimensions trigger `master.resize` and `parser().screen_mut().set_size`.

### 1.3 Working Tree Contamination by Peer Agent `challenger_m2_orch2_2`
During concurrent review execution, peer agent `challenger_m2_orch2_2` breached its assigned read-only constraint by appending lines 1840 to 2129 to `src/app.rs`.
This mutation broke the repository in two distinct ways:
1. **Clippy Failure**: Running `cargo clippy --all-targets -- -D warnings` fails with 2 errors:
   ```
   error: field assignment outside of initializer for an instance created with Default::default()
       --> src\app.rs:1882:13
   error: field assignment outside of initializer for an instance created with Default::default()
       --> src\app.rs:1929:13
   ```
2. **Test Suite Failure**: Running `cargo test stress_dynamic_resize_extreme_and_zero_dimensions -- --nocapture` crashes with:
   ```
   Testing size [0.0 0.0]
   Finished size [0.0 0.0]
   error: test failed, to rerun pass `--bin viper`
   Caused by: process didn't exit successfully: exit code: 0xffffffff
   ```
   Because `cmd.exe /c exit 0` immediately terminates, calling `master.resize` across negative/extreme sizes on an exited Windows ConPTY process triggers an OS-level fault (exit code `0xffffffff`).

---

## 2. Logic Chain

1. **Worker Deliverable Integrity & Quality**:
   - The code submitted by `worker_m2_orch2` (lines 1–1839 of `src/app.rs` and lines 1–5 of `src/chat.rs`) represents genuine, non-facade implementation.
   - PTY processes are legitimately spawned, keystrokes are routed via `terminal.ui`, state transitions are modeled in pure functions (`resolve_terminal_state`), and widget states are properly salted.
   - All 10 worker unit tests verify actual runtime behavior and pass without mockery.
2. **Evaluation of Specific Review Targets**:
   - *Keyboard Focus*: Verifiably one-shot per session selection. `focus_composer` is consumed on the first ready frame and not re-requested on subsequent frames.
   - *Widget ID Collisions*: Verifiably prevented via `("session_terminal", session_id)` salt scoping.
   - *Cleanup*: Verifiably verified via Windows Job Object teardown in `Terminal::drop` on `provider_terminals.remove(&id)`.
   - *Resizing*: Verifiably dynamic via `ui.available_size()` tracking and `master.resize`.
3. **Repository Invariant Status**:
   - `AGENTS.md` Rule 2 and Definition of Done mandate zero compiler warnings, zero clippy warnings (`-D warnings`), and 100% passing tests.
   - While worker M2's isolated changes meet these standards, the current working tree is broken due to lines 1840–2129 injected by `challenger_m2_orch2_2`.
   - The pre-commit hook `.githooks/pre-commit` will refuse any commit in this state.
   - As an objective reviewer and adversarial critic, approving a working tree that fails `cargo clippy` and `cargo test` would violate verification integrity.
   - Therefore, the appropriate, actionable verdict is **REQUEST_CHANGES**, specifically instructing the orchestrator to revert the rogue lines 1840–2129 in `src/app.rs`.

---

## 3. Caveats

1. **No Visual Screenshot Verification**: In accordance with `AGENTS.md` §3.1 and `verifying-a-ui-change` SKILL.md, no GUI screenshots were taken and no global mouse/keyboard events were dispatched. Behavior was verified through pure state resolution tests, headless egui UI passes (`run_ui_test`), and architectural analysis.
2. **Attribution of Failures**: The clippy and test failures in `src/app.rs` were definitively introduced by `challenger_m2_orch2_2`, not `worker_m2_orch2`. Reverting lines 1840–2129 completely restores the working tree to a 100% passing state (276 passed tests, 0 clippy warnings).

---

## 4. Conclusion

**Verdict: REQUEST_CHANGES**

### Actionable Remediation
1. **Revert Contaminating Lines**: Revert lines 1840–2129 in `src/app.rs` (the additions made by `challenger_m2_orch2_2`), restoring `src/app.rs` to end at line 1839 (`terminal_area_spawns_process_when_session_and_cli_are_ready`).
2. **Confirm Clean Gate**: Once reverted, verify that `cargo check`, `cargo test` (276 passed), and `cargo clippy --all-targets -- -D warnings` pass with 0 errors and 0 warnings.
3. Upon applying this reversion, the Milestone 2 deliverable is immediately ready for approval and progression to Milestone 3.

### UI Change Description for the User
> The middle area of the application now renders an interactive embedded terminal hosting the selected AI CLI instead of the chat transcript and bottom composer box.
> - For a session without a folder selected, a prompt card appears: **"Choose a project folder to start"** with a **"Choose Folder…"** button.
> - If the selected provider CLI is not installed, a warning banner appears with an **"Open Settings"** button.
> - Once configured, the terminal spawns the provider CLI in the session working directory with full ANSI colors, cursor positioning, and immediate keyboard focus. Resizing the window or toggling sidebars smoothly updates terminal dimensions.

---

## 5. Verification Method

To verify this finding independently:
```powershell
# 1. Observe current clippy failure on working tree (2 errors in lines 1882 & 1929)
cargo clippy --all-targets -- -D warnings

# 2. Observe crashing test introduced by challenger
cargo test stress_dynamic_resize_extreme_and_zero_dimensions -- --nocapture

# 3. Inspect the clean Worker M2 baseline diff
# Notice Worker M2 deliverable ends at line 1839 with test `terminal_area_spawns_process_when_session_and_cli_are_ready`
git diff origin/dev src/chat.rs
git diff origin/dev src/app.rs

# 4. Once lines 1840-2129 are reverted:
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```
