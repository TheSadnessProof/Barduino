## 2026-09-19T01:45:00Z
You are m2_explorer_3.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\m2_explorer_3

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md

YOUR OBJECTIVE:
Design the unit test suite for Milestone 2 in `src/app.rs` and related modules.

SCOPE & INVESTIGATION:
1. Review test naming and style rules in `AGENTS.md` (§5.4): sentence-named tests, slice pattern destructuring, prose asserts.
2. Review the UI verification playbook in `.agents/skills/verifying-a-ui-change/SKILL.md` (testing logic moved out of rendering, headless verification).
3. Design unit tests covering:
   - Central view displays terminal area instead of chat composer.
   - Unconfigured session without folder shows empty state guidance and does not attempt terminal spawn.
   - Session with missing executable displays warning and does not panic.
   - Terminal resizing calculates rows and columns correctly based on allocated rect.
   - Keyboard focus flag transfer.
4. Provide ready-to-paste Rust test code for the worker.

CONSTRAINTS:
- Read-only. DO NOT edit files outside your working directory.
- Deliver findings in `c:\Users\ditob\Documents\viper\.agents\m2_explorer_3\handoff.md`.
- Send message to parent upon completion.
