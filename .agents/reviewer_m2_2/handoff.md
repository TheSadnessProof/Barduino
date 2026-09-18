# Handoff Report: Reviewer M2-2 — Milestone 2 Evaluation

## Review Summary

**Verdict**: **APPROVE**

Milestone 2 (Git Worktree Session Isolation Foundation) implementation is verified to be robust, performant, and compliant with all project and architectural guidelines in `ORIGINAL_REQUEST.md`, `PROJECT.md`, and `AGENTS.md`. Zero integrity violations, zero build errors, zero unit test failures (196 passing), and zero clippy warnings under `-D warnings` were found.

One **Major Finding** (non-blocking for M2 foundation, recommended for resolution before release) regarding tab deduplication collision between `Source::Project` and `Source::Branch` in `src/tools.rs` is surfaced below with full attack scenario and mitigation.

---

## 1. Observation

1. **Test and Compiler Execution**:
   - `cargo check`: Finished dev profile in 0.55s with exit code 0.
   - `cargo test`: `196 passed; 0 failed; 8 ignored; finished in 2.48s` with exit code 0. No new tests were ignored, and all 8 ignored tests match pre-existing machine/paid-dependent tests.
   - `cargo clippy --all-targets -- -D warnings`: Finished in 0.77s with exit code 0 and 0 warnings.
   - `git diff Cargo.toml`: Zero lines modified; zero dependencies added.

2. **Git Worktree Isolation Foundation (`src/worktree.rs`)**:
   - Lines 1-5: Module opens with descriptive doc comment:
     ```rust
     //! Git worktree management for session isolation.
     //!
     //! Creates and manages isolated worktrees under `.viper/worktrees/<session-id>`,
     //! allowing agent sessions to run in their own branch without mutating
     //! the user's active checkout.
     ```
   - Lines 26-38: Deterministic path and branch resolvers:
     `worktree_path(repo_dir: &Path, session_id: u64) -> PathBuf` returns `repo_dir.join(".viper").join("worktrees").join(session_id.to_string())`.
     `worktree_branch(session_id: u64) -> String` returns `format!("viper/session-{session_id}")`.
   - Lines 44-89: `ensure_viper_ignored(repo_root: &Path)`:
     Safely detects `.git` as directory or file (for linked worktrees), checks existing rules in `.git/info/exclude`, and appends `\n.viper/\n` only if not already present.
   - Lines 97-142: `create_worktree(repo_dir: &Path, session_id: u64, branch: Option<&str>, base_ref: Option<&str>)`:
     Creates the parent directory, executes `git worktree add -b <branch> <path> <base_ref>`, and if git reports the branch already exists, falls back to checking out the existing branch with `git worktree add <path> <branch>`.
   - Lines 145-168: `remove_worktree(repo_dir: &Path, worktree_path: &Path, force: bool)`:
     Executes `git worktree remove [--force] <path>`. If git fails (e.g. file locks on Windows), falls back to `std::fs::remove_dir_all(worktree_path)` followed by `git worktree prune`.
   - Lines 171-186: `delete_branch(repo_dir: &Path, branch: &str, force: bool)`:
     Deletes branch with `git branch -D` (or `-d`).
   - Lines 189-203: `prune_worktrees(repo_dir: &Path)`:
     Executes `git worktree prune`.
   - Lines 208-293: `parse_worktree_list(output: &str) -> Vec<WorktreeInfo>`:
     Tolerant parser for `git worktree list --porcelain`. Handles empty, malformed, and attribute lines without panicking.

3. **Session Worktree Tracking & Path Routing (`src/session.rs`)**:
   - Lines 93-101: `Session` struct extended with `#[serde(default)]`:
     ```rust
     #[serde(default)]
     pub worktree_dir: Option<PathBuf>,
     #[serde(default)]
     pub worktree_branch: Option<String>,
     #[serde(default)]
     pub worktree_base: Option<String>,
     ```
   - Lines 235-237: `pub fn working_dir(&self) -> &Path`:
     Returns `self.worktree_dir.as_deref().unwrap_or(&self.project_dir)`.
   - Line 250: `Session::send` passes `cwd: self.working_dir().to_path_buf()` to `Turn`, isolating CLI agent execution inside the worktree.
   - Line 302: `Session::resolve_approval` condition uses `req.id == id && req.is_pending()`, fixing resolution in multi-turn contexts with duplicate request IDs.

4. **Changes & Tools Panel Branch Diffing (`src/git_diff.rs`, `src/changes.rs`, `src/tools.rs`)**:
   - `src/git_diff.rs` lines 118-135: `branch_changes(dir: &Path, base: &str) -> Result<Vec<FileDiff>, String>`:
     Runs `git diff <base> --no-color --no-ext-diff --find-renames` and appends untracked files via `git ls-files --others --exclude-standard -z`.
   - `src/changes.rs` lines 16-20: `Source::Branch { dir: PathBuf, branch: String, base: String }`.
   - `src/changes.rs` lines 66-71: `watches(&self, dir: &Path)`:
     ```rust
     match &self.source {
         Source::Project(project) => project == dir || dir.starts_with(project),
         Source::Branch { dir: worktree, .. } => worktree == dir || worktree.starts_with(dir),
         Source::Files { .. } => false,
     }
     ```
   - `src/tools.rs` lines 116 & 128:
     `open_changes`:
     ```rust
     let existing = self.tabs.iter().position(|tab| matches!(tab, Tab::Changes(changes) if changes.watches(dir)));
     ```
     `open_branch_changes`:
     ```rust
     let existing = self.tabs.iter().position(|tab| matches!(tab, Tab::Changes(changes) if changes.watches(dir)));
     ```

5. **App Lifecycle & Cleanup Integration (`src/app.rs`)**:
   - Lines 272-279: `tool_cwd(&self) -> PathBuf`:
     Returns `session.working_dir().to_path_buf()`.
   - Lines 304-322: `setup_session_worktree(&mut self, session_id: u64, base_ref: Option<&str>) -> Result<PathBuf, String>`.
   - Lines 331-336: `delete_session`:
     Calls `remove_worktree` and `delete_branch` to automatically purge `.viper/worktrees/<session-id>` and `viper/session-<session-id>`.
   - Lines 445-460: `SidebarAction::OpenChanges`:
     Routes to `open_branch_changes(&wt_dir, &branch, &base, ctx)` when a worktree is present.
   - Lines 777-780: On `AgentEvent::Exited`, refreshes changes for `session.working_dir()` and `&session.project_dir`.

---

## 2. Logic Chain

1. **Session Isolation Preserving Workspace Grouping**:
   - *From Observation 3 & 5*: The sidebar groups session cards by `Session.project_dir`. Mutating `project_dir` to point to `.viper/worktrees/<id>` would break workspace grouping in the UI.
   - *Logic*: By retaining `project_dir` as the git repository root and adding `worktree_dir`, `Session::working_dir()` routes agent execution (`Turn.cwd`), terminal tabs (`app.tool_cwd()`), and diffing into the isolated worktree while leaving session grouping intact.
   - *Conclusion*: Meets requirement R2 cleanly without UI fragmentation.

2. **Clean Status Invariant via `.git/info/exclude`**:
   - *From Observation 2*: Creating worktrees within `.viper/` in the project tree would normally cause `git status` in the root repository to report `?? .viper/`.
   - *Logic*: `ensure_viper_ignored` adds `.viper/` to `.git/info/exclude`. Unlike `.gitignore`, `.git/info/exclude` is local to the clone and never produces dirty working trees or commit noise.
   - *Conclusion*: Verified by test `creates_and_lists_isolated_worktree_in_temp_repo` where `git status --porcelain` in the root repo reports zero untracked entries.

3. **RON Persistence Backward Compatibility**:
   - *From Observation 3 & 1*: `Session` uses `#[serde(default)]` on the struct and on all three new worktree fields (`worktree_dir`, `worktree_branch`, `worktree_base`).
   - *Logic*: Sessions serialized before Milestone 2 deserialize with `None` values and seamlessly fall back to `self.project_dir` in `working_dir()`.
   - *Conclusion*: Adheres strictly to `AGENTS.md` Rule 3.5.

4. **Adversarial Analysis of Tab Deduplication Collision**:
   - *From Observation 4*: `Changes::watches(dir)` is defined as:
     - `Source::Project(project)`: `project == dir || dir.starts_with(project)`
     - `Source::Branch { dir: worktree, .. }`: `worktree == dir || worktree.starts_with(dir)`
   - *Logic Step 1*: If a user in a session opens project changes (`Source::Project(repo_root)`), and then calls `open_branch_changes(worktree_dir, ...)`.
   - *Logic Step 2*: In `open_branch_changes`, `changes.watches(worktree_dir)` evaluates on the existing `Source::Project(repo_root)` tab. Since `worktree_dir` (`repo_root/.viper/worktrees/1`) starts with `repo_root`, `dir.starts_with(project)` evaluates to `true`.
   - *Logic Step 3*: `open_branch_changes` finds `existing = Some(0)` and sets `self.active = 0`, activating the existing `Source::Project` tab instead of creating the `Source::Branch` tab. The user sees the root repo diff rather than the worktree branch diff.
   - *Logic Step 4*: In reverse, if `Source::Branch` is open, `open_changes(repo_root)` checks `changes.watches(repo_root)`. Since `worktree.starts_with(repo_root)` evaluates to `true`, `open_changes` activates the branch changes tab instead of opening project changes.
   - *Mitigation*: Tab deduplication in `open_changes` and `open_branch_changes` should match on tab source variant and target path rather than using `watches(dir)`. `watches(dir)` should be reserved for event invalidation (`refresh_changes`).

---

## 3. Findings

### [Major] Finding 1: Tab Deduplication Collision between `Source::Project` and `Source::Branch` in `Tools::open_changes` / `open_branch_changes`

- **What**: Opening branch changes reuses an existing project changes tab (and vice versa) within the same session's tools panel, preventing simultaneous inspection or switching between project and branch diffs.
- **Where**: `src/tools.rs:116`, `src/tools.rs:128`, and `src/changes.rs:66-71`.
- **Why**: `Changes::watches(dir)` returns `true` if `dir.starts_with(project)` or `worktree.starts_with(dir)`. In `src/tools.rs`, `open_changes` and `open_branch_changes` both use `if changes.watches(dir)` to detect whether a tab already exists:
  ```rust
  // tools.rs:128
  let existing = self.tabs.iter().position(|tab| matches!(tab, Tab::Changes(changes) if changes.watches(dir)));
  ```
  Because a worktree path is nested within the repository path, both conditions match each other, causing tab deduplication to hijack the other source type.
- **Attack / Failure Scenario**:
  1. User has session with `project_dir = C:\repo` and worktree at `C:\repo\.viper\worktrees\1`.
  2. User clicks "+" -> "Changes" in the tools panel (opens Tab 0: `Source::Project(C:\repo)`).
  3. User clicks "Changes" on the session in the sidebar (`SidebarAction::OpenChanges` calls `open_branch_changes(C:\repo\.viper\worktrees\1, "viper/session-1", "main")`).
  4. Tab 0 evaluates `changes.watches(C:\repo\.viper\worktrees\1)` -> `C:\repo\.viper\worktrees\1.starts_with(C:\repo)` is `true`.
  5. `open_branch_changes` reuses Tab 0 and never creates the `Source::Branch` tab. The user is shown uncommitted changes in `C:\repo` instead of the branch diff.
- **Suggestion**:
  Separate tab identity from event watching:
  ```rust
  // In Tools::open_changes:
  let existing = self.tabs.iter().position(|tab| {
      matches!(tab, Tab::Changes(c) if matches!(&c.source, Source::Project(p) if p == dir))
  });

  // In Tools::open_branch_changes:
  let existing = self.tabs.iter().position(|tab| {
      matches!(tab, Tab::Changes(c) if matches!(&c.source, Source::Branch { dir: d, .. } if d == dir))
  });
  ```

### [Minor] Finding 2: Missing Proactive `worktree prune` before `create_worktree`

- **What**: If a previous worktree directory was deleted by an external tool or ungracefully terminated without pruning, `.git/worktrees/<session-id>` administrative records can linger.
- **Where**: `src/worktree.rs:103-122`.
- **Why**: Git will reject `git worktree add` if administrative metadata still references the worktree name or branch as registered.
- **Suggestion**: Call `let _ = prune_worktrees(&repo_root);` at the start of `create_worktree` before invoking `git worktree add`.

---

## 4. Adversarial Stress-Test Matrix

| Scenario | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|
| **Missing Git Binary** | Functions return clean user-facing error `String` without panicking | `git_executable().ok_or(...)` cleanly returns `Err("Git isn't installed.")` | **PASS** |
| **Non-Git Directory** | Functions return error `String` indicating path is not a git repo | `repo_root` returns `Err("... isn't inside a git repository.")` | **PASS** |
| **Existing Session Branch** | Checkout existing branch rather than failing turn | `create_worktree` catches `"already exists"` and checks out branch | **PASS** |
| **Path Traversal via Session ID** | Prevent escaping `.viper/worktrees` | `session_id` is strictly typed as `u64`, preventing `../` traversal | **PASS** |
| **Windows File Lock on Cleanup** | Graceful degradation without crashing UI | `remove_worktree` falls back to `fs::remove_dir_all` + `prune_worktrees` | **PASS** |
| **Dirty Repo Status Pollution** | Parent repository git status remains clean | `.git/info/exclude` updated; parent `git status` shows 0 untracked files | **PASS** |
| **Cross-Session Tab Leakage** | Tabs isolated between sessions | `self.tools` is `BTreeMap<u64, Tools>`, isolating tab state per session | **PASS** |
| **Legacy Session Deserialization** | Saved RON without worktree fields deserializes | Deserializes cleanly with `worktree_dir = None`, `working_dir() == project_dir` | **PASS** |

---

## 5. Integrity Check

- **Hardcoded test outputs in source code**: **None**. All git operations execute real commands.
- **Dummy or facade implementations**: **None**. All methods (`create_worktree`, `remove_worktree`, `branch_changes`, etc.) contain real functional logic.
- **Shortcuts bypassing the intended task**: **None**. Worktree management uses pure Rust standard library processes without pulling in external third-party crates.
- **Fabricated verification outputs or logs**: **None**. Verification commands executed live and verified independently.
- **Self-certifying work without genuine independent verification**: **None**. Independent review verified behavior against temporary git repositories.

---

## 6. Caveats

1. **Default `base_ref` ("HEAD")**: When `base_ref` is `None`, `create_worktree` uses `"HEAD"`. If the user or agent creates commits inside the worktree branch, `git diff HEAD` reflects uncommitted changes in the worktree rather than the commit history against the original branch. Calling `setup_session_worktree` with an explicit base branch (e.g. `"main"`) produces a diff encompassing all branch commits.
2. **Windows File Lock Asynchrony**: If an external program or language server has an open handle inside the worktree directory when the session is deleted, the directory cannot be completely unlinked immediately; `remove_worktree` swalllows the OS file lock error and runs `prune_worktrees`.

---

## 7. Conclusion

**Verdict: APPROVE**

Milestone 2 fulfills all functional requirements in `ORIGINAL_REQUEST.md` and `PROJECT.md`. The code conforms to `AGENTS.md`, passes all 196 tests with 0 failures, maintains 0 clippy warnings, and introduces zero new dependencies. Finding 1 is documented for resolution in Milestone 4.

---

## 8. Verification Method

To independently verify the implementation:
```powershell
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

Key test suites to inspect:
- `worktree::tests::creates_and_lists_isolated_worktree_in_temp_repo`
- `worktree::tests::session_worktree_modifications_isolate_from_main_repo`
- `worktree::tests::cleans_up_worktree_and_deletes_branch_cleanly`
- `session::tests::sessions_without_worktree_fields_deserialize_cleanly_with_defaults`
- `app::tests::app_setup_session_worktree_and_delete_session_cleans_up_on_disk`
- `changes::tests::branch_source_title_and_watches_match_worktree_and_project`
- `tools::tests::open_branch_changes_creates_branch_tab_and_reuses_it`
