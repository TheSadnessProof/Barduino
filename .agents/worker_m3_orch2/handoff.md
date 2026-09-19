# Milestone 3 Implementation & Verification Report

**Agent**: `worker_m3_orch2`  
**Working Directory**: `c:\Users\ditob\Documents\viper\.agents\worker_m3_orch2`  
**Milestone**: Milestone 3 — Per-Session Lifecycle, Switching & Saved State  
**Status**: Complete  

---

## 1. Observation

### Source Code Inspection & Target File Modifications
Only `src/app.rs` was modified, strictly observing exclusive file ownership. The production implementations for session terminal lifecycle in `src/app.rs`:
- **`provider_terminals` storage** (`src/app.rs:144`):
  `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>`
  Keeps running PTY instances keyed by session ID without inclusion in `SavedState`.
- **Session Switching & Focus Transfer** (`src/app.rs:457-461`):
  `SidebarAction::Select(id)` sets `self.state.active_session = id` and `self.active_session_mut().focus_composer = true`.
- **Focus Consumption** (`src/app.rs:595-618`):
  `resolve_terminal_state` consumes `std::mem::take(&mut session.focus_composer)` as `take_keyboard: bool` in `TerminalState::Ready`.
- **Lazy Terminal Spawning** (`src/app.rs:687-698`):
  `terminal_area` uses `self.provider_terminals.entry(session_id).or_insert_with(...)` to lazily spawn the CLI in `cwd` with `Terminal::start_command`.
- **Process Teardown on Session Deletion** (`src/app.rs:360-382`):
  `delete_session` executes `self.provider_terminals.remove(&id)` (invoking `Terminal::drop`, which kills child processes via Windows Job Object or Unix process group kill), selects the neighbor session (`index.saturating_sub(1)`), and sets `next.focus_composer = true`.
- **Spawn Error Retry** (`src/app.rs:707-724`):
  When `terminal_entry` is `Err(err)`, clicking "Try again" sets `restart = true`, removing `session_id` from `self.provider_terminals` and requesting repaint.
- **Coexistence with Tools Panel** (`src/app.rs:801-867`):
  `right_panel` renders secondary shells, git diffs, and browser previews under disjoint IDs (`("tools_panel")`, `("terminal", number)`, `("tool_tab", index)`), completely isolated from `id_salt(("session_terminal", session_id))`.

### Verification Suite Additions
Implemented 8 dedicated, sentence-named unit tests at the bottom of `src/app.rs::tests`:
1. `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`
2. `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`
3. `folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd`
4. `failed_terminal_spawn_retry_action_clears_error_and_allows_respawn`
5. `saved_state_ron_serialization_omits_provider_terminals`
6. `comprehensive_legacy_ron_state_loads_and_carries_defaults`
7. `restart_restores_session_and_spawns_cli_lazily_in_session_folder`
8. `middle_provider_terminal_and_tools_panel_coexist_without_interference`

### Command Tool Verification Outputs
- `cargo check`: Finished in 0.31s with exit code 0.
- `cargo test`:
  `test result: ok. 284 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 4.09s`
  (All 8 new unit tests pass; all 276 pre-existing tests pass; 8 ignored tests preserved per Rule 3.2).
- `cargo clippy --all-targets -- -D warnings`: Finished with 0 warnings.
- `git diff src/app.rs`: Only the 8 new test functions added to `src/app.rs::tests`; zero unrelated lines or formatting changes introduced.

---

## 2. Logic Chain

1. **Multi-Session Switching & Terminal Preservation**:
   - `s1` and `s2` are initialized in `state.sessions`. Rendering `terminal_area` with `s1` active creates a PTY in `provider_terminals[&1]` and consumes `focus_composer`.
   - Switching via `SidebarAction::Select(2)` sets `active_session = 2` and primes `s2.focus_composer = true`.
   - Rendering `terminal_area` creates `provider_terminals[&2]` while `provider_terminals[&1]` remains in-memory, alive, and running.
   - Switching back via `SidebarAction::Select(1)` sets `s1.focus_composer = true`. Rendering `terminal_area` hits `provider_terminals.entry(1)`, which already exists. It reuses the exact same terminal without re-spawning, and consumes the focus flag.

2. **Session Deletion & Neighbor Focus Handoff**:
   - Spawning terminals for sessions [1, 2, 3] populates entries 1, 2, and 3 in `provider_terminals`.
   - Deleting active session 2 via `SidebarAction::Delete(2)` invokes `delete_session(2)`.
   - `self.provider_terminals.remove(&2)` drops `Terminal`, triggering `TerminalJob::drop` (terminating the child process tree via Windows `TerminateJobObject(job, 1)` or Unix SIGTERM/SIGKILL) and closing PTY pipes.
   - `delete_session` clamps `active_session` to index 0 (session 1), setting `focus_composer = true`.
   - Session 3's terminal in `provider_terminals[&3]` remains untouched and running.

3. **Folder Change Teardown and Re-spawning**:
   - Changing `project_dir` to a new directory (`dir2`) and evicting `provider_terminals.remove(&10)` simulates folder updating.
   - The subsequent frame calls `resolve_terminal_state`, which resolves `cwd = dir2` with `take_keyboard = true`.
   - `terminal_area` finds entry 10 vacant and lazily spawns a fresh `Terminal::start_command` rooted in `dir2`.

4. **Retry on Spawn Failure**:
   - Injecting `provider_terminals.insert(51, Err("simulated failure".into()))` puts the terminal into an error state.
   - In egui, rendering `terminal_area` displays the error label and "Try again" button.
   - Passing pointer events (warm-up layout -> pointer press -> pointer release) at the button coordinates triggers `ui.button("Try again").clicked()`.
   - This sets `restart = true`, evicting the error entry from `provider_terminals`.
   - On the next frame, `terminal_area` re-evaluates `provider_terminals.entry(51)` and cleanly spawns a valid terminal command.

5. **RON Backward Compatibility & State Cleanliness**:
   - `app.state` (`SavedState`) serializes to a clean RON document with zero references to `provider_terminals`.
   - Deserializing a comprehensive legacy RON fixture containing `Gemini`, `claude_session_id`, obsolete settings, and missing modern fields cleanly maps `Gemini` -> `Provider::Antigravity`, `claude_session_id` -> `agent_session_id`, and applies panel defaults (`apply_panel_defaults = true`).
   - Restoring across restarts initializes `provider_terminals` empty, and the first frame lazily launches the CLI in the session's folder.

6. **Tools Panel Coexistence & Widget ID Disjointness**:
   - Rendering `app.terminal_area(ui)` and `app.right_panel(ui, &frame)` in the same frame exercises both middle and right-panel rendering pipelines.
   - Middle terminal (`id_salt(("session_terminal", session_id))`) and tools panel secondary shells (`push_id(("terminal", number))`) and diffs (`push_id("diff", ...))`) operate without widget collisions or panics.
   - Switching between secondary tools tabs (Terminal -> Changes diff) leaves the active middle session's `provider_terminals` entry completely intact.

---

## 3. Caveats

- **Headless UI Simulation**: In accordance with `AGENTS.md` Rule 3.1, tests do not capture displays or launch real interactive GUI windows. Tests simulate egui frame execution via `run_ui_test`, `run_ui`, and `eframe::Frame::_new_kittest()`.
- **External Dialogs**: Modal folder pickers (`rfd::FileDialog`) cannot be driven headlessly; programmatic path assignment and terminal eviction faithfully model `change_folder`.

---

## 4. Conclusion

Milestone 3 is fully implemented, verified, and passing:
- Per-session terminal lifecycle and map storage in `provider_terminals` operate correctly.
- Multi-session switching preserves existing terminals in place and hands off keyboard focus seamlessly.
- Deleting an active session drops and terminates its process tree, preserves other sessions, and transfers focus to the neighbor.
- Error retry clears failure states and respawns cleanly.
- SavedState serializes and deserializes with 100% backward compatibility and zero terminal leaks.
- Middle terminal and auxiliary tools panel coexist without ID collisions or state interference.
- The entire test suite passes (284 tests passed, 0 failures, 8 ignored) with zero clippy warnings.

---

## 5. Verification Method

To independently verify the implementation:

```powershell
# 1. Compilation check
cargo check

# 2. Run all Milestone 3 lifecycle tests
cargo test -- multi_session_switching deleting_active_session folder_change_clears failed_terminal_spawn saved_state_ron comprehensive_legacy restart_restores middle_provider_terminal

# 3. Run entire test suite (strictly omitting wholesale ignored tests per Rule 3.2)
cargo test

# 4. Strict clippy lint verification
cargo clippy --all-targets -- -D warnings

# 5. Git diff inspection
git diff src/app.rs
```
