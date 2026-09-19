# Milestone 1 Code Review & Adversarial Critic Report

## Review Summary

**Verdict**: APPROVE  
**Milestone**: Milestone 1 (PTY Provider Spawning & Command Builders)  
**Integrity Audit**: Clean — 0 integrity violations detected.  
**Repository Invariants**: Fully satisfied — `cargo check` (0 errors), `cargo test` (266 passed, 0 failed, 8 ignored), `cargo clippy --all-targets -- -D warnings` (0 warnings), git diff clean of formatting noise, 0 dependencies added to `Cargo.toml`.

---

## 1. Observation

Directly observed through tool invocations and source inspection:

1. **Repository Invariants Verification**:
   - `cargo check`:
     ```
     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.62s
     ```
     Exited 0 with 0 errors and 0 warnings.
   - `cargo test`:
     ```
     test result: ok. 266 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 3.55s
     ```
     Exited 0 with 0 failures. No ignored tests were run wholesale or removed.
   - `cargo clippy --all-targets -- -D warnings`:
     ```
     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.41s
     ```
     Exited 0 with zero warnings.
   - `git status`:
     Only modified files in `src/` are:
     - `src/terminal.rs`
     - `src/claude.rs`
     - `src/codex.rs`
     - `src/antigravity.rs`
     - `src/agent.rs`
     `Cargo.toml` and `Cargo.lock` are untouched (0 new dependencies added).
   - `git diff --check src/`:
     Exited 0 with zero whitespace or line-formatting errors.

2. **Source Code Modifications**:
   - `src/terminal.rs`:
     - Lines 176–199: Added `is_batch_script(program: &Path) -> bool` (`#[cfg(windows)]` and `#[cfg(not(windows))]`). Detects `.cmd` and `.bat` extensions case-insensitively and falls back to searching on PATH via `agent::find_on_path`.
     - Lines 201–208: Added `comspec() -> PathBuf`, resolving `%ComSpec%` with fallback to `cmd.exe` on PATH or bare `"cmd.exe"`.
     - Lines 210–222: Added `wrap_batch_command(program: &Path, args: &[String]) -> (PathBuf, Vec<String>)`.
     - Lines 224–247: Added `build_command(cwd: &Path, program: &Path, args: &[String]) -> CommandBuilder`. On Windows, batch files are wrapped with `comspec()` and `/c`; on Unix and non-batch binaries, the executable is invoked directly. Injects `TERM=xterm-256color` and `COLORTERM=truecolor`, and sets working directory to `cwd`.
     - Lines 250–256: Refactored `Terminal::start(cwd: &Path, shell: &Path, ctx: egui::Context) -> Result<Self, String>` to delegate to `Terminal::spawn`.
     - Lines 258–272: Added `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>`. Validates `cwd.is_dir()` and delegates to `build_command` and `Terminal::spawn`.
     - Lines 274–324: Private `Terminal::spawn(cmd: CommandBuilder, ctx: egui::Context, spawn_err_msg: &str) -> Result<Self, String>`. Consolidated PTY initialization, slave spawning, Win32 Job Object creation (`TerminalJob`), background reader thread, CSI cursor position callback processing (`Replies`), and repainting.
     - Lines 326–340: Added `Terminal::parser(&self) -> MutexGuard<'_, Parser>`, `Terminal::has_exited(&mut self) -> bool`, and `Terminal::write_all(&self, bytes: &[u8])`.
     - Lines 969–1248: Added 16 sentence-named unit and stress tests covering environment variables, batch wrapping, direct binary execution, invalid paths, PTY output capture, and process tree termination.
   - `src/claude.rs`:
     - Lines 66–100: Added `interactive_args(_cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`. Omitted headless flags (`-p`, `--output-format`, `--verbose`, `--include-partial-messages`, `--permission-prompts none`). Configured permission modes (`ReadOnly` -> `default` + `--disallowed-tools Bash`, `AcceptEdits` -> `acceptEdits`, `Full` -> `--dangerously-skip-permissions`, `Plan` -> `plan`). Added `--model`, `--effort`, and `--resume` flags.
   - `src/codex.rs`:
     - Lines 79–110: Added `interactive_args(cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`. Omitted headless tokens (`exec`, `--json`, `-`). Configured interactive resumption (`resume <id>`), working directory (`-C <cwd>`), sandbox policy (`-s read-only`, `-s workspace-write`, `--dangerously-bypass-approvals-and-sandbox`), and model/effort flags (`-m <model>`, `-c model_reasoning_effort=<effort>`).
   - `src/antigravity.rs`:
     - Lines 70–100: Added `interactive_args(cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`. Omitted headless flags (`--output-format stream-json`, `-p <prompt>`). Added `--add-dir <cwd>`, mapped permission modes (`AcceptEdits` -> `--mode accept-edits`, `Full` -> `--dangerously-skip-permissions`, `Plan` -> `--mode plan`), handled model and effort with `-low`, `-medium`, `-high` deduplication, and added `--conversation <id>` for resumption.
   - `src/agent.rs`:
     - Lines 414–429: Added `build_interactive_command(provider: Provider, exe: &Path, cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> (PathBuf, Vec<String>)`, uniformly routing to each provider's `interactive_args`.
     - Lines 1129–1618: Added 13 unit tests verifying flag construction across all providers, fresh turns, resumed sessions, and permission modes.

---

## 2. Logic Chain

1. **Integrity Forensics Evaluation**:
   - Examined source files for hardcoded test outcomes, empty facades, or bypassed logic. All functions (`is_batch_script`, `comspec`, `wrap_batch_command`, `build_command`, `Terminal::start_command`, `interactive_args`, `build_interactive_command`) contain real, genuine production logic.
   - Tests execute real processes via PTY and verify actual process termination, output capture, and parser screen state. No mock-bypassing or fabricated outputs exist.
   - Conclusion: Zero integrity violations.

2. **Adversarial Assessment of Windows Batch Script Wrapping**:
   - *Attack Angle 1: Non-batch binary falsely wrapped with cmd.exe*:
     `is_batch_script` checks `has_batch_ext` (`cmd` or `bat`, case-insensitive). If an executable has extension `.exe` or any other extension, `has_batch_ext` is false and `program.extension().is_none()` is false. Non-batch executables like `agy.exe`, `codex.exe`, `python.exe` are never wrapped with `cmd.exe`.
   - *Attack Angle 2: Batch script without extension on PATH*:
     If `program` has no extension (e.g. `Path::new("claude")`), `program.extension().is_none()` is true. It checks `agent::find_on_path(&[&format!("{name}.cmd"), &format!("{name}.bat")])`. If found, it wraps it. If not found, it does not wrap.
   - *Attack Angle 3: Non-Windows environments*:
     On non-Windows (`#[cfg(not(windows))]`), `is_batch_script` is a constant `false`, and `build_command` uses `CommandBuilder::new(program)` directly without `cmd.exe /c`.
   - *Attack Angle 4: Paths containing spaces*:
     `CommandBuilder` in `portable_pty` quotes path and argument tokens according to Windows escaping rules. In `wrap_batch_command`, tokens are kept in a `Vec<String>` where the script path is a distinct argument element after `/c`.

3. **Adversarial Assessment of Interactive vs Headless Flag Separation**:
   - *Claude Code*:
     Headless mode passed `-p`, `--verbose`, `--output-format stream-json`, `--include-partial-messages`, and `--permission-prompts none`. All 5 are completely removed from `interactive_args`. `Full` permission mode correctly uses `--dangerously-skip-permissions` instead of internal headless strings.
   - *Codex*:
     Headless mode passed `exec`, `--json`, and `-` (reading prompt from stdin). In `interactive_args`, `exec`, `--json`, and `-` are omitted. Resuming a conversation uses `codex resume <id>` as the initial subcommand (without `exec`). Working directory `-C <cwd>` and sandbox flags (`-s read-only`, `-s workspace-write`, `--dangerously-bypass-approvals-and-sandbox`) are properly formatted.
   - *Antigravity*:
     Headless mode passed `--output-format stream-json` and `-p <prompt>`. In `interactive_args`, these are omitted. `--add-dir <cwd>` is included (required by Antigravity). Suffix deduplication for models ending in `-low`, `-medium`, `-high` prevents duplicate `--effort` flags that cause CLI errors.

4. **Terminal::spawn Behavioral Equivalence & Resource Cleanup**:
   - `Terminal::start` constructs `cmd = CommandBuilder::new(shell)`, runs `configure(&mut cmd, shell)`, sets `cmd.cwd(cwd)`, and passes it to `Terminal::spawn(cmd, ctx, "Couldn't start the shell")`. This produces the exact same setup and error string as the previous implementation.
   - PTY slave handle is explicitly dropped in the parent process (`drop(pair.slave)`), allowing EOF detection when the child process terminates.
   - `TerminalJob` uses Windows Job Objects configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. When `Terminal` is dropped, `TerminalJob::drop` calls `TerminateJobObject` and `CloseHandle`. Grandchild processes (such as child processes of `cmd.exe` or compilers) are terminated immediately. Stress tests `stress_terminal_cleanly_kills_child_and_grandchild_process_tree` and `stress_terminal_cleanly_kills_batch_script_process_tree` independently confirm zero orphaned background processes.
   - On Windows, `has_exited()` inspects both `self.exited.load(Ordering::Relaxed)` and `self.child.try_wait()`, avoiding hangs if the PTY pipe remains open after child process death.

---

## 3. Caveats

- **Scope Delimitation**:
  This review covers Milestone 1 backend PTY execution and interactive command building. Wiring the embedded terminal into `app.rs` CentralPanel UI and replacing `chat_area` is part of Milestone 2 per the project roadmap.
- **Allowed Attributes**:
  The newly introduced public methods (`Terminal::start_command`, `Terminal::write_all`, `interactive_args`, `build_interactive_command`) carry `#[allow(dead_code)]` with descriptive comments explaining their upcoming use in Milestone 2. This satisfies the invariant of 0 warnings under `cargo clippy --all-targets -- -D warnings`.
- **Ignored Tests**:
  The 8 pre-existing `#[ignore]` tests (such as paid CLI tests and host-dependent tests) remain intact and ignored as mandated by `AGENTS.md §3.2`.

---

## 4. Conclusion

Milestone 1 is implemented cleanly, robustly, and with complete adherence to repository invariants and architectural standards:
- All 4 review criteria in the prompt are satisfied without defects.
- Codebase builds with 0 errors (`cargo check`), passes all 266 unit and stress tests (`cargo test`), and produces 0 warnings (`cargo clippy --all-targets -- -D warnings`).
- No formatting noise, no unauthorized dependencies, and no integrity violations.

**Verdict**: **APPROVE**

---

## 5. Verification Method

To independently reproduce this verification:

1. **Compilation Check**:
   ```powershell
   cargo check
   ```
   *Expected*: exits 0 in < 1s with 0 errors and 0 warnings.

2. **Run Full Test Suite**:
   ```powershell
   cargo test
   ```
   *Expected*: `266 passed; 0 failed; 8 ignored` in ~3-4s.

3. **Targeted Terminal & Stress Tests**:
   ```powershell
   cargo test terminal::tests::
   ```
   *Expected*: 16 passed, 0 failed, 2 ignored.

4. **Targeted Interactive Command Tests**:
   ```powershell
   cargo test agent::tests::interactive
   cargo test agent::tests::build_interactive_command
   ```
   *Expected*: all 13 interactive builder tests pass.

5. **Clippy Linter Verification**:
   ```powershell
   cargo clippy --all-targets -- -D warnings
   ```
   *Expected*: exits 0 with 0 warnings.

6. **Scope and Cleanliness Audit**:
   ```powershell
   git status
   git diff --check src/
   git diff Cargo.toml
   ```
   *Expected*: only the 5 target source files are modified, no whitespace warnings, `Cargo.toml` is untouched.
