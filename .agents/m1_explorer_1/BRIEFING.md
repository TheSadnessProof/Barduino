# BRIEFING — 2026-09-19T00:41:00Z

## Mission
Provide a precise technical design and implementation blueprint for Milestone 1 in `src/terminal.rs`.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigation, analysis, synthesis, blueprint
- Working directory: c:\Users\ditob\Documents\viper\.agents\m1_explorer_1
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: Milestone 1

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Do NOT edit files outside working directory
- Follow AGENTS.md rules

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: 2026-09-19T00:37:05Z

## Investigation State
- **Explored paths**: `src/terminal.rs`, `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, `Cargo.toml`, `portable-pty` internals.
- **Key findings**:
  1. ConPTY Error 193 occurs because `CreateProcessW` cannot directly execute `.cmd`/`.bat` files without `cmd.exe /c`.
  2. `build_command` resolves batch extensions case-insensitively and prepends `cmd.exe /c` on Windows.
  3. `TERM=xterm-256color` and `COLORTERM=truecolor` enable 24-bit TrueColor and full ANSI escapes across CLIs.
  4. `TerminalJob` and Win32 Job Object assign child PIDs and kill the entire tree on drop.
  5. `Replies` callback answers ConPTY cursor position / status queries so it does not hang.
  6. `build_interactive_command` handles Claude, Codex, and Antigravity interactive CLI flags.
- **Unexplored areas**: Milestone 2 and 3 UI swap in `src/app.rs`.

## Key Decisions Made
- Designed `Terminal::start_command(cwd, program, args, ctx)` sharing `spawn` helper with `Terminal::start`.
- Designed `build_interactive_command` in `agent.rs` delegating to `interactive_args` in `claude.rs`, `codex.rs`, and `antigravity.rs`.
- Created comprehensive 5-component report in `handoff.md`.

## Artifact Index
- DISPATCH.md — Received dispatch instructions
- BRIEFING.md — Persistent working memory
- progress.md — Liveness heartbeat
- handoff.md — 5-component handoff report with exact implementation blueprint and test suites
