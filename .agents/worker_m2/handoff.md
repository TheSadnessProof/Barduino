# Handoff Report: Milestone 2 — Git Worktree Session Isolation Foundation

## 1. Observation

1. **Session Workspace & Path Handling**:
   - In `src/session.rs`, `Session` previously tracked only `pub project_dir: PathBuf`, and `Session::send` directly passed `cwd: self.project_dir.clone()` to `Turn` (lines 230-237).
   - `src/app.rs` `tool_cwd` previously resolved `session.project_dir.clone()` (lines 247-254).
   - Sidebar session grouping in `src/sidebar.rs` groups sessions by `session.project_dir` via `group_by_workspace` (lines 548-563).

2. **Worktree Directory Structure & Git Untracked Status**:
   - Creating a worktree inside a repository under `.viper/worktrees/<session_id>` without exclusion causes `git status --porcelain` to report `?? .viper/`.
   - Appending `\n.viper/\n` to `.git/info/exclude` hides the `.viper/` directory completely from `git status` while keeping `.gitignore` clean and untracked repository files untouched.

3. **Porcelain Format & Diffing**:
   - `git worktree list --porcelain` outputs structured records separated by blank lines with headers `worktree <path>`, `HEAD <sha>`, `branch refs/heads/<branch>`, and attributes `bare`, `detached`, `locked`, `prunable`.
   - `git diff <base> --no-color --no-ext-diff --find-renames` inside a worktree captures both commits on the worktree branch since `<base>` and working tree uncommitted edits.

4. **Approval Request Resolution**:
   - `Session::resolve_approval` in `src/session.rs` previously broke on the first entry matching `req.id == id`, which prevented resolving a subsequent pending approval if an earlier turn contained an already-resolved approval with the identical ID.
   - Updating the condition to `&& req.id == id && req.is_pending()` resolved this defect and cleanly passed both single and multiple-turn approval lifecycle scenarios.

5. **Tool Commands and Results**:
   - `cargo check`: Finished in 1.58s with exit code 0.
   - `cargo test`: `196 passed; 0 failed; 8 ignored; finished in 2.07s` with exit code 0.
   - `cargo clippy --all-targets -- -D warnings`: Finished with exit code 0 and 0 warnings.
   - `git diff Cargo.toml`: Zero changes (no external dependencies added).

---

## 2. Logic Chain

1. **Separation of Project Root vs Working Directory**:
   - *From Observation 1.1 & 1.3*: `Session.project_dir` is required by the sidebar to group conversations by workspace project. Changing `Session.project_dir` directly to the worktree path would fragment the workspace grouping in the sidebar.
   - *Therefore*: We retained `Session.project_dir` as the root repository path and added `#[serde(default)]` fields `worktree_dir: Option<PathBuf>`, `worktree_branch: Option<String>`, and `worktree_base: Option<String>`.
   - *From Observation 1.1*: A helper method `working_dir(&self) -> &Path` returns `self.worktree_dir.as_deref().unwrap_or(&self.project_dir)`. Passing `self.working_dir()` to `Turn.cwd` and `app.tool_cwd()` isolates all agent process execution, terminal tabs, and file tools into the worktree without mutating the root repository.

2. **Clean Repository State via `.git/info/exclude`**:
   - *From Observation 1.2*: To ensure worktrees under `.viper/worktrees/<session_id>` do not pollute user status or dirty git tracking, `worktree::ensure_viper_ignored` inspects `.git/info/exclude` (handling both standard directories and linked worktree pointer files) and appends `.viper/` if not present.

3. **Isolated Worktree Lifecycle Management (`src/worktree.rs`)**:
   - *From Observation 1.3*: We encapsulated git worktree operations in a pure Rust standard-library module `src/worktree.rs`:
     - `worktree_path(repo_dir, session_id)` -> `.viper/worktrees/<session_id>`
     - `worktree_branch(session_id)` -> `viper/session-{session_id}`
     - `parse_worktree_list(&str) -> Vec<WorktreeInfo>` (tolerant parser for porcelain output)
     - `create_worktree(repo_dir, session_id, branch, base_ref)`
     - `remove_worktree(repo_dir, worktree_path, force)`
     - `delete_branch(repo_dir, branch, force)`
     - `prune_worktrees(repo_dir)`
     - `list_worktrees(repo_dir)`

4. **Changes Panel Worktree & Branch Integration**:
   - *From Observation 1.3*: Added `git_diff::branch_changes(dir, base)` to compute diffs against a base ref and include untracked worktree files.
   - Extended `changes::Source` with `Branch { dir: PathBuf, branch: String, base: String }`.
   - Updated `Changes::watches(dir)` so that watching checks if `dir` matches either the worktree path or the root project path (`worktree == dir || worktree.starts_with(dir)`).
   - Extended `Tools::open_branch_changes` and wired `SidebarAction::OpenChanges` to open branch diff tabs for active worktree sessions.

5. **App Lifecycle and Automatic Cleanup**:
   - Added `ViperApp::setup_session_worktree(&mut self, session_id: u64, base_ref: Option<&str>)` to configure session isolation.
   - Updated `ViperApp::delete_session` to automatically remove the worktree via `worktree::remove_worktree` and delete the associated branch via `worktree::delete_branch` when a worktree-enabled session is deleted.
   - Refreshed changes for `session.working_dir()` and `&session.project_dir` upon `AgentEvent::Exited`.

---

## 3. Caveats

- **Bare Repositories**: As noted in the Explorer report, worktree session isolation requires repositories with a working tree (`--is-inside-work-tree`).
- **Windows Process File Locking**: If an external program holds open handles in `.viper/worktrees/<session_id>`, `git worktree remove` may fail to delete some files; `remove_worktree` includes a fallback that attempts filesystem directory removal and runs `git worktree prune`.
- No other caveats.

---

## 4. Conclusion

Milestone 2 (Git Worktree Session Isolation Foundation) is fully implemented, verified, and integrated into Viper:
1. `src/worktree.rs` provides complete git worktree lifecycle operations, path resolution, and porcelain output parsing with zero external dependencies.
2. `src/session.rs` tracks worktree state with backward-compatible RON serialization, resolves active working directories, and fixes the approval resolution matching defect.
3. `src/git_diff.rs`, `src/changes.rs`, and `src/tools.rs` support branch diffing and worktree watching in the Changes panel.
4. `src/app.rs` provides `tool_cwd` resolution, session worktree setup, automatic cleanup on deletion, and post-turn diff refreshes.
5. All 196 tests pass, `cargo clippy --all-targets -- -D warnings` reports 0 warnings, and `Cargo.toml` is untouched.

---

## 5. Verification Method

### 5.1 Verification Commands
Execute from repository root `C:\Users\ditob\Documents\viper`:
```powershell
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

### 5.2 Specific Test Cases
- **Worktree Unit & Integration Tests**:
  - `worktree::tests::path_and_branch_formatting`
  - `worktree::tests::parse_worktree_list_handles_empty_and_junk_inputs`
  - `worktree::tests::parses_porcelain_worktree_output`
  - `worktree::tests::creates_and_lists_isolated_worktree_in_temp_repo`
  - `worktree::tests::session_worktree_modifications_isolate_from_main_repo`
  - `worktree::tests::cleans_up_worktree_and_deletes_branch_cleanly`
- **Session & Diff Tests**:
  - `session::tests::working_dir_falls_back_to_project_dir_when_worktree_none`
  - `session::tests::working_dir_uses_worktree_dir_when_present`
  - `session::tests::sessions_without_worktree_fields_deserialize_cleanly_with_defaults`
  - `session::tests::duplicate_approval_ids_across_turns_behavior`
  - `changes::tests::branch_source_title_and_watches_match_worktree_and_project`
  - `changes::tests::project_source_watches_project_and_nested_paths`
  - `tools::tests::open_branch_changes_creates_branch_tab_and_reuses_it`
- **App Session Lifecycle Tests**:
  - `app::tests::session_with_worktree_fields_serializes_and_deserializes_in_saved_state`
  - `app::tests::app_setup_session_worktree_and_delete_session_cleans_up_on_disk`

### 5.3 Invalidation Conditions
- If `git status` in the root repository reports `.viper/` as untracked after worktree creation, `ensure_viper_ignored` has failed.
- If deleting a session leaves `.viper/worktrees/<session_id>` or the `viper/session-<id>` branch lingering, cleanup has failed.
- If existing RON sessions fail to deserialize due to missing worktree fields, backward compatibility has failed.
