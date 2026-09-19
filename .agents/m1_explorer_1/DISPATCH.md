## 2026-09-19T00:37:05Z

You are m1_explorer_1.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\m1_explorer_1

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\AGENTS.md

YOUR OBJECTIVE:
Provide a precise technical design and implementation blueprint for Milestone 1 in `src/terminal.rs`.

SCOPE & INVESTIGATION:
1. Examine `src/terminal.rs` and how `Terminal::start` currently works.
2. Design the public `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>` (or taking `portable_pty::CommandBuilder`).
3. Ensure on Windows that if `program` ends with `.cmd` or `.bat` (like npm's `claude.cmd`), it wraps via `cmd.exe /c` to avoid Windows ConPTY error 193.
4. Ensure `TERM=xterm-256color` and `COLORTERM=truecolor` are configured in the environment.
5. Verify `TerminalJob` and Win32 Job Object handling, reader thread, `Replies` callback, and error messages.
6. Provide exact code snippets and line-by-line guidance for the worker.

CONSTRAINTS:
- Read-only. DO NOT edit files outside your working directory.
- Deliver findings in `c:\Users\ditob\Documents\viper\.agents\m1_explorer_1\handoff.md`.
- Send message to parent upon completion.
