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

## 2026-09-19T00:31:41Z

Replace the middle chat transcript and composer in Viper with a dedicated interactive terminal experience that launches the selected AI provider's CLI directly inside an embedded PTY.

Working directory: c:\Users\ditob\Documents\viper
Integrity mode: development

## Requirements

### R1. Dedicated Provider Terminal View
Replace the middle panel's custom chat transcript and message composer (`chat.rs`) with an embedded terminal widget (`terminal.rs`) that directly hosts the active AI session. The middle area must serve as a full, interactive terminal interface for the AI CLI rather than a custom markdown/chat GUI.

### R2. Direct Interactive Provider CLI Execution
When a session is created or opened, the embedded terminal in the middle panel must spawn the selected provider's CLI executable (`claude`, `codex`, or `agy` / `antigravity`) directly in interactive mode inside a pseudo-terminal (PTY) using `portable-pty` and `vt100`:
- The process must run in the session's active working directory (`session.working_dir()`).
- All native interactive TUI features of the CLIs (ANSI escapes, full-screen redrawing, cursor positioning, interactive permission and approval prompts, arrow keys, and keybindings) must work seamlessly without interference from chat stream parsers.
- Terminal resize events (window dimensions changing) must propagate to the underlying PTY so lines wrap and layout dynamically.

### R3. Per-Session Terminal State and Lifecycle Management
- Each session in the left sidebar must own its dedicated terminal process and vt100 parser state.
- Switching between sessions in the sidebar must switch the middle view to that session's active terminal buffer and hand over keyboard focus immediately.
- Deleting or closing a session must terminate the associated CLI process group cleanly without leaving orphan processes or hanging handles.
- Saved state must gracefully handle sessions across restarts, restarting the provider CLI in the session directory when reopened.

### R4. Sidebar and Auxiliary Tools Integration
- Preserve the left sidebar (project folders, session creation, renaming, deleting).
- Preserve the right-side tools panel (secondary shell terminals, git branch/working tree changes diffing, embedded browser live preview, and panel toggle controls).
- Provider selection (Claude Code, Codex, Antigravity) must determine which CLI executable is launched in the session's middle terminal.

## Acceptance Criteria

### Terminal Integration & UI
- [ ] The middle panel renders the embedded terminal running the AI provider's CLI instead of the chat bubble/composer UI.
- [ ] Typing into the middle panel sends keystrokes (including Enter, Backspace, Ctrl combinations, and arrow keys) directly to the running CLI.
- [ ] ANSI escape codes, colored text, bold/dim text, and cursor positioning render accurately via the embedded terminal emulator.
- [ ] Resizing the main application window or collapsing/expanding the sidebars dynamically resizes the PTY dimensions (columns and rows) of the active CLI session.

### Multi-Session Management
- [ ] Creating a new session spawns a fresh interactive CLI instance for the chosen provider in that session's working directory.
- [ ] Switching sessions in the sidebar switches the middle view to the corresponding session's terminal display and routes keyboard input to it.
- [ ] Deleting a session stops its underlying CLI process and frees system resources.

### Stability & Codebase Invariants
- [ ] `cargo check` passes cleanly without compilation errors.
- [ ] `cargo test` passes for the entire test suite with new tests covering session terminal lifecycle.
- [ ] `cargo clippy --all-targets` passes with 0 warnings.
- [ ] No forbidden dependencies added (conforms to `AGENTS.md` Rule 3.4).
- [ ] Backward compatibility of saved state is preserved (conforms to `AGENTS.md` Rule 3.5).

