## 2026-09-19T01:50:05Z

You are worker_m2_orch2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md
c:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md

EXPLORER BLUEPRINTS (Follow these blueprints closely):
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_1\handoff.md`
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_2\handoff.md`
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_3\handoff.md`
- `c:\Users\ditob\Documents\viper\.agents\m2_explorer_3\test_suite_proposal.rs`

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

EXCLUSIVE FILE OWNERSHIP:
You own and may edit:
- `src/app.rs`
- `src/chat.rs`
Do NOT modify other source files without approval.

OBJECTIVE - IMPLEMENT MILESTONE 2:
1. In `src/chat.rs`:
   - Add `#![allow(dead_code)] // Preserved for conversation tests and transition.` at top of file so existing chat tests remain functional without dead-code compiler warnings.
2. In `src/app.rs`:
   - Prune unused imports (`use crate::chat::{self, ComposerAction};` and `ApprovalDecision`).
   - Add `provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>` to `ViperApp`.
   - Initialize `provider_terminals: BTreeMap::new()` in `ViperApp::new` and `ViperApp::test_app`.
   - In `delete_session` and `change_folder`, call `self.provider_terminals.remove(&id);`.
   - Define `TerminalState` enum and `pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState`. Consumes `session.focus_composer` via `std::mem::take` when transitioning to `Ready`.
   - Implement `fn terminal_area(&mut self, ui: &mut egui::Ui)` replacing `chat_area`:
     - `TerminalState::NeedsFolder`: Centered empty state prompt ("Choose a project folder to start") with button wired to `self.change_folder()`.
     - `TerminalState::MissingExecutable`: Banner with install hint and button wired to `self.view = View::Settings`.
     - `TerminalState::Ready`: Retrieves or starts `Terminal::start_command` in `self.provider_terminals`, salts egui ID with `ui.scope_builder(egui::UiBuilder::new().id_salt(("session_terminal", session_id)), ...)`, executes `terminal.ui(ui, take_keyboard)`, and handles `restart` on process exit by removing the terminal from the map.
   - In `ViperApp::ui`, replace `View::Chat => self.chat_area(ui)` with `View::Chat => self.terminal_area(ui)`. Keep `self.left_panel(ui)` and `self.right_panel(ui, frame)` completely intact.
   - Add unit tests from `m2_explorer_1` and `m2_explorer_3` in `src/app.rs::tests`.

VERIFICATION REQUIREMENTS:
- Run `cargo check` (0 errors).
- Run `cargo test` (all unit tests pass, nothing newly ignored; DO NOT run ignored tests wholesale).
- Run `cargo clippy --all-targets -- -D warnings` (0 warnings).
- Do NOT run `cargo fmt`.
- Do NOT add any dependencies to `Cargo.toml`.

When finished, compile your handoff report in:
`c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2\handoff.md`
and send a message to parent reporting completion.
