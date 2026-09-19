# Empirical Challenge & Stress Test Report: Milestone 4

**Agent ID**: challenger_m4_orch2_1  
**Milestone**: Milestone 4 — Final Integration Verification & Integrity Audit  
**Date**: 2026-09-19  
**Verdict**: **APPROVE**

---

## 1. Observation

### 1.1 Compilation & Clippy Invariants
- `cargo check`:
  ```
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s
  ```
  0 errors, 0 warnings.
- `cargo clippy --all-targets -- -D warnings`:
  ```
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.34s
  ```
  0 warnings across all crate targets and unit tests.

### 1.2 Full Unit Test Suite (`cargo test`)
- Command: `cargo test`
  ```
  test result: ok. 284 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 3.71s
  ```
- Ignored tests verified (all 8 are pre-existing tests documented in `AGENTS.md` Rule 3.2; zero newly ignored tests):
  1. `src/terminal.rs:842` — `terminal::tests::closing_a_terminal_stops_programs_started_in_it`
  2. `src/terminal.rs:863` — `terminal::tests::finds_the_shells_on_this_computer`
  3. `src/codex.rs:731` — `codex::tests::reads_codex_limits_from_this_computer`
  4. `src/plan.rs:510` — `plan::tests::checks_real_plan_limits`
  5. `src/plan.rs:538` — `plan::tests::full_access_really_runs_commands`
  6. `src/agent.rs:774` — `agent::tests::runs_the_real_antigravity_cli`
  7. `src/agent.rs:859` — `agent::tests::runs_the_real_codex_cli`
  8. `src/models.rs:211` — `models::tests::real_models_come_from_the_clis`

### 1.3 Safe Ignored Test Execution (`closing_a_terminal`)
- Command: `cargo test -- --ignored closing_a_terminal --nocapture`
  ```
  before closing: ["23112 PING.EXE"]
  after closing: []
  test terminal::tests::closing_a_terminal_stops_programs_started_in_it ... ok

  test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 291 filtered out; finished in 3.39s
  ```
  Empirically verifies that closing a terminal terminates child and grandchild processes (`PING.EXE`) using Windows Job Objects.

### 1.4 Interactive Command Builders (`cargo test interactive`)
- Command: `cargo test interactive`
  ```
  test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 278 filtered out; finished in 0.00s
  ```
  Verified across Claude, Codex, and Antigravity:
  - Working directory propagation (`-C <cwd>` for Codex, `--add-dir <cwd>` for Antigravity, process working directory for Claude).
  - All four `PermissionMode` variants correctly mapped (`ReadOnly`, `AcceptEdits`, `Full`, `Plan`).
  - Session resume flags properly formatted (`--resume <id>`, `resume <id>`, `--conversation <id>`).
  - Model selection and reasoning effort flags (`--model`, `--effort`, `-m`, `-c model_reasoning_effort=<effort>`).
  - Strict omission of headless-only flags (`--output-format`, `stream-json`, `exec`, `--json`, `-`, `--print`, `-p`).

### 1.5 Terminal Stress Tests (`cargo test terminal::tests::stress`)
- Command: `cargo test terminal::tests::stress`
  ```
  test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 286 filtered out; finished in 2.70s
  ```
  Tests verified:
  - `stress_terminal_cleanly_kills_child_and_grandchild_process_tree`: Confirms process tree cleanup on `Terminal::drop`.
  - `stress_terminal_cleanly_kills_batch_script_process_tree`: Confirms Windows batch script (`.bat`) wrapping and process tree kill on drop.
  - `stress_terminal_failure_mode_invalid_directory`: Confirms graceful error handling for nonexistent directory or file-as-cwd.
  - `stress_terminal_failure_mode_nonexistent_executable`: Confirms graceful error handling for missing executable.
  - `stress_terminal_receives_term_and_colorterm_in_child_process`: Confirms injection of `TERM=xterm-256color` and `COLORTERM=truecolor`.
  - `stress_terminal_rapid_spawn_and_drop`: Confirms 20 consecutive spawn and drop cycles without handle leaks or deadlocks.

### 1.6 App Lifecycle & RON Compatibility (`cargo test app::tests`)
- Command: `cargo test app::tests`
  ```
  test result: ok. 36 passed; 0 failed; 0 ignored; 0 measured; 256 filtered out; finished in 1.08s
  ```
  Key tests passing:
  - `central_view_renders_terminal_area_without_chat_composer`: Middle panel renders interactive terminal instead of chat transcript/composer.
  - `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`: Switching sessions preserves terminal buffers and routes keyboard focus without mouse click.
  - `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`: Deletion cleanly terminates CLI process and purges terminal map entry.
  - `saved_state_ron_serialization_omits_provider_terminals`: Ephemeral `provider_terminals` map is excluded from RON serialization.
  - `comprehensive_legacy_ron_state_loads_and_carries_defaults`: Old saved states deserialize cleanly with all legacy aliases preserved.

### 1.7 Clean Git Status in `src/`
- Command: `git status -s src/`
  ```
   M src/agent.rs
   M src/antigravity.rs
   M src/app.rs
   M src/chat.rs
   M src/claude.rs
   M src/codex.rs
   M src/terminal.rs
  ```
- Command: `git status Cargo.toml Cargo.lock` -> Clean. No new dependencies added.
- Zero untracked files or scratch files in `src/`.

---

## 2. Logic Chain

1. **Requirement R1 (Dedicated Provider Terminal View)**:
   In `src/app.rs`, `CentralPanel::default().show` routes `View::Chat` to `self.terminal_area(ui)`. The old message transcript and composer bubbles are completely replaced with `Terminal::ui`. Verified empirically by `central_view_renders_terminal_area_without_chat_composer`.

2. **Requirement R2 (Direct Interactive Provider CLI Execution)**:
   `build_interactive_command` in `src/agent.rs` constructs interactive execution arguments for Claude, Codex, and Antigravity. Windows batch script handling (`cmd.exe /c`) prevents ConPTY error 193. `Terminal::start_command` initializes PTY with `TERM=xterm-256color` and `COLORTERM=truecolor`. Verified empirically by 14 unit tests in `agent::tests` and 6 stress tests in `terminal::tests`.

3. **Requirement R3 (Per-Session Terminal State and Lifecycle Management)**:
   `ViperApp` owns `provider_terminals: BTreeMap<u64, Result<Terminal, String>>`.
   - Each session maintains its own terminal buffer and PTY process.
   - Switching sessions sets `session.focus_composer = true`, transferred to `terminal.ui(ui, take_keyboard)`.
   - Session deletion in `delete_session` drops `Terminal`, triggering `TerminalJob::kill()` and `Child::kill()`, killing all subprocesses.
   - Serialization to RON completely omits `provider_terminals`, ensuring persistent state compatibility.
   Verified empirically by `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`, `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`, and `saved_state_ron_serialization_omits_provider_terminals`.

4. **Requirement R4 (Sidebar and Auxiliary Tools Integration)**:
   The sidebar (left) and tools panel (right) remain fully functional. Secondary terminals, git diffs, and live preview browser coexist without state collisions or PTY interference. Verified empirically by `middle_provider_terminal_and_tools_panel_coexist_without_interference`.

5. **Stability and Invariants**:
   Zero compiler warnings, zero clippy warnings (`-D warnings`), zero added dependencies, and zero newly ignored tests.

---

## 3. Caveats

- **Visual Observation**: In strict accordance with `AGENTS.md` Rule 3.1, no full-screen captures or mouse/keyboard hijacking was performed. UI rendering and focus transitions were verified through headless `run_ui_test` egui harnesses.
- **Paid Ignored Tests**: The account-spending integration tests (`runs_the_real_*`) remain ignored per `AGENTS.md` Rule 3.2. Only the free, local `closing_a_terminal_stops_programs_started_in_it` test was executed.

---

## 4. Conclusion

**Verdict: APPROVE**.
The implementation for Milestone 4 satisfies all user requirements and acceptance criteria in `ORIGINAL_REQUEST.md` (§ 2026-09-19T00:31:41Z) and strictly respects all invariants in `AGENTS.md`. The test suite is fully passing (284 passed, 0 failed, 8 pre-existing ignored), process tree cleanup is robust, and the codebase is completely clean.

---

## 5. Verification Method

To independently verify all findings, run the following commands:

```powershell
# 1. Compiler and Clippy
cargo check
cargo clippy --all-targets -- -D warnings

# 2. Complete Unit Test Suite
cargo test

# 3. Process Tree Cleanup Ignored Test
cargo test -- --ignored closing_a_terminal --nocapture

# 4. Interactive Builders & Terminal Stress Tests
cargo test interactive
cargo test terminal::tests::stress
cargo test app::tests

# 5. Git Status Invariance
git status -s src/
git status Cargo.toml
```
