## 2026-09-19T00:37:05Z

You are m1_explorer_2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\m1_explorer_2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\AGENTS.md

YOUR OBJECTIVE:
Provide a precise technical design and implementation blueprint for Milestone 1 in `src/agent.rs` (and provider modules if needed).

SCOPE & INVESTIGATION:
1. Examine `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, and `src/antigravity.rs`.
2. Design `build_interactive_command`:
   ```rust
   pub fn build_interactive_command(
       provider: Provider,
       exe: &Path,
       cwd: &Path,
       model: Option<&str>,
       effort: Option<&str>,
       resume_id: Option<&str>,
       permission_mode: PermissionMode,
   ) -> (PathBuf, Vec<String>)
   ```
   or returning a configured `portable_pty::CommandBuilder`.
3. Ensure flags for interactive mode match CLI capabilities:
   - Claude: `--model`, `--effort`, `--resume`, `--permission-mode` (or `--dangerously-skip-permissions` for Full mode). Omit `-p`, `--output-format`.
   - Codex: `codex` / `codex resume <id>`, `-C <cwd>`, `-m <model>`, `-c model_reasoning_effort=<effort>`, `-s <mode>` (or `--dangerously-bypass-approvals-and-sandbox`).
   - Antigravity: `--add-dir <cwd>`, `--conversation <id>`, `--model <model>`, `--effort <effort>`, `--mode <mode>` (or `--dangerously-skip-permissions`). Omit `-p`, `--output-format`.
4. Provide exact code snippets and line-by-line guidance for the worker.

CONSTRAINTS:
- Read-only. DO NOT edit files outside your working directory.
- Deliver findings in `c:\Users\ditob\Documents\viper\.agents\m1_explorer_2\handoff.md`.
- Send message to parent upon completion.
