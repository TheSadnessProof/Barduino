## 2026-09-19T00:37:05Z
You are m1_explorer_3.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\m1_explorer_3

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\AGENTS.md

YOUR OBJECTIVE:
Design the unit test suite for Milestone 1 in `src/terminal.rs` and `src/agent.rs`.

SCOPE & INVESTIGATION:
1. Review test naming and style rules in `AGENTS.md` (§5.4): sentence-named tests, slice pattern destructuring, prose asserts.
2. Design unit tests for:
   - `build_interactive_command` for Claude (verifying flags, omitting headless flags).
   - `build_interactive_command` for Codex (verifying fresh vs resume, sandbox modes).
   - `build_interactive_command` for Antigravity (verifying `--add-dir`, conversation resume).
   - Windows `.cmd` batch wrapping behavior.
   - PTY command execution and process startup.
3. Provide ready-to-paste Rust test code for the worker.

CONSTRAINTS:
- Read-only. DO NOT edit files outside your working directory.
- Deliver findings in `c:\Users\ditob\Documents\viper\.agents\m1_explorer_3\handoff.md`.
- Send message to parent upon completion.
