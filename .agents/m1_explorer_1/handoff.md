# Milestone 1 Technical Design & Implementation Blueprint: Interactive PTY Terminal Spawning

## 1. Observation

### Current Implementation in `src/terminal.rs`
- In `src/terminal.rs:177-230`, `Terminal::start(cwd: &Path, shell: &Path, ctx: egui::Context) -> Result<Self, String>` currently starts only a shell (e.g. `pwsh`, `cmd`, `bash`).
- It constructs `CommandBuilder::new(shell)` and configures it via `configure(&mut cmd, shell)` (`src/terminal.rs:575-583`):
  ```rust
  fn configure(cmd: &mut CommandBuilder, shell: &Path) {
      let name = shell.file_stem().unwrap_or_default().to_string_lossy().to_lowercase();
      match name.as_str() {
          "pwsh" | "powershell" => cmd.arg("-NoLogo"),
          "cmd" => {}
          _ => cmd.env("TERM", "xterm-256color"),
      }
  }
  ```
- In `src/terminal.rs:186-191`, `pair.slave.spawn_command(cmd)` spawns the child process into the pseudo-terminal (PTY) using Windows ConPTY or Unix PTY via `portable-pty`.
- In `src/terminal.rs:192`, `let job = TerminalJob::new(child.process_id());` attaches the root process to a Win32 Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`src/terminal.rs:78-88`) or process group kill on Unix (`src/terminal.rs:142-146`). When the `Terminal` struct is dropped, `Terminal::drop` calls `self.job.kill()` and `self.child.kill()` (`src/terminal.rs:420-425`).
- In `src/terminal.rs:196-201`, `Parser::new_with_callbacks(size.0, size.1, SCROLLBACK_LINES, Replies::default())` installs the `Replies` callback (`src/terminal.rs:27-54`). Windows ConPTY issues CSI queries on startup (`\x1b[6n` cursor position and `\x1b[5n` status report). If not answered immediately by the reader thread (`src/terminal.rs:213-220`), ConPTY blocks and produces no output.

### Windows ConPTY Error 193 with `.cmd` / `.bat` Scripts
- In `src/claude.rs:25-28`, `claude::find_executable()` returns npm-installed batch wrappers:
  ```rust
  if let Some(app_data) = std::env::var_os("APPDATA") {
      fallbacks.push(PathBuf::from(app_data).join("npm").join("claude.cmd"));
  }
  ```
- Windows `CreateProcessW` directly executes PE binary executables (`.exe`). When passed a batch script (`.cmd` or `.bat`) as the application name, Windows returns `ERROR_BAD_EXE_FORMAT` (code 193 / 0xC1, *"%1 is not a valid Win32 application"*).
- In `portable_pty::CommandBuilder` on Windows, `search_path` and `cmdline` format the executable directly into `CreateProcessW` arguments. Passing `claude.cmd` directly causes ConPTY to fail immediately with error 193.
- Batch scripts on Windows must be invoked via `cmd.exe /c "<path_to_script.cmd>" <args...>`.

### Environment Variables
- `src/terminal.rs:581` currently only sets `TERM=xterm-256color` for non-PowerShell, non-cmd shells.
- Interactive AI CLIs (Claude Code, Codex CLI, Antigravity `agy`) use terminal detection:
  - `TERM=xterm-256color` enables ANSI escape sequences, 256 colors, and cursor manipulation.
  - `COLORTERM=truecolor` signals direct 24-bit RGB TrueColor support, which `terminal.rs:664` supports (`vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b)`).

### Provider CLI Interactive Flags vs Headless Turns
- Currently in `claude.rs:32-58`, `codex.rs:34-71`, and `antigravity.rs:26-68`, `args(&Turn)` constructs arguments for headless turns:
  - Claude passes `-p`, `--verbose`, `--output-format stream-json`, `--permission-prompts none`.
  - Codex passes `exec`, `--json`, `-`.
  - Antigravity passes `--output-format stream-json`.
- In interactive terminal mode, these headless flags must NOT be passed. Instead, the native TUI interactive mode must run so the user can interact directly via keystrokes and ANSI rendering.

---

## 2. Logic Chain

1. **PTY Generalization**: `Terminal::start` currently only runs shells with fixed arguments. Factoring out `Terminal::spawn(cmd: CommandBuilder, ctx: egui::Context, spawn_err_msg: &str) -> Result<Self, String>` allows both `Terminal::start` (for secondary tools shell) and `Terminal::start_command` (for provider CLI execution) to share identical PTY setup, Win32 Job Object assignment, reader thread, and CSI `Replies` callback without duplication.
2. **Preventing Error 193**: If `program` on Windows has extension `.cmd` or `.bat` (case-insensitive) or resolves to a batch script on `PATH`, wrapping it as `cmd.exe /c <program> <args...>` ensures `CreateProcessW` receives `cmd.exe` as the PE executable and delegates batch execution to `cmd.exe`.
3. **Environment Setup**: In `build_command`, setting `cmd.env("TERM", "xterm-256color")` and `cmd.env("COLORTERM", "truecolor")` ensures every command run in the terminal gets rich ANSI TrueColor formatting without relying on shell heuristics.
4. **Win32 Job Object Inheritance**: When `cmd.exe /c claude.cmd` is spawned, `cmd.exe` is assigned to the Job Object (`TerminalJob`). Windows automatically assigns child processes created by `cmd.exe` (such as `node.exe`, and any compilers/tools spawned by Claude) to the same Job Object. On terminal drop, `TerminalJob::drop` terminates the Job Object, killing the entire process tree cleanly.
5. **Interactive Argument Builders**: Provider interactive commands require distinct arguments from headless turns. Implementing `claude::interactive_args`, `codex::interactive_args`, and `antigravity::interactive_args` routed through `agent::build_interactive_command` isolates CLI syntax rules to their respective modules while providing a uniform `(PathBuf, Vec<String>)` interface for `Terminal::start_command`.

---

## 3. Caveats

- **Existing Tests Untouched**: `Terminal::start` remains fully backward-compatible for existing tools panel shell tabs and tests.
- **Ignored Tests Unchanged**: `closing_a_terminal_stops_programs_started_in_it` and `finds_the_shells_on_this_computer` remain ignored as required by `AGENTS.md §3.2`. New unit tests cover `build_command`, batch script wrapping, environment variables, and invalid binary error reporting in milliseconds without spawning heavy external processes.
- **Future Milestone Decoupling**: Milestone 1 defines only process spawning and argument building (`src/terminal.rs` and `src/agent.rs` / provider modules). The central panel UI swap (`chat_area` to `terminal_area` in `src/app.rs`) and session terminal map lifecycle will be wired in Milestones 2 and 3.

---

## 4. Conclusion & Implementation Blueprint

### Blueprint for `src/terminal.rs`

#### 1. Add Windows Batch Script Resolution & ComSpec Helper
Place above `impl Terminal`:

```rust
#[cfg(windows)]
fn is_batch_script(program: &Path) -> bool {
    let has_batch_ext = |p: &Path| {
        p.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
            .unwrap_or(false)
    };
    if has_batch_ext(program) {
        return true;
    }
    if program.extension().is_none() {
        let name = program.to_string_lossy();
        if let Some(found) = agent::find_on_path(&[&format!("{name}.cmd"), &format!("{name}.bat")]) {
            return has_batch_ext(&found);
        }
    }
    false
}

#[cfg(windows)]
fn comspec() -> PathBuf {
    std::env::var_os("ComSpec")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| agent::find_on_path(&["cmd.exe"]))
        .unwrap_or_else(|| PathBuf::from("cmd.exe"))
}
```

#### 2. Implement `build_command`
```rust
/// Prepares a CommandBuilder for running an arbitrary program inside the terminal,
/// configuring working directory, TrueColor environment variables, and Windows
/// batch script wrapping to avoid ConPTY error 193.
pub fn build_command(cwd: &Path, program: &Path, args: &[String]) -> CommandBuilder {
    #[cfg(windows)]
    let mut cmd = if is_batch_script(program) {
        let mut cmd = CommandBuilder::new(comspec());
        cmd.arg("/c");
        cmd.arg(program);
        cmd
    } else {
        CommandBuilder::new(program)
    };

    #[cfg(not(windows))]
    let mut cmd = CommandBuilder::new(program);

    cmd.args(args);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd
}
```

#### 3. Refactor `impl Terminal`
Replace `Terminal::start` with:
```rust
impl Terminal {
    /// Starts a shell in a pseudo-terminal.
    pub fn start(cwd: &Path, shell: &Path, ctx: egui::Context) -> Result<Self, String> {
        let mut cmd = CommandBuilder::new(shell);
        configure(&mut cmd, shell);
        cmd.cwd(cwd);
        Self::spawn(cmd, ctx, "Couldn't start the shell")
    }

    /// Starts an arbitrary command in a pseudo-terminal with TrueColor environment
    /// and Windows batch file wrapping.
    pub fn start_command(
        cwd: &Path,
        program: &Path,
        args: &[String],
        ctx: egui::Context,
    ) -> Result<Self, String> {
        let cmd = build_command(cwd, program, args);
        Self::spawn(cmd, ctx, "Couldn't start the program")
    }

    fn spawn(cmd: CommandBuilder, ctx: egui::Context, spawn_err_msg: &str) -> Result<Self, String> {
        let size = (24, 80);
        let pair = portable_pty::native_pty_system()
            .openpty(pty_size(size))
            .map_err(|err| format!("Couldn't open a terminal: {err}"))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|err| format!("{spawn_err_msg}: {err}"))?;
        drop(pair.slave);

        let job = TerminalJob::new(child.process_id());

        let mut reader = pair.master.try_clone_reader().map_err(|err| err.to_string())?;
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(|err| err.to_string())?));
        let parser = Arc::new(Mutex::new(Parser::new_with_callbacks(
            size.0,
            size.1,
            SCROLLBACK_LINES,
            Replies::default(),
        )));
        let exited = Arc::new(AtomicBool::new(false));

        let output = Arc::clone(&parser);
        let reply_writer = Arc::clone(&writer);
        let done = Arc::clone(&exited);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let replies = {
                            let mut parser = output.lock().unwrap_or_else(PoisonError::into_inner);
                            parser.process(&buf[..n]);
                            std::mem::take(&mut parser.callbacks_mut().pending)
                        };
                        if !replies.is_empty() {
                            write_all(&reply_writer, &replies);
                        }
                        ctx.request_repaint();
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });

        Ok(Self { parser, writer, master: pair.master, child, job, size, exited, scroll_remainder: 0.0, selection: None })
    }

    /// Whether the process running in the terminal has exited.
    pub fn has_exited(&mut self) -> bool {
        // On Windows the output pipe can stay open after the shell exits, so also ask the process.
        self.exited.load(Ordering::Relaxed) || matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Sends bytes directly to the running process's input.
    pub fn write_all(&self, bytes: &[u8]) {
        write_all(&self.writer, bytes);
    }
```

#### 4. Update Exit Notice in `Terminal::ui`
In `src/terminal.rs:247`:
```rust
        if self.has_exited() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("The process has exited.").weak());
                restart = ui.button("Restart").clicked();
            });
        }
```

#### 5. Unit Tests to Add in `src/terminal.rs::tests`
```rust
    #[test]
    fn build_command_sets_environment_and_working_dir() {
        let cwd = Path::new("/test/dir");
        let program = Path::new("claude");
        let args = vec!["--model".to_owned(), "sonnet".to_owned()];
        let cmd = build_command(cwd, program, &args);
        assert_eq!(cmd.get_cwd(), Some(&std::ffi::OsString::from("/test/dir")));
        assert_eq!(cmd.get_env("TERM"), Some(std::ffi::OsStr::new("xterm-256color")));
        assert_eq!(cmd.get_env("COLORTERM"), Some(std::ffi::OsStr::new("truecolor")));
    }

    #[cfg(windows)]
    #[test]
    fn build_command_wraps_batch_scripts_with_cmd_exe() {
        let cwd = Path::new(r"C:\test\dir");
        let program = Path::new(r"C:\Users\tester\AppData\Roaming\npm\claude.cmd");
        let args = vec!["--model".to_owned(), "sonnet".to_owned()];
        let cmd = build_command(cwd, program, &args);
        let argv = cmd.get_argv();
        assert!(argv[0].to_string_lossy().to_lowercase().ends_with("cmd.exe"), "{argv:?}");
        assert_eq!(argv[1], "/c");
        assert_eq!(argv[2], program.as_os_str());
        assert_eq!(argv[3], "--model");
        assert_eq!(argv[4], "sonnet");
    }

    #[cfg(windows)]
    #[test]
    fn build_command_leaves_executables_unwrapped() {
        let cwd = Path::new(r"C:\test\dir");
        let program = Path::new(r"C:\Program Files\OpenAI\Codex\bin\codex.exe");
        let args = vec!["-m".to_owned(), "o3".to_owned()];
        let cmd = build_command(cwd, program, &args);
        let argv = cmd.get_argv();
        assert_eq!(argv[0], program.as_os_str());
        assert_eq!(argv[1], "-m");
        assert_eq!(argv[2], "o3");
    }

    #[cfg(windows)]
    #[test]
    fn detects_batch_script_extensions_case_insensitively() {
        assert!(is_batch_script(Path::new("claude.cmd")));
        assert!(is_batch_script(Path::new("claude.bat")));
        assert!(is_batch_script(Path::new("CLAUDE.CMD")));
        assert!(is_batch_script(Path::new("CLAUDE.BAT")));
        assert!(!is_batch_script(Path::new("claude.exe")));
        assert!(!is_batch_script(Path::new("claude.ps1")));
    }

    #[test]
    fn start_command_fails_gracefully_on_invalid_binary() {
        let result = Terminal::start_command(
            &std::env::temp_dir(),
            Path::new("nonexistent_binary_12345.exe"),
            &[],
            egui::Context::default(),
        );
        let Err(err) = result else {
            panic!("spawning nonexistent binary should fail");
        };
        assert!(err.contains("Couldn't start the program"), "{err}");
    }
```

---

### Blueprint for Provider Interactive Arguments (`src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`, `src/agent.rs`)

#### 1. `src/claude.rs`
Add `pub fn interactive_args`:
```rust
/// Arguments for starting Claude Code interactively inside an embedded terminal.
pub fn interactive_args(
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mode_str = match permission_mode {
        PermissionMode::ReadOnly => "default",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::Full => "bypassPermissions",
        PermissionMode::Plan => "plan",
    };
    let mut args = vec!["--permission-mode".to_owned(), mode_str.to_owned()];
    if permission_mode == PermissionMode::ReadOnly {
        args.extend(["--disallowed-tools".to_owned(), "Bash".to_owned()]);
    }
    if let Some(model) = model {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = effort {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    if let Some(resume_id) = resume_id {
        args.extend(["--resume".to_owned(), resume_id.to_owned()]);
    }
    args
}
```

#### 2. `src/codex.rs`
Add `pub fn interactive_args`:
```rust
/// Arguments for starting Codex CLI interactively inside an embedded terminal.
pub fn interactive_args(
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let sandbox = match permission_mode {
        PermissionMode::ReadOnly => Some("read-only"),
        PermissionMode::AcceptEdits => Some("workspace-write"),
        PermissionMode::Plan => Some("read-only"),
        PermissionMode::Full => None,
    };
    let mut args = Vec::new();
    if let Some(thread_id) = resume_id {
        args.extend(["resume".to_owned(), thread_id.to_owned()]);
    }
    args.push("--skip-git-repo-check".to_owned());
    match sandbox {
        Some(sandbox) => args.extend(["-c".to_owned(), format!("sandbox_mode={sandbox}")]),
        None => args.push("--dangerously-bypass-approvals-and-sandbox".to_owned()),
    }
    if resume_id.is_none() {
        args.extend(["-C".to_owned(), cwd.display().to_string()]);
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

#### 3. `src/antigravity.rs`
Add `pub fn interactive_args`:
```rust
/// Arguments for starting Antigravity interactively inside an embedded terminal.
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
        PermissionMode::AcceptEdits => args.extend(["--mode".into(), "accept-edits".into()]),
        PermissionMode::Full => args.push("--dangerously-skip-permissions".into()),
        PermissionMode::Plan => args.extend(["--mode".into(), "plan".into()]),
    }
    if let Some(model) = model {
        args.extend(["--model".into(), model.to_owned()]);
        let named_level = ["-low", "-medium", "-high"].iter().any(|level| model.ends_with(level));
        if let Some(effort) = effort
            && !named_level
        {
            args.extend(["--effort".into(), effort.to_owned()]);
        }
    } else if let Some(effort) = effort {
        args.extend(["--effort".into(), effort.to_owned()]);
    }
    if let Some(conversation_id) = resume_id {
        args.extend(["--conversation".into(), conversation_id.to_owned()]);
    }
    args
}
```

#### 4. `src/agent.rs`
Add `pub fn build_interactive_command`:
```rust
/// Builds the executable path and arguments for starting an interactive session terminal.
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
        Provider::Claude => claude::interactive_args(model, effort, resume_id, permission_mode),
        Provider::Codex => codex::interactive_args(cwd, model, effort, resume_id, permission_mode),
        Provider::Antigravity => antigravity::interactive_args(cwd, model, effort, resume_id, permission_mode),
    };
    (exe.to_path_buf(), args)
}
```

Add unit tests in `src/agent.rs::tests`:
```rust
    #[test]
    fn builds_claude_interactive_arguments() {
        let exe = PathBuf::from("claude");
        let cwd = PathBuf::from("/test/dir");
        let (prog, args) = build_interactive_command(
            Provider::Claude,
            &exe,
            &cwd,
            Some("claude-3-7-sonnet"),
            Some("high"),
            Some("sess-123"),
            PermissionMode::ReadOnly,
        );
        assert_eq!(prog, exe);
        assert_eq!(args, [
            "--permission-mode", "default",
            "--disallowed-tools", "Bash",
            "--model", "claude-3-7-sonnet",
            "--effort", "high",
            "--resume", "sess-123",
        ]);
    }

    #[test]
    fn builds_codex_interactive_arguments() {
        let exe = PathBuf::from("codex");
        let cwd = PathBuf::from("/test/dir");
        let (prog, args) = build_interactive_command(
            Provider::Codex,
            &exe,
            &cwd,
            Some("o3-mini"),
            Some("high"),
            None,
            PermissionMode::Full,
        );
        assert_eq!(prog, exe);
        assert_eq!(args, [
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox",
            "-C", "/test/dir",
            "-m", "o3-mini",
            "-c", "model_reasoning_effort=high",
        ]);
    }

    #[test]
    fn builds_antigravity_interactive_arguments() {
        let exe = PathBuf::from("agy");
        let cwd = PathBuf::from("/test/dir");
        let (prog, args) = build_interactive_command(
            Provider::Antigravity,
            &exe,
            &cwd,
            Some("gemini-2.5-pro"),
            Some("high"),
            Some("conv-456"),
            PermissionMode::AcceptEdits,
        );
        assert_eq!(prog, exe);
        assert_eq!(args, [
            "--add-dir", "/test/dir",
            "--mode", "accept-edits",
            "--model", "gemini-2.5-pro",
            "--effort", "high",
            "--conversation", "conv-456",
        ]);
    }
```

---

## 5. Verification Method

To independently verify the implementation:

1. **Compilation Check**:
   ```powershell
   cargo check
   ```
   Must exit with code 0 in < 1s.

2. **Unit Test Suite**:
   ```powershell
   cargo test
   ```
   Must pass all tests (all 237 existing tests + new unit tests for `build_command`, batch wrapping, error handling, and interactive arguments), with 0 failures and nothing newly ignored.
   *Do not run `cargo test -- --ignored`.*

3. **Clippy Linter**:
   ```powershell
   cargo clippy --all-targets -- -D warnings
   ```
   Must exit with code 0 and 0 warnings.

4. **Code Quality & Integrity Check**:
   - `git diff` contains no formatting rewrites on untouched lines (`AGENTS.md §3.3`).
   - `Cargo.toml` is untouched (0 new dependencies, `AGENTS.md §3.4`).
   - All tests use sentence-style names with descriptive prose assert messages (`AGENTS.md §5.4`).

5. **Invalidation Conditions**:
   - Spawning `claude.cmd` fails with Windows error 193.
   - PTY master or slave handle leaks on failure.
   - `TERM` or `COLORTERM` environment variables fail to register on spawned child.
   - Any compiler or clippy warning is introduced.
