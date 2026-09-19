# BRIEFING — 2026-09-19T01:50:05Z

## Mission
Implement Milestone 2: Replace middle chat transcript and composer with interactive provider terminal area in `src/app.rs` and suppress dead code warnings in `src/chat.rs`.

## 🔒 My Identity
- Archetype: worker
- Roles: implementer, qa, specialist
- Working directory: c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2
- Original parent: 7dee640f-6289-4c0a-90bd-57213904e03a
- Milestone: Milestone 2 (Viper Interactive Provider Terminal Integration)

## 🔒 Key Constraints
- Exclusive file ownership: `src/app.rs`, `src/chat.rs`. Do NOT modify other source files without approval.
- Do NOT cheat: no hardcoded test results, facade implementations, or circumventing tasks.
- Keep `left_panel` and `right_panel` completely intact.
- Zero clippy warnings (`cargo clippy --all-targets -- -D warnings`).
- Do NOT run `cargo fmt`.
- Do NOT add dependencies to `Cargo.toml`.
- Do NOT run ignored tests wholesale.
- Never drive the global mouse or keyboard, never take full-screen captures.

## Current Parent
- Conversation ID: 7dee640f-6289-4c0a-90bd-57213904e03a
- Updated: not yet

## Task Summary
- **What to build**: Replace `chat_area` in `src/app.rs` with `terminal_area` hosting interactive provider PTY. Add `provider_terminals` map to `ViperApp`, wire cleanup in `delete_session` and `change_folder`. Define `TerminalState` and `resolve_terminal_state` consuming `session.focus_composer`. In `src/chat.rs`, add `#![allow(dead_code)]`. Add comprehensive unit tests in `src/app.rs`.
- **Success criteria**: `cargo check` clean, `cargo test` passes all tests, `cargo clippy --all-targets -- -D warnings` 0 warnings, honest UI verification description.
- **Interface contracts**: PROJECT.md & M2 explorer blueprints.
- **Code layout**: `src/app.rs`, `src/chat.rs`.

## Key Decisions Made
- Follow blueprint from `m2_explorer_1` and test proposals from `m2_explorer_3`.
- `resolve_terminal_state` cleanly decouples logic from rendering for testability.
- Use `ui.scope_builder` with `id_salt(("session_terminal", session_id))` to avoid widget ID collisions.

## Loaded Skills
- **Source**: c:\Users\ditob\Documents\viper\.agents\skills\verifying-a-ui-change\SKILL.md
- **Local copy**: c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2\skills\verifying-a-ui-change\SKILL.md
- **Core methodology**: Move UI logic out of rendering into testable pure data functions; check for unsalted IDs, mutation across frames, background repaints, and unbounded text; describe visual expectations honestly without claiming false visual observation.

## Change Tracker
- **Files modified**:
  - `src/chat.rs`: Added `#![allow(dead_code)]` at top of file so existing chat tests remain functional without warnings.
  - `src/app.rs`: Pruned unused imports, added `provider_terminals` map to `ViperApp`, wired cleanup in `delete_session` and `change_folder`, implemented `TerminalState` and `resolve_terminal_state` consuming `focus_composer`, implemented `terminal_area` replacing `chat_area`, preserved transitional agent hooks, added 10 unit tests in `src/app.rs::tests`.
- **Build status**: `cargo check` PASS (0 errors, 0 warnings), `cargo test` PASS (276 passed, 0 failed, 8 ignored), `cargo clippy --all-targets -- -D warnings` PASS (0 warnings).
- **Pending issues**: None.

## Quality Status
- **Build/test result**: Pass (276 passed; 0 failed; 8 ignored).
- **Lint status**: 0 warnings with `-D warnings`.
- **Tests added/modified**: 10 unit tests added in `src/app.rs::tests` covering terminal state resolution, focus transfer, unconfigured session prompt, missing CLI warning, PTY process spawn on ready session, and session deletion terminal cleanup.

## Artifact Index
- `c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2\DISPATCH.md` — assignment dispatch
- `c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2\BRIEFING.md` — persistent memory
- `c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2\progress.md` — liveness heartbeat
- `c:\Users\ditob\Documents\viper\.agents\worker_m2_orch2\handoff.md` — 5-component handoff report
