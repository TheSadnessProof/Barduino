# BRIEFING — 2026-09-19T01:50:00Z

## Mission
Investigate dynamic resizing, keystroke routing, and focus locking for the middle terminal in `src/terminal.rs` and `src/app.rs`.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigation, synthesis
- Working directory: c:\Users\ditob\Documents\viper\.agents\m2_explorer_2
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: milestone-2

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Deliver findings in c:\Users\ditob\Documents\viper\.agents\m2_explorer_2\handoff.md
- Send message to parent upon completion

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: 2026-09-19T01:50:00Z

## Investigation State
- **Explored paths**: `src/terminal.rs`, `src/app.rs`, `src/tools.rs`, `src/session.rs`, `src/agent.rs`
- **Key findings**:
  - Dynamic resizing accurately recomputes rows/columns via `ui.available_size()` with monospace font metrics (`FONT_SIZE = 13.0`, `PADDING = 6.0`) and propagates changes to both `master.resize` and `screen_mut().set_size`.
  - Keystroke routing handles Enter, Backspace, Tab, Shift+Tab backtab, arrows (normal, app_cursor, ctrl), Ctrl+[A-Z], bracketed paste, and copy/cut cleanly.
  - Focus locking filter `set_focus_lock_filter` prevents egui widget cycling on Tab, arrows, and Escape.
  - Process exit notice renders cleanly and clicking Restart returns `true`, triggering clean process tree teardown via `TerminalJob` and slot reset.
  - Critical integration requirements for CentralPanel: salted ID `("session_terminal", session.id)`, one-shot `take_keyboard` on session select, and pre-checks for unconfigured session folder and missing CLI executables.
- **Unexplored areas**: None within M2 scope.

## Key Decisions Made
- Fully documented all 4 scope areas in 5-component report `handoff.md`.

## Artifact Index
- DISPATCH.md — Recorded dispatch instructions
- progress.md — Progress log and liveness heartbeat
- handoff.md — 5-component investigation report
