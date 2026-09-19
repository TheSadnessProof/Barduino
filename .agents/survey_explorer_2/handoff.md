# Technical Survey Handoff Report: Provider CLI Implementations and Interactive PTY Process Execution in Viper

## 1. Observation

### 1.1 Provider Executable Discovery (`find_executable` & `settings.rs`)
- **`src/agent.rs` (lines 51-75)**:
  `Provider::command` returns the base command string:
  ```rust
  pub fn command(self) -> &'static str {
      match self {
          Self::Claude => "claude",
          Self::Codex => "codex",
          Self::Antigravity => "agy",
      }
  }
  ```
  `Provider::find` dispatches to each module's `find_executable`:
  ```rust
  pub fn find(self) -> Option<PathBuf> {
      match self {
          Self::Claude => claude::find_executable(),
          Self::Codex => codex::find_executable(),
          Self::Antigravity => antigravity::find_executable(),
      }
  }
  ```
- **`src/claude.rs` (lines 13-29)**:
  ```rust
  pub fn find_executable() -> Option<PathBuf> {
      let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
      if let Some(exe) = agent::find_on_path(names) {
          return Some(exe);
      }
      let home = agent::home_dir()?;
      let mut fallbacks = vec![
          home.join(".local").join("bin").join(names[0]),
          home.join(".claude").join("local").join(names[0]),
      ];
      if let Some(app_data) = std::env::var_os("APPDATA") {
          fallbacks.push(PathBuf::from(app_data).join("npm").join("claude.cmd"));
      }
      fallbacks.into_iter().find(|candidate| candidate.is_file())
  }
  ```
  On the test Windows machine, `claude` was located at `C:\Users\ditob\.local\bin\claude.exe`.
- **`src/codex.rs` (lines 14-31)**:
  ```rust
  pub fn find_executable() -> Option<PathBuf> {
      let name = if cfg!(windows) { "codex.exe" } else { "codex" };
      if let Some(exe) = agent::find_on_path(&[name]) {
          return Some(exe);
      }
      let mut fallbacks = Vec::new();
      if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
          let programs = PathBuf::from(local_app_data).join("Programs");
          fallbacks.push(programs.join("OpenAI").join("Codex").join("bin").join(name));
      }
      if let Some(home) = agent::home_dir() {
          fallbacks.push(home.join(".codex").join("bin").join(name));
      }
      fallbacks.into_iter().find(|candidate| candidate.is_file())
  }
  ```
  On the test Windows machine, `codex` was located at `C:\Users\ditob\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe`.
- **`src/antigravity.rs` (lines 14-23)**:
  ```rust
  pub fn find_executable() -> Option<PathBuf> {
      let name = if cfg!(windows) { "agy.exe" } else { "agy" };
      if let Some(exe) = agent::find_on_path(&[name]) {
          return Some(exe);
      }
      let local_app_data = std::env::var_os("LOCALAPPDATA")?;
      Some(PathBuf::from(local_app_data).join("agy").join("bin").join(name))
          .filter(|exe| exe.is_file())
  }
  ```
  On the test Windows machine, `agy` was located at `C:\Users\ditob\AppData\Local\agy\bin\agy.exe`.
- **`src/settings.rs` (lines 106-133)**:
  `Detected::scan` checks `settings.custom_executables.get(&provider)` first; if absent, it falls back to `provider.find()`. `Detected::get(provider)` returns the configured or detected executable path.

### 1.2 Headless Execution vs Interactive CLI Arguments
- **Claude Code**:
  - Current Headless (`src/claude.rs:32-59`):
    `args = ["-p", "--verbose", "--output-format", "stream-json", "--include-partial-messages", "--permission-mode", mode, "--permission-prompts", "none"]`.
    Stdin: Prompt written to stdin via `stdin.write_all(prompt.as_bytes())`.
  - Interactive CLI (`claude --help`):
    `claude [options] [command] [prompt]`
    Interactive by default. `-p`/`--print` triggers non-interactive print mode.
    Key interactive flags:
    `--resume [session_id]`, `--model <model>`, `--effort <effort>`, `--permission-mode <mode>`, `--add-dir <directories...>`.
    Interactive permission prompts and TUI approvals are native when running without `-p` and `--permission-prompts none`.
- **OpenAI Codex**:
  - Current Headless (`src/codex.rs:34-72`):
    `args = ["exec", "--json", "--skip-git-repo-check", "-c", "sandbox_mode=<mode>", "-C", cwd, "-"]`
    (or `exec resume <thread_id>`).
    Stdin: Prompt piped to stdin ("-").
  - Interactive CLI (`codex --help`, `codex resume --help`):
    `codex [OPTIONS] [PROMPT]`
    "If no subcommand is specified, options will be forwarded to the interactive CLI."
    Subcommands:
    - Fresh session: `codex [OPTIONS] [PROMPT]`
    - Resume session: `codex resume [SESSION_ID]`
    Key interactive flags:
    `-C, --cd <DIR>`, `-m, --model <MODEL>`, `-c model_reasoning_effort=<effort>`, `-s, --sandbox <SANDBOX_MODE>` (or `--dangerously-bypass-approvals-and-sandbox`).
    Codex TUI runs full-screen alternate screen with interactive approvals and arrow navigation.
- **Google Antigravity (`agy`)**:
  - Current Headless (`src/antigravity.rs:26-64`):
    `args = ["--output-format", "stream-json", "--add-dir", cwd, ...]`
    Stdin: Prompt piped to stdin.
  - Interactive CLI (`agy --help`):
    `agy [options]`
    Interactive by default. `-p` or `--print` triggers non-interactive single-prompt mode.
    Key interactive flags:
    `--add-dir <dir>` (critical: adds workspace directory to the session),
    `--conversation <id>` (or `-c` / `--continue` to resume),
    `--model <model>`, `--effort <effort>`, `--mode <accept-edits|plan>`, `--dangerously-skip-permissions`.

### 1.3 Working Directory & Worktree Context
- **`src/session.rs` (lines 235-237)**:
  ```rust
  pub fn working_dir(&self) -> &Path {
      self.worktree_dir.as_deref().unwrap_or(&self.project_dir)
  }
  ```
- **`src/session.rs` (lines 218-220)**:
  ```rust
  pub fn has_folder(&self) -> bool {
      !self.project_dir.as_os_str().is_empty()
  }
  ```
- **`src/worktree.rs` (lines 29-38)**:
  Isolated worktrees are placed under `<repo_dir>/.viper/worktrees/<session_id>` with branch `viper/session-{session_id}`.
- When `session.worktree_dir` is active, `session.working_dir()` returns the worktree folder; otherwise it returns `session.project_dir`.

### 1.4 Process Termination and Resource Management
- **`src/terminal.rs` (lines 56-126)**:
  ```rust
  #[cfg(windows)]
  struct TerminalJob(Option<windows::Win32::Foundation::HANDLE>);
  ```
  Uses `CreateJobObjectW` and `SetInformationJobObject` with `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` setting `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
  Assigns `child.process_id()` via `OpenProcess` and `AssignProcessToJobObject`.
  On `Drop`, executes `TerminateJobObject(job, 1)` and `CloseHandle(job)`.
  Also calls `child.kill()`.
- **`src/terminal.rs` (lines 128-150)**:
  On Unix, stores `pid` and kills `-pgid` with `SIGTERM` (15) and `SIGKILL` (9).
- **`src/app.rs` (lines 323-344)**:
  `delete_session(id)` removes `session` and `tools.remove(&id)`, cleaning up associated terminals and processes.

---

## 2. Logic Chain

### Step 2.1: Executable Resolution Strategy
1. From Observation 1.1, `Detected::get(provider)` already handles checking user overrides in `settings.custom_executables` and falling back to `find_executable()`.
2. On Windows, Anthropic Claude Code can be installed as either a native binary `claude.exe` (via direct installer into `%USERPROFILE%\.local\bin\claude.exe`) or a Node/npm batch script `claude.cmd` (in `%APPDATA%\npm\claude.cmd`).
3. If `CommandBuilder` in `portable-pty` spawns a `.cmd` directly via `CreateProcessW`, Windows may return error 193 (%1 is not a valid Win32 application) if not run through `cmd.exe /c`. Therefore, when the executable extension is `.cmd` or `.bat` on Windows, the command builder must wrap invocation or invoke `cmd.exe /c "<path>"`.
4. Codex and Antigravity (`codex.exe`, `agy.exe`) are native PE executables on Windows and ELF/Mach-O binaries on Unix, spawning directly without shell wrappers.

### Step 2.2: Stream Parsing vs Interactive PTY Execution
1. In the current architecture (`src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`), Viper runs the CLIs in headless/print modes (`-p`, `--output-format stream-json`, `codex exec --json`), intercepts stdout line-by-line, parses JSON into `AgentEvent` variants, and renders a custom chat bubble GUI.
2. In the target architecture (R1-R3 of `ORIGINAL_REQUEST.md`), the middle panel drops `chat::composer` and `chat::conversation` and directly embeds `terminal::Terminal`.
3. Consequently, all headless flags (`-p`, `--output-format stream-json`, `--include-partial-messages`, `--permission-prompts none`, `codex exec`, `codex -`) must be omitted.
4. The provider CLIs run in their default interactive TUI mode:
   - All ANSI escape codes, alternate screen buffers (`\x1b[?1049h`), 24-bit RGB colors, bold/dim styling, and cursor positioning are rendered by `terminal.rs`'s existing `vt100::Parser` and `egui::Painter`.
   - Native interactive permission and approval prompts (e.g. Claude Code's "Allow tool run?", Codex's workspace edit confirmations, Antigravity's interactive prompts) are directly answered by the user via terminal keystrokes (Enter, 'y', 'n', arrow keys).
   - Viper's chat stream parsers are not involved in interactive PTY execution; input and output flow directly between `portable_pty::MasterPty` and the user.

### Step 2.3: Working Directory and Worktree Awareness
1. From Observation 1.3, `session.working_dir()` encapsulates worktree isolation.
2. The PTY process must have its working directory set to `session.working_dir()`.
3. In addition:
   - For `agy`: Antigravity requires `--add-dir <path>` to include the directory in its workspace context. Without this flag, `agy` does not treat the launch folder as the project workspace.
   - For `codex`: Can pass `-C <path>` to explicitly lock the working root.
4. If a session has no folder configured yet (`!session.has_folder()`), the middle panel must not launch an unrooted CLI in an arbitrary directory. Instead, it should display an empty state prompting the user to select a folder (triggering `change_folder()`). Once selected, the terminal is instantiated.

### Step 2.4: Process Group Lifecycle and Clean Teardown
1. When CLIs run interactively in a PTY, they spawn child processes (e.g. bash commands, compilers, test runners, git processes).
2. On Windows, terminating only the parent process leaves grandchild processes orphaned.
3. Observation 1.4 shows `terminal.rs`'s `TerminalJob` already binds `child.process_id()` to a Windows Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Because child processes spawned inside a job object automatically inherit that job object, dropping `TerminalJob` (or exiting Viper) terminates the entire process tree via `TerminateJobObject(job, 1)`.
4. On Unix, `TerminalJob` uses process group killing (`kill(-pgid, 15)` and `kill(-pgid, 9)`).
5. For per-session management:
   - Switching sessions in the sidebar must NOT drop the terminal; it merely toggles which terminal is drawn and holds keyboard focus.
   - Deleting a session (`delete_session(id)`) must drop that session's `Terminal`, triggering `TerminalJob` cleanup.
   - Closing the Viper window drops all `Terminal` instances, cleanly closing ConPTY handles and killing all CLI trees without leaving orphan background processes.

### Step 2.5: Saved State & Cross-Restart Recovery
1. `Terminal` and its underlying OS handles (PTY slave/master, ConPTY pipes, `vt100::Parser`) are non-serializable runtime objects.
2. In `session.rs` and `app.rs`, `SavedState` persists RON metadata (`provider`, `project_dir`, `worktree_dir`, `agent_session_id`, `permission_mode`, `chosen_model`, `effort`).
3. Any runtime terminal state stored in `Session` must be marked `#[serde(skip)]`, or managed in `ViperApp` in a `BTreeMap<u64, Terminal>` (identical to how `tools: BTreeMap<u64, Tools>` is managed).
4. Across application restarts, when a session is reopened, Viper reconstructs a new interactive PTY using the saved session parameters:
   - `cwd`: `session.working_dir()`
   - `resume`: `session.agent_session_id` (if available, e.g. `claude --resume <id>`, `codex resume <id>`, `agy --conversation <id>`)
   - `model` and `effort`: from session configuration.

---

## 3. Caveats

1. **Windows `.cmd` Script Wrapper**:
   On Windows systems where Claude Code is installed via npm (`%APPDATA%\npm\claude.cmd`), invoking `claude.cmd` directly in `portable-pty` without `cmd.exe /c` may fail with `ERROR_BAD_EXE_FORMAT` (193). The command builder should check if the path ends with `.cmd` or `.bat` and format `CommandBuilder::new("cmd.exe")` with `args(["/c", path])`.
2. **Initial Prompt Routing**:
   In headless mode, prompts were sent via piped stdin. In interactive PTY mode, the CLI launches into its interactive REPL/TUI. If a user creates a session with an initial prompt, Claude and Codex accept a trailing `[PROMPT]` argument (or `agy -i <prompt>`), but standard interactive use typically opens the TUI and lets the user type directly into the terminal.
3. **Session Switching Focus**:
   When switching between sessions in the sidebar, `terminal.ui(ui, take_keyboard)` must receive `take_keyboard: true` for the newly selected session, ensuring `response.request_focus()` is called on the next frame so the user can immediately resume typing without clicking the terminal canvas.
4. **Terminal Window Sizing / ConPTY Resizing**:
   `terminal.rs` lines 257-263 already dynamically compute rows/cols based on font glyph metrics and `ui.available_size()`:
   ```rust
   let rows = ((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
   let cols = ((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
   if (rows, cols) != self.size {
       self.size = (rows, cols);
       let _ = self.master.resize(pty_size(self.size));
       self.parser().screen_mut().set_size(rows, cols);
   }
   ```
   This mechanism ensures dynamic resizing when sidebars are collapsed or the main window is resized.

---

## 4. Conclusion & Recommendations

### 4.1 Recommended Command Builders for Interactive PTY Spawning

We recommend introducing a unified command builder function in `src/agent.rs` (or `src/terminal.rs`):

```rust
use portable_pty::CommandBuilder;

pub fn build_interactive_command(
    provider: Provider,
    exe: &Path,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> CommandBuilder {
    #[cfg(windows)]
    let is_batch = exe.extension().map_or(false, |ext| {
        ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")
    });
    #[cfg(not(windows))]
    let is_batch = false;

    let mut cmd = if is_batch {
        let mut c = CommandBuilder::new("cmd.exe");
        c.args(["/c", &exe.display().to_string()]);
        c
    } else {
        CommandBuilder::new(exe)
    };

    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    match provider {
        Provider::Claude => {
            if let Some(model) = model {
                cmd.args(["--model", model]);
            }
            if let Some(effort) = effort {
                cmd.args(["--effort", effort]);
            }
            if let Some(id) = resume_id {
                cmd.args(["--resume", id]);
            }
            match permission_mode {
                PermissionMode::Full => {
                    cmd.arg("--dangerously-skip-permissions");
                }
                PermissionMode::Plan => {
                    cmd.args(["--permission-mode", "plan"]);
                }
                PermissionMode::AcceptEdits => {
                    cmd.args(["--permission-mode", "acceptEdits"]);
                }
                PermissionMode::ReadOnly => {
                    cmd.args(["--permission-mode", "default"]);
                }
            }
        }
        Provider::Codex => {
            if let Some(id) = resume_id {
                cmd.args(["resume", id]);
            }
            cmd.args(["-C", &cwd.display().to_string()]);
            if let Some(model) = model {
                cmd.args(["-m", model]);
            }
            if let Some(effort) = effort {
                cmd.args(["-c", &format!("model_reasoning_effort={effort}")]);
            }
            match permission_mode {
                PermissionMode::Full => {
                    cmd.arg("--dangerously-bypass-approvals-and-sandbox");
                }
                PermissionMode::ReadOnly | PermissionMode::Plan => {
                    cmd.args(["-s", "read-only"]);
                }
                PermissionMode::AcceptEdits => {
                    cmd.args(["-s", "workspace-write"]);
                }
            }
        }
        Provider::Antigravity => {
            cmd.args(["--add-dir", &cwd.display().to_string()]);
            if let Some(id) = resume_id {
                cmd.args(["--conversation", id]);
            }
            if let Some(model) = model {
                cmd.args(["--model", model]);
                let named_level = ["-low", "-medium", "-high"].iter().any(|lvl| model.ends_with(lvl));
                if let Some(effort) = effort && !named_level {
                    cmd.args(["--effort", effort]);
                }
            } else if let Some(effort) = effort {
                cmd.args(["--effort", effort]);
            }
            match permission_mode {
                PermissionMode::Full => {
                    cmd.arg("--dangerously-skip-permissions");
                }
                PermissionMode::Plan => {
                    cmd.args(["--mode", "plan"]);
                }
                PermissionMode::AcceptEdits => {
                    cmd.args(["--mode", "accept-edits"]);
                }
                PermissionMode::ReadOnly => {}
            }
        }
    }

    cmd
}
```

### 4.2 Terminal Generalization in `src/terminal.rs`
Generalize `Terminal::start` to accept a configured `CommandBuilder`:
```rust
impl Terminal {
    pub fn start_command(cmd: CommandBuilder, ctx: egui::Context) -> Result<Self, String> {
        let size = (24, 80);
        let pair = portable_pty::native_pty_system()
            .openpty(pty_size(size))
            .map_err(|err| format!("Couldn't open a terminal: {err}"))?;

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|err| format!("Couldn't start process: {err}"))?;
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

    pub fn start(cwd: &Path, shell: &Path, ctx: egui::Context) -> Result<Self, String> {
        let mut cmd = CommandBuilder::new(shell);
        configure(&mut cmd, shell);
        cmd.cwd(cwd);
        Self::start_command(cmd, ctx)
    }
}
```

### 4.3 App State & Middle View Wiring
1. **Per-Session Storage**:
   In `ViperApp`, maintain:
   ```rust
   session_terminals: std::collections::BTreeMap<u64, Result<Terminal, String>>,
   ```
   (mirrors `tools: std::collections::BTreeMap<u64, Tools>`).
2. **Lifecycle Hooks**:
   - **Session Select / Render**:
     When drawing the central panel for `active_session`:
     - If `!session.has_folder()`, render empty folder prompt with "Select Project Folder" action.
     - If `session.has_folder()`, look up `session_terminals.entry(session.id)`. If absent, build the command with `build_interactive_command` and call `Terminal::start_command(cmd, ctx)`.
     - Render `terminal.ui(ui, take_keyboard)`.
   - **Session Delete**:
     In `delete_session(id)`:
     ```rust
     self.session_terminals.remove(&id); // Automatically drops Terminal and TerminalJob!
     ```
   - **Session Restart**:
     If `terminal.ui()` returns `restart = true` (or user clicks restart button), remove the entry from `session_terminals` so it re-spawns on the next frame.

---

## 5. Verification Method

### 5.1 Static Analysis & Compiler Verification
Execute in powershell at `c:\Users\ditob\Documents\viper`:
```powershell
cargo check
cargo test
cargo clippy --all-targets
```
Expected: 0 errors, 0 clippy warnings, all unit tests pass without newly ignored tests.

### 5.2 Unit Test Plan for Implementation Phase
1. `build_interactive_command` test matrix:
   - For `Provider::Claude`: verify presence of `--model`, `--effort`, `--resume`, and `--permission-mode` flags; verify absence of `-p` and `--output-format`.
   - For `Provider::Codex`: verify `codex` fresh start vs `codex resume <id>`; verify `-C <cwd>` and `-s <mode>`.
   - For `Provider::Antigravity`: verify `--add-dir <cwd>`, `--conversation <id>`, and `--dangerously-skip-permissions`.
2. Worktree directory integration test:
   - Verify that when `session.worktree_dir` is set, `cmd.get_cwd()` matches the worktree path.
3. Windows batch wrapper test:
   - Verify that a path ending in `.cmd` is wrapped in `cmd.exe /c`.
4. Process termination test:
   - Similar to `stopping_a_turn_stops_programs_the_agent_started` in `agent.rs:797`, verify that dropping `Terminal` terminates all child processes in the Job Object.

### 5.3 Invalidation Conditions
- An update to Codex CLI that requires an explicit subcommand for interactive mode other than `codex` / `codex resume`.
- A breaking change in `portable-pty`'s ConPTY backend on Windows.
- Any modification that touches `SavedState` without maintaining RON backward compatibility (`#[serde(default)]` and `#[serde(alias)]`).
