# Milestone 1: Interactive Provider Command Design & Blueprint (`src/agent.rs`)

## Executive Summary
This report provides the complete technical design, architectural blueprint, and ready-to-implement code for Milestone 1's interactive command construction in `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, and `src/antigravity.rs`. The public interface `agent::build_interactive_command` returns `(PathBuf, Vec<String>)`, configuring the exact CLI flags required for native TUI interactive execution inside an embedded PTY while cleanly stripping headless streaming flags (`-p`, `--output-format stream-json`, `--permission-prompts none`, `exec`, stdin prompt tokens).

---

## 1. Observation

### 1.1 Existing Headless Command Generation
Direct inspection of `src/agent.rs`, `src/claude.rs`, `src/codex.rs`, and `src/antigravity.rs` revealed:
- `src/agent.rs` lines 410–430 (`start_turn`): Dispatches to provider modules for headless command execution:
  - `claude::args(&turn)`
  - `codex::args(&turn)`
  - `antigravity::args(&turn)`
- `src/claude.rs` lines 32–59 (`claude::args`): Emits headless flags:
  `["-p", "--verbose", "--output-format", "stream-json", "--include-partial-messages", "--permission-mode", mode, "--permission-prompts", "none"]`.
  When `PermissionMode::ReadOnly`, it additionally passes `["--disallowed-tools", "Bash"]`.
- `src/codex.rs` lines 34–72 (`codex::args`): Emits headless flags:
  `vec!["exec".into()]`, followed by `resume <id>` (if resuming), `--json`, `--skip-git-repo-check`, `-c sandbox_mode=<mode>`, and trailing `-` to read prompt from stdin.
- `src/antigravity.rs` lines 26–64 (`antigravity::args`): Emits print-mode flags:
  `["--output-format", "stream-json", "--add-dir", turn.cwd.display()]`, plus `--mode accept-edits|plan` or `--dangerously-skip-permissions`. Note from line 61: bare `--print` causes Go flag parser exit code 2 errors.

### 1.2 Interactive CLI Capabilities Observed via Live CLIs
Verification against the locally installed provider executables (`claude --help`, `codex --help`, `codex resume --help`, `agy --help`):

1. **Claude Code (`claude`)**:
   - Interactive command: `claude` (no subcommand).
   - Session resume: `-r, --resume [value]` (`--resume <id>`).
   - Model selection: `--model <model>`.
   - Reasoning effort: `--effort <effort>`.
   - Permission mode: `--permission-mode <mode>` (choices: `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan`). Note: `--permission-mode default` is also accepted as alias for standard mode.
   - Bypassing permissions: `--dangerously-skip-permissions` ("Bypass all permission checks").
   - Flags to omit: `-p`, `--output-format`, `--include-partial-messages`, `--permission-prompts`, `--disallowed-tools`, `--verbose`.

2. **OpenAI Codex (`codex`)**:
   - Interactive fresh session: `codex [OPTIONS]` (no subcommand; `exec` must NOT be passed).
   - Interactive resume: `codex resume [OPTIONS] [SESSION_ID]` (subcommand `resume <id>`).
   - Working directory: `-C, --cd <DIR>` (supported on both `codex` and `codex resume`).
   - Model selection: `-m, --model <MODEL>`.
   - Reasoning effort: `-c model_reasoning_effort=<effort>` (Codex config override).
   - Sandbox mode: `-s, --sandbox <SANDBOX_MODE>` (`read-only`, `workspace-write`).
   - Full access: `--dangerously-bypass-approvals-and-sandbox` (omits `-s`).
   - Flags to omit: `exec`, `--json`, `-` (stdin prompt token).

3. **Google Antigravity (`agy`)**:
   - Workspace directory: `--add-dir <DIR>`.
   - Conversation resume: `--conversation <id>`.
   - Model selection: `--model <model>`.
   - Reasoning effort: `--effort <low|medium|high>`. Notice: models ending in `-low`, `-medium`, or `-high` encode effort in the model name and must not receive `--effort`.
   - Execution mode: `--mode <accept-edits|plan>`.
   - Full access: `--dangerously-skip-permissions`.
   - Flags to omit: `--output-format`, `-p`, `--print`.

### 1.3 Architecture Contract
In `orchestrator_2/PROJECT.md` lines 48–72:
- `Terminal::start_command(cwd: &Path, program: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>` (owned by `src/terminal.rs`).
- `build_interactive_command(provider: Provider, exe: &Path, cwd: &Path, model: Option<&str>, effort: Option<&str>, resume_id: Option<&str>, permission_mode: PermissionMode) -> (PathBuf, Vec<String>)` (owned by `src/agent.rs`).

---

## 2. Logic Chain

1. **Decoupling Command Building from PTY Spawning**:
   Returning `(PathBuf, Vec<String>)` rather than a configured `portable_pty::CommandBuilder` keeps `src/agent.rs` pure, testable, and free of PTY lifecycle dependencies. Unit tests can assert exact argv vectors in microseconds without spawning pseudo-terminals or processes.
2. **Modularity Across Provider Modules**:
   Just as `src/agent.rs::start_turn` delegates to `claude::args`, `codex::args`, and `antigravity::args`, `agent::build_interactive_command` should delegate to `claude::interactive_args`, `codex::interactive_args`, and `antigravity::interactive_args`. This keeps provider-specific flag knowledge encapsulated within each provider module.
3. **Handling Windows Batch Files (`.cmd` / `.bat`)**:
   On Windows, Node/npm tools install shims like `claude.cmd`. Windows `CreateProcessW` inside ConPTY rejects batch files with error 193. `Terminal::start_command` in `src/terminal.rs` handles wrapping batch files via `cmd.exe /c` at process spawn time; `build_interactive_command` returns the raw executable path and clean CLI arguments.
4. **Claude Flag Mapping**:
   - `PermissionMode::ReadOnly` → `["--permission-mode", "default"]` (prompts for modifications).
   - `PermissionMode::AcceptEdits` → `["--permission-mode", "acceptEdits"]`.
   - `PermissionMode::Full` → `["--dangerously-skip-permissions"]`.
   - `PermissionMode::Plan` → `["--permission-mode", "plan"]`.
   - Model, effort, and resume ID are appended conditionally.
   - Headless flags (`-p`, `--output-format`, etc.) are omitted.
5. **Codex Flag Mapping**:
   - If `resume_id` is present, prepend `["resume", id]`; otherwise no subcommand.
   - `-C <cwd>` sets working root.
   - `PermissionMode::ReadOnly` and `PermissionMode::Plan` → `["-s", "read-only"]`.
   - `PermissionMode::AcceptEdits` → `["-s", "workspace-write"]`.
   - `PermissionMode::Full` → `["--dangerously-bypass-approvals-and-sandbox"]`.
   - Model is passed with `-m <model>`.
   - Effort is passed via `-c model_reasoning_effort=<effort>`.
   - `exec`, `--json`, and `-` are omitted.
6. **Antigravity Flag Mapping**:
   - `--add-dir <cwd>` is always passed.
   - `PermissionMode::ReadOnly` → no flag needed (default interactive prompt).
   - `PermissionMode::AcceptEdits` → `["--mode", "accept-edits"]`.
   - `PermissionMode::Full` → `["--dangerously-skip-permissions"]`.
   - `PermissionMode::Plan` → `["--mode", "plan"]`.
   - Model is passed via `--model <model>`. Effort is passed via `--effort <effort>` unless the model name already ends in `-low`, `-medium`, or `-high`.
   - If `resume_id` is present, pass `["--conversation", id]`.
   - `--output-format` and `--print` are omitted.

---

## 3. Caveats

1. **No Real Process Tokens Consumed**:
   Investigation verified flag syntax against CLI help screens, dry-run arguments, and codebase test fixtures without initiating billable model turns.
2. **Terminal Job Object & Environment Handling**:
   Setting `TERM=xterm-256color` and `COLORTERM=truecolor`, process group assignment, and Win32 Job Object creation are the responsibility of `Terminal::start_command` in `src/terminal.rs`, not `build_interactive_command`.
3. **Session ID Format**:
   Claude uses session UUIDs; Codex uses thread IDs (UUIDs); Antigravity uses conversation IDs (UUIDs). `resume_id` is passed as `Option<&str>` and forwarded verbatim.

---

## 4. Conclusion & Implementation Blueprint

### 4.1 Interface Contract
```rust
pub fn build_interactive_command(
    provider: Provider,
    exe: &Path,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> (PathBuf, Vec<String>)
```

### 4.2 Exact Code Snippets for Worker

#### Step 1: `src/claude.rs`
Add `interactive_args` right below `claude::args`:

```rust
/// Arguments for an interactive session running in a terminal.
///
/// Unlike headless turns, interactive mode omits `-p`, `--output-format`, and
/// permission suppression flags so Claude Code renders its full terminal interface
/// and interactive prompts.
pub fn interactive_args(
    _cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mut args = Vec::new();
    match permission_mode {
        PermissionMode::ReadOnly => args.extend(["--permission-mode".to_owned(), "default".to_owned()]),
        PermissionMode::AcceptEdits => args.extend(["--permission-mode".to_owned(), "acceptEdits".to_owned()]),
        PermissionMode::Full => args.push("--dangerously-skip-permissions".to_owned()),
        PermissionMode::Plan => args.extend(["--permission-mode".to_owned(), "plan".to_owned()]),
    }
    if let Some(model) = model {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = effort {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    if let Some(session_id) = resume_id {
        args.extend(["--resume".to_owned(), session_id.to_owned()]);
    }
    args
}
```

#### Step 2: `src/codex.rs`
Add `interactive_args` right below `codex::args`:

```rust
/// Arguments for an interactive session running in a terminal.
///
/// Unlike headless `codex exec`, interactive mode launches the interactive CLI
/// (or `codex resume <id>`) with the project root set via `-C` and sandbox policy
/// configured directly via `-s`.
pub fn interactive_args(
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(session_id) = resume_id {
        args.extend(["resume".to_owned(), session_id.to_owned()]);
    }
    args.extend(["-C".to_owned(), cwd.display().to_string()]);
    match permission_mode {
        PermissionMode::ReadOnly | PermissionMode::Plan => {
            args.extend(["-s".to_owned(), "read-only".to_owned()]);
        }
        PermissionMode::AcceptEdits => {
            args.extend(["-s".to_owned(), "workspace-write".to_owned()]);
        }
        PermissionMode::Full => {
            args.push("--dangerously-bypass-approvals-and-sandbox".to_owned());
        }
    }
    if let Some(model) = model {
        args.extend(["-m".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = effort {
        args.extend(["-c".to_owned(), format!("model_reasoning_effort={effort}")]);
    }
    args
}
```

#### Step 3: `src/antigravity.rs`
Add `interactive_args` right below `antigravity::args`:

```rust
/// Arguments for an interactive session running in a terminal.
///
/// Unlike print-mode turns, interactive mode omits `--output-format` and runs
/// directly inside the terminal with the project workspace added via `--add-dir`.
pub fn interactive_args(
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mut args = vec!["--add-dir".to_owned(), cwd.display().to_string()];
    match permission_mode {
        PermissionMode::ReadOnly => {}
        PermissionMode::AcceptEdits => args.extend(["--mode".to_owned(), "accept-edits".to_owned()]),
        PermissionMode::Full => args.push("--dangerously-skip-permissions".to_owned()),
        PermissionMode::Plan => args.extend(["--mode".to_owned(), "plan".to_owned()]),
    }
    if let Some(model) = model {
        args.extend(["--model".to_owned(), model.to_owned()]);
        let named_level = ["-low", "-medium", "-high"].iter().any(|level| model.ends_with(level));
        if let Some(effort) = effort
            && !named_level
        {
            args.extend(["--effort".to_owned(), effort.to_owned()]);
        }
    } else if let Some(effort) = effort {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    if let Some(conversation_id) = resume_id {
        args.extend(["--conversation".to_owned(), conversation_id.to_owned()]);
    }
    args
}
```

#### Step 4: `src/agent.rs`
Add `build_interactive_command` to `src/agent.rs`:

```rust
/// Builds the executable path and command-line arguments for launching a provider's
/// CLI interactively inside an embedded PTY terminal.
///
/// Returns the binary to execute and the slice of arguments to pass. Interactive mode
/// omits headless stream-json flags, print prompts, and permission-denial shims so
/// that the CLI's native terminal UI, keyboard controls, and interactive prompts
/// render directly in the terminal buffer.
pub fn build_interactive_command(
    provider: Provider,
    exe: &Path,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> (PathBuf, Vec<String>) {
    let args = match provider {
        Provider::Claude => claude::interactive_args(cwd, model, effort, resume_id, permission_mode),
        Provider::Codex => codex::interactive_args(cwd, model, effort, resume_id, permission_mode),
        Provider::Antigravity => antigravity::interactive_args(cwd, model, effort, resume_id, permission_mode),
    };
    (exe.to_path_buf(), args)
}
```

---

## 5. Verification Method

### 5.1 Independent Test Suite
Add the following unit tests in `src/agent.rs::tests`:

```rust
#[test]
fn build_interactive_command_configures_claude_without_headless_flags() {
    let exe = PathBuf::from("C:\\tools\\claude.exe");
    let cwd = PathBuf::from("C:\\projects\\demo");
    let (prog, args) = build_interactive_command(
        Provider::Claude,
        &exe,
        &cwd,
        Some("claude-opus"),
        Some("high"),
        Some("session-123"),
        PermissionMode::Full,
    );
    assert_eq!(prog, exe);
    assert!(args.contains(&"--dangerously-skip-permissions".to_owned()));
    assert!(args.windows(2).any(|p| p == ["--model", "claude-opus"]));
    assert!(args.windows(2).any(|p| p == ["--effort", "high"]));
    assert!(args.windows(2).any(|p| p == ["--resume", "session-123"]));
    assert!(!args.iter().any(|a| a == "-p" || a == "--output-format" || a == "--permission-prompts"));
}

#[test]
fn build_interactive_command_configures_codex_fresh_and_resume() {
    let exe = PathBuf::from("C:\\tools\\codex.exe");
    let cwd = PathBuf::from("C:\\projects\\demo");

    // Fresh session: no "exec" or "resume" subcommand
    let (prog, fresh_args) = build_interactive_command(
        Provider::Codex,
        &exe,
        &cwd,
        Some("gpt-5"),
        Some("high"),
        None,
        PermissionMode::ReadOnly,
    );
    assert_eq!(prog, exe);
    assert!(!fresh_args.iter().any(|a| a == "exec" || a == "resume" || a == "--json" || a == "-"));
    assert!(fresh_args.windows(2).any(|p| p == ["-C", "C:\\projects\\demo"]));
    assert!(fresh_args.windows(2).any(|p| p == ["-s", "read-only"]));
    assert!(fresh_args.windows(2).any(|p| p == ["-m", "gpt-5"]));
    assert!(fresh_args.contains(&"model_reasoning_effort=high".to_owned()));

    // Resume session: starts with "resume <id>"
    let (_, resume_args) = build_interactive_command(
        Provider::Codex,
        &exe,
        &cwd,
        None,
        None,
        Some("thread-abc"),
        PermissionMode::Full,
    );
    assert_eq!(&resume_args[0..2], &["resume", "thread-abc"]);
    assert!(resume_args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_owned()));
    assert!(!resume_args.iter().any(|a| a == "-s"));
}

#[test]
fn build_interactive_command_configures_antigravity_workspace_and_conversation() {
    let exe = PathBuf::from("C:\\tools\\agy.exe");
    let cwd = PathBuf::from("C:\\projects\\demo");
    let (prog, args) = build_interactive_command(
        Provider::Antigravity,
        &exe,
        &cwd,
        Some("gemini-3.8-pro"),
        Some("low"),
        Some("conv-xyz"),
        PermissionMode::AcceptEdits,
    );
    assert_eq!(prog, exe);
    assert!(args.windows(2).any(|p| p == ["--add-dir", "C:\\projects\\demo"]));
    assert!(args.windows(2).any(|p| p == ["--mode", "accept-edits"]));
    assert!(args.windows(2).any(|p| p == ["--model", "gemini-3.8-pro"]));
    assert!(args.windows(2).any(|p| p == ["--effort", "low"]));
    assert!(args.windows(2).any(|p| p == ["--conversation", "conv-xyz"]));
    assert!(!args.iter().any(|a| a == "-p" || a == "--print" || a == "--output-format"));
}
```

### 5.2 Verification Commands
Run the verification sequence:
1. `cargo check` — compiles with 0 errors.
2. `cargo test build_interactive_command` — passes all new unit tests.
3. `cargo test` — entire repository test suite passes with 0 failures and 0 regressions.
4. `cargo clippy --all-targets` — verifies 0 warnings remain.
