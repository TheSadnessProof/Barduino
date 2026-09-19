# Milestone 3 Independent & Adversarial Review Report

**Agent**: `reviewer_m3_orch2_2`  
**Working Directory**: `c:\Users\ditob\Documents\viper\.agents\reviewer_m3_orch2_2`  
**Target Milestone**: Milestone 3 — Per-Session Lifecycle, Switching & Saved State (`src/app.rs`)  
**Verdict**: **APPROVE**  
**Overall Risk Assessment**: LOW  

---

## 1. Observation

### Codebase Inspection
1. **Focus Routing & One-Shot Consumption**:
   - In `src/app.rs:606`:
     ```rust
     let take_keyboard = std::mem::take(&mut session.focus_composer);
     ```
   - In `src/terminal.rs:366-368`:
     ```rust
     if response.clicked() || take_keyboard {
         response.request_focus();
     }
     ```
   - `std::mem::take` consumes the flag atomically, replacing it with `false`. On subsequent frames, `take_keyboard` evaluates to `false`, preventing continuous focus stealing.

2. **Session Teardown & Process Tree Termination**:
   - In `src/app.rs:366`:
     ```rust
     self.provider_terminals.remove(&id);
     ```
   - In `src/terminal.rs:521-526`:
     ```rust
     impl Drop for Terminal {
         fn drop(&mut self) {
             self.job.kill();
             let _ = self.child.kill();
         }
     }
     ```
   - In `src/terminal.rs:119-126`:
     ```rust
     #[cfg(windows)]
     impl Drop for TerminalJob {
         fn drop(&mut self) {
             if let Some(job) = self.0.take() {
                 let _ = unsafe { windows::Win32::System::JobObjects::TerminateJobObject(job, 1) };
                 let _ = unsafe { windows::Win32::Foundation::CloseHandle(job) };
             }
         }
     }
     ```
   - Windows Job Objects configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` terminate all descendant processes when closed or terminated, without blocking UI threads.

3. **Graceful Handling of Exited CLIs**:
   - In `src/terminal.rs:331-334`:
     ```rust
     pub fn has_exited(&mut self) -> bool {
         self.exited.load(Ordering::Relaxed) || matches!(self.child.try_wait(), Ok(Some(_)))
     }
     ```
   - In `src/terminal.rs:346-351`:
     ```rust
     if self.has_exited() {
         ui.horizontal(|ui| {
             ui.label(egui::RichText::new("The process has exited.").weak());
             restart = ui.button("Restart").clicked();
         });
     }
     ```
   - In `src/terminal.rs:610-614`:
     ```rust
     fn write_all(writer: &Writer, bytes: &[u8]) {
         let mut writer = writer.lock().unwrap_or_else(PoisonError::into_inner);
         let _ = writer.write_all(bytes);
         let _ = writer.flush();
     }
     ```
   - Writing to an exited process swallows errors via `let _ =`, and the terminal presents a "Restart" button that triggers re-spawning without crashing or panicking.

4. **SavedState Serialization and Error Recovery**:
   - In `src/app.rs:41-61`: `SavedState` derives `Serialize, Deserialize` with `#[serde(default)]` on every field. `provider_terminals` is not in `SavedState` (ephemeral only).
   - In `src/app.rs:175-206`: `ViperApp::new` catches unreadable saves, backs them up to `app.ron.corrupt`, attempts recovery from `app.ron.bak`, and gracefully falls back to `SavedState::default()` with a user-facing banner.

5. **Disjoint Widget ID Spaces**:
   - Middle terminal: `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)` inside `CentralPanel`.
   - Tools panel secondary shells: `ui.push_id(("terminal", *number), ...)` inside `Panel::right("tools_panel")`.
   - Scopes, salt tuples, and parent panels are completely disjoint, preventing egui ID collisions.

6. **8 Unit Tests Implemented by worker_m3_orch2**:
   - `src/app.rs:1841`: `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`
   - `src/app.rs:1912`: `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`
   - `src/app.rs:1973`: `folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd`
   - `src/app.rs:2042`: `failed_terminal_spawn_retry_action_clears_error_and_allows_respawn`
   - `src/app.rs:2121`: `saved_state_ron_serialization_omits_provider_terminals`
   - `src/app.rs:2150`: `comprehensive_legacy_ron_state_loads_and_carries_defaults`
   - `src/app.rs:2237`: `restart_restores_session_and_spawns_cli_lazily_in_session_folder`
   - `src/app.rs:2284`: `middle_provider_terminal_and_tools_panel_coexist_without_interference`

7. **Independent Command Execution Results**:
   - `cargo check`: Passed in 0.81s (0 errors).
   - `cargo test`: `284 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 3.94s`.
   - `cargo clippy --all-targets -- -D warnings`: Passed with 0 warnings in 0.35s.
   - `cargo test -- multi_session_switching deleting_active_session folder_change_clears failed_terminal_spawn saved_state_ron comprehensive_legacy restart_restores middle_provider_terminal`: `10 passed; 0 failed; finished in 0.69s`.

---

## 2. Logic Chain

1. **Integrity Forensics**:
   - Audited source lines and test routines: No hardcoded test fixtures masquerading as runtime logic, no dummy/facade implementations, no shortcuts bypassing PTY or process lifecycle, and no fabricated logs. Tests dynamically simulate egui layouts, frame inputs, and inspect actual memory addresses and process lifecycle states.

2. **Keyboard Focus Robustness**:
   - `session.focus_composer` is initialized or set true on session creation/selection/deletion.
   - Observation 1 proves `resolve_terminal_state` executes `std::mem::take(&mut session.focus_composer)`.
   - The returned `take_keyboard: bool` is true on the first frame and immediately false on all following frames. Focus cannot loop or continuously steal input.

3. **Process Teardown & Resource Safety**:
   - Observation 2 proves `delete_session` removes the session's entry from `self.provider_terminals`.
   - Dropping the entry invokes `Terminal::drop`, which invokes `TerminalJob::drop` and `child.kill()`.
   - On Windows, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` ensures that even if child processes spawned grandchildren, the OS kernel terminates the entire tree.
   - On Unix, process group signals `-pgid` are sent. No blocking wait is called, preventing hangs.

4. **CLI Process Exit Resilience**:
   - Observation 3 proves that if a CLI process terminates, `has_exited()` evaluates cleanly via atomic check and `child.try_wait()`.
   - The UI surfaces a clean "The process has exited." notification and a "Restart" button.
   - Any pending keystroke writes are safely ignored via `let _ = writer.write_all(bytes)`. Switching away and back does not panic or crash.

5. **Corrupt / Legacy SavedState Immunity**:
   - Observation 4 proves `SavedState` is configured with `#[serde(default)]` and `ViperApp::new` handles missing or invalid RON state via backup fallback (`app.ron.bak`) and corrupt state archiving (`keep_unreadable_save`), ultimately defaulting safely without panicking.
   - Observation 6 test 6 proves full backward compatibility with legacy aliases (e.g. `Gemini` -> `Antigravity`, `claude_session_id` -> `agent_session_id`).

6. **UI Widget ID Isolation**:
   - Observation 5 proves that the middle terminal uses an ID salt based on `("session_terminal", session_id)` in `CentralPanel`, while the tools panel uses `("terminal", *number)` in `Panel::right("tools_panel")`.
   - Because egui derives widget IDs hierarchically, the two panels are completely disjoint, preventing collision.

---

## 3. Caveats

- **Visual Rendering in Headless Mode**: In compliance with `AGENTS.md` Rule 3.1, tests do not drive the physical display or capture screenshots. Terminal layout and interaction are tested via headless egui test runners (`run_ui_test`, `run_ui`).
- **External Ignored Tests**: Per `AGENTS.md` Rule 3.2, the 8 pre-existing `#[ignore]` tests were not run wholesale because they either require live paid accounts or specific local CLI installations.

---

## 4. Conclusion

The implementation of Milestone 3 in `src/app.rs` satisfies all requirements and acceptance criteria:
- Per-session terminal lifecycle and map storage in `provider_terminals` are correctly wired and isolated from persisted state.
- Switching sessions preserves existing terminals in place and hands off keyboard focus without stealing focus continuously.
- Deleting an active session drops and terminates its process tree cleanly, preserving remaining sessions and transferring focus to the neighbor.
- Error retries and process exits are handled gracefully without panics.
- Saved state maintains 100% backward compatibility.
- Zero integrity violations, zero clippy warnings, and all 284 unit tests pass cleanly.

**Verdict**: **APPROVE**

---

## 5. Verification Method

To independently reproduce and verify this assessment:

```powershell
# 1. Compile check (zero errors)
cargo check

# 2. Run all unit tests including the 8 new lifecycle tests (zero failures, 8 ignored preserved)
cargo test

# 3. Verify specifically the 8 Milestone 3 lifecycle tests
cargo test -- multi_session_switching deleting_active_session folder_change_clears failed_terminal_spawn saved_state_ron comprehensive_legacy restart_restores middle_provider_terminal

# 4. Strict clippy lint pass (zero warnings)
cargo clippy --all-targets -- -D warnings
```
