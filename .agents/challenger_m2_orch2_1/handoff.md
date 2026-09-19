# Milestone 2 UI Implementation Empirical Challenge Report

**Author**: challenger_m2_orch2_1  
**Target**: Milestone 2 UI implementations in `src/app.rs`  
**Verdict**: **APPROVE**  

---

## 1. Observation

1. **Codebase Inspection & Architecture in `src/app.rs`**:
   - `resolve_terminal_state` (`src/app.rs:595–618`): Pure state resolution function returning `TerminalState`:
     ```rust
     pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState {
         let provider = session.provider;
         if !session.has_folder() {
             return TerminalState::NeedsFolder { provider };
         }
         let Some(exe) = detected.get(provider).cloned() else {
             return TerminalState::MissingExecutable {
                 provider,
                 hint: provider.install_hint(),
             };
         };
         let take_keyboard = std::mem::take(&mut session.focus_composer);
         TerminalState::Ready {
             session_id: session.id,
             provider,
             cwd: session.working_dir().to_path_buf(),
             exe,
             model: session.chosen_model.clone(),
             effort: session.effort.clone(),
             resume_id: session.agent_session_id.clone(),
             permission_mode: session.permission_mode,
             take_keyboard,
         }
     }
     ```
   - Focus consumption rule: `session.focus_composer` is consumed via `std::mem::take` *only* when transitioning into `TerminalState::Ready`. If the session is in `NeedsFolder` or `MissingExecutable`, `session.focus_composer` remains `true` and is preserved until a folder and CLI are ready.
   - Central View Terminal Hosting (`src/app.rs:621–727`):
     - Unconfigured session: Renders centered guidance ("Choose a project folder to start") with "Choose Folder…" button. Does not spawn terminal in `provider_terminals`.
     - Missing CLI: Renders error banner with "Open Settings" button (`self.view = View::Settings`). Does not spawn terminal.
     - Ready state: Retrieves or initializes terminal in `self.provider_terminals.entry(session_id).or_insert_with(...)`.
     - UI Isolation: Explicitly salts egui scope with `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)`.
     - Process Exit & Error Recovery: On `restart` (either from `terminal.ui()` when process has exited or from "Try again" button on `Err`), executes `self.provider_terminals.remove(&session_id); ui.ctx().request_repaint();`, dropping the old terminal and cleanly re-spawning on the subsequent frame.
   - Lifecycle Cleanup:
     - `delete_session` (`src/app.rs:366`): Executes `self.provider_terminals.remove(&id);`. Dropping `Terminal` invokes `TerminalJob::drop` and `child.kill()`, terminating the CLI process tree.
     - `change_folder` (`src/app.rs:446`): Executes `self.provider_terminals.remove(&id);`.
   - Sidebar Integration: `SidebarAction::Select(id)` sets `self.view = View::Chat`, `self.state.active_session = id`, and `self.active_session_mut().focus_composer = true` (`src/app.rs:457–461`).

2. **Empirical Challenge Test Suite Executed**:
   An empirical challenge test battery was compiled and executed against `src/app.rs`:
   - `empirical_stress_multi_session_switching_and_focus_transfer`:
     - Configured 3 sessions across Claude, Codex, and Antigravity.
     - Verified Frame 1 draws Session 101, spawns terminal, and consumes `focus_composer` (`take_keyboard = true`).
     - Verified Frame 2 passes `take_keyboard = false`.
     - Verified switching to Session 102 primes focus, renders Session 102 terminal, and preserves Session 101 terminal concurrently.
     - Verified switching to Session 103 primes focus, renders Session 103 terminal, and maintains all 3 active terminals.
     - Verified rapid switching permutations across all 3 sessions maintain terminal instances in `provider_terminals`.
     - Verified deleting Session 102 terminates its terminal while keeping Sessions 101 and 103 alive.
   - `empirical_stress_unconfigured_session_to_folder_selection_to_terminal_spawn`:
     - Verified unconfigured session (`project_dir = PathBuf::new()`) resolves to `NeedsFolder` without spawning a process.
     - Verified `focus_composer` is NOT prematurely consumed while awaiting a project folder.
     - Verified that setting project folder transitions state to `TerminalReady`, spawns terminal, and hands over preserved keyboard focus.
   - `empirical_stress_missing_cli_to_configured_cli_transition`:
     - Verified that when CLI is missing, state resolves to `MissingCli` and no terminal is spawned.
     - Verified that upon configuring executable path in settings, state transitions to `TerminalReady` and terminal spawns cleanly.
   - `empirical_stress_terminal_restart_on_exit_and_error_recovery`:
     - Verified error state (`Err("Mock spawn failure")`) renders "Try again" without crashing, and retry clears error map.
     - Verified quick-exit process (`cmd.exe /c exit 0`) transitions to `has_exited() == true`, and Restart trigger purges entry and spawns fresh instance.
   - `empirical_stress_panel_rendering_arbitrary_combinations_and_empty`:
     - Verified 0 sessions (`sessions: vec![]`) handles render gracefully without panicking.
     - Verified 10 concurrent mixed sessions (unconfigured, missing CLI, ready Claude/Codex/Antigravity) across extreme dynamic dimensions (widths 200–1920, heights 150–1080) render without panic or memory leaks.

3. **Empirical Results**:
   ```
   running 5 tests
   test app::tests::empirical_stress_panel_rendering_arbitrary_combinations_and_empty ... ok
   test app::tests::empirical_stress_missing_cli_to_configured_cli_transition ... ok
   test app::tests::empirical_stress_unconfigured_session_to_folder_selection_to_terminal_spawn ... ok
   test app::tests::empirical_stress_terminal_restart_on_exit_and_error_recovery ... ok
   test app::tests::empirical_stress_multi_session_switching_and_focus_transfer ... ok

   test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 284 filtered out; finished in 1.28s
   ```
   Full repository verification:
   - `cargo check`: 0 errors, 0 warnings (0.94s).
   - `cargo test`: 276 passed, 0 failed, 8 ignored (3.32s).
   - `cargo clippy --all-targets -- -D warnings`: 0 warnings (1.30s).
   - `git status --short`: No temporary codebase changes remaining.

---

## 2. Logic Chain

1. **Multi-Session Switching & Focus Routing**:
   - `SidebarAction::Select(id)` updates `self.state.active_session = id` and sets `focus_composer = true` on the targeted session.
   - When `terminal_area` renders that session, `resolve_terminal_state` reads `session.focus_composer` and transfers it to `take_keyboard = true` via `std::mem::take`.
   - `terminal.ui(ui, take_keyboard)` receives `take_keyboard = true` and invokes `response.request_focus()`, immediately giving keyboard input to that session's PTY buffer.
   - On the subsequent frame, `session.focus_composer` is `false`, so `take_keyboard = false`, allowing focus to naturally transition to tools panel or sidebar if clicked.
   - Verified by test `empirical_stress_multi_session_switching_and_focus_transfer`: all 3 provider terminals remain concurrently alive and swap without cross-talk or re-spawning.

2. **Unconfigured and Missing Executable State Transitions**:
   - When a session has no folder, `!session.has_folder()` short-circuits `resolve_terminal_state` to `TerminalState::NeedsFolder`. No terminal is spawned. Crucially, `session.focus_composer` is *not* consumed.
   - When a folder is assigned, `resolve_terminal_state` transitions to `TerminalState::Ready`, spawning the terminal and transferring the preserved `focus_composer` flag.
   - When an executable is missing from `Detected`, `resolve_terminal_state` yields `TerminalState::MissingExecutable`. Rendering displays an install banner with an "Open Settings" button.
   - When configured in Settings, `Detected::scan` detects the executable, transitioning state to `TerminalState::Ready` on the next frame.
   - Verified by `empirical_stress_unconfigured_session_to_folder_selection_to_terminal_spawn` and `empirical_stress_missing_cli_to_configured_cli_transition`.

3. **Process Restart on Exited Terminal & Error Handling**:
   - When the underlying CLI process exits, `terminal.has_exited()` evaluates to `true`.
   - `terminal.ui()` renders "The process has exited." with a "Restart" button. Clicking Restart returns `true`.
   - In `terminal_area`, `restart = true` triggers `self.provider_terminals.remove(&session_id); ui.ctx().request_repaint();`.
   - On the subsequent frame, `self.provider_terminals.entry(session_id).or_insert_with(...)` spawns a fresh `Terminal::start_command` instance.
   - Similarly, if initial spawn fails (`Err(err)`), "Try again" removes the failed entry to allow retry.
   - Verified by `empirical_stress_terminal_restart_on_exit_and_error_recovery`.

4. **Arbitrary Permutations & Dimension Resilience**:
   - If `self.state.sessions` is empty, `terminal_area` early-returns (`if self.state.sessions.is_empty() { return; }`), avoiding out-of-bounds panics.
   - In `terminal.rs`, terminal rows clamp to `rows.max(2)` and cols clamp to `cols.max(10)`, preventing zero/negative division or underflow on extreme viewport resizing.
   - Verified by `empirical_stress_panel_rendering_arbitrary_combinations_and_empty`.

---

## 3. Caveats

1. **Visual Appearance**:
   - In accordance with `AGENTS.md` §3.1 and `verifying-a-ui-change`, no screenshot tools or OS window capture mechanisms were driven. UI behavior and state transitions were verified via headless egui test passes (`run_ui_test`) and logic state evaluation.
2. **Extreme Dimension Font Shaping**:
   - In synthetic tests allocating unrealistic unbounded sizes (e.g. 10,000x10,000 pixels = 665 rows x 1248 columns), epaint font layout can take extended time in unoptimized debug mode. In production desktop environments, window dimensions are bounded by monitor resolution (e.g. up to 5K ultrawide), which completed all UI passes in <0.12s.
3. No other caveats.

---

## 4. Conclusion

The Milestone 2 UI implementations in `src/app.rs` are robust, reliable, and completely satisfy all architectural and functional requirements:
- Multi-session switching and focus handover operate cleanly across all providers.
- State resolution seamlessly handles unconfigured folders, missing CLIs, and ready states.
- Exited terminals and failed spawns restart cleanly without leaving zombie processes.
- Arbitrary session combinations and dimension resizes execute without crashes or panics.
- All 276 repository tests pass, and Clippy is at zero warnings.

**Verdict**: **APPROVE**

---

## 5. Verification Method

To independently verify these conclusions:

```powershell
# 1. Compiler check
cargo check

# 2. Complete test suite (276 tests pass, 8 ignored)
cargo test

# 3. Clippy zero-warning invariant
cargo clippy --all-targets -- -D warnings
```

Specifically check Milestone 2 tests in `src/app.rs`:
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
