# Worker M1 Dispatch: Interactive In-App Approvals Foundation

## Objective
Implement Milestone 1: Interactive In-App Approvals Foundation in Viper.

## Context & Inputs
- User Request: `C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md` (under `## 2026-09-18T20:52:15Z`)
- Repository Rules: `C:\Users\ditob\Documents\viper\AGENTS.md`
- Project Architecture: `C:\Users\ditob\Documents\viper\.agents\orchestrator_1\PROJECT.md`
- Explorer Specification: `C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_1\handoff.md`

## Files You Own Exclusively
- `src/agent.rs`
- `src/session.rs`
- `src/chat.rs`
- `src/app.rs`
- `src/sidebar.rs`

## Detailed Implementation Tasks
1. **Core Approval Types (`src/agent.rs`)**:
   - Add `ApprovalStatus` (`#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]` with `Pending`, `Approved`, `Denied`).
   - Add `ApprovalDecision` (`Approved`, `Denied`).
   - Add `ApprovalResponse` (`id: String`, `decision: ApprovalDecision`).
   - Add `ApprovalRequest` (`id: String`, `tool_name: String`, `detail: String`, `#[serde(default)] edit: Option<FileEdit>`, `#[serde(default)] status: ApprovalStatus`). Provide `new`, `is_pending`, and `resolve(&mut self, decision: ApprovalDecision) -> bool`.
   - Add `AgentEvent::ApprovalRequest(ApprovalRequest)`.
   - In `RunningTurn`, add `approval_tx: Option<std::sync::mpsc::Sender<ApprovalResponse>>` and `pub fn respond_approval(&self, id: &str, decision: ApprovalDecision) -> bool`.
2. **Session Integration (`src/session.rs`)**:
   - Add `Entry::Approval(ApprovalRequest)` to `pub enum Entry`.
   - In `Session::handle_event`, handle `AgentEvent::ApprovalRequest(request)`: flush streamed text and push `Entry::Approval(request)`.
   - Add `pub fn has_pending_approval(&self) -> bool`, `pub fn pending_approval(&self) -> Option<&ApprovalRequest>`, and `pub fn resolve_approval(&mut self, id: &str, decision: ApprovalDecision) -> bool` which updates the matching entry and forwards to `turn.respond_approval(id, decision)`.
3. **Chat Widget & Action Routing (`src/chat.rs` and `src/app.rs`)**:
   - In `src/chat.rs`, add `ConversationAction::Approve(String)` and `ConversationAction::Deny(String)`.
   - In `show_entry`, render `Entry::Approval`:
     - Clean container styling with tool name/type badge.
     - Action description/detail and diff view if `edit` is present (`edit_view`).
     - When pending: render `[✓ Approve]` and `[✕ Deny]` buttons emitting the corresponding `ConversationAction`.
     - When resolved: show neat `✓ Approved` (green) or `✕ Denied` (red) pill badges.
   - In `src/app.rs`, handle `ConversationAction::Approve(id)` and `ConversationAction::Deny(id)` by calling `self.active_session_mut().resolve_approval(&id, ApprovalDecision::...)` and requesting repaint.
4. **Sidebar Indicator (`src/sidebar.rs`)**:
   - Add `SessionState::WaitingForApproval`.
   - In `session_state(session: &Session)`, check `session.has_pending_approval()` first to return `SessionState::WaitingForApproval`.
   - Render an amber status dot for waiting sessions.
5. **Unit Tests**:
   - Test approval request creation, pending state, resolution to approved and denied.
   - Test that resolving an already-resolved request returns false and preserves status.
   - Test session handling of `AgentEvent::ApprovalRequest` updating entries and `has_pending_approval`.
   - Test `resolve_approval` on session updates the entry and relays to running turn.
   - Test sidebar `session_state` reflects waiting for approval and returns to idle when resolved.
   - Test RON backward compatibility: existing sessions deserialize without errors, and sessions with `Entry::Approval` round-trip cleanly.
6. **Strict Quality Gates**:
   - Run `cargo check`.
   - Run `cargo test`.
   - Run `cargo clippy --all-targets -- -D warnings`.
   - Ensure ZERO warnings and ZERO errors.
   - Do NOT run `cargo fmt`.
   - Do NOT add any dependencies to `Cargo.toml`.

## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## 2026-09-18T20:56:43Z
You are Worker M1 implementing Milestone 1: Interactive In-App Approvals Foundation.
Your working directory is: C:\Users\ditob\Documents\viper\.agents\worker_m1
Read your instructions in: C:\Users\ditob\Documents\viper\.agents\worker_m1\DISPATCH.md
Read the user request in: C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md
Read repository rules in: C:\Users\ditob\Documents\viper\AGENTS.md
Read the Explorer report in: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_1\handoff.md
