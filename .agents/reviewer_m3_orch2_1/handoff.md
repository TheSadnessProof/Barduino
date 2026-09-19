# Milestone 3 Review & Adversarial Audit Report

**Agent**: `reviewer_m3_orch2_1`  
**Roles**: Reviewer, Adversarial Critic  
**Working Directory**: `c:\Users\ditob\Documents\viper\.agents\reviewer_m3_orch2_1`  
**Milestone**: Milestone 3 — Per-Session Lifecycle, Switching & Saved State (`src/app.rs`)  
**Verdict**: **APPROVE**  

---

## 1. Observation

### 1.1 Source Code Architecture in `src/app.rs`
1. **Ephemeral PTY Map Storage** (`src/app.rs:144`):
   ```rust
   provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>,
   ```
   `provider_terminals` resides exclusively on `ViperApp` and is strictly omitted from `SavedState` (`src/app.rs:41-61`).
2. **Session Switching & Keyboard Focus Routing** (`src/app.rs:457-461`):
   ```rust
   SidebarAction::Select(id) => {
       self.view = View::Chat;
       self.state.active_session = id;
       self.active_session_mut().focus_composer = true;
   }
   ```
3. **Focus Flag Consumption** (`src/app.rs:606-617`):
   ```rust
   let take_keyboard = std::mem::take(&mut session.focus_composer);
   TerminalState::Ready {
       session_id: session.id,
       ...
       take_keyboard,
   }
   ```
   And `terminal_area` passes `take_keyboard` to `terminal.ui(ui, take_keyboard)` (`src/app.rs:705`), which invokes `response.request_focus()` and sets egui focus lock filter for Tab, arrows, and Escape (`src/terminal.rs:366-377`).
4. **Process Teardown on Deletion** (`src/app.rs:364-381`):
   ```rust
   let removed = self.state.sessions.remove(index);
   self.tools.remove(&id);
   self.provider_terminals.remove(&id);
   ...
   } else if self.state.active_session == id {
       let next = &mut self.state.sessions[index.saturating_sub(1)];
       next.focus_composer = true;
       self.state.active_session = next.id;
   }
   ```
   Dropping the `Terminal` in `provider_terminals.remove(&id)` drops `TerminalJob` (`src/terminal.rs:119-125`), terminating the child process tree via Windows `TerminateJobObject(job, 1)` or Unix `kill(pid, SIGKILL)` and `self.child.kill()`.
5. **Folder Change Eviction** (`src/app.rs:445-446`):
   ```rust
   self.tools.remove(&id);
   self.provider_terminals.remove(&id);
   ```
   Clearing `provider_terminals` ensures the subsequent frame resolves the new `cwd` and re-spawns lazily.
6. **Spawn Failure Retry** (`src/app.rs:707-724`):
   ```rust
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
   ...
   if restart {
       self.provider_terminals.remove(&session_id);
       ui.ctx().request_repaint();
   }
   ```
7. **Disjoint Scope and Widget IDs** (`src/app.rs:702` vs `src/app.rs:805`):
   - Middle terminal: `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)`
   - Auxiliary tools panel: `egui::Panel::right(egui::Id::new("tools_panel"))`, tab IDs `("terminal", number)` and `("tool_tab", index)`.

### 1.2 Verification Test Suite Additions (`src/app.rs::tests`)
Observed 8 dedicated tests added by `worker_m3_orch2`:
- `multi_session_switching_preserves_terminals_in_map_and_transfers_focus` (`src/app.rs:1841`)
- `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal` (`src/app.rs:1912`)
- `folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd` (`src/app.rs:1973`)
- `failed_terminal_spawn_retry_action_clears_error_and_allows_respawn` (`src/app.rs:2042`)
- `saved_state_ron_serialization_omits_provider_terminals` (`src/app.rs:2121`)
- `comprehensive_legacy_ron_state_loads_and_carries_defaults` (`src/app.rs:2150`)
- `restart_restores_session_and_spawns_cli_lazily_in_session_folder` (`src/app.rs:2237`)
- `middle_provider_terminal_and_tools_panel_coexist_without_interference` (`src/app.rs:2284`)

### 1.3 Execution Tool Outputs
- `cargo check`:
  `Finished 'dev' profile [unoptimized + debuginfo] target(s) in 0.46s` (Exit code: 0)
- `cargo test -- multi_session_switching deleting_active_session folder_change_clears failed_terminal_spawn saved_state_ron comprehensive_legacy restart_restores middle_provider_terminal`:
  `test result: ok. 10 passed; 0 failed; 0 ignored; finished in 0.64s` (Exit code: 0)
- `cargo test` (Entire test suite):
  `test result: ok. 284 passed; 0 failed; 8 ignored; finished in 3.92s` (Exit code: 0; 8 ignored tests preserved per Rule 3.2).
- `cargo clippy --all-targets -- -D warnings`:
  `Finished 'dev' profile [unoptimized + debuginfo] target(s) in 0.36s` (0 warnings, Exit code: 0).
- `git diff Cargo.toml`: Empty (Zero dependencies added, conforms to Rule 3.4).
- `git diff src/app.rs`: Exactly 8 new unit tests added, no unrelated reformatting (conforms to Rules 3.3 and 3.6).

---

## 2. Logic Chain

1. **In-Memory Terminal Longevity Across Switches (Observations 1.1.1, 1.1.2, 1.2.1)**:
   - `provider_terminals` maps `session_id -> Result<Terminal, String>`.
   - In `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`, session 1 is rendered, spawning terminal at address `term1_addr`.
   - Switching to session 2 renders session 2's terminal while session 1 remains alive in `provider_terminals[&1]`.
   - Switching back to session 1 invokes `provider_terminals.entry(1)`, reusing the terminal at `term1_addr` without re-spawning.
   - Focus is primed with `session.focus_composer = true` on `SidebarAction::Select` and consumed during `resolve_terminal_state` to immediately request widget focus in `terminal.ui`.

2. **Clean Process Teardown & Focus Handoff on Deletion (Observations 1.1.4, 1.2.2)**:
   - When session 2 is deleted among sessions [1, 2, 3], `provider_terminals.remove(&2)` is executed.
   - Dropping `Terminal` triggers `TerminalJob::drop`, which calls Windows `TerminateJobObject` or Unix process group kill, eliminating orphaned child processes.
   - `delete_session` clamps `active_session` to index 0 (session 1) and sets `focus_composer = true`.
   - Session 3's terminal remains active in `provider_terminals[&3]` and continues running (`!has_exited()`).

3. **Folder Change Teardown and Lazy Re-spawning (Observations 1.1.5, 1.2.3)**:
   - Changing `project_dir` removes the old terminal from `provider_terminals`.
   - The subsequent frame invokes `resolve_terminal_state`, which derives `cwd = dir2` and sets `take_keyboard = true`.
   - `terminal_area` lazily calls `Terminal::start_command` in `dir2`.

4. **Interactive Error Recovery via "Try Again" (Observations 1.1.6, 1.2.4)**:
   - When a spawn error occurs (`Err(err)`), `terminal_area` draws an error banner and "Try again" button.
   - Emulating pointer click triggers `restart = true`.
   - `self.provider_terminals.remove(&session_id)` clears the error and requests repaint, allowing a clean spawn on the subsequent frame.

5. **RON Backward Compatibility & SavedState Hygiene (Observations 1.1.1, 1.2.5, 1.2.6, 1.2.7)**:
   - `SavedState` does not include `provider_terminals`.
   - `saved_state_ron_serialization_omits_provider_terminals` confirms `ron::to_string(&app.state)` contains no terminal handles.
   - `comprehensive_legacy_ron_state_loads_and_carries_defaults` verifies legacy RON strings (containing `Gemini`, `claude_session_id`, missing modern fields) deserialize smoothly with expected defaults and aliases.
   - `restart_restores_session_and_spawns_cli_lazily_in_session_folder` verifies that `ViperApp` boots with an empty `provider_terminals` map and lazily spawns the CLI upon first frame render.

6. **Tools Panel Coexistence & ID Isolation (Observations 1.1.7, 1.2.8)**:
   - Middle terminal uses `id_salt(("session_terminal", session_id))` while right panel uses `("tools_panel")` and `("terminal", number)`.
   - Rendering both simultaneously and switching tools panel tabs leaves the middle session's PTY pointer address completely unchanged and unperturbed.

7. **Integrity Forensics Assessment**:
   - Zero hardcoded test outputs or mock bypasses detected.
   - Tests spawn real OS processes (`cmd.exe`/`/bin/sh`), drive real egui event pipelines, and verify actual pointer addresses and process states.
   - Zero clippy warnings, zero compilation errors, zero unauthorized dependency additions.

---

## 3. Caveats

- **Headless egui Execution**: UI rendering tests use headless egui harnesses (`run_ui_test`, `run_ui`, `eframe::Frame::_new_kittest()`) in strict accordance with `AGENTS.md` Rule 3.1 (no driving global mouse/keyboard, no screenshots).
- **Native CLI In-Process Emulation**: In unit tests, standard platform shells (`cmd.exe` or `/bin/sh`) are configured as custom executables for providers to verify PTY lifecycle deterministically without triggering external network calls or paid API charges.

---

## 4. Conclusion

Milestone 3 (Per-Session Lifecycle, Switching & Saved State) in `src/app.rs` meets all functional, architectural, and quality requirements. The implementation correctly handles multi-session lifecycle, process cleanup on deletion, folder switching, error retries, RON backward compatibility, and auxiliary tools coexistence without regression.

**Verdict**: **APPROVE**

---

## 5. Verification Method

To independently reproduce and verify this review:

```powershell
# 1. Verify compilation
cargo check

# 2. Run the 8 Milestone 3 lifecycle tests
cargo test -- multi_session_switching deleting_active_session folder_change_clears failed_terminal_spawn saved_state_ron comprehensive_legacy restart_restores middle_provider_terminal

# 3. Run entire test suite (284 tests pass, 8 ignored preserved per Rule 3.2)
cargo test

# 4. Enforce zero clippy warnings
cargo clippy --all-targets -- -D warnings

# 5. Verify git diff cleanliness
git diff src/app.rs
git diff Cargo.toml
```
