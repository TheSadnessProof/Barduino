# Milestone 1 Quality & Adversarial Review Report

## Review Summary

**Verdict**: APPROVE

Milestone 1 changes in `src/terminal.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, and `src/agent.rs` correctly, robustly, and cleanly implement the backend PTY command execution and provider interactive argument building required by Milestone 1 of the Viper interactive terminal integration.

---

## 1. Observation

### Code Changes Inspected
1. **`src/terminal.rs`**:
   - Lines 27–29: `pub struct Replies { pub pending: Vec<u8> }` exported to allow `Terminal::parser` to expose a valid public return signature.
   - Lines 176–199: `is_batch_script(program: &Path) -> bool` detects `.cmd` and `.bat` extensions case-insensitively, falling back to searching `PATH` via `agent::find_on_path` when `program` has no extension. A Unix stub returning `false` is provided under `#[cfg(not(windows))]`.
   - Lines 201–208: `comspec() -> PathBuf` inspects `%ComSpec%`, verifies that the candidate is a valid existing file (`p.is_file()`), falls back to finding `cmd.exe` on `PATH`, and defaults to `PathBuf::from("cmd.exe")`.
   - Lines 210–222: `wrap_batch_command(program: &Path, args: &[String]) -> (PathBuf, Vec<String>)` wraps batch scripts via `cmd.exe /c <script> <args...>` on Windows.
   - Lines 227–247: `build_command(cwd: &Path, program: &Path, args: &[String]) -> CommandBuilder` wraps batch scripts with `comspec()` and `/c` on Windows, preserves native executables unwrapped, sets working directory `cwd`, and injects `TERM=xterm-256color` and `COLORTERM=truecolor`.
   - Lines 251–256: Refactored `Terminal::start` to invoke private `Terminal::spawn(cmd, ctx, "Couldn't start the shell")`, preserving existing shell tab behavior with zero regressions.
   - Lines 260–272: `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>` validates `cwd.is_dir()`, builds command via `build_command`, and spawns inside the PTY via `Terminal::spawn`.
   - Lines 326–340: Added public accessors `Terminal::parser()`, `Terminal::has_exited()`, and `Terminal::write_all(&self, bytes: &[u8])`.
   - Lines 969–1248: Added 16 unit and stress tests in `terminal::tests` covering environment variables, batch wrapping, case insensitivity, binary passthrough, PTY startup, child process cleanup, batch process tree cleanup, and failure modes.

2. **`src/claude.rs`**:
   - Lines 66–100: `pub fn interactive_args(_cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`:
     - Strips headless tokens (`-p`, `--output-format`, `--verbose`, `--include-partial-messages`, `--permission-prompts none`).
     - Maps `PermissionMode::ReadOnly` to `["--permission-mode", "default", "--disallowed-tools", "Bash"]`.
     - Maps `PermissionMode::AcceptEdits` to `["--permission-mode", "acceptEdits"]`.
     - Maps `PermissionMode::Full` to `["--dangerously-skip-permissions"]`.
     - Maps `PermissionMode::Plan` to `["--permission-mode", "plan"]`.
     - Appends `--model <model>`, `--effort <effort>`, and `--resume <session_id>` when present.

3. **`src/codex.rs`**:
   - Lines 79–110: `pub fn interactive_args(cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`:
     - Strips headless flags (`exec`, `--json`, `-`).
     - Adds `["resume", session_id]` as subcommand when resuming.
     - Adds `["-C", cwd.display().to_string()]` for working directory.
     - Maps `PermissionMode::ReadOnly` and `Plan` to `["-s", "read-only"]`.
     - Maps `PermissionMode::AcceptEdits` to `["-s", "workspace-write"]`.
     - Maps `PermissionMode::Full` to `["--dangerously-bypass-approvals-and-sandbox"]`.
     - Appends `["-m", model]` and `["-c", "model_reasoning_effort=<effort>"]`.

4. **`src/antigravity.rs`**:
   - Lines 70–100: `pub fn interactive_args(cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> Vec<String>`:
     - Strips `--output-format stream-json` and bare `--print`/`-p`.
     - Adds `["--add-dir", cwd.display().to_string()]`.
     - Maps `PermissionMode::AcceptEdits` to `["--mode", "accept-edits"]`, `Full` to `["--dangerously-skip-permissions"]`, and `Plan` to `["--mode", "plan"]`.
     - De-duplicates `--effort` when `model` name already encodes level (`-low`, `-medium`, `-high`).
     - Appends `["--conversation", resume_id]`.

5. **`src/agent.rs`**:
   - Lines 413–429: `pub fn build_interactive_command(...) -> (PathBuf, Vec<String>)` dispatches to each provider's `interactive_args`.
   - Lines 1125–1618: Added 13 comprehensive unit tests validating flag generation across fresh/resumed turns and all permission modes.

### Verification Commands & Outputs
1. `cargo check`:
   - Command: `cargo check`
   - Output: `Finished dev profile [unoptimized + debuginfo] target(s) in 1.20s` (0 errors, 0 warnings).
2. `cargo test`:
   - Command: `cargo test`
   - Output: `test result: ok. 266 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 3.38s`.
3. `cargo clippy --all-targets -- -D warnings`:
   - Command: `cargo clippy --all-targets -- -D warnings`
   - Output: `Finished dev profile [unoptimized + debuginfo] target(s) in 3.54s` (0 warnings).
4. Formatting & Diff Audit:
   - Command: `git diff --stat -w` vs `git diff --stat`
   - Result: Identical diff stats (1104 insertions, 49 deletions across 8 files). No unsolicited reformatting or whitespace churn.
5. Dependency Audit:
   - File `Cargo.toml`: Clean, 0 dependencies added or changed. Conforms to `AGENTS.md §3.4`.

---

## 2. Logic Chain

1. **Elimination of ConPTY Error 193**:
   - Direct observation: Windows `CreateProcessW` fails with error 193 (*"%1 is not a valid Win32 application"*) when asked to execute script files directly.
   - Deduction: Because Claude Code on Windows is frequently installed via npm as `claude.cmd`, spawning it directly in `portable_pty` would crash.
   - Implementation: In `terminal.rs`, `is_batch_script` detects `.cmd` and `.bat` scripts case-insensitively and routes them through `comspec()` with `/c`. Testing confirms batch scripts spawn cleanly and grandchild processes are monitored and killed properly.
2. **Backward Compatibility & Regression Prevention**:
   - Direct observation: Existing secondary shell tabs in `src/tools.rs` rely on `Terminal::start`.
   - Deduction: Refactoring `Terminal::start` to invoke `Terminal::spawn(cmd, ctx, "Couldn't start the shell")` ensures zero behavioral regression for shell tabs, while providing a clean entry point `Terminal::start_command` for arbitrary commands.
3. **ANSI TrueColor Support**:
   - Direct observation: Interactive TUIs (Claude, Codex, Antigravity) detect terminal capabilities via `TERM` and `COLORTERM`.
   - Implementation: `build_command` sets `TERM=xterm-256color` and `COLORTERM=truecolor`. A dedicated test verified that the child process reading these variables in ConPTY receives them correctly.
4. **Interactive vs Headless Mode Decoupling**:
   - Direct observation: Headless runs require streaming JSON and suppression flags, whereas interactive terminals require native TUIs without stream formatting.
   - Implementation: Each provider module provides a pure `interactive_args` function and `agent::build_interactive_command` unifies them. The resulting arguments omit headless stream flags (`-p`, `--output-format`, `exec`, `--json`, `-`) and configure interactive flags (`-C`, `--add-dir`, `--dangerously-skip-permissions`, etc.).
5. **Process Group Lifetime Invariants**:
   - Direct observation: Windows Win32 Job Objects (`TerminalJob`) configure `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
   - Implementation: On `Terminal` drop, the underlying process tree (including grandchild processes started by batch scripts) is terminated cleanly. Stress tests verify that no orphan processes remain.

---

## 3. Adversarial Challenges & Edge Case Mining

### Challenge 1: Windows Batch Script Quoting and Paths with Spaces
- **Hypothesis**: Could `cmd.exe /c` strip quotes when executing scripts with spaces in their path?
- **Analysis**: Windows `cmd.exe /c` has a rule that preserves outer quotes if there is a single quoted executable string. When `build_command` invokes `comspec()`, `portable_pty` handles argument passing. Testing confirmed batch script execution succeeds under ConPTY.

### Challenge 2: Nonexistent Executable or Invalid CWD Handling
- **Hypothesis**: Could passing an invalid working directory or missing executable panic or crash the application?
- **Test Result**: `stress_terminal_failure_mode_invalid_directory` and `stress_terminal_failure_mode_nonexistent_executable` both pass, returning structured `Result::Err(String)` messages without panics.

### Challenge 3: Antigravity Reasoning Effort Flag Duplication
- **Hypothesis**: Models named with reasoning effort suffixes (e.g., `gemini-3.8-flash-high`) might cause invalid flag errors if `--effort high` is also passed.
- **Verification**: `antigravity::interactive_args` checks `model.ends_with(level)` for `["-low", "-medium", "-high"]` and omits `--effort` when already encoded in the model name. Unit test `antigravity_interactive_skips_effort_when_model_name_encodes_it` confirms this.

### Challenge 4: Process Leak During Rapid Spawning and Dropping
- **Hypothesis**: Rapidly spawning and immediately dropping terminals could exhaust PTY handles or leak Win32 Job Objects.
- **Test Result**: `stress_terminal_rapid_spawn_and_drop` spawned and dropped 20 terminal instances in sequence without error, handle exhaustion, or leak.

---

## 4. Integrity Violation Forensic Audit

- Hardcoded test results or expected outputs embedded in source code? **NONE.**
- Dummy or facade implementations that look correct but implement no real logic? **NONE.**
- Shortcuts that bypass the intended task? **NONE.**
- Fabricated verification outputs, logs, or attestation artifacts? **NONE.**
- Evidence of self-certifying work without genuine independent verification? **NONE.**

---

## 5. Caveats

- **Scope Boundary**:
  Milestone 1 intentionally encompasses backend PTY execution and interactive command builders (`src/terminal.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, and `src/agent.rs`). UI wiring into `src/app.rs` (central panel replacement, resize handling, focus lock, session switching) is scheduled for Milestones 2 and 3.
- **Pre-existing Ignored Tests**:
  The 8 pre-existing ignored tests (`closing_a_terminal_stops_programs_started_in_it`, `finds_the_shells_on_this_computer`, paid API tests) remain ignored in accordance with `AGENTS.md §3.2`.

---

## 6. Conclusion

The Milestone 1 implementation satisfies all functional requirements, architectural invariants, edge case defenses, and code quality standards:
- `cargo check`: 0 errors.
- `cargo test`: 266 passed; 0 failed; 8 ignored.
- `cargo clippy --all-targets -- -D warnings`: 0 warnings.
- No unauthorized dependencies added to `Cargo.toml`.
- No unsolicited formatting changes introduced.

**Verdict: APPROVE.** Ready to proceed to Milestone 2.

---

## 7. Verification Method

To independently reproduce and verify this review:
1. Check compilation:
   ```powershell
   cargo check
   ```
2. Run test suite:
   ```powershell
   cargo test
   ```
3. Run clippy with denied warnings:
   ```powershell
   cargo clippy --all-targets -- -D warnings
   ```
4. Verify git status and diff:
   ```powershell
   git status
   git diff --stat -w
   ```
