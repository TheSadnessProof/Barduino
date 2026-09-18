# Explorer 1 Dispatch: Approvals Architecture

Investigate the codebase for Requirement R1: Interactive In-App Approvals Foundation.
Read `C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md` and `C:\Users\ditob\Documents\viper\AGENTS.md`.
Examine `src/agent.rs`, `src/session.rs`, `src/chat.rs`, `src/app.rs`, and provider files (`claude.rs`, `codex.rs`, `antigravity.rs`).
Map how permissions/approvals currently work (PermissionMode, AgentEvent, Turn, process lifecycle).
Determine how mid-turn approval requests can be represented in AgentEvent/Session/Entry, how UI interaction (approve/deny) should be structured in chat/session, and how the approval/denial response is relayed back to the agent or recorded.
Produce an architecture & implementation plan in `handoff.md` in your working directory.

## 2026-09-19T00:53:13Z
You are Explorer 1 investigating Requirement R1 (Interactive In-App Approvals Foundation).
Your working directory is: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_1
Read your instructions in: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_1\DISPATCH.md
Read the user request in: C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md
Read repository rules in: C:\Users\ditob\Documents\viper\AGENTS.md

Explore the codebase (using search and view tools) to analyze:
1. Current agent turn lifecycle, PermissionMode, AgentEvent, Turn, and child process execution in `src/agent.rs`.
2. How sessions and chat entries handle events, tool uses, permissions in `src/session.rs` and `src/chat.rs`.
3. How approvals can be modeled: mid-turn approval requests, approval tokens/IDs, states (Pending, Approved, Denied), responses back to the process or session state.
4. UI widget integration in chat and session panels.
5. Unit testing strategy for approval state transitions and protocol events.

Write your findings and proposed technical design to `C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_1\handoff.md`.
Then send a brief message with your key findings and handoff path to the orchestrator.
