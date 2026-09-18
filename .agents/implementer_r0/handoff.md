# Handoff Report: CLI Invocation and Permission Adapter Alignment

## Summary of Changes

### 1. Antigravity CLI Execution Fix (R1)
- **File touched**: `src/antigravity.rs`
- **Issue**: Previously, `antigravity::args` pushed bare `--print` to `agy` CLI invocations. In Go's `flag` parser, `-print` is a string flag requiring an argument. Passing bare `--print` resulted in Go flag parser failure: `flag needs an argument: -print` with exit code 2.
- **Fix**: Removed bare `--print` from `antigravity::args`. When invoked with `--output-format stream-json`, `agy` operates non-interactively and reads prompt input from stdin as expected. In `parse_line`, permission failures with non-empty `denied_tools` suppress generic error strings so that permission constraints are cleanly reflected as tool denials.
- **Tests**: Updated `builds_print_mode_arguments` and `prompts_do_not_appear_on_the_command_line` to assert `--print` is absent and prompts remain off argv. Added `permission_failure_with_denied_tools_omits_error`.

### 2. Codex Planning Mode & Denial Telemetry Alignment (R2)
- **Files touched**: `src/codex.rs`, `src/agent.rs`
- **Issue**: Codex CLI lacks a native planning mode flag (`--mode plan`). Sessions configured in `PermissionMode::Plan` only set `sandbox_mode=read-only`, giving the model no guidance that it was in plan-only mode. Consequently, the model attempted file modifications that failed against the sandbox. Furthermore, `parse_line` in `codex.rs` hardcoded `denied_tools: Vec::new()`, leaving tool denial notifications empty when sandbox violations occurred.
- **Fix**:
  - Added `codex::plan_prompt` which prepends planning guidance ("You are in plan mode. Formulate an implementation plan...") to prompts when `Provider::Codex` runs under `PermissionMode::Plan` in `start_turn`.
  - Added sandbox execution error detection to `codex::completed_item` and `codex::parse_line`. Failed file edits and sandbox-blocked command executions are tracked and populated in `denied_tools` on turn completion/failure, suppressing generic crash errors on permission blocks.
- **Tests**: Added `plan_mode_guides_the_model_to_plan_without_edits`, `captures_failed_file_change_as_denied_edit_tool`, `captures_sandbox_command_failure_as_denied_shell_tool`, and `turn_failed_due_to_sandbox_reports_denied_tool_without_error`. Added `codex_plan_mode_prepares_planning_prompt` in `src/agent.rs`.

### 3. Claude Read-Only Mode Alignment (R3) & Denial Notice Handling
- **Files touched**: `src/claude.rs`, `src/session.rs`
- **Issue**: In `PermissionMode::ReadOnly`, command execution tools like `Bash` were not explicitly restricted upfront, risking turn failures due to unhandled permission prompts in headless mode.
- **Fix**:
  - In `claude::args`, when `PermissionMode::ReadOnly` is set, added `--disallowed-tools Bash`. Added `--permission-prompts none` so that any permission prompts in headless mode are denied automatically rather than failing turns.
  - In `claude::parse_line`, permission denial results with non-empty `denied_tools` suppress generic error messages.
  - In `session::handle_event`, `self.error_shown` is marked `true` when `denied_tools` is non-empty, preventing `AgentEvent::Exited` from outputting duplicate or unhandled process exit crash errors.
- **Tests**: Added `read_only_restricts_command_tools_and_bypasses_prompts` and `permission_failure_with_denied_tools_omits_error` in `src/claude.rs`. Added `denied_tools_suppresses_process_exit_error` in `src/session.rs`.

## Verification Record

- **Deep Verification (ran actual tests):**
  - Ran `cargo check`: 0 errors.
  - Ran `cargo clippy --all-targets`: 0 warnings.
  - Ran full test suite `cargo test`: 157 passed, 0 failed, 8 ignored (none newly ignored).
  - Executed actual `agy` CLI with stream-json and stdin prompt to verify execution without Go flag parser errors.
- **Shallow Verification (manual run only):**
  - Eyeballed session notice formatting text when denied tools are rendered.
- **Unverified aspects:**
  - Paid live accounts through ignored CLI integration tests (`runs_the_real_antigravity_cli`, `runs_the_real_codex_cli`, `full_access_really_runs_commands`), as per AGENTS.md rule 3.2 prohibiting running wholesale ignored tests without explicit instruction.
