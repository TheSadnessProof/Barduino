# Milestone 3 Investigation Report: Per-Session Lifecycle, Multi-Session Switching & Process Teardown

## 1. Observation

### Codebase Inspection
- **`src/app.rs:144`**:
  `ViperApp` owns per-session terminal instances in:
  ```rust
  provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>,
  ```
- **`src/app.rs:230`, `src/app.rs:264`**:
  `provider_terminals` is initialized as `BTreeMap::new()` both on application startup and in test fixtures.
- **`src/app.rs:43-61` (`SavedState`)**:
  `SavedState` serializes only configuration and history (`sessions`, `active_session`, `settings`, `usage`, `plan`, etc.). `provider_terminals` is deliberately omitted from `SavedState`, preserving 100% RON backward compatibility across app restarts.
- **`src/session.rs:136`, `src/session.rs:180`**:
  `Session` contains `#[serde(skip)] pub focus_composer: bool`.
  `Session::new` initializes `focus_composer: true`.
- **`src/app.rs:457-461` (`SidebarAction::Select`)**:
  When a session is selected from the sidebar:
  ```rust
  SidebarAction::Select(id) => {
      self.view = View::Chat;
      self.state.active_session = id;
      self.active_session_mut().focus_composer = true;
  }
  ```
- **`src/app.rs:595-618` (`resolve_terminal_state`)**:
  Consumes `focus_composer` when transitioning to `TerminalState::Ready`:
  ```rust
  let take_keyboard = std::mem::take(&mut session.focus_composer);
  TerminalState::Ready {
      session_id: session.id,
      ...
      take_keyboard,
  }
  ```
- **`src/app.rs:687-725` (`terminal_area`)**:
  Lazily spawns or retrieves the active session's terminal:
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

  let mut restart = false;
  ui.scope_builder(
      egui::UiBuilder::new().id_salt(("session_terminal", session_id)),
      |ui| match terminal_entry {
          Ok(terminal) => {
              restart = terminal.ui(ui, take_keyboard);
          }
          Err(err) => { ... }
      },
  );

  if restart {
      self.provider_terminals.remove(&session_id);
      ui.ctx().request_repaint();
  }
  ```
- **`src/terminal.rs:366-378` (`Terminal::ui`)**:
  Immediate focus handoff and input locking:
  ```rust
  if response.clicked() || take_keyboard {
      response.request_focus();
  }
  let focused = response.has_focus();
  if focused {
      ui.memory_mut(|m| {
          m.set_focus_lock_filter(
              response.id,
              egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
          );
      });
      self.handle_input(ui);
  }
  ```
- **`src/terminal.rs:58-126`, `src/terminal.rs:521-526` (`TerminalJob` & `Terminal::drop`)**:
  Process group and job object teardown on terminal drop:
  ```rust
  impl Drop for Terminal {
      fn drop(&mut self) {
          self.job.kill();
          let _ = self.child.kill();
      }
  }
  ```
  On Windows:
  ```rust
  impl Drop for TerminalJob {
      fn drop(&mut self) {
          if let Some(job) = self.0.take() {
              let _ = unsafe { windows::Win32::System::JobObjects::TerminateJobObject(job, 1) };
              let _ = unsafe { windows::Win32::Foundation::CloseHandle(job) };
          }
      }
  }
  ```
  On Unix:
  ```rust
  impl TerminalJob {
      fn kill(&self) {
          if let Some(pid) = self.0 {
              let pgid = pid as i32;
              unsafe {
                  let _ = kill(-pgid, 15); // SIGTERM
                  let _ = kill(-pgid, 9);  // SIGKILL
              }
          }
      }
  }
  ```
- **`src/app.rs:360-382` (`delete_session`)**:
  Session removal purges both secondary tools and provider terminals, and selects the neighbor session with keyboard focus:
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
- **`src/app.rs:439-447` (`change_folder`)**:
  Changing folder removes the stale terminal:
  ```rust
  if session.entries.is_empty() {
      let id = session.id;
      session.project_dir = dir;
      self.tools.remove(&id);
      self.provider_terminals.remove(&id);
  } else {
      let permission_mode = session.permission_mode;
      self.new_session(dir, permission_mode);
  }
  ```
- **Existing Test Execution**:
  Ran `cargo test` and `cargo clippy --all-targets`:
  - 276 tests passed, 0 failed, 8 ignored.
  - 0 clippy warnings.

---

## 2. Logic Chain

### 2.1 Multi-Session PTY Survival and Buffer Preservation
1. `ViperApp.provider_terminals` is an in-memory `BTreeMap<u64, Result<Terminal, String>>`.
2. When switching from Session A to Session B (`SidebarAction::Select(B)`), `app.rs:457-461` updates `self.state.active_session = B`. It performs NO operations on `provider_terminals`.
3. Therefore, Session A's PTY, parser, and background reader thread remain completely untouched and alive in `provider_terminals[&A]`.
4. While Session B is active, Session A's reader thread continues reading PTY bytes from the background process into Session A's `Arc<Mutex<Parser>>`.
5. When switching back from Session B to Session A, `terminal_area` calls `self.provider_terminals.entry(A).or_insert_with(...)`. Because entry `A` already exists, `or_insert_with` does not spawn a new process. The existing `Terminal` is rendered, displaying the fully preserved vt100 screen buffer, scrollback history, and current cursor position.

### 2.2 Immediate Keyboard Focus Without Clicking
1. Upon switching to Session B (`SidebarAction::Select(B)`), `app.rs:460` sets `self.active_session_mut().focus_composer = true`.
2. In `terminal_area`, `resolve_terminal_state` executes `take_keyboard = std::mem::take(&mut session.focus_composer)`. Thus, `take_keyboard == true` on the first frame of rendering Session B.
3. In `Terminal::ui`, line 366 executes `if response.clicked() || take_keyboard { response.request_focus(); }`.
4. The egui widget response for the terminal area immediately requests focus without requiring any mouse click.
5. Lines 371-377 apply `set_focus_lock_filter` so that navigation keys (Tab, arrow keys, Escape) stay trapped inside the terminal rather than navigating egui UI widgets.
6. Subsequent frames have `take_keyboard == false`, maintaining normal focus semantics until the next session switch.

### 2.3 Process Teardown on Session Deletion
1. When a session is deleted via `SidebarAction::Delete(id)` or `delete_session(id)`, `app.rs:366` executes `self.provider_terminals.remove(&id)`.
2. Removing the entry from the `BTreeMap` drops the `Terminal` instance.
3. `Terminal::drop` invokes `self.job.kill()` and `self.child.kill()`.
4. On Windows, `TerminalJob::kill()` invokes `TerminateJobObject(job, 1)`, terminating the child process and all child/grandchild subprocesses created by the CLI. `TerminalJob::drop` closes the job handle.
5. On Unix, `TerminalJob::kill()` sends `SIGTERM` and `SIGKILL` to the process group (`-pgid`).
6. Dropping `master: Box<dyn MasterPty>` closes the pseudo-terminal pipe. The background reader thread receives EOF (`Ok(0)` or `Err`) and exits cleanly.
7. If the deleted session was the active session, `delete_session` clamps `active_session` to the adjacent session (`index.saturating_sub(1)`), sets `next.focus_composer = true`, and transfers view and focus seamlessly.

### 2.4 Folder Change Teardown and Re-spawning
1. When an empty/unconfigured session's folder is changed in `change_folder()`, `app.rs:446` executes `self.provider_terminals.remove(&id)`.
2. This drops the old terminal (killing any process previously associated with it).
3. `session.project_dir` is updated to the newly selected directory.
4. On the subsequent frame, `resolve_terminal_state` sees a valid folder and returns `TerminalState::Ready { cwd: new_dir, ... }`.
5. `self.provider_terminals.entry(id).or_insert_with(...)` detects that entry `id` is vacant and spawns a fresh `Terminal::start_command` in the new directory.

### 2.5 Persistence & SavedState Invariants
1. `SavedState` in `src/app.rs:43-61` does not store `provider_terminals`.
2. `Session` in `src/session.rs:136` marks runtime fields (`focus_composer`, `turn`, `streaming`, etc.) with `#[serde(skip)]`.
3. As a result, saving state writes pure RON documents without OS handles or PTY states.
4. When Viper restarts, old saved sessions deserialize without error; provider terminals are created lazily on demand when each session is opened.

---

## 3. Caveats

- **External File Dialog GUI (`rfd::FileDialog`)**: `change_folder` invokes `rfd::FileDialog::new().pick_folder()`, which opens a native platform GUI window. In headless CI / automated test runs, this modal dialog cannot be driven directly. Tests of folder change lifecycle should test programmatic directory updates or `provider_terminals.remove` eviction rather than triggering the modal dialog.
- **Provider Switching on Active Session**: Currently `Session::can_change_provider()` allows changing provider only when `entries.is_empty() && !is_running()`. In the interactive terminal model, `provider_terminals` persists the process until deleted, exited, or restarted. Changing provider settings in `Settings` affects new sessions; changing an existing session's provider after its terminal has spawned requires restarting the terminal or creating a new session.
- **Unix Process Group Drop**: While Windows uses Windows Job Objects (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` with `TerminateJobObject`), on Unix `Terminal::drop` calls `self.job.kill()` and `self.child.kill()`. The implementer should ensure Unix process group signaling handles detached daemons consistently if non-standard shells are invoked.

---

## 4. Conclusion

The core architecture for Milestone 3 (Per-Session Lifecycle, Multi-Session Switching & Process Teardown) is fundamentally sound, fully wired into `src/app.rs`, `src/sidebar.rs`, and `src/terminal.rs`, and verified against existing tests (276 passed, 0 warnings):
1. **PTY Longevity**: Multi-session terminals remain alive in `provider_terminals` when switching between sessions.
2. **Immediate Focus**: Switching sessions primes `session.focus_composer`, which `resolve_terminal_state` consumes as `take_keyboard: true`, requesting focus in `Terminal::ui` without a mouse click.
3. **Buffer Integrity**: Re-selecting an already spawned session retrieves the existing `Terminal` from `provider_terminals` with its full vt100 parser state and history intact.
4. **Clean Teardown**: Deleting a session purges `provider_terminals`, invoking Windows Job Object termination / Unix process group termination to kill all processes cleanly without orphans.
5. **Folder Changes**: Changing folder purges `provider_terminals[&id]` and re-spawns the terminal in the new path.
6. **RON Compatibility**: `SavedState` remains 100% clean and backward-compatible.

### Recommendations for Milestone 3 Completion:
To bring Milestone 3 to completion with rigorous verification, the following dedicated unit tests should be added to `src/app.rs`:
1. `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`: Test 2 sessions in `app`, verify switching 1 -> 2 -> 1 preserves both in `provider_terminals` and toggles `focus_composer` appropriately.
2. `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`: Test deleting session 2 of [1, 2, 3], verify session 2 is purged, session 1 becomes active with `focus_composer == true`, and session 3 is untouched.
3. `folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd`: Verify that resetting/updating `project_dir` and clearing `provider_terminals` causes next `terminal_area` pass to spawn in the new path.
4. `failed_terminal_spawn_retry_action_clears_error_and_allows_respawn`: Verify the `restart` logic in `terminal_area` when a terminal reports an error or exit.

---

## 5. Verification Method

To independently verify these findings and check codebase integrity:

1. **Verify compilation**:
   ```powershell
   cargo check
   ```
2. **Verify unit test suite**:
   ```powershell
   cargo test
   ```
   (Expected: all 276 unit tests pass with 0 failures and 8 ignored).
3. **Verify clippy cleanliness**:
   ```powershell
   cargo clippy --all-targets
   ```
   (Expected: 0 warnings).
4. **Inspect key code locations**:
   - `src/app.rs:360-382`: `delete_session`
   - `src/app.rs:439-447`: `change_folder`
   - `src/app.rs:457-461`: `SidebarAction::Select`
   - `src/app.rs:595-618`: `resolve_terminal_state`
   - `src/app.rs:687-725`: `terminal_area`
   - `src/terminal.rs:366-378`: `Terminal::ui`
   - `src/terminal.rs:521-526`: `Terminal::drop`
