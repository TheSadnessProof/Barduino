# BRIEFING — 2026-09-18T21:18:00Z

## Mission
Investigate Requirement R3: Webview Live Preview & Artifact Integration Foundation across browser.rs, tools.rs, app.rs, session.rs, and chat.rs, and formulate an architecture and implementation plan.

## 🔒 My Identity
- Archetype: Teamwork explorer
- Roles: Read-only investigation: analyze problems, synthesize findings, produce structured reports
- Working directory: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3
- Original parent: 3bbc3f41-8b4d-4783-8322-748205e3bbb5
- Milestone: Preview & Artifact Integration Foundation Survey

## 🔒 Key Constraints
- Read-only investigation — do NOT implement
- Do NOT edit files in src/
- Only write within C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3
- Preserve repository invariants: zero new Cargo dependencies, no cargo fmt, no ignored test runs wholesale

## Current Parent
- Conversation ID: 3bbc3f41-8b4d-4783-8322-748205e3bbb5
- Updated: not yet

## Investigation State
- **Explored paths**: `src/browser.rs`, `src/tools.rs`, `src/app.rs`, `src/session.rs`, `src/agent.rs`, `src/chat.rs`, `src/line_diff.rs`, `src/main.rs`, `Cargo.toml`.
- **Key findings**:
  1. `Browser` wraps `wry::WebView` in a single native window shared by all sessions; navigation via `Command::Load(url)` and `Command::Reload`.
  2. `normalize_url` handles schemes with `://`, but raw file paths (`C:\...`, `/...`) currently get corrupted into `https://...`. Pure-Rust `path_to_file_url` is needed.
  3. `AgentEvent::ToolUse` with `FileEdit` (Claude) and tool calls/details (Codex/Antigravity) can be scanned for previewable extensions (`.html`, `.htm`, `.svg`, `.xhtml`).
  4. One-click preview can be wired via `ConversationAction::Preview` in `chat.rs` and `mount_preview` in `tools.rs`.
  5. Auto-refresh can be driven on `AgentEvent::Exited` in `app.rs` via `browser.reload()` when an open artifact is updated on disk.
- **Unexplored areas**: None for R3 problem boundary.

## Key Decisions Made
- Separated pure logic (artifact detection, URL encoding/decoding, reload decisions) from egui/wry rendering for 100% offline headless unit testing.
- Verified zero new dependencies needed (standard library path manipulation is sufficient for file:// URL construction).
- Formulated backward-compatible `SavedBrowserState` serialization with `#[serde(default)]`.

## Artifact Index
- C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3\DISPATCH.md — Dispatch instructions and history
- C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3\progress.md — Liveness and task progress tracking
- C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3\handoff.md — 5-component handoff report
