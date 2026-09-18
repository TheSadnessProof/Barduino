# Worker M2 Dispatch: Git Worktree Session Isolation Foundation

## Objective
Implement Milestone 2: Git Worktree Session Isolation Foundation in Viper.

## Context & Inputs
- User Request: `C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md` (under `## 2026-09-18T20:52:15Z`)
- Repository Rules: `C:\Users\ditob\Documents\viper\AGENTS.md`
- Project Architecture: `C:\Users\ditob\Documents\viper\.agents\orchestrator_1\PROJECT.md`
- Explorer Specification: `C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_2\handoff.md`

## Files You Own Exclusively
- `src/worktree.rs` (New module - remember to register `mod worktree;` in `src/main.rs` or `src/app.rs`)
- `src/session.rs`
- `src/git_diff.rs`
- `src/changes.rs`
- `src/tools.rs`
- `src/app.rs`
- `src/main.rs` (only to add `mod worktree;`)

## Detailed Tasks
1. **Create `src/worktree.rs`**:
   - Open module with a `//!` doc line.
   - Implement `WorktreeInfo` struct.
   - Implement pure functions: `worktree_path(repo_dir: &Path, session_id: u64) -> PathBuf` (`.viper/worktrees/<session_id>`) and `worktree_branch(session_id: u64) -> String` (`viper/session-<session_id>`).
   - Implement `parse_worktree_list(output: &str) -> Vec<WorktreeInfo>` (pure, tolerant parser for `git worktree list --porcelain`).
   - Implement `ensure_viper_ignored(repo_root: &Path) -> Result<(), String>` which appends `.viper/` to `<repo>/.git/info/exclude` if not already present.
   - Implement `create_worktree(repo_dir: &Path, session_id: u64, branch: Option<&str>, base_ref: Option<&str>) -> Result<(PathBuf, String), String>`.
   - Implement `remove_worktree(repo_dir: &Path, worktree_path: &Path, force: bool) -> Result<(), String>`.
   - Implement `delete_branch(repo_dir: &Path, branch: &str, force: bool) -> Result<(), String>`.
   - Implement `prune_worktrees(repo_dir: &Path) -> Result<(), String>`.
   - Implement `list_worktrees(repo_dir: &Path) -> Result<Vec<WorktreeInfo>, String>`.
   - Use standard library and `crate::agent::hidden_command`. No external dependencies!
2. **Session Worktree Awareness (`src/session.rs`)**:
   - Add fields to `Session`:
     ```rust
     #[serde(default)]
     pub worktree_dir: Option<PathBuf>,
     #[serde(default)]
     pub worktree_branch: Option<String>,
     #[serde(default)]
     pub worktree_base: Option<String>,
     ```
   - Implement `pub fn working_dir(&self) -> &Path` returning `self.worktree_dir.as_deref().unwrap_or(&self.project_dir)`.
   - In `Session::send(&mut self, ...)`: use `cwd: self.working_dir().to_path_buf()`.
   - Apply Challenger M1 recommendation in `Session::resolve_approval`: match `&& req.id == id && req.is_pending()`.
3. **Changes Panel Branch & Worktree Awareness (`src/git_diff.rs`, `src/changes.rs`, `src/tools.rs`)**:
   - In `src/git_diff.rs`, add `pub fn branch_changes(dir: &Path, base: &str) -> Result<Vec<FileDiff>, String>` running `git diff <base> --no-color --no-ext-diff --find-renames` and adding untracked files via `ls-files`.
   - In `src/changes.rs`, extend `Source` with `Branch { dir: PathBuf, branch: String, base: String }`.
   - In `Changes::refresh`, handle `Source::Branch` by calling `git_diff::branch_changes(dir, base)`.
   - In `Changes::watches(&self, dir: &Path)`, match if `dir` matches either `worktree_dir` or `project_dir`.
   - In `src/tools.rs`, ensure `open_changes` or `refresh_changes` handles worktree paths.
4. **App Session Creation & Cleanup (`src/app.rs`)**:
   - Update `tool_cwd(&self)` to use `session.working_dir().to_path_buf()`.
   - Implement helper or option to initialize a worktree for a session when desired: `pub fn setup_session_worktree(&mut self, session_id: u64, base_ref: Option<&str>) -> Result<PathBuf, String>`.
   - In `delete_session(&mut self, id: u64)`, if the removed session has `worktree_dir: Some(ref path)`, invoke `worktree::remove_worktree` and delete the associated branch.
5. **Unit & Integration Tests**:
   - Tests for `parse_worktree_list` parsing porcelain output (single, multiple, bare, detached, locked).
   - Tests for `worktree_path` and `worktree_branch` format.
   - Tests for `working_dir` fallback when `worktree_dir` is None vs Some.
   - Tests for RON backward compatibility: sessions without worktree fields deserialize with `None`.
   - Real git tests using temporary throwaway repos (`std::env::temp_dir().join(...)` with `-c user.name=Test -c user.email=test@example.com`):
     - Worktree creation, branch creation, and verification via `list_worktrees`.
     - `.git/info/exclude` addition prevents root repository from reporting `.viper/` as untracked.
     - Worktree removal and branch deletion cleanly cleans up.
     - `branch_changes` detects edits made in worktree compared to base.
6. **Quality Gates**:
   - `cargo check`
   - `cargo test`
   - `cargo clippy --all-targets -- -D warnings`
   - 0 errors, 0 warnings.
   - Do NOT run `cargo fmt`.
   - Do NOT add any dependencies to `Cargo.toml`.


## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## 2026-09-18T21:08:54Z
You are Worker M2 implementing Milestone 2: Git Worktree Session Isolation Foundation.
Your working directory is: C:\Users\ditob\Documents\viper\.agents\worker_m2
Read your instructions in: C:\Users\ditob\Documents\viper\.agents\worker_m2\DISPATCH.md
Read the user request in: C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md
Read repository rules in: C:\Users\ditob\Documents\viper\AGENTS.md
Read the Explorer report in: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_2\handoff.md

