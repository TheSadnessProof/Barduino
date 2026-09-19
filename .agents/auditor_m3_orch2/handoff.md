# Forensic Audit Report: Milestone 3 Implementation & Tests

**Auditor Agent**: `auditor_m3_orch2`  
**Working Directory**: `c:\Users\ditob\Documents\viper\.agents\auditor_m3_orch2`  
**Work Product**: `src/app.rs` (Milestone 3: Per-Session Lifecycle, Switching & Saved State)  
**Profile**: General Project  
**Integrity Mode**: Development (from `.agents/ORIGINAL_REQUEST.md` §`2026-09-19T00:31:41Z`)  
**Verdict**: CLEAN  

---

## 1. Observation

### Static Code Analysis & Verbatim Evidence
1. **`provider_terminals` Ephemeral Storage** (`src/app.rs:144`):
   ```rust
   /// The interactive provider CLI terminal for each session, kept by session ID
   /// so switching sessions swaps the terminal buffer and keeps the CLI running.
   provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>,
   ```
   - Defined strictly on `struct ViperApp`, completely omitted from `struct SavedState` (`src/app.rs:40-61`).
   - Initialized empty on startup in `ViperApp::new` (`src/app.rs:230`) and `ViperApp::test_app` (`src/app.rs:264`).

2. **Genuine PTY Terminal Spawning** (`src/app.rs:687-698`):
   ```rust
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
   ```
   - Calls `terminal::Terminal::start_command`, which invokes `portable_pty::native_pty_system().openpty()`, spawns the child process via `pair.slave.spawn_command(cmd)`, binds it to a Windows Job Object (`TerminalJob::new(child.process_id())`), sets up `vt100::Parser`, and spawns an asynchronous reader thread.
   - Zero facade functions, zero hardcoded strings, and zero stubbing in production code.

3. **Session Switching & Immediate Keyboard Focus Transfer** (`src/app.rs:457-461`, `595-618`):
   - `SidebarAction::Select(id)` sets `self.state.active_session = id` and `self.active_session_mut().focus_composer = true`.
   - `resolve_terminal_state` executes `let take_keyboard = std::mem::take(&mut session.focus_composer);` and returns `TerminalState::Ready { ..., take_keyboard }`.
   - `terminal_area` passes `take_keyboard` to `terminal.ui(ui, take_keyboard)`, locking keyboard input to the newly active terminal emulator on the switch frame without user mouse click.

4. **Process Teardown on Session Deletion** (`src/app.rs:360-382`):
   ```rust
   fn delete_session(&mut self, id: u64) {
       let Some(index) = self.state.sessions.iter().position(|s| s.id == id) else { return };
       let removed = self.state.sessions.remove(index);
       self.tools.remove(&id);
       self.provider_terminals.remove(&id);
       ...
       if self.state.sessions.is_empty() {
           self.new_session(removed.project_dir, removed.permission_mode);
       } else if self.state.active_session == id {
           let next = &mut self.state.sessions[index.saturating_sub(1)];
           next.focus_composer = true;
           self.state.active_session = next.id;
       }
   }
   ```
   - `self.provider_terminals.remove(&id)` drops the terminal instance.
   - `Terminal::drop` (`src/terminal.rs:521-526`) executes `self.job.kill()` and `let _ = self.child.kill()`.
   - `TerminalJob::drop` (`src/terminal.rs:119-126`) executes `windows::Win32::System::JobObjects::TerminateJobObject(job, 1)` and `CloseHandle(job)`, terminating the entire child and grandchild process tree cleanly.
   - Active session deletion selects neighbor `index.saturating_sub(1)` and primes `focus_composer = true`.

5. **Spawn Error Recovery** (`src/app.rs:707-724`):
   - `Err(err)` renders error banner with a "Try again" button.
   - Clicking "Try again" sets `restart = true`, evicting `self.provider_terminals.remove(&session_id)` and calling `ui.ctx().request_repaint()` so the next frame re-attempts startup.

6. **Repository Rules Compliance Checks**:
   - `git diff HEAD -- Cargo.toml`: Zero diff. No dependencies added.
   - `cargo clippy --all-targets -- -D warnings`: Completed in 0.34s with **0 warnings**.
   - `cargo test`: **284 passed; 0 failed; 8 ignored; 0 measured; finished in 3.87s**.
   - `git grep '#\[ignore\]' src/`: Exactly 8 occurrences, matching the canonical ignored tests permitted by `AGENTS.md` Rule 3.2. Zero newly ignored tests.
   - Formatting: Verified that `cargo fmt` was not run across the codebase (confirmed by `cargo fmt -- --check` diff presence on untouched code). Changes in `src/app.rs` are strictly targeted edits and new tests.

7. **Milestone 3 Test Suite Behavioral Verification** (`src/app.rs:1840-2350`):
   All 8 dedicated Milestone 3 tests run real logic and verify empirical invariants:
   - `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`: Verifies pointer stability (`term1_addr == term1_addr_after`) across session switches and `focus_composer` consumption.
   - `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`: Verifies process teardown in map, survival of neighbor sessions, and focus transfer.
   - `folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd`: Verifies directory change drops old terminal and respawns rooted in new directory.
   - `failed_terminal_spawn_retry_action_clears_error_and_allows_respawn`: Verifies egui button click handler purges error entries and allows clean respawn.
   - `saved_state_ron_serialization_omits_provider_terminals`: Asserts `!ron_str.contains("provider_terminals")` and roundtrips cleanly.
   - `comprehensive_legacy_ron_state_loads_and_carries_defaults`: Asserts legacy `Gemini` -> `Antigravity`, `claude_session_id` -> `agent_session_id`, and defaults.
   - `restart_restores_session_and_spawns_cli_lazily_in_session_folder`: Asserts `provider_terminals` starts empty and lazily spawns on initial draw.
   - `middle_provider_terminal_and_tools_panel_coexist_without_interference`: Asserts pointer stability and zero widget ID collisions when rendering middle PTY and tools panel tabs together.

---

## 2. Logic Chain

1. **Static Authenticity**:
   - Inspection of `src/app.rs` confirmed that `provider_terminals` is declared as `BTreeMap<u64, Result<terminal::Terminal, String>>` directly on `ViperApp`.
   - Inspection of `SavedState` confirmed that `provider_terminals` is not part of `SavedState`, guaranteeing that runtime PTY handles cannot leak into persisted state files.
   - Inspection of `src/terminal.rs:119-126` and `521-526` confirms that process teardown genuinely terminates processes via Windows Job Objects (`TerminateJobObject(job, 1)`), avoiding orphaned processes.

2. **Absence of Cheating / Bypass Patterns**:
   - No hardcoded test responses or facade return values exist in `resolve_terminal_state` or `terminal_area`.
   - The tests do not assert static dummy values; they verify memory addresses of running `Terminal` structs across simulated egui frames, verify process status via `has_exited()`, simulate egui pointer clicks, and deserialize actual legacy RON payloads.

3. **Repository Rules & Invariants**:
   - `Cargo.toml` is completely unchanged (0 new dependencies).
   - `cargo clippy --all-targets -- -D warnings` reports 0 warnings.
   - `cargo test` executes all 284 active tests with 0 failures.
   - The 8 ignored tests are the pre-existing system-dependent integration tests; no unit tests were ignored or disabled.
   - Code formatting matches the house style without mass-reformatting.

---

## 3. Caveats

- **Visual Inspection**: In strict adherence to `AGENTS.md` Rule 3.1, no full-screen captures or live window observation was attempted. All UI behavior was verified through headless egui context frames (`run_ui_test`, `run_ui`).
- **Interactive External CLIs**: Headless tests use standard system shells (`cmd.exe` / `/bin/sh`) to test PTY spawning and lifecycle without spending user funds or requiring external network access.

---

## 4. Conclusion

**Verdict: CLEAN**

Milestone 3 implementation and tests in `src/app.rs` satisfy all functional requirements and acceptance criteria from `ORIGINAL_REQUEST.md` (§`2026-09-19T00:31:41Z`) and `PROJECT.md`. No integrity violations, facades, or test bypasses were detected. The work product is authentic, robust, and fully compliant with repository rules.

---

## 5. Verification Method

To independently reproduce the forensic verification:

```powershell
# 1. Verify compilation
cargo check

# 2. Verify strict clippy zero-warning invariant
cargo clippy --all-targets -- -D warnings

# 3. Verify Milestone 3 unit test suite
cargo test -- multi_session_switching deleting_active_session folder_change_clears failed_terminal_spawn saved_state_ron comprehensive_legacy restart_restores middle_provider_terminal

# 4. Verify full test suite (preserving Rule 3.2 on ignored tests)
cargo test

# 5. Verify zero additions to dependencies
git diff HEAD -- Cargo.toml

# 6. Verify count of ignored tests remains exactly 8
git grep '#\[ignore\]' src/
```
