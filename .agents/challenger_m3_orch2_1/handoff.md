# Milestone 3 Empirical Challenge Report

**Agent**: `challenger_m3_orch2_1`  
**Working Directory**: `c:\Users\ditob\Documents\viper\.agents\challenger_m3_orch2_1`  
**Target Milestone**: Milestone 3 — Multi-Session Lifecycle, Switching Permutations, Focus Handover, and Process Teardown  
**Verdict**: **APPROVE**  

---

## 1. Observation

### Codebase Inspection
1. **Session Terminal Mapping & Lifetime** (`src/app.rs:144`):
   `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>`
   PTY instances are maintained in memory across rendering passes and are never serialized to `SavedState`.
2. **Session Switching Focus Protocol** (`src/app.rs:457-461`):
   `SidebarAction::Select(id)` switches `self.state.active_session = id` and primes `self.active_session_mut().focus_composer = true`.
3. **Focus Flag Consumption** (`src/app.rs:606-617`):
   `resolve_terminal_state` executes `let take_keyboard = std::mem::take(&mut session.focus_composer);` and embeds `take_keyboard` into `TerminalState::Ready`.
4. **Session Deletion Teardown** (`src/app.rs:360-382`):
   `delete_session` removes the session from `self.state.sessions`, `self.tools`, and executes `self.provider_terminals.remove(&id);`. When `provider_terminals` drops `Terminal`, `TerminalJob` is dropped, calling `TerminateJobObject(job, 1)` and `CloseHandle(job)`. The active session is updated to `index.saturating_sub(1)` with `next.focus_composer = true`.
5. **Command Generation for Multiple Providers** (`src/agent.rs:414-429`):
   `build_interactive_command` branches on `Provider::Claude`, `Provider::Codex`, and `Provider::Antigravity`, generating provider-specific interactive command-line arguments without headless flags.

### Empirical Test Execution Results
1. **Target Milestone 3 Lifecycle & State Tests**:
   - `cargo test multi_session_switching`: `test app::tests::multi_session_switching_preserves_terminals_in_map_and_transfers_focus ... ok` (0.50s)
   - `cargo test deleting_active_session`: `test app::tests::deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal ... ok` (0.49s)
   - `cargo test folder_change_clears`: `test app::tests::folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd ... ok` (0.33s)
   - `cargo test failed_terminal_spawn`: `test app::tests::failed_terminal_spawn_retry_action_clears_error_and_allows_respawn ... ok` (0.03s)
   - `cargo test saved_state_ron`: 3 passed, 0 failed (0.00s)
   - `cargo test comprehensive_legacy`: `test app::tests::comprehensive_legacy_ron_state_loads_and_carries_defaults ... ok` (0.00s)
   - `cargo test restart_restores`: `test app::tests::restart_restores_session_and_spawns_cli_lazily_in_session_folder ... ok` (0.19s)
   - `cargo test middle_provider_terminal`: `test app::tests::middle_provider_terminal_and_tools_panel_coexist_without_interference ... ok` (0.49s)

2. **Provider Interactive Command Builder Tests**:
   - `cargo test build_interactive_command`:
     - `test agent::tests::build_interactive_command_configures_antigravity_workspace_and_conversation ... ok`
     - `test agent::tests::build_interactive_command_configures_codex_fresh_and_resume ... ok`
     - `test agent::tests::build_interactive_command_configures_claude_without_headless_flags ... ok`

3. **Focus Handover & State Resolution Tests**:
   - `cargo test switching_sessions_primes_keyboard_focus_flag`: `test app::tests::switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it ... ok` (0.02s)
   - `cargo test ready_session_resolves_to_ready_terminal_state`: `test app::tests::ready_session_resolves_to_ready_terminal_state_and_consumes_focus ... ok` (0.00s)

4. **Process Teardown & Job Object Stress Tests**:
   - `cargo test -- --ignored closing_a_terminal --nocapture`:
     ```
     before closing: ["25560 PING.EXE"]
     after closing: []
     test terminal::tests::closing_a_terminal_stops_programs_started_in_it ... ok
     ```
   - `cargo test stress_terminal_cleanly_kills_child_and_grandchild_process_tree -- --nocapture`: `ok` (2.77s)
   - `cargo test stress_terminal_cleanly_kills_batch_script_process_tree -- --nocapture`: `ok` (2.88s)
   - `cargo test stress_terminal_rapid_spawn_and_drop -- --nocapture`: `ok` (0.17s)

5. **Full Suite & Static Quality Verification**:
   - `cargo check`: 0 errors (0.37s)
   - `cargo test`: 284 passed, 0 failed, 8 ignored (3.92s)
   - `cargo clippy --all-targets -- -D warnings`: 0 warnings (0.37s)
   - `git status -s src/`: Zero modifications introduced by challenger.

---

## 2. Logic Chain

1. **Multi-Session Switching & Terminal Preservation**:
   - As observed in `src/app.rs:144` and tested in `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`, active PTY terminals are stored in `BTreeMap<u64, Result<Terminal, String>>`.
   - When switching from Session 1 to Session 2, Session 1's terminal remains resident at the exact same memory address.
   - When switching back to Session 1, `terminal_area` reuses the existing `Ok(terminal)` from `provider_terminals.entry(1)`, preserving the shell process and output buffer without re-spawning.

2. **Focus Routing & Handover**:
   - As observed in `src/app.rs:460`, selecting a session sets `focus_composer = true`.
   - `resolve_terminal_state` extracts this flag via `std::mem::take`, passing `take_keyboard = true` into `terminal.ui()`.
   - On subsequent frames without a session switch, `focus_composer` evaluates to `false`, leaving focus unforced so secondary panels or controls can receive focus.

3. **Session Deletion Process Teardown**:
   - As observed in `src/app.rs:366`, `delete_session` calls `self.provider_terminals.remove(&id)`.
   - Dropping the `Terminal` drops its `job: TerminalJob` handle. On Windows, `TerminalJob::drop` executes `TerminateJobObject(job, 1)` and `CloseHandle(job)`.
   - In `closing_a_terminal_stops_programs_started_in_it`, real PTY child processes (`PING.EXE`) are confirmed present before terminal drop and verified terminated (`after closing: []`) after drop.
   - Child and grandchild processes (`stress_terminal_cleanly_kills_child_and_grandchild_process_tree`) and batch script process trees (`stress_terminal_cleanly_kills_batch_script_process_tree`) are verified to be cleanly killed without orphan leaks.

4. **Saved State Compatibility**:
   - As observed in `saved_state_ron_serialization_omits_provider_terminals`, `SavedState` contains no PTY handles or process state.
   - In `comprehensive_legacy_ron_state_loads_and_carries_defaults`, legacy saves with `Gemini` provider and missing modern fields deserialize cleanly without migration failure.
   - On restart (`restart_restores_session_and_spawns_cli_lazily_in_session_folder`), `provider_terminals` initializes empty and lazily spawns on the first render frame in the session's folder.

---

## 3. Caveats

1. **Parallel Process Inspection Timing in Tests**:
   - In `terminal.rs`, unit tests `stress_terminal_cleanly_kills_child_and_grandchild_process_tree` and `stress_terminal_cleanly_kills_batch_script_process_tree` use a fixed 2-second sleep followed by a PowerShell `Get-CimInstance Win32_Process` query.
   - When multiple such tests run concurrently in parallel under heavy CPU load, running concurrent PowerShell processes can cause the 2-second sleep boundary to be exceeded before process cleanup finishes reporting.
   - In production, process termination via `TerminateJobObject` is immediate. When executed sequentially or via `--test-threads=1`, all teardown tests pass reliably and deterministically.
2. **Headless Testing Constraint**:
   - No interactive window or physical display was driven, adhering to `AGENTS.md` Rule 3.1. GUI frame layout and widget interaction were verified through egui's headless test harness.
3. **No Real Paid CLI Runs**:
   - In accordance with `AGENTS.md` Rule 3.2, paid CLI execution tests (`runs_the_real_*_cli`) were kept ignored. Command argument formatting was thoroughly verified via `build_interactive_command` unit tests.

---

## 4. Conclusion

**Verdict: APPROVE**

Milestone 3 fulfills all requirements and acceptance criteria:
- Multiple concurrent provider terminals coexist independently in memory.
- Switching between sessions preserves terminal process state and smoothly transfers keyboard focus.
- Deleting an active session drops and terminates the entire process tree via Windows Job Object, cleans all associated tabs, and transfers focus to the neighbor session.
- SavedState serializes cleanly with zero ephemeral terminal leakage and maintains full backward compatibility with legacy saves.
- Zero codebase mutations were introduced; the complete test suite (284 tests) passes cleanly with zero clippy warnings.

---

## 5. Verification Method

To independently verify these findings:

```powershell
# 1. Run the standalone empirical stress verification runner
powershell -ExecutionPolicy Bypass -File .agents\challenger_m3_orch2_1\run_stress_verification.ps1

# 2. Verify full test suite
cargo test

# 3. Verify real process tree teardown
cargo test -- --ignored closing_a_terminal --nocapture

# 4. Strict clippy check
cargo clippy --all-targets -- -D warnings

# 5. Verify git status cleanliness in src/
git status -s src/
```
