# Project: Viper High-Level Functional Foundations

## Architecture
Viper is a pure Rust desktop GUI for agentic coding built with `eframe`/`egui`.
This project implements high-level functional code foundations across three core capabilities:
1. **Interactive In-App Approvals Foundation**: Mid-turn agent permission requests (running shell commands or modifying files), surfaced in session history and chat, allowing user approve/deny and relaying back to running turn process.
2. **Git Worktree Session Isolation Foundation**: Worktree creation under `.viper/worktrees/<session-id>`, branch isolation (`viper/session-<session-id>`), path resolution, `.git/info/exclude` management, cleanup, and Changes panel branch-aware diffing.
3. **Webview Live Preview & Artifact Integration Foundation**: Detection of previewable web artifacts (.html, .htm, .svg, .xhtml), pure Rust file-to-URL conversion (`file:///...`), live preview mounting in Tools panel Browser tab, one-click preview buttons, and turn-exit auto-refresh.

### Code Layout
- `src/agent.rs`: Core types (`Provider`, `PermissionMode`, `Turn`, `RunningTurn`, `AgentEvent`, `ApprovalStatus`, `ApprovalDecision`, `ApprovalRequest`, `ApprovalResponse`), process lifecycle.
- `src/worktree.rs`: Git worktree management, creation, listing, pruning, branch cleanup, `.git/info/exclude`.
- `src/preview.rs`: Web artifact detection, pure-Rust path-to-URL conversion, session artifact extraction, tokenization & command verb stripping.
- `src/session.rs`: `Session`, `Entry`, `ApprovalRequest`, worktree fields, approval resolution, working directory resolution.
- `src/chat.rs`: Chat rendering, `ConversationAction::Approve`, `ConversationAction::Deny`, `ConversationAction::Preview`, approval widgets, working directory relative path resolution.
- `src/sidebar.rs`: Session list, workspace grouping (preserving `project_dir` grouping), `SessionState::WaitingForApproval`.
- `src/git_diff.rs`: Diff engine, `branch_changes` against base branch.
- `src/changes.rs`: `Source::Branch`, branch-aware working tree / commit diffs.
- `src/browser.rs`: `BrowserState.auto_refresh`, `Browser::reload()`, URL normalization with local path handling.
- `src/tools.rs`: `mount_preview`, auto-refresh on active browser tab, preview options in add menu, collision-free changes tabs.
- `src/app.rs`: Routing actions (`ConversationAction::Approve`, `Deny`, `Preview`), worktree session creation/cleanup, turn-exit refresh.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Approval Core Data Models | `ApprovalStatus`, `ApprovalDecision`, `ApprovalRequest`, `ApprovalResponse` | M1 | Survey 1 |
| 2 | AgentEvent & Entry Approval Variants | `AgentEvent::ApprovalRequest`, `Entry::Approval` with serde backward compatibility | M1 | Survey 1 |
| 3 | Bidirectional Process Relay | `RunningTurn` approval channel and `respond_approval` method | M1 | Survey 1 |
| 4 | Session Approval Resolution | `has_pending_approval`, `pending_approval`, `resolve_approval` on `Session` | M1 | Survey 1 |
| 5 | Interactive Chat Approval Widget | Approve/Deny UI buttons, tool call info, diff preview, `ConversationAction` emission | M1 | Survey 1 |
| 6 | Sidebar Approval State | `SessionState::WaitingForApproval` amber indicator for paused sessions | M1 | Survey 1 |
| 7 | Git Worktree Module | `create_worktree`, `remove_worktree`, `list_worktrees`, `prune_worktrees`, `delete_branch` | M2 | Survey 2 |
| 8 | Worktree Path & Branch Resolution | Deterministic path `.viper/worktrees/<id>` and branch `viper/session-<id>` | M2 | Survey 2 |
| 9 | Git Info Exclude Management | Auto-add `.viper/` to `.git/info/exclude` to avoid dirtying git status or `.gitignore` | M2 | Survey 2 |
| 10 | Session Worktree Awareness | `worktree_dir`, `worktree_branch`, `worktree_base`, `working_dir()` with serde default | M2 | Survey 2 |
| 11 | Changes Panel Branch Diffing | `Source::Branch` and `git_diff::branch_changes` for branch vs base diffs | M2 | Survey 2 |
| 12 | Worktree Session Lifecycle & Cleanup | App deletion cleanup hook for worktree removal | M2 | Survey 2 |
| 13 | Web Artifact Detection Module | `is_previewable_web_path`, `extract_previewable_artifacts` for html, svg, templates | M3 | Survey 3 |
| 14 | Pure-Rust Path & URL Conversion | `path_to_file_url`, `file_url_to_path`, and `normalize_url` local path routing | M3 | Survey 3 |
| 15 | WebView Programmatic Reload | `Browser::reload()` and `Command::Reload` dispatch in `browser.rs` | M3 | Survey 3 |
| 16 | Tools Panel Preview Mounting | `mount_preview` to find/create browser tab and switch to it | M3 | Survey 3 |
| 17 | One-Click Chat Preview Button | `[👁 Preview]` button in chat tool rows emitting `ConversationAction::Preview` | M3 | Survey 3 |
| 18 | Turn-Exit Auto-Refresh | `AgentEvent::Exited` triggers browser reload if viewing session web artifact | M3 | Survey 3 |
| 19 | Comprehensive Unit & Integration Tests | Tests for approvals, worktrees, webview preview, and backward compatibility | M4 | Survey 1,2,3 |
| 20 | Repository Invariant & Clippy Audit | Verify 0 errors, 0 clippy warnings, no new deps, no formatting drift | M4 | All |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| 1 | M1: Interactive Approvals Foundation | Features 1-6 (Approval data models, Entry::Approval, RunningTurn relay, Session methods, chat UI widget, sidebar indicator) | None | DONE |
| 2 | M2: Git Worktree Session Isolation | Features 7-12 (src/worktree.rs, git_diff::branch_changes, changes::Source::Branch, Session worktree fields, cleanup) | None | DONE |
| 3 | M3: Webview Live Preview & Artifacts | Features 13-18 (src/preview.rs, Browser::reload, Tools::mount_preview, chat Preview button, auto-refresh) | None | DONE |
| 4 | M4: Final Verification & Test Suite | Features 19-20 (Full test pass, clippy check, serde backward-compat verification, final forensic audit) | M1, M2, M3 | DONE |

## Interface Contracts
### Approval Protocol
- `ApprovalRequest`: `{ id: String, tool_name: String, detail: String, edit: Option<FileEdit>, status: ApprovalStatus }`
- `ApprovalDecision`: `Approved | Denied`
- `Session::resolve_approval(&mut self, id: &str, decision: ApprovalDecision) -> bool`
- `ConversationAction::Approve(String)` / `ConversationAction::Deny(String)`
- `RunningTurn::respond_approval(&self, id: &str, decision: ApprovalDecision) -> bool`

### Worktree Protocol
- `worktree::create_worktree(repo_dir: &Path, session_id: u64, branch: Option<&str>, base_ref: Option<&str>) -> Result<(PathBuf, String), String>`
- `worktree::remove_worktree(repo_dir: &Path, worktree_path: &Path, force: bool) -> Result<(), String>`
- `worktree::list_worktrees(repo_dir: &Path) -> Result<Vec<WorktreeInfo>, String>`
- `Session::working_dir(&self) -> &Path` (falls back to `project_dir` when `worktree_dir` is None)
- `git_diff::branch_changes(dir: &Path, base: &str) -> Result<Vec<FileDiff>, String>`

### Preview Protocol
- `preview::is_previewable_web_path(path: &Path) -> bool`
- `preview::path_to_file_url(path: &Path) -> String`
- `preview::file_url_to_path(url: &str) -> Option<PathBuf>`
- `preview::extract_previewable_artifacts(entries: &[Entry], project_dir: &Path) -> Vec<PathBuf>`
- `Tools::mount_preview(&mut self, url: &str, reload_if_loaded: bool)`
- `ConversationAction::Preview(PathBuf)`
- `Browser::reload(&mut self)`
