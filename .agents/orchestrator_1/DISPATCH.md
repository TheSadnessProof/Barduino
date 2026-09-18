# Dispatch Instructions

## 2026-09-18T20:52:15Z

Implement high-level functional code foundations (types, protocols, core data models, and UI wiring) in Viper for three key capabilities: interactive mid-turn in-app approvals, git worktree session isolation, and embedded webview live preview.

Working directory: C:\Users\ditob\Documents\viper
Integrity mode: development

Requirements:
- R1. Interactive In-App Approvals Foundation:
  Introduce high-level data models, event definitions, and UI structures to handle mid-turn agent approval requests. When an agent requests permission to execute sensitive actions (such as running shell commands or modifying files), surface an interactive approval state in the session and chat log allowing the user to approve or deny the action and relay the response back to the agent process.
- R2. Git Worktree Session Isolation Foundation:
  Implement high-level git worktree management enabling sessions on a repository to operate in an isolated branch/worktree (e.g. under `.viper/worktrees/<session-id>`) rather than colliding in the root project directory. Wire worktree awareness into session creation and connect changes detection so branches can be inspected in the Changes panel.
- R3. Webview Live Preview & Artifact Integration Foundation:
  Provide high-level integration between agent-generated web artifacts (HTML, SVG, web templates) and the embedded WebView. When a session generates or updates previewable web files, enable automatic or one-click live preview mounting and refreshing in the tools panel.

Acceptance Criteria:
- In-app approval types, events, and interactive UI widgets are wired into the session and chat flow, with unit tests validating state transitions.
- Worktree creation, path resolution, and cleanup utilities are implemented and verified with tests using temporary git repositories.
- Webview live preview detection and URL/file mounting logic are integrated into the tools panel context with unit tests.
- `cargo check` compiles with 0 errors.
- `cargo test` passes all unit tests with 0 failures and nothing newly ignored.
- `cargo clippy --all-targets -- -D warnings` passes with 0 warnings.
- Repository invariants are preserved: zero unauthorized new dependencies in `Cargo.toml`, no unrequested reformatting, and backward-compatible session serialization.
