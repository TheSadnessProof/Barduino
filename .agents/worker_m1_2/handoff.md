# Milestone 1 Implementation Handoff Report

## 1. Observation

### Codebase Changes Implemented Across Owned Files
1. **`src/terminal.rs`**:
   - Lines 176–246: Added `is_batch_script(program: &Path) -> bool` (case-insensitive detection of `.cmd` and `.bat` extensions, falling back to `agent::find_on_path`), `comspec() -> PathBuf` (resolving `%ComSpec%` or searching for `cmd.exe`), and `wrap_batch_command(program: &Path, args: &[String]) -> (PathBuf, Vec<String>)`.
   - Lines 226–246: Added `build_command(cwd: &Path, program: &Path, args: &[String]) -> CommandBuilder` which wraps batch scripts with `cmd.exe /c` on Windows to eliminate ConPTY Error 193, sets the working directory to `cwd`, and injects `TERM=xterm-256color` and `COLORTERM=truecolor`.
   - Lines 258–266: Implemented `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>`.
   - Lines 248–255: Refactored `Terminal::start` to invoke the private `Terminal::spawn(cmd: CommandBuilder, ctx: egui::Context, spawn_err_msg: &str) -> Result<Self, String>`, maintaining 100% backward compatibility.
   - Lines 320–335: Exposed `pub fn parser(&self)`, `pub fn has_exited(&mut self)`, and `pub fn write_all(&self, bytes: &[u8])`.
   - Lines 25–29: Updated `Replies` struct visibility to `pub struct Replies` so `Terminal::parser` has a valid public return signature.
   - Lines 965–1123: Added 10 sentence-named unit tests in `src/terminal.rs::tests` covering environment variables, batch script wrapping, case insensitivity, direct binary execution, PTY process startup and output capturing, dimensions, and error handling.
2. **`src/claude.rs`**:
   - Lines 66–105: Implemented `pub fn interactive_args(_cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`. Strips headless flags (`-p`, `--output-format stream-json`, `--verbose`, `--include-partial-messages`, `--permission-prompts none`) and maps permission modes (`ReadOnly` -> `default` + `--disallowed-tools Bash`, `AcceptEdits` -> `acceptEdits`, `Full` -> `--dangerously-skip-permissions`, `Plan` -> `plan`). Appends `--model`, `--effort`, and `--resume` when provided.
3. **`src/codex.rs`**:
   - Lines 79–117: Implemented `pub fn interactive_args(cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`. Strips headless tokens (`exec`, `--json`, `-`). Sets working directory via `-C <cwd>`, handles `resume <id>` as the initial subcommand when resuming, configures sandboxing via `-s read-only`, `-s workspace-write`, or `--dangerously-bypass-approvals-and-sandbox`, and appends `-m <model>` and `-c model_reasoning_effort=<effort>`.
4. **`src/antigravity.rs`**:
   - Lines 70–105: Implemented `pub fn interactive_args(cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`. Strips `--output-format stream-json` and `-p`/`--print`. Adds `--add-dir <cwd>`, maps permission modes (`AcceptEdits` -> `--mode accept-edits`, `Full` -> `--dangerously-skip-permissions`, `Plan` -> `--mode plan`), configures model and effort (suppressing `--effort` when the model name ends in `-low`, `-medium`, or `-high`), and appends `--conversation <id>` for session resumption.
5. **`src/agent.rs`**:
   - Lines 413–429: Implemented `pub fn build_interactive_command(provider: Provider, exe: &Path, cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> (PathBuf, Vec<String>)`, dispatching uniformly to each provider's `interactive_args`.
   - Lines 1125–1539: Added 13 sentence-named unit tests in `src/agent.rs::tests` covering flag construction for Claude, Codex, and Antigravity across fresh and resumed turns and all permission modes.

### Test & Lint Verification Outputs
- `cargo check`:
  `Finished 'dev' profile [unoptimized + debuginfo] target(s) in 0.38s` (0 errors, 0 warnings).
- `cargo test`:
  `test result: ok. 260 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 2.11s`. (Previously 237 passed; 23 new unit tests pass, 0 regressions, all 8 pre-existing ignored tests remain ignored).
- `cargo clippy --all-targets -- -D warnings`:
  `Finished 'dev' profile [unoptimized + debuginfo] target(s) in 0.39s` (0 warnings).

---

## 2. Logic Chain

1. **Eliminating ConPTY Error 193**:
   Windows `CreateProcessW` inside `portable_pty` rejects non-PE executables (like `claude.cmd` created by `npm install -g @anthropic-ai/claude-code`) with error 193 (*"%1 is not a valid Win32 application"*). In `src/terminal.rs`, detecting `.cmd` and `.bat` extensions case-insensitively and launching them via `cmd.exe /c <script> <args...>` routes the invocation through Windows' command interpreter, which successfully executes batch scripts within ConPTY.
2. **Terminal Generalization with Zero Regressions**:
   Extracting `Terminal::spawn(cmd, ctx, err_msg)` consolidates PTY creation (`openpty`), slave spawning, Win32 Job Object creation (`TerminalJob`), background reader thread spawning with CSI cursor reply callback handling, and repainting. `Terminal::start` simply passes `configure(&mut cmd, shell)` to `Terminal::spawn`, ensuring existing shell tabs in the tools panel behave identically.
3. **ANSI TrueColor Environment Injection**:
   Interactive CLIs query the environment to determine ANSI color and cursor formatting support. In `build_command`, explicitly injecting `TERM=xterm-256color` and `COLORTERM=truecolor` guarantees that all interactive sessions display 24-bit TrueColor and proper escape sequences on Windows and Unix alike.
4. **Decoupled Interactive Command Building**:
   Having `agent::build_interactive_command` return `(PathBuf, Vec<String>)` decouples CLI argument synthesis from terminal PTY execution. This enables rapid, deterministic, pure unit testing in `agent.rs` (< 2ms) without spawning processes or making billable network calls.
5. **Headless vs Interactive Flag Separation**:
   In headless mode, flags like `-p`, `--output-format stream-json`, `exec`, and `--permission-prompts none` suppress user interaction. In interactive terminal mode, these flags are omitted, while native TUI flags (`--model`, `--effort`, `--resume`, sandbox flags, and permission bypass flags) are configured to let the CLI render its own interactive prompts directly into the PTY buffer.

---

## 3. Caveats

- **Milestone Boundaries**:
  Milestone 1 implements the backend PTY command runner and provider interactive argument builders. UI wiring in `src/app.rs` (replacing `chat_area` with `terminal_area` in `CentralPanel`) and session lifecycle maps will be completed in Milestones 2 and 3.
- **Pre-existing Ignored Tests**:
  The 8 pre-existing ignored tests (`closing_a_terminal_stops_programs_started_in_it`, `finds_the_shells_on_this_computer`, paid API tests) were preserved as ignored per `AGENTS.md §3.2`.
- **Allowed Attributes**:
  Items prepared for Milestone 2 UI integration (`build_command`, `wrap_batch_command`, `Terminal::start_command`, `Terminal::write_all`, `interactive_args`, `build_interactive_command`) have `#[allow(dead_code)]` with comments explaining their usage, keeping `cargo clippy --all-targets -- -D warnings` at zero warnings.

---

## 4. Conclusion

Milestone 1 is complete, fully functional, and verified:
- `src/terminal.rs`: `is_batch_script`, `comspec`, `wrap_batch_command`, `build_command`, and `Terminal::start_command` are implemented and verified.
- `src/claude.rs`: `interactive_args` is implemented and verified.
- `src/codex.rs`: `interactive_args` is implemented and verified.
- `src/antigravity.rs`: `interactive_args` is implemented and verified.
- `src/agent.rs`: `build_interactive_command` is implemented and verified.
- Test suite expanded from 237 to 260 passing tests (+23 new sentence-named unit tests).
- All acceptance criteria satisfied: 0 errors on `cargo check`, 0 failures on `cargo test`, 0 warnings on `cargo clippy --all-targets -- -D warnings`, no formatting changes, and zero `Cargo.toml` dependencies added.

---

## 5. Verification Method

To independently verify the implementation:

1. **Compilation Check**:
   ```powershell
   cargo check
   ```
   Expected: exits 0 in < 1s with 0 errors and 0 warnings.

2. **Run Full Test Suite**:
   ```powershell
   cargo test
   ```
   Expected: 260 passed; 0 failed; 8 ignored in ~2s.

3. **Targeted Terminal Tests**:
   ```powershell
   cargo test terminal::tests::
   ```
   Expected: 16 passed; 0 failed; 2 ignored.

4. **Targeted Interactive Command Tests**:
   ```powershell
   cargo test agent::tests::interactive
   cargo test agent::tests::build_interactive_command
   ```
   Expected: all interactive command builder tests pass.

5. **Linter Check**:
   ```powershell
   cargo clippy --all-targets -- -D warnings
   ```
   Expected: exits 0 with 0 warnings.

6. **File Scope & Integrity Verification**:
   ```powershell
   git status
   ```
   Expected: only `src/terminal.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, and `src/agent.rs` are modified. `Cargo.toml` is untouched.
