## 2026-09-19T00:32:56Z

You are survey_explorer_2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\survey_explorer_2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (pay special attention to section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md

YOUR OBJECTIVE:
Conduct a technical survey of the provider CLI implementations and interactive process execution in Viper.

SCOPE & INVESTIGATION:
1. Examine `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, `src/settings.rs`, and any related files.
2. Analyze how `find_executable` locates `claude`, `codex`, and `antigravity` / `agy` across platforms (especially Windows).
3. Compare the current headless/subcommand arguments (e.g. streaming JSON, prompt stdin) vs what arguments/environment are required to launch each provider CLI interactively inside a PTY.
4. Check how working directories (`session.working_dir()`, git worktrees) must be passed to the spawned PTY command.
5. Check how process termination, child process groups, and resource cleanup must be handled on session close/delete to prevent orphan CLI processes or hanging handles.
6. Provide concrete recommendations for command builders for interactive PTY spawning for each provider.

CONSTRAINTS:
- You are read-only. DO NOT modify any code or files outside your working directory.
- Deliver your findings in a structured, comprehensive handoff report at `c:\Users\ditob\Documents\viper\.agents\survey_explorer_2\handoff.md`.
- When done, send a message to parent reporting completion.
