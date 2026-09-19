## 2026-09-19T00:32:56Z
You are survey_explorer_1.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\survey_explorer_1

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (pay special attention to section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md

YOUR OBJECTIVE:
Conduct an architectural and technical survey of the terminal and PTY infrastructure in Viper for hosting interactive AI provider CLIs in the middle panel.

SCOPE & INVESTIGATION:
1. Examine `src/terminal.rs`, `Cargo.toml`, and related files.
2. Analyze how `portable-pty` and `vt100` are currently implemented for secondary shell terminals in the tools panel.
3. Investigate how keystroke capture, Enter, Backspace, Ctrl combinations, arrow keys, and keyboard focus are handled.
4. Investigate how ANSI escape codes, colors, cursor positioning, and screen rendering work.
5. Investigate how terminal resizing (window dimensions / layout changes) propagates to the underlying PTY and vt100 parser.
6. Identify what changes or extensions to `src/terminal.rs` or new abstractions are needed to host the AI CLI directly in the middle panel.
7. Note any constraints, edge cases, or potential pitfalls (especially on Windows conpty).

CONSTRAINTS:
- You are read-only. DO NOT modify any code or files outside your working directory.
- Deliver your findings in a structured, comprehensive handoff report at `c:\Users\ditob\Documents\viper\.agents\survey_explorer_1\handoff.md`.
- When done, send a message to parent reporting completion.
