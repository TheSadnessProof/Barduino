## 2026-09-19T02:17:52Z

You are worker_m3_orch2.
Your working directory is: c:\Users\ditob\Documents\viper\.agents\worker_m3_orch2

You MUST read the authoritative user request at:
c:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md (specifically section `## 2026-09-19T00:31:41Z`).
Also read:
c:\Users\ditob\Documents\viper\AGENTS.md
c:\Users\ditob\Documents\viper\.agents\orchestrator_2\PROJECT.md

EXPLORER HANDOFF REPORTS (Follow these closely):
- `c:\Users\ditob\Documents\viper\.agents\m3_explorer_1\handoff.md`
- `c:\Users\ditob\Documents\viper\.agents\m3_explorer_2\handoff.md`

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

EXCLUSIVE FILE OWNERSHIP:
You own and may edit:
- `src/app.rs`
Do NOT modify other source files in this milestone.

OBJECTIVE - IMPLEMENT MILESTONE 3:
Fulfill Milestone 3 (Per-Session Lifecycle, Switching & Saved State) and implement comprehensive unit tests in `src/app.rs::tests`:

1. `multi_session_switching_preserves_terminals_in_map_and_transfers_focus`:
   Create 2 sessions with ready executables (e.g. cmd.exe on Windows, /bin/sh on Unix), render session 1, verify spawned and focus consumed. Switch to session 2 via `app.handle_sidebar(SidebarAction::Select(2))`, render, verify session 2 spawned and focus consumed while session 1 remains alive in `provider_terminals`. Switch back to session 1, verify session 1 terminal was preserved (not re-spawned) and focus flag was transferred.

2. `deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal`:
   Create sessions [1, 2, 3], spawn terminals for them, delete active session 2 via `app.handle_sidebar(SidebarAction::Delete(2))` or `delete_session(2)`. Verify session 2 is removed from `provider_terminals` (dropping it and killing its process tree), session 1 becomes active with `focus_composer == true`, and session 3 terminal is preserved.

3. `folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd`:
   Verify that changing `project_dir` on a session and clearing `provider_terminals[&id]` results in the subsequent frame re-resolving state with the new `cwd` and re-spawning a fresh terminal in that new directory.

4. `failed_terminal_spawn_retry_action_clears_error_and_allows_respawn`:
   Simulate a spawn error in `provider_terminals.insert(id, Err("simulated failure".into()))`. Render `terminal_area`, verify "Try again" action clears the error, and subsequent render with a valid command spawns cleanly.

5. `saved_state_ron_serialization_omits_provider_terminals`:
   Assert that serializing `app.state` to RON (`ron::to_string(&app.state)`) produces valid RON that contains zero references to `"provider_terminals"`, and that deserializing it back roundtrips cleanly without error.

6. `comprehensive_legacy_ron_state_loads_and_carries_defaults`:
   Construct a legacy RON state string representing an older Viper build (containing `"Gemini"`, `"claude_session_id"`, missing modern fields). Deserialize it via `ron::from_str::<SavedState>`, verify it loads cleanly, maps `Gemini` to `Provider::Antigravity`, and carries over settings.

7. `restart_restores_session_and_spawns_cli_lazily_in_session_folder`:
   Construct a `ViperApp` simulating restart from a loaded `SavedState`. Verify `provider_terminals` starts empty, and the first render of `terminal_area` lazily spawns the CLI in the session's folder.

8. `middle_provider_terminal_and_tools_panel_coexist_without_interference`:
   In a test with both a ready middle session terminal and secondary tools panel terminals/diffs, render both `app.terminal_area(ui)` and `app.right_panel(ui, frame)` (or headless tool ui). Verify that both render without panics or widget ID collisions, and switching tool tabs leaves the middle session's `provider_terminals` entry completely intact.

RULES & INVARIANTS:
- Sentence-named tests per AGENTS.md §5.4.
- Do NOT run `cargo fmt`.
- Verify with `cargo check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`. Clippy MUST be at zero warnings!
- Do NOT run ignored tests wholesale.
- Deliver your handoff report to `c:\Users\ditob\Documents\viper\.agents\worker_m3_orch2\handoff.md` and send a message to parent when finished.
