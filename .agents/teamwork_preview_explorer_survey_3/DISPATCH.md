# Explorer 3 Dispatch: Webview Live Preview Architecture

Investigate the codebase for Requirement R3: Webview Live Preview & Artifact Integration Foundation.
Read `C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md` and `C:\Users\ditob\Documents\viper\AGENTS.md`.
Examine `src/browser.rs`, `src/tools.rs`, `src/app.rs`, `src/session.rs`, `src/chat.rs`.
Map how the embedded browser/WebView is currently wired in `tools.rs` and `app.rs`, how agent tool outputs (file edits, writes, artifacts) are tracked or can be detected (e.g. .html, .svg, templates), how URLs/files can be mounted/loaded, and how live preview triggers/refreshes should be wired into the tools panel and session context.
Produce an architecture & implementation plan in `handoff.md` in your working directory.

## 2026-09-18T20:53:13Z
You are Explorer 3 investigating Requirement R3 (Webview Live Preview & Artifact Integration Foundation).
Your working directory is: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3
Read your instructions in: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3\DISPATCH.md
Read the user request in: C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md
Read repository rules in: C:\Users\ditob\Documents\viper\AGENTS.md

Explore the codebase (using search and view tools) to analyze:
1. Embedded WebView implementation in `src/browser.rs`, `src/tools.rs`, and `src/app.rs`.
2. How web artifacts (HTML, SVG, web templates) created/edited by agent turns can be detected from AgentEvents (e.g. ToolUse FileEdit, writes) or session history.
3. Live preview mounting logic: converting local file paths / artifacts to file:// or preview URLs, updating the Browser state.
4. One-click and auto-refresh mechanisms in the tools panel.
5. Unit testing strategy for previewable artifact detection and URL/file mounting.

Write your findings and proposed technical design to `C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3\handoff.md`.
Then send a brief message with your key findings and handoff path to the orchestrator.
