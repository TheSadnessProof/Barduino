# BRIEFING — 2026-09-19T00:35:00Z

## Mission
Conduct architectural and technical survey of the terminal and PTY infrastructure in Viper for hosting interactive AI provider CLIs in the middle panel.

## 🔒 My Identity
- Archetype: explorer
- Roles: survey, architectural investigation
- Working directory: c:\Users\ditob\Documents\viper\.agents\survey_explorer_1
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: Terminal/PTY Survey for Middle Panel AI CLI

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Do NOT modify any code or files outside your working directory
- Deliver findings in handoff.md with 5 components
- Windows first, pure Rust, eframe/egui, no async runtime

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: 2026-09-19T00:35:00Z

## Investigation State
- **Explored paths**: `Cargo.toml`, `src/terminal.rs`, `src/tools.rs`, `src/app.rs`, `src/session.rs`, `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, `src/sidebar.rs`
- **Key findings**:
  1. `portable-pty` (0.9.0) and `vt100` (0.16.2) are fully functional and tested in `src/terminal.rs` for tools panel shells.
  2. ConPTY startup handshake is handled via `Replies` struct answering DSR cursor query (`\x1b[6n`), device status (`\x1b[5n`), device attributes (`\x1b[c`).
  3. Windows process tree cleanup is handled via `TerminalJob` assigning `child.process_id()` to a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
  4. Dynamic resize is implemented via `(rect, response) = ui.allocate_exact_size(ui.available_size(), ...)` checking `(rows, cols) != self.size` and calling `master.resize(...)` + `parser().screen_mut().set_size(...)`.
  5. Keystroke capture handles `Event::Text`, `Event::Paste` (with `bracketed_paste`), `Event::Copy` (interrupt 0x03 if no selection), `Event::Cut` (0x18), and `key_sequence` for Enter (`\r`), Backspace (`0x7f`), Tab, Escape, Arrows (with Ctrl/app_cursor), Home, End, PageUp, PageDown, Delete, and Ctrl+A..Z.
  6. Egui focus lock is applied via `m.set_focus_lock_filter(...)` for tab, arrows, escape so egui doesn't steal focus.
  7. Key adaptations needed for middle panel: generalize `Terminal::start` to run arbitrary executables (`Terminal::start_command` or `start_provider`), handle Windows `.cmd` batch scripts (e.g. `claude.cmd` via `cmd.exe /c`), wire dedicated `Terminal` into `Session` (or session-keyed map) with lifecycle management (restart, clean drop on deletion), and replace `chat_area` with middle terminal rendering.
- **Unexplored areas**: None for read-only survey scope. Ready for handoff synthesis.

## Key Decisions Made
- Confirmed that `Terminal` can be generalized cleanly without new external dependencies.
- Confirmed `TerminalJob` needs `unsafe impl Send for TerminalJob {}` if stored inside multi-threaded data structures, though `ViperApp` and `Session` on main thread work without it.
- Confirmed ConPTY Windows constraints are already well-handled by the existing `terminal.rs` architecture and can be extended to host AI CLIs directly.

## Artifact Index
- DISPATCH.md — incoming dispatch instructions
- progress.md — liveness heartbeat and milestone tracking
- BRIEFING.md — persistent working memory
- handoff.md — final 5-component handoff report
