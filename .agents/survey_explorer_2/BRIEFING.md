# BRIEFING — 2026-09-19T00:35:00Z

## Mission
Conduct a technical survey of provider CLI implementations and interactive process execution in Viper for replacing chat transcript/composer with dedicated interactive provider PTY terminals.

## 🔒 My Identity
- Archetype: explorer
- Roles: survey, investigation, synthesis
- Working directory: c:\Users\ditob\Documents\viper\.agents\survey_explorer_2
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: technical-survey-provider-clis

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Do NOT modify any files outside c:\Users\ditob\Documents\viper\.agents\survey_explorer_2
- Deliver findings in handoff.md with 5 components
- When done, notify parent via send_message

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: 2026-09-19T00:35:00Z

## Investigation State
- **Explored paths**: `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, `src/settings.rs`, `src/terminal.rs`, `src/tools.rs`, `src/session.rs`, `src/app.rs`, `src/worktree.rs`, `Cargo.toml`.
- **Key findings**:
  1. `find_executable` in each provider module prioritizes PATH, then OS-specific standard directories (npm/local/appdata on Windows).
  2. Spawning CLI interactively inside PTY drops headless flags (`-p`, `--output-format stream-json`, `codex exec --json`, bare stdin pipes) in favor of native interactive invocation (`claude`, `codex [resume]`, `agy`).
  3. Working directory must be passed via `cmd.cwd(session.working_dir())` (handles both root project directory and `.viper/worktrees/<session-id>`). Provider-specific args: `codex -C <dir>`, `agy --add-dir <dir>`.
  4. Process termination requires Windows Job Objects (`JOBOBJECT_EXTENDED_LIMIT_INFORMATION` + `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) to terminate the entire process tree on session delete or Viper exit.
  5. Multi-session management: Map `u64 -> Terminal` (either in `ViperApp` or per-`Session` with `#[serde(skip)]`).
- **Unexplored areas**: None within the requested scope.

## Key Decisions Made
- Recommending `portable_pty::CommandBuilder` builders per provider and generalizing `terminal::Terminal::start_command`.

## Artifact Index
- DISPATCH.md — record of incoming dispatch
- BRIEFING.md — persistent state memory
- progress.md — liveness heartbeat
- handoff.md — final survey report
