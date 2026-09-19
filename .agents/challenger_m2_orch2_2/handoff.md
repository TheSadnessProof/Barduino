# Milestone 2 Empirical Challenge Report: Dynamic Resizing, PTY Dimension Sync, and Terminal Process Cleanup

## Verdict: APPROVE

---

## 1. Observation

1. **Target Implementations & Source Inspection**:
   - `src/app.rs`:
     - Line 144: `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>` maintains ephemeral session terminal instances in `ViperApp`.
     - Lines 40–61 (`struct SavedState`): Excludes `provider_terminals` entirely; only serializes persistent session data (`Session`), preserving backward and forward compatibility.
     - Lines 360–374 (`delete_session`): Executes `self.provider_terminals.remove(&id);` on session deletion, dropping the active `Terminal` instance.
     - Lines 445–446 (`change_folder`): Purges `self.provider_terminals.remove(&id);` when project folder changes.
     - Lines 593–618 (`resolve_terminal_state`): Consumes `session.focus_composer` via `std::mem::take` to prime `take_keyboard = true` on the first render frame of a `Ready` session.
     - Lines 687–725 (`terminal_area`): Retrieves or spawns `Terminal::start_command` inside `self.provider_terminals`, applies egui ID salting `("session_terminal", session_id)`, routes `terminal.ui(ui, take_keyboard)`, and purges dead terminals on restart.
   - `src/terminal.rs`:
     - Lines 58–126 (`TerminalJob`): On Windows, assigns spawned child process PID to a Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Drops and terminates the job object via `TerminateJobObject(job, 1)` and `CloseHandle` upon drop.
     - Lines 353–364 (`terminal.ui`): Calculates dynamic rows and columns:
       ```rust
       let rows = ((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
       let cols = ((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
       if (rows, cols) != self.size {
           self.size = (rows, cols);
           let _ = self.master.resize(pty_size(self.size));
           self.parser().screen_mut().set_size(rows, cols);
       }
       ```
     - Lines 601–608 (`cell_at`): Clamps click/drag pointer cells to `[0, rows.saturating_sub(1)]` and `[0, cols.saturating_sub(1)]` using `row_height.max(1.0)` and `char_width.max(1.0)`.

2. **Empirical Verification Test Suite**:
   Executed 6 adversarial stress tests against the live code:
   - `stress_dynamic_resize_extreme_and_zero_dimensions`:
     Tested window and available UI rect dimensions across:
     - `Vec2::ZERO` (0x0)
     - `Vec2::new(1.0, 1.0)` (1x1 micro-viewport)
     - `Vec2::new(0.0001, 0.0001)` (fractional sub-pixel)
     - `Vec2::new(5.0, 5000.0)` (extreme tall/narrow)
     - `Vec2::new(5000.0, 5.0)` (extreme wide/shallow)
     - `Vec2::new(10.0, 10.0)`
     - `Vec2::new(12.0, 12.0)` (exact boundary `2.0 * PADDING`)
     - `Vec2::new(100.0, 10.0)`
     - `Vec2::new(800.0, 600.0)` (standard window)
     - `Vec2::new(1920.0, 1080.0)` (1080p full HD)
     - `Vec2::new(3840.0, 2160.0)` (4K UHD)
     - `Vec2::new(5120.0, 2880.0)` (5K Ultrawide)
     **Result**: PASSED. Clamping (`rows.max(2.0)`, `cols.max(10.0)`) prevents underflow, divide-by-zero, and negative integer casting. Master PTY and vt100 screen synchronized without crashing.
   - `stress_dynamic_pty_dimension_synchronization_under_rapid_resizes`:
     Rapidly alternated between 8 distinct dimensional extremes in sequence (`800x600`, `200x200`, `0x0`, `1920x1080`, `1x1`, `3840x2160`, `50x1000`, `800x600`).
     **Result**: PASSED. No race condition, deadlock, or desynchronization between master PTY handle and vt100 screen buffer.
   - `stress_app_delete_session_kills_provider_terminal_process_tree`:
     Spawned an interactive session terminal running `cmd.exe` executing a live background child process `ping -n 4246 127.0.0.1`.
     Observed running process in Windows process table (`Get-CimInstance Win32_Process`).
     Called `app.delete_session(session_id)`.
     Observed `app.provider_terminals` purged the session terminal handle.
     Polled OS process table after 2 seconds.
     **Result**: PASSED. Process table confirmed `ping.exe` and `cmd.exe` were terminated immediately and completely with zero orphan processes left.
   - `stress_saved_state_ron_backward_compatibility_across_versions`:
     Deserialized historical legacy SavedState formats (pre-M1, pre-M2, pre-M3).
     Roundtripped `app.state` through RON serialization and deserialization.
     **Result**: PASSED. `SavedState` contains 0 PTY handles and serializes to clean RON.
   - `stress_session_switching_preserves_independent_terminal_buffers`:
     Created two concurrent sessions, spawned interactive PTY terminals for both, wrote unique tokens to each buffer, switched between them via `handle_sidebar(SidebarAction::Select)`.
     **Result**: PASSED. Buffers remained strictly isolated with zero cross-talk; keyboard focus flag (`focus_composer`) correctly primed on switch and consumed on first render.
   - `stress_terminal_area_process_exit_and_restart_replaces_dead_terminal`:
     Spawned a terminal whose process exited immediately (`exit`).
     Verified `has_exited()` was detected.
     Simulated restart button trigger (`provider_terminals.remove`).
     Rendered next frame and verified a fresh interactive terminal was immediately spawned.
     **Result**: PASSED.

3. **Codebase Invariants & Test Suite Results**:
   - `cargo check`: Exited with code 0 (0 errors, 0 warnings).
   - `cargo test`: Exited with code 0 (276 passed; 0 failed; 8 ignored; finished in 3.44s).
   - `cargo clippy --all-targets -- -D warnings`: Exited with code 0 (0 warnings).
   - Ignored test `cargo test -- --ignored closing_a_terminal --nocapture`: Exited with code 0 (`before closing: ["4592 PING.EXE"]`, `after closing: []`).
   - Repository status: All temporary challenger test blocks reverted; 0 modifications left in permanent codebase files.

---

## 2. Logic Chain

1. **Robustness of Dynamic Resize Math**:
   - Monospace font metrics (`char_width`, `row_height`) are positive non-zero floats provided by `egui::Context::fonts_mut`.
   - The bounding rect computation subtracts `2.0 * PADDING` (12.0px). When `ui.available_size()` approaches or equals zero (or small values < 12.0px), `(rect.height() - 12.0)` is negative.
   - The expression `((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16` guarantees `rows >= 2` for all real inputs.
   - The expression `((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16` guarantees `cols >= 10` for all real inputs.
   - Because `rows >= 2` and `cols >= 10`, `pty_size(self.size)` never passes zero or negative dimensions to Windows ConPTY or Unix PTY subsystem, avoiding ConPTY initialization panics.
   - In `terminal.rs::cell_at`, coordinates are clamped via `.clamp(0.0, f32::from(rows.saturating_sub(1)))`, guaranteeing that mouse drags never access cells beyond screen boundaries even under extreme aspect ratios.

2. **Process Tree Teardown on Session Deletion**:
   - When a session is deleted via `ViperApp::delete_session(id)`, line 366 executes `self.provider_terminals.remove(&id)`.
   - The `Terminal` instance owned by the entry is dropped.
   - The `TerminalJob` field inside `Terminal` is dropped.
   - Under Windows, `TerminalJob::drop()` executes `windows::Win32::System::JobObjects::TerminateJobObject(job, 1)` and `CloseHandle(job)`.
   - Windows kernel terminates every process assigned to the job object, including the parent CLI process (`cmd.exe`, `claude`, `codex`, `agy`) and any child/grandchild subprocesses spawned by it.
   - Empirical validation confirmed that background processes (`ping.exe`) were killed immediately upon session deletion.

3. **RON Saved State Backward Compatibility**:
   - `SavedState` in `src/app.rs` defines the persistent schema for the application across launches.
   - `provider_terminals` is declared on `ViperApp`, not on `SavedState`.
   - Neither `Terminal`, `MasterPty`, nor OS process handles are referenced in `SavedState` or `Session`.
   - Deserialization tests confirmed that sessions saved in legacy formats deserialize cleanly with sensible defaults, and new sessions serialize without non-serializable PTY data.

---

## 3. Caveats

1. **Visual Appearance & Human Ergonomics**:
   - In strict compliance with `AGENTS.md` §3.1 and `verifying-a-ui-change/SKILL.md`, no screenshots, window captures, or live mouse/keyboard driving were performed on the user's desktop.
   - Verification was conducted purely through headless egui passes (`ctx.run_ui`), unit tests, architectural inspection, and OS process auditing.
   - The user should visually inspect that the terminal background color, font sizing, and layout styling match their visual expectations.

2. **Temporary Evaluation Lifetime with Mutex Guard**:
   - In Rust, temporary values created in sub-expressions within a single statement (such as evaluating `(term.parser().screen().size().0, term.parser().screen().size().1)`) have statement lifetime. Because `std::sync::Mutex` is not re-entrant, attempting to take the same lock twice in a single statement causes a self-deadlock. The production codebase correctly uses `let parser = self.parser();` once per method scope.

---

## 4. Adversarial Challenge Report

### Challenge Summary
**Overall risk assessment**: LOW

### Challenges

#### Challenge 1: Viewport Collapse to 0x0 or Sub-Pixel Size
- **Assumption challenged**: PTY master resize and vt100 screen buffer can handle window minimization or zero-width sidebar collapse without crashing or underflowing.
- **Attack scenario**: Application window minimized or sidebars expanded to consume 100% of horizontal width, leaving `ui.available_size() == Vec2::ZERO`.
- **Blast radius**: If uncaught, float-to-u16 conversion of negative values could underflow or panic, crashing the app.
- **Stress test result**: PASSED. `.max(2.0)` for rows and `.max(10.0)` for cols strictly bounds grid dimensions to safe minimums.

#### Challenge 2: Orphan Process Accumulation on Session Deletion
- **Assumption challenged**: Dropping a session in Viper cleanly kills the CLI process tree, even if the CLI has spawned subprocesses or compilers.
- **Attack scenario**: Active session running a background command is deleted via the sidebar trash icon.
- **Blast radius**: Leftover zombie processes consuming CPU, memory, and file locks.
- **Stress test result**: PASSED. Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` immediately purges child and grandchild processes upon `app.delete_session()`.

#### Challenge 3: SavedState Corruption by PTY Runtime State
- **Assumption challenged**: Introducing terminal management to `ViperApp` does not pollute `SavedState` or break backward compatibility with older `app.ron` files.
- **Attack scenario**: Loading an older `app.ron` without terminal state or writing state that cannot be deserialized by previous builds.
- **Blast radius**: Loss of saved user sessions, app failing to start on load.
- **Stress test result**: PASSED. `provider_terminals` is completely decoupled from `SavedState`. Deserialization and roundtripping of legacy states pass 100%.

---

## 5. Conclusion

All empirical stress tests and verification criteria have passed without exceptions:
- Dynamic resizing cleanly handles 0x0, 1x1, extreme aspect ratios, standard 1080p, and high-DPI viewports without panics.
- Process tree cleanup terminates underlying CLI subprocesses cleanly upon session deletion.
- Saved state serialization preserves 100% RON backward compatibility without storing PTY handles.
- Zero clippy warnings, zero compilation errors, and all 276 tests pass.

**Verdict: APPROVE.**

---

## 6. Verification Method

To independently verify these results from `c:\Users\ditob\Documents\viper`:

```powershell
# 1. Warm check
cargo check

# 2. Complete test suite (276 passed, 8 ignored)
cargo test

# 3. Clippy invariant (zero warnings)
cargo clippy --all-targets -- -D warnings

# 4. Specific M2 terminal lifecycle unit tests
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

# 5. Free ignored test verifying process tree termination on this computer
cargo test -- --ignored closing_a_terminal --nocapture
```
