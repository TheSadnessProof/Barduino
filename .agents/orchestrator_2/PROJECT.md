# Project: Viper Interactive Provider Terminal Integration

## Architecture
Replace the middle chat transcript and composer in Viper (`chat.rs`) with an embedded interactive PTY terminal widget (`terminal.rs`) that directly hosts the active AI provider's CLI (`claude`, `codex`, or `agy` / `antigravity`).

### Component Structure
1. **Interactive PTY Execution (`src/terminal.rs` & `src/agent.rs`)**:
   - `Terminal::start_command`: Generalizes PTY process creation for arbitrary commands.
   - Windows `.cmd` execution support: Wraps `.cmd` / `.bat` binaries via `cmd.exe /c` to avoid Windows ConPTY error 193.
   - PTY environment configuration: Injects `TERM=xterm-256color` and `COLORTERM=truecolor`.
   - `build_interactive_command`: Constructs interactive CLI commands for `Provider::Claude`, `Provider::Codex`, and `Provider::Antigravity`, supporting working directory (`session.working_dir()`), model selection, reasoning effort, conversation resume, and permission mode flags.
   - Windows Job Objects (`TerminalJob`): Ensures full process tree termination on terminal drop.
2. **Central Terminal View (`src/app.rs`)**:
   - Replaces `chat_area` with `terminal_area` in `CentralPanel`.
   - Handles unconfigured sessions (`!session.has_folder()`) with a clean folder selection prompt.
   - Handles uninstalled CLI executables with guidance to settings.
   - Embeds `terminal.ui(ui, take_keyboard)` with dynamic sizing (`master.resize`) and focus locking (`set_focus_lock_filter`).
   - Retains the left sidebar (projects, sessions, settings) and right tools panel (secondary shells, git diffs, browser live preview).
3. **Session Lifecycle & State Compatibility (`src/session.rs` & `src/app.rs`)**:
   - Ephemeral per-session terminal state in `ViperApp.provider_terminals: BTreeMap<u64, Result<Terminal, String>>`.
   - `SavedState` in `app.rs` preserves 100% RON backward compatibility (no non-serializable PTY handles in saved state).
   - Sidebar session switching immediately transfers view and keyboard focus (`take_keyboard = true`).
   - Session deletion cleanly drops the terminal and terminates the underlying process tree.

## Feature Inventory
| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| F1 | Generalize PTY Process Spawning | Add `Terminal::start_command` (or `start_process`) with `.cmd` batch handling, environment variables, and process job cleanup | M1 | ORIGINAL_REQUEST §R2 |
| F2 | Provider Interactive Command Builders | Implement interactive argument builders for Claude, Codex, and Antigravity with working directory, model, and flags | M1 | ORIGINAL_REQUEST §R2 |
| F3 | Middle Panel Terminal View Replacement | Replace `chat_area` in `app.rs` with `terminal_area`, hosting the active session's embedded PTY | M2 | ORIGINAL_REQUEST §R1 |
| F4 | Dynamic Window and Sidebar Resizing | Propagate `ui.available_size()` changes to PTY columns/rows and vt100 parser | M2 | ORIGINAL_REQUEST §R2 |
| F5 | Interactive Keystroke and ANSI Support | Support Enter, Backspace, Ctrl keys, arrows, Tab, bracketed paste, ANSI TrueColor, and cursor positioning | M2 | ORIGINAL_REQUEST §R2 |
| F6 | Per-Session Terminal Lifecycle & State | Maintain dedicated terminal instances per session, clean process termination on deletion | M3 | ORIGINAL_REQUEST §R3 |
| F7 | Multi-Session Switching & Immediate Focus | Switching sessions switches terminal buffer and immediately transfers keyboard focus without mouse click | M3 | ORIGINAL_REQUEST §R3 |
| F8 | Saved State RON Backward Compatibility | Ensure `SavedState` serializes and deserializes without breaking existing session files | M3 | ORIGINAL_REQUEST §R3, AGENTS.md §3.5 |
| F9 | Sidebar & Auxiliary Tools Panel Retention | Preserve project grouping, session controls, secondary shell terminals, diffs, and live preview browser | M3 | ORIGINAL_REQUEST §R4 |
| F10 | Comprehensive Test Suite & Integrity Forensics | Full test suite verification with sentence-named tests, zero clippy warnings, and clean forensic audit | M4 | ORIGINAL_REQUEST Acceptance Criteria |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|-------------|--------|
| M1 | PTY Provider Spawning & Command Builders | F1, F2: Generalize `src/terminal.rs`, implement `build_interactive_command` in `src/agent.rs` or `src/terminal.rs`, unit tests | none | DONE (266 passed tests, 0 clippy warnings) |
| M2 | Middle Panel UI Terminal Area & Interaction | F3, F4, F5: Implement `terminal_area` in `src/app.rs`, handle unconfigured sessions/missing CLIs, dynamic resize and keystroke focus | M1 | DONE (276 passed tests, 0 clippy warnings) |
| M3 | Per-Session Lifecycle, Switching & Saved State | F6, F7, F8, F9: Wire `provider_terminals` in `ViperApp`, handle session switching, deletion cleanup, and RON backward compatibility tests | M2 | DONE (284 passed tests, 0 clippy warnings) |
| M4 | Final Integration Verification & Integrity Audit | F10: Full test pass, clippy zero warnings, adversarial stress verification, forensic integrity audit | M3 | DONE (284 passed tests, 0 clippy warnings) |

## Interface Contracts
### `src/terminal.rs`
```rust
impl Terminal {
    pub fn start_command(
        cwd: &Path,
        program: &Path,
        args: &[String],
        ctx: egui::Context,
    ) -> Result<Self, String>;
}
```

### `src/agent.rs`
```rust
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

### `src/app.rs`
```rust
impl ViperApp {
    fn terminal_area(&mut self, ui: &mut egui::Ui);
}
```

## Code Layout
- `src/terminal.rs`: General PTY process spawning (`start_command`), Windows batch wrapping, PTY resize, and vt100 terminal rendering.
- `src/agent.rs`: Interactive command formatting for providers, CLI executable resolution.
- `src/app.rs`: Central panel UI routing (`terminal_area`), session terminal map management, session deletion teardown, focus routing.
- `src/session.rs`: Session data structures and working directory resolution.
