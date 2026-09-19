## 2026-09-19T00:32:56Z
You are survey_explorer_3.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\survey_explorer_3

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (pay special attention to section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md

YOUR OBJECTIVE:
Conduct a technical survey of ViperApp state management, session lifecycle, UI routing, and backward compatibility for replacing chat transcript/composer with an interactive terminal.

SCOPE & INVESTIGATION:
1. Examine `src/app.rs`, `src/session.rs`, `src/chat.rs`, `src/sidebar.rs`, `src/tools.rs`.
2. Analyze how the middle panel is currently rendered in `ViperApp::ui` / `chat.rs`, and how to replace it with the embedded terminal widget while preserving sidebar and tools panels.
3. Analyze `Session` struct and `SavedState`:
   - How per-session terminal state (PTY session / parser) should be held (runtime state vs saved state).
   - What fields are persisted in `SavedState` (RON format). How to ensure 100% backward compatibility per AGENTS.md Rule 3.5 (`#[serde(default)]`, aliases, etc.).
   - How sessions across restarts should be handled (restarting provider CLI in session directory when reopened).
4. Analyze multi-session switching:
   - How switching sessions in the sidebar updates the middle view to that session's active terminal buffer.
   - How keyboard focus is immediately handed over to the active session's terminal.
   - How deleting a session cleans up its terminal process and resources.
5. Review existing tests in `src/session.rs`, `src/app.rs`, etc., and identify what new tests are needed for session terminal lifecycle.

CONSTRAINTS:
- You are read-only. DO NOT modify any code or files outside your working directory.
- Deliver your findings in a structured, comprehensive handoff report at `c:\Users\ditob\Documents\viper\.agents\survey_explorer_3\handoff.md`.
- When done, send a message to parent reporting completion.
