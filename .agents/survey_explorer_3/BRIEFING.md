# BRIEFING — 2026-09-19T00:36:30Z

## Mission
Conduct a technical survey of ViperApp state management, session lifecycle, UI routing, and backward compatibility for replacing chat transcript/composer with an interactive terminal.

## 🔒 My Identity
- Archetype: explorer
- Roles: survey, analysis, architectural investigation
- Working directory: c:\Users\ditob\Documents\viper\.agents\survey_explorer_3
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: survey_terminal_replacement

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Do NOT modify any code or files outside working directory
- 100% backward compatibility with SavedState (AGENTS.md Rule 3.5)
- Zero unauthorized dependencies (AGENTS.md Rule 3.4)
- Clippy zero warnings, tests pass, formatting preserved

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: 2026-09-19T00:36:30Z

## Investigation State
- **Explored paths**: `src/app.rs`, `src/session.rs`, `src/chat.rs`, `src/sidebar.rs`, `src/tools.rs`, `src/terminal.rs`, `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`
- **Key findings**:
  - `ViperApp::ui` cleanly routes panels: `left_panel`, `right_panel`, and `CentralPanel`. Replacing `chat_area` with `terminal_area` preserves sidebar and tools panels without altering their layout or state.
  - Per-session terminal state (`Terminal`, `Parser`, PTY handles) must be kept strictly at runtime (either on `ViperApp.provider_terminals: BTreeMap<u64, Result<Terminal, String>>` or `Session.terminal` marked `#[serde(skip)]`).
  - SavedState (RON) remains 100% backward-compatible: all existing fields and aliases (`Gemini`, `claude_session_id`) remain intact, and no PTY handles are serialized.
  - On restart, sessions deserialize cleanly and spawn fresh interactive CLI processes in `session.working_dir()` on demand.
  - Sidebar session switching (`SidebarAction::Select(id)`) switches to the session's active terminal buffer and immediately transfers focus via `take_keyboard` / `request_focus()`.
  - Session deletion (`delete_session(id)`) drops the terminal, invoking `job.kill()` (Windows Job Object with kill-on-close, Unix process group SIGKILL) and child kill, preventing orphan processes.
  - `terminal.rs` already supports dynamic PTY resizing on egui rect changes, full ANSI color palettes, application cursor sequences, and bracketed paste. A `start_process` helper is needed to launch CLI binaries (with Windows `.cmd` wrapper support).
- **Unexplored areas**: None. Technical survey is comprehensive across all 5 requested scope areas.

## Key Decisions Made
- Concluded investigation and produced comprehensive handoff report at `c:\Users\ditob\Documents\viper\.agents\survey_explorer_3\handoff.md`.

## Artifact Index
- c:\Users\ditob\Documents\viper\.agents\survey_explorer_3\DISPATCH.md — Initial dispatch instructions
- c:\Users\ditob\Documents\viper\.agents\survey_explorer_3\BRIEFING.md — Working memory
- c:\Users\ditob\Documents\viper\.agents\survey_explorer_3\progress.md — Liveness heartbeat
- c:\Users\ditob\Documents\viper\.agents\survey_explorer_3\handoff.md — Final handoff report
