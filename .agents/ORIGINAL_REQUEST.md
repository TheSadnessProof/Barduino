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
