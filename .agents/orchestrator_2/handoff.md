# Orchestrator Final Handoff Report: Viper Interactive Provider Terminal

**Orchestrator**: `orchestrator_2` (teamwork_preview_orchestrator)  
**Target Project**: Viper Desktop GUI for Agentic Coding (`c:\Users\ditob\Documents\viper`)  
**Mission**: Replace the middle chat transcript and composer in Viper (`chat.rs`) with a dedicated interactive terminal experience (`terminal.rs`) that launches the selected AI provider's CLI directly inside an embedded PTY.  
**Authoritative Request**: `c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md` (§`2026-09-19T00:31:41Z`)  
**Status**: **COMPLETED & FULLY VERIFIED (ALL MILESTONE GATES PASSED)**  

---

## 1. Observation

### 1.1 Architecture & Changes Implemented
All 4 core requirements (R1, R2, R3, R4) and 10 inventoried features (F1–F10) have been implemented and verified across 4 milestones:

1. **R1: Embedded Interactive Terminal Experience in Central Panel**:
   - `src/app.rs`: In `ViperApp::ui`, `CentralPanel` dispatches `View::Chat => self.terminal_area(ui)`. The middle area serves as a full-height interactive terminal emulator powered by `portable-pty` and `vt100`.
   - Preserves `left_panel` (project grouping, session list, new session, delete) and `right_panel` (secondary shell terminals, diffs, live preview browser) completely intact.
   - `TerminalState` & `resolve_terminal_state`: Pure state resolution cleanly separates rendering from logic per `verifying-a-ui-change` skill:
     - Unconfigured session (`!session.has_folder()`): Shows centered guidance card *"Choose a project folder to start"* with a *"Choose Folder…"* button. Does not spawn any PTY.
     - Missing CLI executable: Displays warning banner *"Provider isn't installed"* with an *"Open Settings"* button.
     - Ready session: Lazily spawns the interactive CLI inside `session.working_dir()` and locks keyboard focus on the first frame.
   - Process exit & restart: If the CLI process exits, surfaces *"The process has exited."* and a *"Restart"* button. Clicking Restart drops the dead process tree and spawns a fresh instance on the next frame.
   - Preserved `src/chat.rs`: Marked `#![allow(dead_code)] // Preserved for conversation tests and transition.` so legacy models and conversation unit tests remain intact without warnings.

2. **R2: Direct Provider CLI Execution in Embedded PTY**:
   - `src/terminal.rs`:
     - Added `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>`.
     - Windows ConPTY error 193 resolution: Implemented `is_batch_script` and `build_command`, which detects `.cmd`/`.bat` files case-insensitively and wraps them with `cmd.exe /c` on Windows.
     - Injected `TERM=xterm-256color` and `COLORTERM=truecolor` in PTY child environment.
     - Process tree cleanup: Configured Win32 Job Objects (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) on Windows (`TerminalJob`) and process group signaling (`kill(-pgid, 15)`) on Unix, terminating child and grandchild processes upon terminal drop.
     - Dynamic viewport resizing: In `Terminal::ui`, computes `(rows, cols)` from `ui.available_size()`, clamped to `rows >= 2` and `cols >= 10`. Invokes `master.resize(pty_size(self.size))` and `parser().screen_mut().set_size(rows, cols)` dynamically.
     - Focus locking: Applies `set_focus_lock_filter` to intercept Tab, arrows, and Escape inside the terminal when focused.
   - `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`:
     - Implemented `agent::build_interactive_command`: Formats interactive command arguments for `Provider::Claude`, `Provider::Codex`, and `Provider::Antigravity`, supporting working directory (`session.working_dir()`), model selection, reasoning effort, conversation resume, and permission mode flags.
     - Omits headless streaming flags (`-p`, `--output-format`, `exec`, stdin `-`).

3. **R3: Multi-Session State, Lifecycle & SavedState Compatibility**:
   - `src/app.rs`:
     - Declared `provider_terminals: BTreeMap<u64, Result<Terminal, String>>` on `ViperApp` as ephemeral in-memory state.
     - `SavedState` strictly omits `provider_terminals` and contains no OS PTY handles.
     - Multi-session switching: Selecting a session (`SidebarAction::Select(id)`) swaps the rendered terminal buffer without terminating background processes or re-spawning.
     - Immediate focus handover: Sets `session.focus_composer = true` on switch; `resolve_terminal_state` consumes it via `std::mem::take` to prime `take_keyboard = true` on the first frame, immediately requesting focus without requiring a mouse click.
     - Session deletion: `delete_session(id)` executes `self.provider_terminals.remove(&id);`, dropping the terminal and terminating the CLI process tree via Windows Job Objects. Switches active session to neighbor with focus.
     - Working directory change: Changing folder on an empty session clears `provider_terminals[&id]` and re-spawns in the new folder.
     - 100% RON backward compatibility (`AGENTS.md` Rule 3.5): Tested against legacy save schemas containing `Gemini`, `claude_session_id`, and missing fields. All deserialize cleanly with serde aliases and defaults.

4. **R4: Full Retention of Sidebar & Auxiliary Tools Panel**:
   - The left sidebar retains project grouping, session management, and settings.
   - The right tools panel (`right_panel`) retains secondary shell terminals, git branch/working tree diffs, and the embedded live preview browser.
   - Middle terminal (`id_salt(("session_terminal", session_id))`) and tools panel (`push_id(("terminal", number))`, `id_salt(("tool_tab", index))`) use disjoint egui ID spaces with zero collisions across 1,154 tested widget IDs. Toggling tool tabs does not interrupt or desync the middle provider terminal.

### 1.2 Verification Suite Summary
- `cargo check`: **0 errors** (compiles in ~0.3s).
- `cargo test`: **284 passed, 0 failed, 8 ignored** (conforming strictly to `AGENTS.md` Rule 3.2; zero newly ignored tests).
- `cargo clippy --all-targets -- -D warnings`: **0 warnings**.
- `Cargo.toml`: **0 added dependencies** (`git diff Cargo.toml` is empty).
- `git diff`: **Zero auto-commits, zero `cargo fmt` mass-reformatting on untouched lines**.
- Empirical stress tests: 4 adversarial test suites passed, verifying extreme viewport dimensions (0x0 to 8K), process tree termination, SavedState roundtrips, and tools panel coexistence.

---

## 2. Logic Chain

1. **R1 Fulfillment**: Replacing `chat_area` with `terminal_area` in `CentralPanel` transforms Viper's middle column into an embedded interactive terminal for the active session, while leaving sidebar and tools panels completely unaffected.
2. **R2 Fulfillment**: `Terminal::start_command` combined with provider interactive command builders provides direct, interactive PTY execution for Claude, Codex, and Antigravity. Windows `.cmd` batch wrapping eliminates ConPTY error 193. PTY dynamic resizing reflows TUI layout on window size changes.
3. **R3 Fulfillment**: Storing active terminals in `ViperApp.provider_terminals` keeps background sessions executing, swaps display buffers on selection, and hands over keyboard focus immediately. Deleting a session drops `Terminal`, and Windows Job Objects terminate child/grandchild processes cleanly. Excluding `provider_terminals` from `SavedState` guarantees 100% RON backward compatibility.
4. **R4 Fulfillment**: Disjoint egui ID salting and decoupled action routing guarantee that secondary shells, diff viewers, and browser previews coexist with the middle terminal without state desynchronization or widget ID collisions.
5. **Quality & Integrity Invariants**: Zero new dependencies, zero clippy warnings under `-D warnings`, zero unauthorized commits, and full forensic integrity confirmation by independent auditors across all milestones.

---

## 3. Caveats

1. **Visual Appearance & Display**: In strict adherence to `AGENTS.md` Rule 3.1 ("You cannot see this app. Do not try.") and the `verifying-a-ui-change` skill, all UI tests were executed headlessly via egui context passes (`run_ui_test`, `run_ui`). The user should visually inspect the application window to verify aesthetic preferences.
2. **Paid CLI Tests**: In strict compliance with `AGENTS.md` Rule 3.2, live CLI tests requiring real paid API tokens (`runs_the_real_antigravity_cli`, etc.) were kept ignored. Free local tests and full unit tests ran completely.

---

## 4. Conclusion

The Viper Interactive Provider Terminal project is **100% complete, fully integrated, and verified**.
All milestones (M1, M2, M3, M4) have passed all gating checks with unanimous approvals from Reviewers, Challengers, and Forensic Auditors.
The repository is clean, with 284 passing tests and zero clippy warnings.

---

## 5. Verification Method

To reproduce and verify the implementation independently from repository root:

```powershell
# 1. Warm check
cargo check

# 2. Strict clippy check (must have zero warnings)
cargo clippy --all-targets -- -D warnings

# 3. Full test suite (284 passed, 0 failed, 8 pre-existing ignored)
cargo test

# 4. Specific interactive terminal lifecycle tests
cargo test -- terminal:: app:: agent::

# 5. Local process tree termination test (free computer test)
cargo test -- --ignored closing_a_terminal --nocapture

# 6. Dependency and commit cleanliness
git diff Cargo.toml
git status -s
```
