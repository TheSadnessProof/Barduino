# Forensic Integrity Audit Report — Milestone 1

**Work Product**: Milestone 1 Implementation (`src/terminal.rs`, `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`)
**Profile**: General Project (Development Mode per `ORIGINAL_REQUEST.md`)
**Verdict**: CLEAN

---

## 1. Observation

### Implementation Inspection
1. **`src/terminal.rs`**:
   - Lines 176–199: `is_batch_script(program: &Path) -> bool` inspects extension case-insensitively for `.cmd` and `.bat`, falling back to `agent::find_on_path` when the path lacks an extension.
   - Lines 201–208: `comspec() -> PathBuf` inspects `%ComSpec%`, falling back to PATH resolution for `cmd.exe`.
   - Lines 212–222: `wrap_batch_command(program: &Path, args: &[String]) -> (PathBuf, Vec<String>)` wraps batch scripts on Windows with `cmd.exe /c` while leaving binary executables untouched.
   - Lines 228–246: `build_command(cwd: &Path, program: &Path, args: &[String]) -> CommandBuilder` configures working directory, wraps batch scripts via `comspec()` on Windows, and injects `TERM=xterm-256color` and `COLORTERM=truecolor`.
   - Lines 261–272: `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>` validates `cwd.is_dir()`, builds the command, and invokes `Terminal::spawn`.
   - Lines 274–324: `Terminal::spawn(cmd: CommandBuilder, ctx: egui::Context, spawn_err_msg: &str) -> Result<Self, String>` opens the PTY pair (`portable_pty::native_pty_system().openpty`), spawns the slave process, attaches a Windows Job Object (`TerminalJob`), starts a reader thread delivering bytes into the vt100 `Parser` and writing replies back to the PTY, and repaints via `ctx.request_repaint()`.
   - Lines 326–340: `Terminal::parser`, `Terminal::has_exited`, and `Terminal::write_all` provide genuine public accessors.

2. **`src/claude.rs`**:
   - Lines 67–100: `interactive_args(cwd, model, effort, resume_id, permission_mode)` omits headless flags (`-p`, `--output-format`, `--permission-prompts none`), maps permission modes (`ReadOnly` -> `default` + `--disallowed-tools Bash`, `AcceptEdits` -> `acceptEdits`, `Full` -> `--dangerously-skip-permissions`, `Plan` -> `plan`), and formats `--model`, `--effort`, and `--resume`.

3. **`src/codex.rs`**:
   - Lines 80–110: `interactive_args(cwd, model, effort, resume_id, permission_mode)` omits `exec`, `--json`, and `-`, passes `resume <id>` when resuming, passes `-C <cwd>`, sets sandbox flags (`-s read-only`, `-s workspace-write`, `--dangerously-bypass-approvals-and-sandbox`), and formats `-m` and `-c model_reasoning_effort=...`.

4. **`src/antigravity.rs`**:
   - Lines 71–100: `interactive_args(cwd, model, effort, resume_id, permission_mode)` omits `--output-format` and `-p`/`--print`, adds `--add-dir <cwd>`, maps permission modes (`--mode accept-edits`, `--dangerously-skip-permissions`, `--mode plan`), passes `--model` and conditional `--effort` (omitting when model name carries `-low`, `-medium`, or `-high`), and passes `--conversation <id>` when resuming.

5. **`src/agent.rs`**:
   - Lines 414–429: `build_interactive_command(provider, exe, cwd, model, effort, resume_id, permission_mode) -> (PathBuf, Vec<String>)` dispatches cleanly to each provider's `interactive_args`.

### Empirical Audit Execution Results
- `git diff Cargo.toml`:
  Empty output. Zero dependencies added.
- `git diff -w --stat` vs `git diff --stat`:
  Identical stat: `8 files changed, 1104 insertions(+), 49 deletions(-)`. Zero whitespace or formatting churn on untouched lines.
- `cargo check`:
  `Finished 'dev' profile [unoptimized + debuginfo] target(s) in 0.37s` (0 errors).
- `cargo clippy --all-targets -- -D warnings`:
  `Finished 'dev' profile [unoptimized + debuginfo] target(s) in 0.72s` (0 warnings).
- `cargo test`:
  `test result: ok. 266 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 3.36s`.
  All 8 pre-existing ignored tests remain ignored; 0 tests were newly ignored.
- Test Authenticity:
  Unit tests in `src/terminal.rs` (lines 965–1248) and `src/agent.rs` (lines 1125–1618) execute real commands (`cmd.exe /c echo`, `ping -n ...`), verify output on the vt100 screen buffer, test environment variable propagation, and empirically verify process tree termination using `processes_with(...)`.

---

## 2. Logic Chain

1. **Authenticity of Implementation**:
   Inspection of `Terminal::start_command`, `build_command`, `wrap_batch_command`, `build_interactive_command`, and `interactive_args` shows zero facade patterns, zero dummy return constants, and no stub implementations. All logic performs genuine path manipulation, argument filtering, environment configuration, process spawning via `portable-pty`, and vt100 reader thread integration.
2. **Absence of Hardcoded Cheating & Facade Bypasses**:
   Test assertions in `src/terminal.rs` and `src/agent.rs` do not assert against self-certifying tautologies or hardcoded fake strings. `terminal_start_command_runs_process_and_captures_output` and `stress_terminal_receives_term_and_colorterm_in_child_process` execute real OS processes in a PTY and wait for terminal screen parser capture. `stress_terminal_cleanly_kills_child_and_grandchild_process_tree` asserts actual Win32 process termination across child and grandchild processes.
3. **Strict Adherence to Repository Invariants**:
   - `Cargo.toml` is completely untouched (conforms to `AGENTS.md §3.4`).
   - Untouched lines were not reformatted (`git diff -w` exactly matches `git diff`, conforming to `AGENTS.md §3.3`).
   - Zero clippy warnings with `-D warnings` (conforms to `AGENTS.md §2`).
   - No ignored tests were added or executed wholesale (conforms to `AGENTS.md §3.2`).

---

## 3. Caveats

- **Milestone Boundaries**:
  Milestone 1 covers backend PTY execution and interactive command building. Embedding the terminal widget into `ViperApp` (`app.rs`) to replace `chat_area` is assigned to Milestone 2 per `PROJECT.md`.
- **Pre-existing Ignored Tests**:
  The 8 pre-existing ignored tests (e.g. paid CLI integration and system-specific tests) were verified to remain ignored as required by `AGENTS.md §3.2`.

---

## 4. Conclusion

**Verdict**: **CLEAN**

Milestone 1 satisfies all forensic integrity checks:
1. Genuine implementation confirmed across all required functions.
2. Hardcoding, facades, dummy stubs, and cheating bypasses: NONE detected.
3. Repository invariants (zero new dependencies, zero clippy warnings, no untouched line churn, authentic tests) strictly maintained.
4. Binary verdict: **CLEAN**. Milestone 1 is verified and approved for Milestone 2 progression.

---

## 5. Verification Method

To reproduce and verify these forensic findings independently:

1. **Verify No Forbidden Dependencies**:
   ```powershell
   git diff Cargo.toml
   ```
   *Expected*: Empty diff.

2. **Verify Untouched Line Hygiene**:
   ```powershell
   git diff -w --stat
   git diff --stat
   ```
   *Expected*: Both outputs match exactly.

3. **Verify Compilation & Zero Warnings**:
   ```powershell
   cargo check
   cargo clippy --all-targets -- -D warnings
   ```
   *Expected*: Exits 0 with 0 errors and 0 warnings.

4. **Verify Full Test Suite & Real Process Spawning**:
   ```powershell
   cargo test
   ```
   *Expected*: 266 passed; 0 failed; 8 ignored in ~3s.
