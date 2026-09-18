> [!WARNING] **Skepticism Disclaimer**
> Deep adversarial probing identified that process exit crashes carrying sandbox or permission errors (when turn.completed/failed or result events were absent or sparse) still displayed unhandled red crash boxes instead of permission notices, and MCP tool denials in Codex went unrecorded; all identified vulnerabilities have been addressed and verified with 164 unit tests, while live paid LLM runs remain skipped per AGENTS.md policy.

## 1. What the prior attempt got wrong
- **Issue 1 (`session.rs` process exit crash on permission/sandbox violations):**
  - **Input:** CLI termination where `AgentEvent::Exited` carried a permission or sandbox error (e.g. `Codex exited with code 1. Sandbox violation: writing to file is forbidden in read-only mode.`) without a prior `Finished` event setting `error_shown`.
  - **Expected:** Process exit error is converted into an `Entry::Notice` informing the user that the tool was disallowed and guiding them to switch permission modes.
  - **Actual:** Pushed `Entry::Error`, rendering a red unhandled crash box.
  - **Root cause:** `Session::handle_event` on `AgentEvent::Exited` only checked `!self.error_shown` before blindly pushing `Entry::Error`, without evaluating whether the process failure was caused by permission or sandbox constraints.
- **Issue 2 (`session.rs` Finished event error fallback when `denied_tools` is empty):**
  - **Input:** Provider `AgentEvent::Finished` returning empty `denied_tools` but with `error: Some("Permission denied: command execution not allowed in read-only mode")`.
  - **Expected:** Inferred denied tool notice (`Entry::Notice`) rendered.
  - **Actual:** `Entry::Error` displayed as an unhandled crash box.
  - **Root cause:** `Session::handle_event` only created `Entry::Notice` when `!denied_tools.is_empty()`, without checking if the finished `error` message itself signified a permission constraint.
- **Issue 3 (`codex.rs` MCP and custom tool permission failure uncaptured):**
  - **Input:** Codex tool item of type `mcp_tool_call` failing due to read-only sandbox restrictions (`"Permission denied: committing is disallowed in read-only mode"`).
  - **Expected:** Record denied tool `"git.commit"` in denial telemetry.
  - **Actual:** Ignored in `completed_item` because only `file_change` and `command_execution` were inspected for sandbox errors.
  - **Root cause:** `codex.rs` hardcoded checks strictly to `file_change` and `command_execution` item types, neglecting `mcp_tool_call` and generic tool types.
- **Issue 4 (`claude.rs` terminal_reason permission indicators ignored):**
  - **Input:** Claude CLI result message returning `terminal_reason: "permission_denied"` with generic `subtype: "error"` and empty `permission_denials`.
  - **Expected:** Inferred denied tool `"Bash"`.
  - **Actual:** `denied_tools` remained empty and turn was marked as a raw error.
  - **Root cause:** `claude.rs` inspected `subtype` and `result` text but ignored `terminal_reason`.

## 2. What I changed
- **`src/session.rs`**:
  - Added `is_permission_error` and `infer_denied_tool` helpers.
  - In `AgentEvent::Finished`, if `denied_tools` is empty but `error` is a permission/sandbox violation, infer the denied tool and display a permission notice instead of an error entry.
  - In `AgentEvent::Exited`, if `!self.error_shown` and the process exit error reflects permission or sandbox restrictions, display `Entry::Notice` instead of `Entry::Error`.
  - Added unit tests `process_exit_permission_error_becomes_denied_tool_notice` and `finished_permission_error_without_denied_tools_becomes_notice`.
- **`src/codex.rs`**:
  - Generalized `completed_item` so that any failed tool item (`file_change`, `command_execution`, `mcp_tool_call`, etc.) with a sandbox or permission error records its tool name into denial telemetry.
  - Enhanced `item_output` to fall back to `error.message` and `error` string for `command_execution` and `mcp_tool_call` when output text is empty.
  - Expanded `is_sandbox_error` to recognize `read_only` and `disallowed`.
  - Added unit test `mcp_tool_permission_failure_records_denied_tool`.
- **`src/claude.rs`**:
  - Inspected `terminal_reason` in `parse_line` for `"result"`, detecting `"permission"` and `"disallowed"` statuses and mapping command keywords to `"Bash"`.
  - Added unit test `terminal_reason_identifies_denied_tool`.
- **`src/antigravity.rs`**:
  - Enhanced error extraction in `parse_line` for `"result"` to inspect `error.message`, `error` string, `message`, and `response`.
- **`src/agent.rs`**:
  - Expanded `codex_plan_mode_prepares_planning_prompt` unit test to verify that `Plan` mode prompts are transformed only for Codex and left intact for other providers.

## 3. Verification Record
- **Deep Verification (ran actual tests):**
  - `cargo check`: Finished dev profile in 0.85s with 0 errors.
  - `cargo test`: 164 passed, 0 failed, 8 ignored in 1.94s (0 newly ignored).
  - `cargo clippy --all-targets`: 0 warnings in 1.72s.
- **Shallow Verification (manual only):**
  - Verified Go flag parser behavior on `agy.exe`: confirmed `--print` expects an argument while omitting `--print` allows clean stdin streaming under `--output-format stream-json`.
  - Verified `claude.exe` help output: confirmed `--disallowed-tools Bash` and `--permission-prompts none` flag compatibility.
  - Verified `codex.exe` exec help output: confirmed `sandbox_mode=read-only` via `-c` and stdin prompt ingestion via `-`.
- **Unverified aspects:**
  - Remote paid account CLI executions (`runs_the_real_antigravity_cli`, `runs_the_real_codex_cli`, `full_access_really_runs_commands`) were intentionally not executed, per AGENTS.md rule 3.2.

## 4. Known Issues
- None. All identified functional gaps, edge cases, and robustness risks have been addressed and validated with automated tests.

## 5. Remaining risk & next step
- No remaining defects identified. All requirements R1, R2, and R3 and acceptance criteria are fully met with 0 clippy warnings and 164 passing tests.
- Proceed to Victory Audit.
