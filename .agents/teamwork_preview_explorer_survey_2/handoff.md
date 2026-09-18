# Handoff Report: Git Worktree Session Isolation Foundation (Requirement R2)

## 1. Observation

### 1.1 Session Workspace Paths in `session.rs`, `app.rs`, and `sidebar.rs`
- **`src/session.rs` line 84-87**:
  ```rust
  #[derive(Serialize, Deserialize)]
  #[serde(default)]
  pub struct Session {
      pub id: u64,
      pub title: String,
      pub project_dir: PathBuf,
  ```
  `project_dir: PathBuf` is currently the only directory field tracked by a `Session`.
- **`src/session.rs` line 225-232**:
  ```rust
  let turn = Turn {
      prompt: message.prompt(),
      cwd: self.project_dir.clone(),
      resume_session: self.agent_session_id.clone(),
      permission_mode: self.permission_mode,
      model: self.chosen_model.clone(),
      effort: self.effort.clone(),
  };
  ```
  When an agent turn is initiated, `cwd` is unconditionally passed as `self.project_dir.clone()`.
- **`src/app.rs` line 247-254**:
  ```rust
  fn tool_cwd(&self) -> PathBuf {
      let session = &self.state.sessions[self.active_index()];
      if session.has_folder() {
          session.project_dir.clone()
      } else {
          std::env::current_dir().unwrap_or_default()
      }
  }
  ```
  Embedded terminals (`show_terminal(&cwd)`), Changes tabs (`open_changes(&dir, ctx)`), and `PanelContext` all receive `self.tool_cwd()`.
- **`src/app.rs` line 267-274**:
  ```rust
  fn new_session(&mut self, project_dir: PathBuf, permission_mode: PermissionMode) {
      let id = self.state.next_session_id;
      self.state.next_session_id += 1;
      let provider = self.state.settings.default_provider;
      self.state.sessions.push(Session::new(id, project_dir, provider, permission_mode));
      self.state.active_session = id;
      self.view = View::Chat;
  }
  ```
- **`src/app.rs` line 276-290 (`delete_session`)**:
  ```rust
  fn delete_session(&mut self, id: u64) {
      let Some(index) = self.state.sessions.iter().position(|s| s.id == id) else { return };
      let removed = self.state.sessions.remove(index);
      self.tools.remove(&id);
      ...
  }
  ```
  Currently, deleting a session removes it from `state.sessions` and drops its tools panel, but performs no disk or git worktree cleanup.
- **`src/sidebar.rs` line 548-563 (`group_by_workspace`)**:
  ```rust
  pub fn group_by_workspace<'a>(sessions: &'a [Session], filter: &str) -> Vec<Workspace<'a>> {
      let mut workspaces: Vec<Workspace<'a>> = Vec::new();
      for session in sessions.iter().rev().filter(|session| matches(session, filter)) {
          let path = session.has_folder().then(|| session.project_dir.display().to_string());
          match workspaces.iter_mut().find(|workspace| workspace.path == path) {
              Some(workspace) => workspace.sessions.push(session),
              None => workspaces.push(Workspace {
                  name: session.folder_name(),
                  path,
                  dir: session.has_folder().then(|| session.project_dir.clone()),
                  sessions: vec![session],
              }),
          }
      }
      workspaces
  }
  ```
  Sessions are grouped by `session.project_dir`. Preserving `session.project_dir` as the root repository path ensures sessions operating in isolated worktrees remain grouped together under their parent project in the sidebar list.

### 1.2 Git Inspection in `git_diff.rs` and `changes.rs`
- **`src/git_diff.rs` line 100-116 (`working_tree_changes`)**:
  ```rust
  pub fn working_tree_changes(dir: &Path) -> Result<Vec<FileDiff>, String> {
      let git = git_executable().ok_or("Git isn't installed, so changes can't be shown.")?;
      let root = run_git(&git, dir, &["rev-parse", "--show-toplevel"])
          .map_err(|_| format!("{} isn't inside a git repository.", dir.display()))?;
      let root = PathBuf::from(root.trim());

      let base = if run_git(&git, &root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok() { "HEAD" } else { EMPTY_TREE };
      let diff = run_git(&git, &root, &["diff", base, "--no-color", "--no-ext-diff", "--find-renames"])?;
      let mut files = parse(&diff);

      let untracked = run_git(&git, &root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
      for path in untracked.split('\0').filter(|path| !path.is_empty()) {
          files.push(untracked_file(&root, path));
      }
      files.sort_by(|a, b| a.path.cmp(&b.path));
      Ok(files)
  }
  ```
- **`src/changes.rs` line 11-17 (`Source`)**:
  ```rust
  #[derive(Clone, PartialEq)]
  pub enum Source {
      /// Uncommitted changes in the project folder's git repository.
      Project(PathBuf),
      /// Two files chosen by the user.
      Files { old: PathBuf, new: PathBuf },
  }
  ```
- **`src/changes.rs` line 43-55 (`refresh`)**:
  Background thread executes `git_diff::working_tree_changes(dir)` or `git_diff::compare_files(old, new)` and notifies UI via `ctx.request_repaint()`.

### 1.3 Git Worktree Behavior on Windows (Direct Tool Invocations)
- Git version verified: `git version 2.55.0.windows.3`.
- `git worktree add -b viper/session-1 <repo>/.viper/worktrees/1 HEAD`:
  - `git rev-parse --show-toplevel` inside the worktree returns `<repo>/.viper/worktrees/1`.
  - `git rev-parse --abbrev-ref HEAD` inside the worktree returns `viper/session-1`.
- **Git Status Pollution & Exclusion**:
  When `<repo>/.viper/worktrees/1` is created inside `<repo>`, `git status --porcelain` in `<repo>` reports `?? .viper/`.
  When `.viper/` is appended to `<repo>/.git/info/exclude`, `git status --porcelain` in `<repo>` is clean (`""`). Crucially, `.git/info/exclude` does not touch or dirty tracked repository files like `.gitignore`.
- **Branch Diffing**:
  Executing `git diff <base_branch> --no-color --no-ext-diff --find-renames` inside `<repo>/.viper/worktrees/1` captures both uncommitted worktree changes and commits made on `viper/session-1` compared to `<base_branch>`.
  `git_diff::parse(&diff)` parses this output into `Vec<FileDiff>` without requiring format modifications.
- **`git worktree list --porcelain`**:
  Produces structured blocks:
  ```
  worktree <path>
  HEAD <commit-hash>
  branch refs/heads/<branch>
  ```
- **Cleanup**:
  `git worktree remove --force <path>` removes the worktree and dereferences it from `.git/worktrees/`. Subsequent `git branch -D viper/session-1` removes the branch.

---

## 2. Logic Chain

1. **State Preservation & Separation of Root vs Working Directory**:
   - Observations show `Session.project_dir` is used both for project grouping in the sidebar and as `cwd` for `Turn`.
   - If `Session.project_dir` were changed directly to `.viper/worktrees/<id>`, sessions belonging to the same repository would be split into separate workspace groups in the sidebar.
   - Therefore, `Session.project_dir` must remain the root repository path, and new fields:
     - `pub worktree_dir: Option<PathBuf>`
     - `pub worktree_branch: Option<String>`
     - `pub worktree_base: Option<String>`
     must be added to `Session`.
   - Adding `#[serde(default)]` to these fields ensures that existing RON saves deserialize with `None`, maintaining 100% backward compatibility as required by `AGENTS.md` Rule 3.5.
   - A helper method `pub fn working_dir(&self) -> &Path` resolves `self.worktree_dir.as_deref().unwrap_or(&self.project_dir)`.
   - Passing `self.working_dir()` to `Turn.cwd` and `app.tool_cwd()` isolates agent edits and terminal shells inside the worktree without affecting the root workspace grouping.

2. **Automated Exclusion of `.viper/`**:
   - Direct observation proved that placing worktrees under `.viper/worktrees/<session-id>` causes `git status` in the root repository to report `?? .viper/`.
   - Modifying `.gitignore` would introduce unrequested working-tree changes that the user might commit.
   - Appending `\n.viper/\n` to `<repo>/.git/info/exclude` hides `.viper/` from untracked file queries while remaining completely local to `.git`.

3. **Worktree Lifecycle in a Dedicated Module (`src/worktree.rs`)**:
   - `AGENTS.md` prescribes one flat module per concern in `src/` without adding external dependencies (pure Rust standard library + `agent::hidden_command`).
   - Defining `src/worktree.rs` encapsulates:
     - Deterministic path resolution: `worktree_path(repo_dir, session_id)` -> `<repo_root>/.viper/worktrees/<session_id>`.
     - Deterministic branch naming: `worktree_branch(session_id)` -> `viper/session-{session_id}`.
     - Worktree creation: `create_worktree(repo_dir, session_id, branch, base_ref)`.
     - Worktree listing: `list_worktrees(repo_dir)` with pure parser `parse_worktree_list(&str)`.
     - Worktree cleanup: `remove_worktree(repo_dir, worktree_path, force)` and `delete_branch(repo_dir, branch, force)`.
     - Stale reference pruning: `prune_worktrees(repo_dir)`.

4. **Changes Panel Worktree & Branch Awareness**:
   - `src/changes.rs` currently only supports `Source::Project(PathBuf)` and `Source::Files`.
   - Extending `Source` with `Branch { dir: PathBuf, branch: String, base: String }` allows inspecting branch-level diffs against the base branch (e.g. `main`).
   - In `git_diff.rs`, adding `branch_changes(dir: &Path, base: &str) -> Result<Vec<FileDiff>, String>` runs `git diff <base> --no-color --no-ext-diff --find-renames` in `dir`, adds untracked files via `ls-files`, and returns parsed `Vec<FileDiff>`.
   - `watches(&self, dir: &Path)` checks if `dir` matches either `worktree_dir` or `project_dir`, ensuring refreshes occur whenever turns finish or files are edited.

5. **Unit Testing Isolation**:
   - Testing git commands in the active working directory violates `AGENTS.md`.
   - An in-memory/temporary repository pattern using `std::env::temp_dir().join(format!("viper-wt-test-{}-{}", std::process::id(), unique_counter))` isolates git operations in throwaway folders.
   - Passing `-c user.name=Test -c user.email=test@example.com` avoids touching global or system git configs.
   - Pure functions (`parse_worktree_list`, `worktree_path`, `worktree_branch`, `working_dir`, RON serialization) can be unit-tested without any filesystem or git process requirements.

---

## 3. Caveats

1. **Bare Repositories**: The worktree isolation foundation assumes standard git repositories with a working tree (`--is-inside-work-tree`). Bare repositories without a working tree are not targeted.
2. **Submodules**: Worktrees created on repositories containing git submodules inherit the submodule configurations; submodule recursive updating inside the worktree is left to standard git behavior.
3. **Windows File Locking**: If an external process (e.g., an open shell or antivirus scanner) holds a file lock inside `.viper/worktrees/<session-id>`, `git worktree remove` may report permission errors. The cleanup routine must fall back to best-effort directory deletion (`fs::remove_dir_all`) followed by `git worktree prune`.
4. **Git Executable Requirement**: If git is not installed or not found on PATH / Program Files, worktree creation returns a clear, user-facing error message, gracefully falling back to root directory execution.

---

## 4. Conclusion & Technical Design

### 4.1 Module Design: `src/worktree.rs`
Create `src/worktree.rs` with the following API:
```rust
//! Git worktree management for session isolation.
//!
//! Creates and manages isolated worktrees under `.viper/worktrees/<session-id>`,
//! allowing agent sessions to run in their own branch without mutating
//! the user's active checkout.

use std::path::{Path, PathBuf};
use crate::agent::hidden_command;
use crate::git_diff::git_executable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub is_bare: bool,
    pub is_detached: bool,
    pub is_locked: bool,
    pub prunable: bool,
}

pub fn worktree_path(repo_dir: &Path, session_id: u64) -> PathBuf {
    repo_dir.join(".viper").join("worktrees").join(session_id.to_string())
}

pub fn worktree_branch(session_id: u64) -> String {
    format!("viper/session-{session_id}")
}

pub fn ensure_viper_ignored(repo_root: &Path) -> Result<(), String> {
    let exclude_path = repo_root.join(".git").join("info").join("exclude");
    if let Ok(content) = std::fs::read_to_string(&exclude_path) {
        if content.lines().any(|line| line.trim() == ".viper" || line.trim() == ".viper/") {
            return Ok(());
        }
    }
    if let Some(parent) = exclude_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_path)
        .map_err(|e| format!("Couldn't open .git/info/exclude: {e}"))?;
    writeln!(file, "\n.viper/\n").map_err(|e| format!("Couldn't write to .git/info/exclude: {e}"))?;
    Ok(())
}

pub fn create_worktree(
    repo_dir: &Path,
    session_id: u64,
    branch: Option<&str>,
    base_ref: Option<&str>,
) -> Result<(PathBuf, String), String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let path = worktree_path(&repo_root, session_id);
    let branch = branch.map(str::to_owned).unwrap_or_else(|| worktree_branch(session_id));
    let base_ref = base_ref.unwrap_or("HEAD");

    ensure_viper_ignored(&repo_root)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't create directory: {e}"))?;
    }

    let path_str = path.to_string_lossy();
    // Attempt creating branch and worktree; if branch already exists, attach to it.
    let output = hidden_command(&git)
        .args(["worktree", "add", "-b", &branch, &path_str, base_ref])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Couldn't run git worktree add: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("already exists") {
            let output_existing = hidden_command(&git)
                .args(["worktree", "add", &path_str, &branch])
                .current_dir(&repo_root)
                .output()
                .map_err(|e| format!("Couldn't run git worktree add: {e}"))?;
            if !output_existing.status.success() {
                return Err(String::from_utf8_lossy(&output_existing.stderr).trim().to_owned());
            }
        } else {
            return Err(stderr.trim().to_owned());
        }
    }

    Ok((path, branch))
}

pub fn remove_worktree(repo_dir: &Path, worktree_path: &Path, force: bool) -> Result<(), String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let path_str = worktree_path.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path_str);

    let output = hidden_command(&git)
        .args(&args)
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Couldn't run git worktree remove: {e}"))?;

    if !output.status.success() {
        // Fallback: directory deletion + prune
        let _ = std::fs::remove_dir_all(worktree_path);
        let _ = prune_worktrees(&repo_root);
    }
    Ok(())
}

pub fn delete_branch(repo_dir: &Path, branch: &str, force: bool) -> Result<(), String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let flag = if force { "-D" } else { "-d" };
    let output = hidden_command(&git)
        .args(["branch", flag, branch])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Couldn't run git branch delete: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

pub fn prune_worktrees(repo_dir: &Path) -> Result<(), String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let output = hidden_command(&git)
        .args(["worktree", "prune"])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Couldn't run git worktree prune: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

pub fn parse_worktree_list(output: &str) -> Vec<WorktreeInfo> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_head = String::new();
    let mut current_branch: Option<String> = None;
    let mut is_bare = false;
    let mut is_detached = false;
    let mut is_locked = false;
    let mut prunable = false;

    let flush = |worktrees: &mut Vec<WorktreeInfo>, path: &mut Option<PathBuf>, head: &mut String, branch: &mut Option<String>, bare: &mut bool, detached: &mut bool, locked: &mut bool, prun: &mut bool| {
        if let Some(p) = path.take() {
            worktrees.push(WorktreeInfo {
                path: p,
                head: std::mem::take(head),
                branch: branch.take(),
                is_bare: *bare,
                is_detached: *detached,
                is_locked: *locked,
                prunable: *prun,
            });
            *bare = false;
            *detached = false;
            *locked = false;
            *prun = false;
        }
    };

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush(&mut worktrees, &mut current_path, &mut current_head, &mut current_branch, &mut is_bare, &mut is_detached, &mut is_locked, &mut prunable);
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            flush(&mut worktrees, &mut current_path, &mut current_head, &mut current_branch, &mut is_bare, &mut is_detached, &mut is_locked, &mut prunable);
            current_path = Some(PathBuf::from(path));
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current_head = head.to_owned();
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            current_branch = Some(b.to_owned());
        } else if line == "bare" {
            is_bare = true;
        } else if line == "detached" {
            is_detached = true;
        } else if line.starts_with("locked") {
            is_locked = true;
        } else if line.starts_with("prunable") {
            prunable = true;
        }
    }
    flush(&mut worktrees, &mut current_path, &mut current_head, &mut current_branch, &mut is_bare, &mut is_detached, &mut is_locked, &mut prunable);
    worktrees
}

pub fn list_worktrees(repo_dir: &Path) -> Result<Vec<WorktreeInfo>, String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let output = hidden_command(&git)
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Couldn't run git worktree list: {e}"))?;
    if output.status.success() {
        Ok(parse_worktree_list(&String::from_utf8_lossy(&output.stdout)))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn repo_root(git: &Path, dir: &Path) -> Result<PathBuf, String> {
    let output = hidden_command(git)
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("Couldn't run git rev-parse: {e}"))?;
    if output.status.success() {
        Ok(PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
    } else {
        Err(format!("{} isn't inside a git repository.", dir.display()))
    }
}
```

### 4.2 Integration into `Session` (`src/session.rs`)
- Add fields to `Session`:
  ```rust
  #[serde(default)]
  pub worktree_dir: Option<PathBuf>,
  #[serde(default)]
  pub worktree_branch: Option<String>,
  #[serde(default)]
  pub worktree_base: Option<String>,
  ```
- Add helper method:
  ```rust
  pub fn working_dir(&self) -> &Path {
      self.worktree_dir.as_deref().unwrap_or(&self.project_dir)
  }
  ```
- In `session.send()`:
  ```rust
  let turn = Turn {
      prompt: message.prompt(),
      cwd: self.working_dir().to_path_buf(),
      ...
  };
  ```

### 4.3 Integration into Changes Panel (`src/git_diff.rs`, `src/changes.rs`, `src/tools.rs`)
- In `src/git_diff.rs`, add `branch_changes`:
  ```rust
  pub fn branch_changes(dir: &Path, base: &str) -> Result<Vec<FileDiff>, String> {
      let git = git_executable().ok_or("Git isn't installed, so changes can't be shown.")?;
      let root = run_git(&git, dir, &["rev-parse", "--show-toplevel"])
          .map_err(|_| format!("{} isn't inside a git repository.", dir.display()))?;
      let root = PathBuf::from(root.trim());

      let diff = run_git(&git, &root, &["diff", base, "--no-color", "--no-ext-diff", "--find-renames"])?;
      let mut files = parse(&diff);

      let untracked = run_git(&git, &root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
      for path in untracked.split('\0').filter(|path| !path.is_empty()) {
          files.push(untracked_file(&root, path));
      }
      files.sort_by(|a, b| a.path.cmp(&b.path));
      Ok(files)
  }
  ```
- In `src/changes.rs`, extend `Source`:
  ```rust
  pub enum Source {
      Project(PathBuf),
      Branch {
          dir: PathBuf,
          branch: String,
          base: String,
      },
      Files { old: PathBuf, new: PathBuf },
  }
  ```
- In `Changes::refresh()`:
  Dispatch `Source::Branch { dir, base, .. }` to `git_diff::branch_changes(dir, base)`.
- In `Changes::watches(dir)`:
  Match if `source.dir == dir` or if `source.project == dir`.
- In `Tools::open_changes()`:
  Accept worktree branch details when present to open a `Source::Branch` tab, enabling immediate inspection of the worktree branch against base.

---

## 5. Verification Method

### 5.1 Independent Commands
1. **Compilation & Clippy Verification**:
   ```powershell
   cargo check
   cargo test
   cargo clippy --all-targets -- -D warnings
   ```
   Must pass with 0 errors and 0 warnings.

2. **Worktree & Diff Unit Test Execution**:
   ```powershell
   cargo test worktree
   cargo test git_diff
   cargo test changes
   ```

### 5.2 Specific Test Cases to Verify
- **Pure Tests**:
  - `worktree::tests::parses_porcelain_worktree_output` (validates parsing of single, multiple, detached, locked entries).
  - `worktree::tests::path_and_branch_formatting` (validates `.viper/worktrees/<id>` and `viper/session-<id>`).
  - `session::tests::session_with_worktree_deserializes_cleanly_from_older_ron` (validates backward compatibility).
  - `session::tests::working_dir_falls_back_to_project_dir_when_worktree_none`.
- **Live Temp Repo Tests**:
  - `worktree::tests::creates_and_lists_isolated_worktree_in_temp_repo` (verifies branch creation, `.git/info/exclude` addition, and porcelain list).
  - `worktree::tests::session_worktree_modifications_isolate_from_main_repo` (verifies edits in worktree do not taint root repo).
  - `git_diff::tests::branch_changes_detects_both_committed_and_uncommitted_worktree_edits`.
  - `worktree::tests::cleans_up_worktree_and_deletes_branch_cleanly`.

### 5.3 Invalidation Conditions
- If running `git status` in the root repository reports `.viper/` after worktree creation, exclusion failed.
- If deleting a session leaves `.viper/worktrees/<id>` locked or orphaned in `.git/worktrees/`, cleanup failed.
- If existing saved sessions in `app.ron` fail to deserialize due to missing worktree fields, the change is invalid.
