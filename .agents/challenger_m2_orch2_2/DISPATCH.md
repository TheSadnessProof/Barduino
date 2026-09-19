## 2026-09-19T01:58:13Z

<USER_REQUEST>
You are challenger_m2_orch2_2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\challenger_m2_orch2_2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md
c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2\handoff.md

YOUR OBJECTIVE:
Empirically stress-test dynamic resizing, PTY dimension synchronization, and terminal process cleanup in `src/app.rs`.

SCOPE & CHALLENGE:
1. Verify dynamic resize calculations across window dimensions (narrow, wide, extreme aspect ratios, 0x0).
2. Verify that deleting a session drops its running terminal and terminates the process tree immediately.
3. Verify that `SavedState` RON serialization and deserialization remain 100% backward compatible without storing PTY handles.
4. Conclude with a clear verdict: APPROVE or REQUEST_CHANGES.

CONSTRAINTS:
- Do NOT modify permanent codebase files.
- Deliver report in `c:\Users\ditob\Documents\viper\.agents\challenger_m2_orch2_2\handoff.md`.
- Send message to parent with verdict when complete.
</USER_REQUEST>
