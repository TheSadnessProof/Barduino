## 2026-09-19T02:14:00Z

You are m3_explorer_1.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\m3_explorer_1

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md

OBJECTIVE:
Investigate Milestone 3: Per-Session Lifecycle, Multi-Session Switching & Process Teardown.

SCOPE & INVESTIGATION:
1. Examine `src/app.rs`, `src/sidebar.rs`, `src/session.rs`, and `src/terminal.rs`.
2. Inspect how sessions are created (`new_session`), selected (`SidebarAction::Select`), renamed (`SidebarAction::Rename`), and deleted (`SidebarAction::Delete`).
3. Verify the behavior of `provider_terminals`:
   - When switching from Session A to Session B, does Session A's PTY stay alive in `provider_terminals`?
   - Does Session B receive immediate keyboard focus without needing a click?
   - If Session B was already spawned, is its terminal buffer preserved when switching back from Session A?
   - When Session A is deleted, is its terminal removed from `provider_terminals` and its process tree killed?
   - When Session A's folder is changed, is its terminal removed and re-spawned in the new directory?
4. Identify any edge cases or missing coverage in current code, and recommend any code enhancements or dedicated unit tests needed for Milestone 3.

CONSTRAINTS:
- You are read-only. DO NOT modify any code or files outside your working directory.
- Deliver findings in a structured handoff report at `c:\Users\ditob\Documents\viper\.agents\m3_explorer_1\handoff.md`.
- When done, send a message to parent reporting completion.
