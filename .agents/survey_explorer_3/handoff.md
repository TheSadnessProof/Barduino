# Technical Survey Handoff Report: Interactive Terminal Middle Panel Replacement

**Author**: survey_explorer_3  
**Date**: 2026-09-19T00:36:30Z  
**Objective**: Technical survey of ViperApp state management, session lifecycle, UI routing, and backward compatibility for replacing the middle chat transcript/composer with an interactive terminal hosting AI provider CLIs.

---

## 1. Observation

### 1.1 Architecture & State Ownership (`src/app.rs`)
- **`SavedState` (lines 30–50)**:
  ```rust
  #[derive(Serialize, Deserialize)]
  #[serde(default)]
  struct SavedState {
      sessions: Vec<Session>,
      active_session: u64,
      next_session_id: u64,
      show_sessions: bool,
      show_tools: bool,
      settings: Settings,
      usage: UsageLog,
      plan: std::collections::BTreeMap<Provider, PlanUsage>,
      browser: BrowserState,
      #[serde(skip_serializing)]
      browser_address: String,
      #[serde(default = "saved_before_panel_defaults")]
      apply_panel_defaults: bool,
  }
  ```
- **`ViperApp` Runtime State (lines 125–157)**:
  `ViperApp` owns `state: SavedState` (persisted), and non-persisted runtime fields:
  - `detected: Detected`: CLI executables found on disk.
  - `view: View`: Current middle view (`View::Chat` or `View::Settings`).
  - `tools: std::collections::BTreeMap<u64, Tools>`: Per-session right-hand auxiliary panel (secondary terminals, changes, browser).
  - `sidebar: Sidebar`: Left-hand panel state (filter, rename, delete confirmation).
  - `browser: Browser`: System WebView instance shared among tabs.
  - `events_tx / events_rx`: MPSC channel for background headless agent turns.
- **Top-Level Layout Rendering (lines 814–823)**:
  ```rust
  fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
      self.browser.release_focus_on_click(ui.ctx());
      self.notice_banner(ui);
      self.left_panel(ui);
      self.right_panel(ui, frame);
      egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
          View::Chat => self.chat_area(ui),
          View::Settings => self.settings_area(ui),
      });
  }
  ```
- **Current Middle Panel Rendering (`chat_area`, lines 531–605)**:
  `chat_area` splits the central space into:
  1. `egui::Panel::bottom(egui::Id::new("composer_panel"))...show(ui, |ui| chat::composer(ui, session, models, installed))` returning `ComposerAction`.
  2. `egui::CentralPanel::default().show(ui, |ui| chat::conversation(ui, session, settings, markdown))` returning `ConversationAction`.
  3. Matches actions: `ComposerAction::Send` calls `self.send(ui.ctx())`, `Stop` calls `session.stop()`, `ConversationAction::SelectProvider`, `ChangeFolder`, `Approve`, `Deny`, `Preview`.

### 1.2 Session Struct & Persistence (`src/session.rs`)
- **`Session` Definition (lines 87–151)**:
  ```rust
  #[derive(Serialize, Deserialize)]
  #[serde(default)]
  pub struct Session {
      pub id: u64,
      pub title: String,
      pub project_dir: PathBuf,
      pub worktree_dir: Option<PathBuf>,
      pub worktree_branch: Option<String>,
      pub worktree_base: Option<String>,
      pub provider: Provider,
      pub permission_mode: PermissionMode,
      pub chosen_model: Option<String>,
      pub effort: Option<String>,
      pub last_usage: Option<Usage>,
      #[serde(alias = "claude_session_id")]
      pub agent_session_id: Option<String>,
      pub model: Option<String>,
      pub entries: Vec<Entry>,
      pub input: String,
      pub elements: Vec<PickedElement>,
      pub browser: BrowserState,

      #[serde(skip)]
      pub streaming: String,
      #[serde(skip)]
      pub focus_composer: bool,
      #[serde(skip)]
      turn: Option<RunningTurn>,
      #[serde(skip)]
      stop_requested: bool,
      #[serde(skip)]
      error_shown: bool,
      #[serde(skip)]
      pub slash_selected: usize,
      #[serde(skip)]
      pub slash_query: String,
      #[serde(skip)]
      pub slash_dismissed: bool,
  }
  ```
- **Working Directory (`Session::working_dir`, lines 235–237)**:
  ```rust
  pub fn working_dir(&self) -> &Path {
      self.worktree_dir.as_deref().unwrap_or(&self.project_dir)
  }
  ```
- **Session State & Detail in Sidebar (`src/sidebar.rs:28–69`)**:
  `session_state(session: &Session) -> SessionState` returns `Running`, `WaitingForApproval`, `Failed`, or `Idle`.
  `session_detail(session: &Session) -> String` returns e.g. `"Claude · 3 turns"` or `"Claude · opus"`.

### 1.3 Embedded Terminal Implementation (`src/terminal.rs`)
- **`Terminal` Struct (lines 162–174)**:
  ```rust
  pub struct Terminal {
      parser: Arc<Mutex<Parser>>,
      writer: Writer,
      master: Box<dyn MasterPty + Send>,
      child: Box<dyn Child + Send + Sync>,
      job: TerminalJob,
      size: (u16, u16),
      exited: Arc<AtomicBool>,
      scroll_remainder: f32,
      selection: Option<Selection>,
  }
  ```
- **Process Management & Cleanup (`TerminalJob`, lines 58–126, 420–425)**:
  - Windows: `TerminalJob` encapsulates a Windows Job Object handle configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
  - Unix: `TerminalJob` uses `kill(-pgid, 15)` and `kill(-pgid, 9)`.
  - In `Drop for Terminal`:
    ```rust
    impl Drop for Terminal {
        fn drop(&mut self) {
            self.job.kill();
            let _ = self.child.kill();
        }
    }
    ```
- **Dynamic Resize & Input Handling (lines 256–278)**:
  ```rust
  let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
  let rows = ((rect.height() - 2.0 * PADDING) / row_height).floor().max(2.0) as u16;
  let cols = ((rect.width() - 2.0 * PADDING) / char_width).floor().max(10.0) as u16;
  if (rows, cols) != self.size {
      self.size = (rows, cols);
      let _ = self.master.resize(pty_size(self.size));
      self.parser().screen_mut().set_size(rows, cols);
  }
  if response.clicked() || take_keyboard {
      response.request_focus();
  }
  if focused {
      ui.memory_mut(|m| {
          m.set_focus_lock_filter(
              response.id,
              egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
          );
      });
      self.handle_input(ui);
  }
  ```
- **ANSI Styling & Rendering (lines 585–680)**:
  `row_layout` converts `vt100::Screen` cells into `egui::text::LayoutJob`, supporting 16 ANSI colors, 256-color palette, 24-bit RGB, dim, italic, underline, and inverse video.

### 1.4 Provider Executable Detection (`src/agent.rs`, `claude.rs`, `codex.rs`, `antigravity.rs`)
- `Provider::Claude`: `claude::find_executable()` checks PATH, `~/.local/bin`, `~/.claude/local`, `%APPDATA%\npm\claude.cmd`.
- `Provider::Codex`: `codex::find_executable()` checks PATH, `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin`, `~/.codex/bin`.
- `Provider::Antigravity`: `antigravity::find_executable()` checks PATH, `%LOCALAPPDATA%\agy\bin\agy.exe`.
- In interactive mode, CLIs do NOT want `-p` or `--output-format stream-json`; they run interactively attached to a PTY.
- On Windows, `claude.cmd` is a batch script generated by npm. Spawning `.cmd` files in Windows ConPTY via `portable_pty::CommandBuilder` requires invoking `cmd.exe /c` (or wrapping via `agent::hidden_command` logic), whereas `.exe` files execute directly.

---

## 2. Logic Chain

### 2.1 Preserving Sidebars while Replacing Middle Panel
1. `ViperApp::ui` (in `src/app.rs:814–823`) executes panel rendering in standard egui order:
   - `self.left_panel(ui)` (Sidebar, resizable or collapsed rail)
   - `self.right_panel(ui, frame)` (Tools panel, resizable or collapsed rail)
   - `egui::CentralPanel::default()...` (Middle panel taking the remaining available space)
2. In the current implementation, `CentralPanel` calls `self.chat_area(ui)` when `self.view == View::Chat`.
3. If `chat_area(ui)` is replaced with a dedicated `terminal_area(ui)`:
   - Neither `left_panel` nor `right_panel` is touched or structurally modified.
   - The left sidebar retains project grouping, session creation, renaming, deleting, folding, and settings access (`SidebarAction`).
   - The right tools panel (`tools.rs`) retains auxiliary shell terminals, git branch/uncommitted changes diffs, and the live preview browser tabs (`ToolsAction`).
   - `ui.available_size()` passed to `Terminal::ui` dynamically tracks the middle panel dimensions between the left and right panels. Resizing either sidebar or the whole window automatically adjusts the rows/cols of the terminal and resizes the underlying PTY via `self.master.resize(pty_size(self.size))`.

### 2.2 Per-Session Terminal State: Runtime vs. Saved State
1. A PTY handle (`MasterPty`, `Child`, `Writer`, `vt100::Parser`) is operating-system-level ephemeral state. It cannot be serialized to RON or persisted across application restarts.
2. AGENTS.md Rule 3.5 mandates:
   - "Do not break saved state. `SavedState` is persisted as RON by eframe. Users have existing sessions on disk."
   - "Adding a field: give it a `#[serde(default)]` or a `Default` so old saves load."
3. Two architectural placements for per-session terminal state:
   - **Approach A (Field on `Session` with `#[serde(skip)]`)**:
     Add `#[serde(skip)] pub terminal: Option<Result<Terminal, String>>` and `#[serde(skip)] pub focus_terminal: bool` to `Session`.
     *Reasoning*: When a `Session` is removed from `self.state.sessions`, its `Terminal` is dropped in the same destructor, immediately killing the child process group without needing manual map synchronization. Serde completely ignores skipped fields during RON serialization and assigns `None` on deserialization.
   - **Approach B (Map on `ViperApp`)**:
     Add `terminals: std::collections::BTreeMap<u64, Result<Terminal, String>>` to `ViperApp`.
     *Reasoning*: Matches `app.tools: BTreeMap<u64, Tools>`. Keeps `Session` strictly as a serializable data struct.
   - **Synthesis**: Approach B is cleanest for architectural separation (separating UI widgets and processes from pure session data), while Approach A provides automatic RAII cleanup on session drop. If Approach B is adopted, `delete_session` and `change_folder` must explicitly call `self.terminals.remove(&id)`, exactly as is already done with `self.tools.remove(&id)`.

### 2.3 Handling Restarts Across Launches
1. When Viper closes:
   - `SavedState` is serialized to `app.ron` (and backed up to `app.ron.bak`).
   - All `Terminal` structs in memory are dropped, triggering `job.kill()` and `child.kill()`. No orphan processes or handles remain.
2. When Viper reopens:
   - `SavedState` is deserialized from `app.ron`.
   - `sessions` contains the saved metadata: `id`, `title`, `project_dir`, `worktree_dir`, `provider`, etc.
   - Terminal state starts uninitialized (`None`).
   - When the active session is rendered in `terminal_area`, the terminal is spawned lazily in `session.working_dir()` with the session's chosen `provider`.
   - If a session does not yet have a project folder (`!session.has_folder()`), an empty-state screen is displayed with a "Choose project folder" button and provider selector. The CLI spawns the moment the folder is selected.

### 2.4 Multi-Session Switching and Focus Handover
1. When the user selects session `B` in the sidebar:
   - `SidebarAction::Select(id)` is caught by `handle_sidebar(action, ctx)`.
   - `self.state.active_session = id;`
   - `take_keyboard` is flagged for the active session (either via a flag on the session/map or `self.focus_active_terminal = true`).
2. On the next frame in `ui()`:
   - `terminal_area` retrieves session `B`'s terminal from the map (or `session.terminal`).
   - If session `B`'s terminal was already running, its active `vt100` screen buffer is immediately painted to the screen.
   - Session `A`'s background reader thread remains running in the background, updating its own `vt100::Parser`.
   - `terminal.ui(ui, take_keyboard = true)` is called for session `B`.
   - `response.request_focus()` is executed, and `set_focus_lock_filter` captures Tab, Arrow keys, and Escape for session `B`.
   - Focus is immediately active in session `B` without requiring a mouse click.

### 2.5 Session Deletion and Resource Cleanup
1. When the user confirms deletion of session `id` in the sidebar:
   - `SidebarAction::Delete(id)` calls `self.delete_session(id)`.
2. In `delete_session(id)`:
   - Removes the session from `self.state.sessions`.
   - Removes auxiliary tools: `self.tools.remove(&id)`.
   - Removes provider terminal: `self.terminals.remove(&id)` (or dropped with `Session`).
   - Dropping `Terminal` invokes:
     - `self.job.kill()`: On Windows, `TerminateJobObject` terminates the entire process tree (all subprocesses spawned by the CLI). On Unix, `kill(-pgid, 15)` and `kill(-pgid, 9)` terminate the process group.
     - `self.child.kill()` terminates the direct PTY child process.
   - Cleans up worktrees on disk if configured (`worktree::remove_worktree` and `worktree::delete_branch`).
   - If the deleted session was the active one, `self.state.active_session = next.id` selects the adjacent session, and its terminal immediately receives keyboard focus.

### 2.6 CLI Spawning Mechanism in `terminal.rs`
1. `Terminal::start` currently takes `(cwd: &Path, shell: &Path, ctx: egui::Context)` and configures it as a shell (running pwsh/cmd/bash).
2. For provider CLIs, `terminal.rs` needs a general constructor, e.g.:
   ```rust
   pub fn start_process(
       cwd: &Path,
       exe: &Path,
       args: &[String],
       ctx: egui::Context,
   ) -> Result<Self, String>
   ```
3. On Windows, if `exe` is a `.cmd` or `.bat` file (e.g. npm-installed `claude.cmd`):
   - ConPTY / `portable_pty::CommandBuilder::new("claude.cmd")` can fail with OS error 193 if executed directly without a shell interpreter.
   - Handling: if `cfg!(windows)` and `exe.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))`:
     `let mut cmd = CommandBuilder::new("cmd.exe"); cmd.arg("/c"); cmd.arg(exe);`
   - Otherwise, `CommandBuilder::new(exe)`.
4. Environment variables:
   - Always set `TERM=xterm-256color` and `COLORTERM=truecolor` on the command builder so that interactive TUI frameworks (e.g. Ink, Bubbletea, Curses) detect full terminal capabilities, cursor positioning, and 24-bit RGB support.
5. Interactive TUI Keybindings:
   - Arrow keys, Enter (`\r`), Backspace (`0x7f`), Tab (`\t`), Shift+Tab (`\x1b[Z`), Escape (`\x1b`), Ctrl+C (`0x03`), and Ctrl+[A-Z] are already fully mapped in `key_sequence` in `terminal.rs`.
   - Bracketed paste (`\x1b[200~...\x1b[201~`) is already supported in `handle_input`.

---

## 3. Caveats

1. **CLI Version Variations & Interactive Prompts**:
   - Claude Code, Codex, and Antigravity each have their own authentication and interactive setup flows (e.g. browser OAuth, terms agreement). In an interactive terminal, these native prompts display directly in the PTY. The user interacts with them via standard terminal keystrokes.
2. **Windows ConPTY Availability**:
   - `portable-pty` on Windows requires Windows 10 build 1809 (ConPTY) or newer. All supported target platforms for Viper satisfy this.
3. **Existing Chat Code Retention**:
   - `src/chat.rs` contains ~2040 lines of markdown parsing, diff rendering, and composer code. The user request specifies replacing the middle chat transcript and composer with the interactive terminal view. `chat.rs` should either be deprecated or superseded by the terminal view in `app.rs`, while ensuring any helper types needed by existing tests remain intact or tests are updated.
4. **Session Working Directory Precondition**:
   - An interactive CLI process cannot spawn without a valid filesystem directory. When a user creates a session via "New Session" without choosing a folder yet (`!session.has_folder()`), terminal spawn must wait until `project_dir` is selected.

---

## 4. Conclusion & Architectural Design

### 4.1 Summary of Architectural Plan
1. **Per-Session Terminal Storage**:
   Store active provider terminals in `ViperApp`:
   ```rust
   pub struct ViperApp {
       ...
       provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>,
       focus_terminal: bool,
   }
   ```
   This guarantees that `SavedState` remains 100% backward-compatible (no change to persistent RON structure) and matches `app.tools: BTreeMap<u64, Tools>`.
2. **Terminal Process Spawning (`terminal.rs`)**:
   Add `Terminal::start_process(cwd: &Path, exe: &Path, args: &[String], ctx: egui::Context) -> Result<Self, String>`.
   - Handles Windows `.cmd` wrappers via `cmd.exe /c` where needed.
   - Injects `TERM=xterm-256color` and `COLORTERM=truecolor`.
   - Configures `TerminalJob` with Windows Job Object kill-on-close and Unix process group killing.
3. **Middle Panel UI Routing (`app.rs`)**:
   Replace `self.chat_area(ui)` in `ViperApp::ui` with `self.terminal_area(ui)`.
   - If `!session.has_folder()`: Renders empty session setup view (provider selection cards + "Choose folder" button).
   - If provider executable is not found: Renders warning with install hint and "Open Settings" button.
   - If ready: Renders `terminal.ui(ui, take_keyboard)`.
   - When window or sidebars are resized, `allocate_exact_size` dynamically calculates rows and columns and resizes the PTY.
4. **Lifecycle & Session Management**:
   - **Selection**: `SidebarAction::Select(id)` updates `active_session` and sets `self.focus_terminal = true`.
   - **Creation**: `new_session` starts a fresh session metadata entry; terminal spawns on first render once folder is set.
   - **Deletion**: `delete_session` removes from `self.provider_terminals`, dropping `Terminal` and killing all child processes.
   - **Restarts**: Terminal handles are skipped in `SavedState`. On app launch, sessions deserialize cleanly with uninitialized terminal state and spawn fresh on demand.

---

## 5. Verification Method

### 5.1 Verification Commands
The implementer can verify the implementation with the following commands:
1. `cargo check` (must pass with 0 errors)
2. `cargo clippy --all-targets` (must pass with 0 warnings)
3. `cargo test` (must pass all 237+ tests without regression)

### 5.2 Specific Test Cases to Add (Sentence-Named per AGENTS.md §5.4)
In `src/app.rs` or `src/session.rs`:
- `fn saved_state_survives_ron_roundtrip_without_persisting_terminal_handles()`:
  Confirms that creating and saving state with sessions does not output any terminal/process fields to RON.
- `fn legacy_saved_state_loads_cleanly_into_terminal_sessions()`:
  Confirms that legacy RON strings (from chat-based Viper) load cleanly without errors or schema mismatches.
- `fn deleting_session_removes_its_provider_terminal_and_cleans_up_resources()`:
  Confirms that calling `app.delete_session(id)` removes the terminal entry from `provider_terminals` and terminates the process.
- `fn switching_active_session_hands_over_terminal_focus()`:
  Confirms that changing `active_session` flags the terminal to request keyboard focus on the next frame.
- `fn provider_terminal_spawns_in_session_working_directory()`:
  Confirms that when starting a provider terminal, the working directory matches `session.working_dir()` (including isolated worktrees when configured).
- `fn session_without_folder_waits_for_folder_before_spawning_terminal()`:
  Confirms that an unconfigured session does not trigger a spawn error until a folder is assigned.

### 5.3 Invalidation Conditions
This survey and design would be invalidated if:
- A new third-party dependency is introduced into `Cargo.toml` without explicit user permission (violating AGENTS.md Rule 3.4).
- `SavedState` schema is modified in a way that breaks deserialization of existing `app.ron` backups (violating AGENTS.md Rule 3.5).
- Interactive keystrokes (Enter, Backspace, Ctrl+C, arrows) are intercepted or suppressed by parent egui focus containers instead of reaching the terminal PTY.
