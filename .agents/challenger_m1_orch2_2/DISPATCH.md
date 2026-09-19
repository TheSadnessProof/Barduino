## 2026-09-19T00:48:01Z

You are challenger_m1_orch2_2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\challenger_m1_orch2_2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\.agents\worker_m1_2\handoff.md

YOUR OBJECTIVE:
Empirically stress-test ConPTY lifecycle, environment variables, and process tree termination under Win32 Job Objects.

SCOPE & CHALLENGE:
1. Verify `Terminal::start_command` environment variables:
   - Does the child process actually receive `TERM=xterm-256color` and `COLORTERM=truecolor`?
2. Verify process termination:
   - When a `Terminal` is dropped, is the child process and any subprocesses cleanly killed by `TerminalJob`?
3. Verify failure modes:
   - Nonexistent executable, invalid directory, rapid spawn and drop.
4. Conclude with a clear verdict: APPROVE or REQUEST_CHANGES.

CONSTRAINTS:
- Do NOT modify permanent codebase files.
- Deliver your report in `c:\Users\ditob\Documents\viper\.agents\challenger_m1_orch2_2\handoff.md`.
- Send message to parent with verdict when complete.
