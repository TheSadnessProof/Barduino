# BRIEFING — 2026-09-19T02:17:30Z

## Mission
Investigate Milestone 3: SavedState Backward Compatibility & Tools Panel Coexistence, verifying serialization contracts, clean restart spawning, widget ID isolation, and tab switching coexistence.

## 🔒 My Identity
- Archetype: explorer
- Roles: investigation, synthesis
- Working directory: c:\Users\ditob\Documents\viper\.agents\m3_explorer_2
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: Milestone 3 (SavedState Backward Compatibility & Tools Panel Coexistence)

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Do NOT modify any code or files outside your working directory (.agents/m3_explorer_2/)
- Deliver findings in a structured handoff report at .agents/m3_explorer_2/handoff.md
- Follow AGENTS.md rules strictly (Rule 3.1 no full screen capture, Rule 3.5 backward compatibility)

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: 2026-09-19T02:17:30Z

## Investigation State
- **Explored paths**: `src/app.rs`, `src/tools.rs`, `src/terminal.rs`, `src/session.rs`, `src/agent.rs`, `src/settings.rs`, `src/browser.rs`, `src/sidebar.rs`
- **Key findings**:
  1. `provider_terminals` is strictly an ephemeral field on `ViperApp` (`src/app.rs:144`), completely absent from `SavedState` (`src/app.rs:43-61`). `eframe::App::save` serializes only `SavedState`.
  2. Rule 3.5 backward compatibility is preserved: `#[serde(default)]` on structs, `#[serde(alias = "Gemini")]` for `Antigravity`, `#[serde(alias = "claude_session_id")]` for `agent_session_id`, `carry_browser_over`, and `apply_panel_defaults`.
  3. On launch/restart, sessions are restored into `self.state.sessions`. `provider_terminals` initializes empty. When rendered, `resolve_terminal_state` checks `session.has_folder()`. Ready sessions lazily spawn their CLI in `session.working_dir()` via `build_interactive_command` and `Terminal::start_command`. Unconfigured sessions show folder picker without spawning.
  4. Widget IDs between middle provider terminal (`("session_terminal", session_id)`) and right tools panel (`("terminal", number)`, `("tool_tab", index)`, `("diff", path)`) are mathematically disjoint.
  5. Keystroke input and focus routing are mutually exclusive via `response.has_focus()`. Whichever terminal is focused locks Tab, arrows, Escape via `set_focus_lock_filter`. Shortcuts (`Ctrl+\``, `Ctrl+Shift+B`) are consumed in `logic()` before input handling.
  6. Toggling tools panel tabs (Diffs, Secondary Terminal, Browser) only mutates `tools.active` and has zero effect on the middle provider terminal. Resizing/collapsing the tools panel dynamically adjusts `CentralPanel` and triggers PTY resize `master.resize(pty_size(rows, cols))` seamlessly.
- **Unexplored areas**: None remaining in scope.

## Key Decisions Made
- Confirmed zero architectural defects in existing M1/M2/M3 foundations.
- Formulated concrete recommendations for 5 dedicated Milestone 3 regression tests to lock in backward compatibility and tools panel coexistence contracts.

## Artifact Index
- c:\Users\ditob\Documents\viper\.agents\m3_explorer_2\BRIEFING.md — Working memory
- c:\Users\ditob\Documents\viper\.agents\m3_explorer_2\progress.md — Heartbeat and step log
- c:\Users\ditob\Documents\viper\.agents\m3_explorer_2\handoff.md — Final handoff report
