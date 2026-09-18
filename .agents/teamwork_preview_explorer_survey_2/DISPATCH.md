# Explorer 2 Dispatch: Worktree Architecture

Investigate the codebase for Requirement R2: Git Worktree Session Isolation Foundation.
Read `C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md` and `C:\Users\ditob\Documents\viper\AGENTS.md`.
Examine `src/session.rs`, `src/app.rs`, `src/sidebar.rs`, `src/git_diff.rs`, `src/changes.rs`.
Map how sessions associate with workspaces/directories, how git diffs are obtained and watched, how worktrees can be created (under `.viper/worktrees/<session-id>`), path resolution for session working dirs, cleanup utilities, and how the Changes panel can be made worktree/branch aware.
Produce an architecture & implementation plan in `handoff.md` in your working directory.

## 2026-09-18T20:53:13Z
You are Explorer 2 investigating Requirement R2 (Git Worktree Session Isolation Foundation).
Your working directory is: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_2
Read your instructions in: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_2\DISPATCH.md
Read the user request in: C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md
Read repository rules in: C:\Users\ditob\Documents\viper\AGENTS.md

Explore the codebase (using search and view tools) to analyze:
1. How sessions currently track workspace paths in `src/session.rs`, `src/app.rs`, and `src/sidebar.rs`.
2. How `src/git_diff.rs` and `src/changes.rs` inspect git status and diffs.
3. How to build git worktree management: creation, path resolution under `.viper/worktrees/<session-id>`, branch creation, and cleanup.
4. Integrating worktree awareness into session creation and the Changes panel so branch/worktree diffs can be viewed.
5. Unit testing strategy using temporary git repositories (without external dependencies or mutating the user's repo).

Write your findings and proposed technical design to `C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_2\handoff.md`.
Then send a brief message with your key findings and handoff path to the orchestrator.

