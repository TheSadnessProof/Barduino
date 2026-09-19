# BRIEFING — 2026-09-19T01:50:00Z

## Mission
Provide a precise technical design and blueprint for Milestone 2: Replacing `chat_area` with `terminal_area` in `src/app.rs`.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigation, synthesis
- Working directory: c:\Users\ditob\Documents\viper\.agents\m2_explorer_1
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: Milestone 2 — Terminal Area in App

## 🔒 Key Constraints
- Read-only investigation — do NOT implement outside working directory
- Follow AGENTS.md rules strictly
- Do not add dependencies
- Tolerant, idiomatic Rust, zero clippy warnings

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: 2026-09-19T01:50:00Z

## Investigation State
- **Explored paths**: `src/app.rs`, `src/terminal.rs`, `src/agent.rs`, `src/session.rs`, `src/tools.rs`, `src/chat.rs`, `ORIGINAL_REQUEST.md`, `PROJECT.md`
- **Key findings**:
  - `chat_area` (lines 531-605) renders composer and conversation; will be replaced with `terminal_area`.
  - Pure decision logic extracted into `resolve_terminal_state` returning `TerminalState` enum (`NeedsFolder`, `MissingExecutable`, `Ready`).
  - Terminal spawning and dynamic sizing via `terminal.ui(ui, take_keyboard)` seamlessly handles PTY resize, ANSI rendering, and keyboard focus.
  - Ephemeral runtime map `provider_terminals: BTreeMap<u64, Result<Terminal, String>>` on `ViperApp` ensures 100% RON backward compatibility.
  - Left sidebar (`left_panel`) and right tools panel (`right_panel`) remain completely intact.
- **Unexplored areas**: None. Design is complete.

## Key Decisions Made
- Extracted `TerminalState` and `resolve_terminal_state` to decouple UI rules from egui rendering for pure unit testing per `verifying-a-ui-change` skill.
- Salted session terminal ID with `("session_terminal", session_id)` to avoid egui ID collisions across sessions.
- Mapped `take_keyboard` to `std::mem::take(&mut session.focus_composer)` so initial load and sidebar switches automatically focus the terminal.
- Preserved zero clippy warnings by marking/removing unused chat imports in `src/app.rs`.

## Artifact Index
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_1\DISPATCH.md` — Incoming dispatch log
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_1\BRIEFING.md` — Persistent memory
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_1\progress.md` — Liveness heartbeat
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_1\handoff.md` — 5-component handoff report and blueprint
