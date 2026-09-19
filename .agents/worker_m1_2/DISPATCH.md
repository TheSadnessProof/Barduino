## 2026-09-19T00:41:08Z

You are worker_m1_2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\worker_m1_2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md

EXPLORER HANDOFF REPORTS (Follow these blueprints closely):
- `c:\Users\ditob\Documents\viper\.agents\m1_explorer_1\handoff.md`
- `c:\Users\ditob\Documents\viper\.agents\m1_explorer_2\handoff.md`
- `c:\Users\ditob\Documents\viper\.agents\m1_explorer_3\handoff.md`

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

EXCLUSIVE FILE OWNERSHIP:
You own and may edit:
- `src/terminal.rs`
- `src/claude.rs`
- `src/codex.rs`
- `src/antigravity.rs`
- `src/agent.rs`
Do NOT modify other source files in this milestone.

OBJECTIVE - IMPLEMENT MILESTONE 1:
1. In `src/terminal.rs`:
   - Implement `is_batch_script` and `comspec` helper functions for Windows (detecting `.cmd` and `.bat` case-insensitively).
   - Implement `build_command(cwd: &Path, program: &Path, args: &[String]) -> CommandBuilder`:
     - Wraps batch scripts via `cmd.exe /c` on Windows to eliminate ConPTY Error 193.
     - Leaves `.exe` / Unix binaries direct.
     - Injects `TERM=xterm-256color` and `COLORTERM=truecolor`.
     - Sets working directory to `cwd`.
   - Implement `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>`.
   - Refactor `Terminal::start` to share the private `Terminal::spawn(cmd, ctx, err_msg)` implementation, preserving 100% backward compatibility.
   - Add sentence-named unit tests in `src/terminal.rs::tests` for command building, batch wrapping, and error handling.
2. In `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`:
   - Implement `pub fn interactive_args(...) -> Vec<String>` per the explorer blueprints.
   - Strip headless flags (`-p`, `--output-format`, `exec`, stdin token `-`).
   - Configure interactive flags (`--model`, `--effort`, `--resume`, sandbox/permission modes).
3. In `src/agent.rs`:
   - Implement `pub fn build_interactive_command(provider: Provider, exe: &Path, cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> (PathBuf, Vec<String>)`.
   - Add sentence-named unit tests in `src/agent.rs::tests` verifying Claude, Codex, and Antigravity interactive argument construction.

VERIFICATION REQUIREMENTS:
- Run `cargo check` (0 errors).
- Run `cargo test` (all tests pass, nothing newly ignored; DO NOT run ignored tests wholesale).
- Run `cargo clippy --all-targets -- -D warnings` (0 warnings).
- Do NOT run `cargo fmt`.
- Do NOT add any dependencies to `Cargo.toml`.

When finished, compile your handoff report in:
`c:\Users\ditob\Documents\viper\.agents\worker_m1_2\handoff.md`
and send a message to parent reporting completion.
