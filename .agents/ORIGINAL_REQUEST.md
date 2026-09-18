# Original User Request

## 2026-09-18T20:06:54Z

This is a single self-contained fix; keep it small and focused.

Fix the Antigravity (agy) CLI invocation where passing bare `--print` causes Go flag parser exit code 2 errors, and align permission adapter handling across Codex, Claude, and Antigravity so that permission modes operate cleanly without unexpected crashes or silent tool denials.

Working directory: C:\Users\ditob\Documents\viper
Integrity mode: development

## Requirements

### R1. Antigravity CLI Execution Fix
Ensure that `agy` invocations receive CLI flags in the format required by the Go `flag` parser so that prompts execute without triggering `flag needs an argument: -print` or exit code 2.

### R2. Codex Planning Mode & Denial Telemetry Alignment
Ensure that Codex sessions configured with `PermissionMode::Plan` guide the model to formulate an implementation plan rather than attempting file edits that fail against the read-only sandbox. Capture sandbox execution errors so that denied actions are properly reflected in session tool denial notifications.

### R3. Claude Read-Only Mode Alignment
Ensure Claude sessions under `PermissionMode::ReadOnly` cleanly restrict command execution tools without failing turns due to permission prompts.

## Acceptance Criteria

### Execution & Test Suite
- [ ] Invocations of the Antigravity CLI pass valid argument syntax and execute without Go flag parser exit code 2 errors.
- [ ] Running a session in `Plan` mode under Codex provides planning guidance and prevents unauthorized sandbox write attempts.
- [ ] When a provider blocks an action due to permission constraints, the failure is detected and reported as a denied tool rather than an unhandled crash.
- [ ] `cargo check` compiles with 0 errors.
- [ ] `cargo test` passes all unit tests with 0 failures and nothing newly ignored.
- [ ] `cargo clippy --all-targets` passes with 0 warnings.
- [ ] No unrelated lines or formatting changes are introduced.

## 2026-09-18T20:52:15Z

Implement high-level functional code foundations (types, protocols, core data models, and UI wiring) in Viper for three key capabilities: interactive mid-turn in-app approvals, git worktree session isolation, and embedded webview live preview.

Working directory: C:\Users\ditob\Documents\viper
Integrity mode: development

## Requirements

### R1. Interactive In-App Approvals Foundation
Introduce high-level data models, event definitions, and UI structures to handle mid-turn agent approval requests. When an agent requests permission to execute sensitive actions (such as running shell commands or modifying files), surface an interactive approval state in the session and chat log allowing the user to approve or deny the action and relay the response back to the agent process.

### R2. Git Worktree Session Isolation Foundation
Implement high-level git worktree management enabling sessions on a repository to operate in an isolated branch/worktree (e.g. under `.viper/worktrees/<session-id>`) rather than colliding in the root project directory. Wire worktree awareness into session creation and connect changes detection so branches can be inspected in the Changes panel.

### R3. Webview Live Preview & Artifact Integration Foundation
Provide high-level integration between agent-generated web artifacts (HTML, SVG, web templates) and the embedded WebView. When a session generates or updates previewable web files, enable automatic or one-click live preview mounting and refreshing in the tools panel.

## Acceptance Criteria

### High-Level Foundations & Verification
- [ ] In-app approval types, events, and interactive UI widgets are wired into the session and chat flow, with unit tests validating state transitions.
- [ ] Worktree creation, path resolution, and cleanup utilities are implemented and verified with tests using temporary git repositories.
- [ ] Webview live preview detection and URL/file mounting logic are integrated into the tools panel context with unit tests.
- [ ] `cargo check` compiles with 0 errors.
- [ ] `cargo test` passes all unit tests with 0 failures and nothing newly ignored.
- [ ] `cargo clippy --all-targets -- -D warnings` passes with 0 warnings.
- [ ] Repository invariants are preserved: zero unauthorized new dependencies in `Cargo.toml`, no unrequested reformatting, and backward-compatible session serialization.
