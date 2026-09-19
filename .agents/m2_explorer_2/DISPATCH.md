## 2026-09-19T01:45:00Z

You are m2_explorer_2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\m2_explorer_2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md

YOUR OBJECTIVE:
Investigate dynamic resizing, keystroke routing, and focus locking for the middle terminal in `src/terminal.rs` and `src/app.rs`.

SCOPE & INVESTIGATION:
1. Analyze how `Terminal::ui(ui, take_keyboard)` works in `src/terminal.rs`:
   - Monospace font metrics and `ui.available_size()` calculation for rows and columns.
   - PTY resize propagation via `self.master.resize(pty_size(self.size))` and `screen_mut().set_size(rows, cols)`.
   - Focus locking filter: `m.set_focus_lock_filter(response.id, EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })`.
   - Handling of keystrokes: Enter (`\r`), Backspace (`0x7f`), Tab, arrow keys, Ctrl+[A-Z], bracketed paste.
2. Check how window resizing, sidebar expanding/collapsing, or panel width dragging affects the middle terminal layout.
3. Check restart button and process exit notice (`The process has exited. [Restart]`).
4. Detail any edge cases, coordinate adjustments, or focus subtleties when hosting the terminal in the central panel.

CONSTRAINTS:
- Read-only. DO NOT edit files outside your working directory.
- Deliver findings in `c:\Users\ditob\Documents\viper\.agents\m2_explorer_2\handoff.md`.
- Send message to parent upon completion.
