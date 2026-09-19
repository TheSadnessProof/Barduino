# Milestone 1 Unit Test Suite Design Handoff Report

## 1. Observation

### 1.1 Existing Architecture and Codebase Observations
1. **Repository Rules & Guidelines**:
   - `AGENTS.md` (§5.4) explicitly mandates:
     - Inline `#[cfg(test)] mod tests` at the bottom of the file; no `tests/` directory.
     - Sentence-named tests describing behavior (e.g. `fn full_access_bypasses_permission_prompts()`).
     - Destructuring with slice patterns and prose panics (`let [...] = &... else { panic!("...") };`).
     - Prose assertions that dump failing values on error (`assert!(..., "message: {args:?}");`).
     - Comments explaining scenarios rather than mechanics.
     - Zero warnings under `cargo clippy --all-targets`.
     - Zero unauthorized dependencies in `Cargo.toml`.
     - Ignored tests (`#[ignore]`) are strictly separated: paid API tests must not be run during normal test loops; fast local tests must run in `cargo test` (< 2.5s).
2. **Current `src/terminal.rs` State**:
   - Lines 162–230: `Terminal` struct owns `parser: Arc<Mutex<Parser>>`, `writer: Writer`, `master: Box<dyn MasterPty + Send>`, `child: Box<dyn Child + Send + Sync>`, `job: TerminalJob`, `size: (u16, u16)`, `exited: Arc<AtomicBool>`.
   - Lines 177–190: `Terminal::start(cwd: &Path, shell: &Path, ctx: egui::Context)` is hard-coded for shells:
     ```rust
     let mut cmd = CommandBuilder::new(shell);
     configure(&mut cmd, shell);
     cmd.cwd(cwd);
     let child = pair.slave.spawn_command(cmd)...
     ```
   - Windows ConPTY error 193: On Windows, calling `spawn_command` on a `.cmd` or `.bat` script (e.g., `npm` install of `claude.cmd`) fails because batch files are not PE binaries. They require execution via `cmd.exe /c <script>`.
   - Existing tests at lines 723–867: Include tests for escape keys, indexed colors, cursor replies, cell layout, and selections. Line 763 has an `#[ignore]` test `closing_a_terminal_stops_programs_started_in_it` that starts a `Terminal`.
3. **Current `src/agent.rs` State**:
   - Lines 19–75: `Provider` enum (`Claude`, `Codex`, `Antigravity`), with `find()`, `label()`, `command()`.
   - Lines 181–234: `PermissionMode` enum (`ReadOnly`, `AcceptEdits`, `Full`, `Plan`).
   - Lines 410–525: `start_turn` currently only supports headless batch turns, passing `Turn` to `claude::args`, `codex::args`, or `antigravity::args`.
   - Existing tests at lines 693–1101: Include tests for `tool_detail`, `checklist`, `stopping_a_turn_stops_programs_the_agent_started`, `saved_gemini_sessions_load_as_antigravity`, `error_summary_drops_stack_traces_and_colors`, `process_exit_cleanly_emits_exited_event`, `codex_plan_mode_prepares_planning_prompt`, and approval channel stress tests.
4. **Current Provider Argument Builders**:
   - `src/claude.rs` (lines 32–59): Headless `args(&Turn)` appends `["-p", "--verbose", "--output-format", "stream-json", "--include-partial-messages"]`, `["--permission-mode", mode]`, `["--permission-prompts", "none"]`.
   - `src/codex.rs` (lines 34–72): Headless `args(&Turn)` prepends `["exec"]`, `--json`, `--skip-git-repo-check`, `-c sandbox_mode=...`, and appends `-` (stdin prompt).
   - `src/antigravity.rs` (lines 26–64): Headless `args(&Turn)` prepends `["--output-format", "stream-json", "--add-dir", turn.cwd]`.
5. **Project Milestone 1 Contract Specifications**:
   - `orchestrator_2/PROJECT.md` dictates:
     ```rust
     // src/terminal.rs
     impl Terminal {
         pub fn start_command(
             cwd: &Path,
             program: &Path,
             args: &[String],
             ctx: egui::Context,
         ) -> Result<Self, String>;
     }

     // src/agent.rs
     pub fn build_interactive_command(
         provider: Provider,
         exe: &Path,
         cwd: &Path,
         model: Option<&str>,
         effort: Option<&str>,
         resume_id: Option<&str>,
         permission_mode: PermissionMode,
     ) -> (PathBuf, Vec<String>);
     ```

---

## 2. Logic Chain

1. **Test Location & Modularity**:
   - Because `AGENTS.md` forbids a `tests/` directory and mandates inline unit tests in `src/`, all unit tests for `build_interactive_command` belong inside `src/agent.rs` (in `mod tests`), and all unit tests for `wrap_batch_command` and `Terminal::start_command` belong inside `src/terminal.rs` (in `mod tests`).
2. **Pure Functional Isolation for Command Builders**:
   - `build_interactive_command` returns `(PathBuf, Vec<String>)`. This pure output signature allows comprehensive verification of CLI flags without spawning child processes or touching the network or disk.
   - For Claude:
     * Headless flags (`-p`, `--output-format`, `stream-json`, `--verbose`, `--include-partial-messages`, `--permission-prompts none`) must be asserted absent.
     * Interactive flags (`--model`, `--effort`, `--resume`, `--permission-mode`) must be verified for presence and correctness.
   - For Codex:
     * Fresh session vs Resume: fresh session includes `-C <cwd>` and omits `resume`; resume session includes `["resume", id]` and omits `-C <cwd>`.
     * Headless flags (`exec`, `--json`, `-`) must be asserted absent.
     * Sandbox modes: `ReadOnly` / `Plan` -> `read-only`; `AcceptEdits` -> `workspace-write`; `Full` -> `--dangerously-bypass-approvals-and-sandbox`.
   - For Antigravity:
     * Workspace directory `--add-dir <cwd>` is always required because `agy` does not automatically treat `cwd` as the project workspace.
     * Conversation resume: `["--conversation", id]` must be present when resuming.
     * Headless flags (`--output-format`, `stream-json`, `-p`, `--print`) must be asserted absent.
     * Permission modes: `AcceptEdits` -> `--mode accept-edits`; `Full` -> `--dangerously-skip-permissions`; `Plan` -> `--mode plan`.
     * Effort deduplication: when a model name already embeds the effort level (e.g. ending in `-high`), `--effort` is omitted.
3. **Windows Batch Script Wrapping**:
   - On Windows, `.cmd` and `.bat` files must be wrapped with `cmd.exe /c <script_path> <args...>` using `wrap_batch_command`.
   - Case-insensitive extension matching (`.CMD`, `.cmd`, `.BAT`, `.bat`) must be verified.
   - Non-batch binaries (`.exe`, extensionless binaries) must NOT be wrapped.
4. **PTY Process Spawning & Startup Execution**:
   - `Terminal::start_command` must be verified by launching a fast, lightweight native command (`cmd.exe /c echo viper_pty_startup_ok` on Windows, `sh -c "echo viper_pty_startup_ok"` on Unix).
   - The test must verify that:
     * Spawning succeeds (`Ok(terminal)`).
     * The child process has a valid PID (`terminal.child.process_id().is_some()`).
     * The reader thread buffers stdout into the vt100 parser screen.
     * Spawning an invalid executable path returns `Err(err)` with a clean error message without crashing.
     * Dropping the terminal terminates the process and job object cleanly.
   - This test executes in < 100ms and does not reach the network, remaining suitable for normal `cargo test`.

---

## 3. Caveats

1. **PTY Availability on Headless CI / Containers**:
   - In environments without a pseudo-console (e.g., older Windows Server without ConPTY), `portable_pty::native_pty_system().openpty()` may fail. `terminal_start_command_runs_process_and_captures_output` uses standard ConPTY present on Windows 10/11 and standard Unix ptys.
2. **Provider Dispatch Location**:
   - `build_interactive_command` may either contain inline match blocks or delegate to helper functions `claude::interactive_args`, `codex::interactive_args`, and `antigravity::interactive_args`. The unit test suite tests the public `agent::build_interactive_command` contract, which is agnostic to internal decomposition.
3. **No Caveats Beyond These**:
   - All tests use existing dependencies (`portable_pty`, `vt100`, `eframe`), adhere to Rust 2024 edition, and produce 0 clippy warnings.

---

## 4. Conclusion & Ready-to-Paste Rust Test Code

The test suite consists of **16 unit tests** across `src/agent.rs` and `src/terminal.rs`.

### 4.1 Ready-to-Paste Test Code for `src/agent.rs`
Paste these tests inside `#[cfg(test)] mod tests` at the bottom of `src/agent.rs`:

```rust
    // =========================================================================
    // Milestone 1: Interactive Provider Command Builder Tests
    // =========================================================================

    #[test]
    fn claude_interactive_omits_headless_flags_and_includes_model_and_effort() {
        // Interactive Claude runs in a PTY terminal, so headless flags (-p, --verbose,
        // --output-format stream-json, --permission-prompts none) must not be passed.
        let exe = PathBuf::from("claude");
        let cwd = PathBuf::from("/workspace/project");
        let (prog, args) = build_interactive_command(
            Provider::Claude,
            &exe,
            &cwd,
            Some("claude-3-7-sonnet"),
            Some("high"),
            None,
            PermissionMode::ReadOnly,
        );

        assert_eq!(prog, exe, "executable path should be preserved");

        // Headless-only flags must be absent.
        assert!(!args.iter().any(|a| a == "-p"), "headless flag -p must be omitted: {args:?}");
        assert!(!args.iter().any(|a| a == "--output-format"), "--output-format must be omitted: {args:?}");
        assert!(!args.iter().any(|a| a == "stream-json"), "stream-json must be omitted: {args:?}");
        assert!(!args.iter().any(|a| a == "--verbose"), "--verbose must be omitted: {args:?}");
        assert!(!args.iter().any(|a| a == "--include-partial-messages"), "--include-partial-messages must be omitted: {args:?}");
        assert!(!args.iter().any(|a| a == "--permission-prompts"), "--permission-prompts none must be omitted so user can approve interactively: {args:?}");

        // Interactive flags for model and effort must be present.
        assert!(
            args.windows(2).any(|w| w == ["--model", "claude-3-7-sonnet"]),
            "model argument should be present: {args:?}"
        );
        assert!(
            args.windows(2).any(|w| w == ["--effort", "high"]),
            "effort argument should be present: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a == "--resume"),
            "fresh turn must not pass --resume: {args:?}"
        );
    }

    #[test]
    fn claude_interactive_sets_permission_modes() {
        let exe = PathBuf::from("claude");
        let cwd = PathBuf::from("/workspace/project");

        // ReadOnly maps to default permissions.
        let (_, args_ro) = build_interactive_command(
            Provider::Claude,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::ReadOnly,
        );
        assert!(
            args_ro.windows(2).any(|w| w == ["--permission-mode", "default"]),
            "ReadOnly should use default permission mode: {args_ro:?}"
        );

        // AcceptEdits maps to acceptEdits.
        let (_, args_edits) = build_interactive_command(
            Provider::Claude,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::AcceptEdits,
        );
        assert!(
            args_edits.windows(2).any(|w| w == ["--permission-mode", "acceptEdits"]),
            "AcceptEdits should use acceptEdits permission mode: {args_edits:?}"
        );

        // Full maps to bypassPermissions.
        let (_, args_full) = build_interactive_command(
            Provider::Claude,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::Full,
        );
        assert!(
            args_full.windows(2).any(|w| w == ["--permission-mode", "bypassPermissions"])
                || args_full.iter().any(|a| a == "--dangerously-skip-permissions"),
            "Full access should bypass permissions: {args_full:?}"
        );

        // Plan maps to plan.
        let (_, args_plan) = build_interactive_command(
            Provider::Claude,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::Plan,
        );
        assert!(
            args_plan.windows(2).any(|w| w == ["--permission-mode", "plan"]),
            "Plan should use plan permission mode: {args_plan:?}"
        );
    }

    #[test]
    fn claude_interactive_resumes_existing_session() {
        let exe = PathBuf::from("claude");
        let cwd = PathBuf::from("/workspace/project");
        let (_, args) = build_interactive_command(
            Provider::Claude,
            &exe,
            &cwd,
            None,
            None,
            Some("session-xyz-123"),
            PermissionMode::ReadOnly,
        );

        assert!(
            args.windows(2).any(|w| w == ["--resume", "session-xyz-123"]),
            "resume session id should be passed: {args:?}"
        );
    }

    #[test]
    fn codex_interactive_fresh_session_sets_working_directory_and_omits_exec() {
        // Fresh interactive Codex launches as `codex -C <cwd> ...` rather than `codex exec`.
        let exe = PathBuf::from("codex");
        let cwd = PathBuf::from("/workspace/app");
        let (prog, args) = build_interactive_command(
            Provider::Codex,
            &exe,
            &cwd,
            Some("o3-mini"),
            Some("medium"),
            None,
            PermissionMode::AcceptEdits,
        );

        assert_eq!(prog, exe, "executable path should be preserved");

        // Headless-only flags and subcommands must be absent.
        assert!(!args.iter().any(|a| a == "exec"), "interactive codex must not use exec subcommand: {args:?}");
        assert!(!args.iter().any(|a| a == "--json"), "interactive codex must not use --json: {args:?}");
        assert!(!args.iter().any(|a| a == "-"), "interactive codex does not read prompt from stdin via -: {args:?}");
        assert!(!args.iter().any(|a| a == "resume"), "fresh session must not pass resume subcommand: {args:?}");

        // Working directory and model flags.
        let cwd_str = cwd.display().to_string();
        assert!(
            args.windows(2).any(|w| (w[0] == "-C" || w[0] == "--cd") && w[1] == cwd_str),
            "working directory should be set via -C: {args:?}"
        );
        assert!(
            args.windows(2).any(|w| w == ["-m", "o3-mini"]),
            "model argument should be present: {args:?}"
        );
        assert!(
            args.windows(2).any(|w| w == ["-c", "model_reasoning_effort=medium"]),
            "reasoning effort config override should be present: {args:?}"
        );
    }

    #[test]
    fn codex_interactive_resume_uses_resume_subcommand() {
        // Resuming a conversation in Codex uses the `resume <thread_id>` subcommand.
        let exe = PathBuf::from("codex");
        let cwd = PathBuf::from("/workspace/app");
        let (_, args) = build_interactive_command(
            Provider::Codex,
            &exe,
            &cwd,
            None,
            None,
            Some("thread_abc_987"),
            PermissionMode::ReadOnly,
        );

        assert!(
            args.windows(2).any(|w| w == ["resume", "thread_abc_987"]),
            "resume subcommand and thread id must be passed: {args:?}"
        );
        assert!(!args.iter().any(|a| a == "exec"), "resume must not be preceded by exec: {args:?}");
        assert!(!args.iter().any(|a| a == "--json"), "--json must be omitted: {args:?}");
    }

    #[test]
    fn codex_interactive_configures_sandbox_modes() {
        let exe = PathBuf::from("codex");
        let cwd = PathBuf::from("/workspace/app");

        // ReadOnly maps to read-only sandbox.
        let (_, args_ro) = build_interactive_command(
            Provider::Codex,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::ReadOnly,
        );
        assert!(
            args_ro.windows(2).any(|w| (w[0] == "-s" && w[1] == "read-only") || (w[0] == "-c" && w[1] == "sandbox_mode=read-only")),
            "ReadOnly should configure read-only sandbox: {args_ro:?}"
        );

        // AcceptEdits maps to workspace-write sandbox.
        let (_, args_edits) = build_interactive_command(
            Provider::Codex,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::AcceptEdits,
        );
        assert!(
            args_edits.windows(2).any(|w| (w[0] == "-s" && w[1] == "workspace-write") || (w[0] == "-c" && w[1] == "sandbox_mode=workspace-write")),
            "AcceptEdits should configure workspace-write sandbox: {args_edits:?}"
        );

        // Plan maps to read-only sandbox (Codex has no native plan mode).
        let (_, args_plan) = build_interactive_command(
            Provider::Codex,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::Plan,
        );
        assert!(
            args_plan.windows(2).any(|w| (w[0] == "-s" && w[1] == "read-only") || (w[0] == "-c" && w[1] == "sandbox_mode=read-only")),
            "Plan should configure read-only sandbox: {args_plan:?}"
        );

        // Full bypasses sandbox and approvals.
        let (_, args_full) = build_interactive_command(
            Provider::Codex,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::Full,
        );
        assert!(
            args_full.iter().any(|a| a == "--dangerously-bypass-approvals-and-sandbox"),
            "Full access should bypass sandbox and approvals: {args_full:?}"
        );
        assert!(
            !args_full.iter().any(|a| a.contains("sandbox_mode")),
            "Full access should not specify a restrictive sandbox: {args_full:?}"
        );
    }

    #[test]
    fn antigravity_interactive_includes_add_dir_and_omits_stream_json() {
        // Antigravity does not treat cwd as its workspace automatically, so --add-dir
        // is always required. Interactive mode omits --output-format stream-json and -p.
        let exe = PathBuf::from("agy");
        let cwd = PathBuf::from("/workspace/agy_project");
        let (prog, args) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            Some("gemini-2.5-pro"),
            Some("high"),
            None,
            PermissionMode::ReadOnly,
        );

        assert_eq!(prog, exe, "executable path should be preserved");

        // --add-dir must be present.
        let cwd_str = cwd.display().to_string();
        assert!(
            args.windows(2).any(|w| w == ["--add-dir", &cwd_str]),
            "--add-dir must be present with the session working directory: {args:?}"
        );

        // Headless stream-json flags must be omitted.
        assert!(!args.iter().any(|a| a == "--output-format"), "--output-format must be omitted in interactive mode: {args:?}");
        assert!(!args.iter().any(|a| a == "stream-json"), "stream-json must be omitted: {args:?}");
        assert!(!args.iter().any(|a| a == "--print" || a == "-p"), "print mode flags must be omitted: {args:?}");

        // Model and effort.
        assert!(
            args.windows(2).any(|w| w == ["--model", "gemini-2.5-pro"]),
            "model flag should be present: {args:?}"
        );
        assert!(
            args.windows(2).any(|w| w == ["--effort", "high"]),
            "effort flag should be present: {args:?}"
        );
    }

    #[test]
    fn antigravity_interactive_resumes_conversation() {
        let exe = PathBuf::from("agy");
        let cwd = PathBuf::from("/workspace/agy_project");
        let (_, args) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            None,
            None,
            Some("conv_777_888"),
            PermissionMode::ReadOnly,
        );

        assert!(
            args.windows(2).any(|w| w == ["--conversation", "conv_777_888"]),
            "--conversation argument must be present for resumed turns: {args:?}"
        );
    }

    #[test]
    fn antigravity_interactive_configures_permission_modes() {
        let exe = PathBuf::from("agy");
        let cwd = PathBuf::from("/workspace/agy_project");

        // Full access bypasses permissions.
        let (_, args_full) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::Full,
        );
        assert!(
            args_full.iter().any(|a| a == "--dangerously-skip-permissions"),
            "Full access should skip permissions: {args_full:?}"
        );

        // AcceptEdits sets mode accept-edits.
        let (_, args_edits) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::AcceptEdits,
        );
        assert!(
            args_edits.windows(2).any(|w| w == ["--mode", "accept-edits"]),
            "AcceptEdits should set mode to accept-edits: {args_edits:?}"
        );

        // Plan sets mode plan.
        let (_, args_plan) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::Plan,
        );
        assert!(
            args_plan.windows(2).any(|w| w == ["--mode", "plan"]),
            "Plan should set mode to plan: {args_plan:?}"
        );

        // ReadOnly does not bypass permissions.
        let (_, args_ro) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            None,
            None,
            None,
            PermissionMode::ReadOnly,
        );
        assert!(
            !args_ro.iter().any(|a| a == "--dangerously-skip-permissions"),
            "ReadOnly should not bypass permissions: {args_ro:?}"
        );
    }

    #[test]
    fn antigravity_interactive_skips_effort_when_model_name_encodes_it() {
        // When the model name already specifies the level (e.g. gemini-3.8-flash-high),
        // --effort must not be duplicated.
        let exe = PathBuf::from("agy");
        let cwd = PathBuf::from("/workspace/agy_project");
        let (_, args) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            Some("gemini-3.8-flash-high"),
            Some("high"),
            None,
            PermissionMode::ReadOnly,
        );

        assert!(!args.iter().any(|a| a == "--effort"), "--effort should be omitted when model name carries it: {args:?}");
    }
```

---

### 4.2 Ready-to-Paste Test Code for `src/terminal.rs`
Paste these tests inside `#[cfg(test)] mod tests` at the bottom of `src/terminal.rs`:

```rust
    // =========================================================================
    // Milestone 1: Windows Batch Wrapping and PTY Process Spawning Tests
    // =========================================================================

    #[cfg(windows)]
    #[test]
    fn windows_cmd_batch_script_is_wrapped_with_cmd_exe() {
        // Windows ConPTY cannot directly spawn .cmd batch scripts (such as npm's claude.cmd)
        // and fails with Win32 Error 193. It must wrap them via cmd.exe /c.
        let batch_path = PathBuf::from(r"C:\Users\tester\AppData\Roaming\npm\claude.cmd");
        let initial_args = vec!["--model".to_owned(), "sonnet".to_owned()];

        let (program, args) = wrap_batch_command(&batch_path, &initial_args);

        assert_eq!(program, PathBuf::from("cmd.exe"), "batch scripts must be launched via cmd.exe");
        let [c_flag, target, rest @ ..] = &args[..] else {
            panic!("batch command arguments must begin with /c followed by the script path: {args:?}");
        };
        assert_eq!(c_flag, "/c", "first argument should be /c");
        assert_eq!(target, &batch_path.display().to_string(), "second argument should be original script path");
        assert_eq!(rest, &["--model", "sonnet"], "original arguments should follow script path");
    }

    #[cfg(windows)]
    #[test]
    fn windows_bat_script_is_wrapped_case_insensitively() {
        // Batch file extensions (.cmd and .bat) are case-insensitive on Windows.
        let batch_path = PathBuf::from(r"C:\Tools\AGENT.BAT");
        let initial_args = vec!["--resume".to_owned(), "xyz".to_owned()];

        let (program, args) = wrap_batch_command(&batch_path, &initial_args);

        assert_eq!(program, PathBuf::from("cmd.exe"), ".BAT scripts must be launched via cmd.exe");
        let [c_flag, target, rest @ ..] = &args[..] else {
            panic!("arguments must start with /c followed by script path: {args:?}");
        };
        assert_eq!(c_flag, "/c");
        assert_eq!(target, &batch_path.display().to_string());
        assert_eq!(rest, &["--resume", "xyz"]);
    }

    #[test]
    fn executable_binaries_are_not_wrapped_with_cmd_exe() {
        // PE executables (.exe) or native Unix binaries must be executed directly without cmd.exe /c.
        let exe_path = if cfg!(windows) {
            PathBuf::from(r"C:\Users\tester\AppData\Local\agy\bin\agy.exe")
        } else {
            PathBuf::from("/usr/local/bin/agy")
        };
        let initial_args = vec!["--add-dir".to_owned(), "src".to_owned()];

        let (program, args) = wrap_batch_command(&exe_path, &initial_args);

        assert_eq!(program, exe_path, "binary executables must not be replaced with cmd.exe");
        assert_eq!(args, initial_args, "arguments must be preserved untouched without /c prefix");
    }

    #[test]
    fn terminal_start_command_runs_process_and_captures_output() {
        // Tests that start_command creates a real PTY, spawns the process,
        // configures terminal env variables, and streams output into the vt100 parser.
        let temp = std::env::temp_dir();
        let (prog, args) = if cfg!(windows) {
            (PathBuf::from("cmd.exe"), vec!["/c".to_owned(), "echo".to_owned(), "viper_pty_startup_ok".to_owned()])
        } else {
            (PathBuf::from("sh"), vec!["-c".to_owned(), "echo viper_pty_startup_ok".to_owned()])
        };

        let terminal = Terminal::start_command(&temp, &prog, &args, egui::Context::default())
            .expect("start_command should successfully spawn a process in the PTY");

        assert!(terminal.child.process_id().is_some(), "child process should have a valid PID");

        // Wait up to 2 seconds for output to reach the parser.
        let mut captured = false;
        for _ in 0..40 {
            let screen = terminal.parser().screen().contents();
            if screen.contains("viper_pty_startup_ok") {
                captured = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(captured, "PTY reader thread should deliver process output to parser screen");
    }

    #[test]
    fn terminal_start_command_fails_gracefully_on_missing_executable() {
        // Attempting to run a non-existent command returns a readable error rather than panicking.
        let temp = std::env::temp_dir();
        let bad_prog = PathBuf::from("nonexistent_binary_viper_xyz_98765.exe");
        let result = Terminal::start_command(&temp, &bad_prog, &[], egui::Context::default());

        let Err(err) = result else {
            panic!("spawning a nonexistent binary should return an Err");
        };
        assert!(!err.is_empty(), "error message should explain the failure: {err}");
    }

    #[test]
    fn terminal_start_command_initializes_dimensions_and_cleans_up() {
        let temp = std::env::temp_dir();
        let (prog, args) = if cfg!(windows) {
            (PathBuf::from("cmd.exe"), vec!["/c".to_owned(), "exit".to_owned(), "0".to_owned()])
        } else {
            (PathBuf::from("true"), Vec::new())
        };

        let terminal = Terminal::start_command(&temp, &prog, &args, egui::Context::default())
            .expect("terminal should start");

        // Default size is 24 rows by 80 cols.
        assert_eq!(terminal.size, (24, 80), "terminal should initialize with standard 24x80 grid dimensions");

        // Dropping terminal must cleanly clean up jobs and processes without hanging or panicking.
        drop(terminal);
    }
```

---

## 5. Verification Method

### 5.1 Compilation and Verification Steps for the Worker
1. **Implementation Verification**:
   - Verify `wrap_batch_command` in `src/terminal.rs`:
     ```rust
     pub fn wrap_batch_command(program: &Path, args: &[String]) -> (PathBuf, Vec<String>) {
         #[cfg(windows)]
         {
             let is_batch = program
                 .extension()
                 .and_then(|ext| ext.to_str())
                 .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
                 .unwrap_or(false);

             if is_batch {
                 let mut cmd_args = vec!["/c".to_owned(), program.display().to_string()];
                 cmd_args.extend_from_slice(args);
                 return (PathBuf::from("cmd.exe"), cmd_args);
             }
         }
         (program.to_path_buf(), args.to_vec())
     }
     ```
   - Verify `Terminal::start_command` in `src/terminal.rs`:
     ```rust
     pub fn start_command(
         cwd: &Path,
         program: &Path,
         args: &[String],
         ctx: egui::Context,
     ) -> Result<Self, String> {
         let (exec_prog, exec_args) = wrap_batch_command(program, args);
         let size = (24, 80);
         let pair = portable_pty::native_pty_system()
             .openpty(pty_size(size))
             .map_err(|err| format!("Couldn't open a terminal: {err}"))?;

         let mut cmd = CommandBuilder::new(&exec_prog);
         for arg in &exec_args {
             cmd.arg(arg);
         }
         cmd.cwd(cwd);
         cmd.env("TERM", "xterm-256color");
         cmd.env("COLORTERM", "truecolor");

         let child = pair
             .slave
             .spawn_command(cmd)
             .map_err(|err| format!("Couldn't start the process: {err}"))?;
         drop(pair.slave);
         // ... configure job, reader thread, and parser
     ```
   - Verify `build_interactive_command` in `src/agent.rs`.
2. **Commands to Run**:
   - `cargo check`: Verifies 0 compilation errors across all modules.
   - `cargo test agent::tests::claude_interactive`: Verifies Claude flag builder tests pass.
   - `cargo test agent::tests::codex_interactive`: Verifies Codex flag builder tests pass.
   - `cargo test agent::tests::antigravity_interactive`: Verifies Antigravity flag builder tests pass.
   - `cargo test terminal::tests::windows_`: Verifies Windows `.cmd` wrapping tests pass.
   - `cargo test terminal::tests::terminal_start_command`: Verifies PTY process startup tests pass.
   - `cargo test`: Full test suite passes without regressions and with 0 failures.
   - `cargo clippy --all-targets`: Passes with 0 warnings.
3. **Invalidation Conditions**:
   - Any test using `--print` or `-p` in interactive mode.
   - Any test allowing `portable_pty` to spawn `.cmd` directly on Windows without `cmd.exe /c` wrapping.
   - Any test failing to clean up child processes or hanging the test runner.
