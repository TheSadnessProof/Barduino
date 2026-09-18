//! Git worktree management for session isolation.
//!
//! Creates and manages isolated worktrees under `.viper/worktrees/<session-id>`,
//! allowing agent sessions to run in their own branch without mutating
//! the user's active checkout.

use std::path::{Path, PathBuf};

use crate::agent::hidden_command;
use crate::git_diff::git_executable;

/// Information about a git worktree reported by `git worktree list --porcelain`.
// Public worktree inspection model used across session isolation commands and tests.
#[allow(dead_code)]
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

/// The deterministic path where an isolated session worktree is placed.
// Deterministic worktree path resolver used for session directory management and tests.
#[allow(dead_code)]
pub fn worktree_path(repo_dir: &Path, session_id: u64) -> PathBuf {
    repo_dir.join(".viper").join("worktrees").join(session_id.to_string())
}

/// The branch name allocated for a session's isolated commits.
// Deterministic branch naming helper used for worktree sessions and tests.
#[allow(dead_code)]
pub fn worktree_branch(session_id: u64) -> String {
    format!("viper/session-{session_id}")
}

/// Appends `.viper/` to `.git/info/exclude` if not already ignored, ensuring
/// worktree folders never dirty the parent repository's untracked file status.
// Helper to prevent .viper working trees from appearing as untracked in the root repo.
#[allow(dead_code)]
pub fn ensure_viper_ignored(repo_root: &Path) -> Result<(), String> {
    let git_item = repo_root.join(".git");
    let exclude_path = if git_item.is_file() {
        // In a linked worktree, .git is a file containing "gitdir: <path>".
        // Resolving through gitdir leads to the common git directory.
        if let Ok(content) = std::fs::read_to_string(&git_item) {
            if let Some(gitdir_line) = content.lines().find(|l| l.starts_with("gitdir:")) {
                let gitdir_path = gitdir_line.trim_start_matches("gitdir:").trim();
                let p = PathBuf::from(gitdir_path);
                if let Some(common) = p.parent().and_then(|p| p.parent()) {
                    common.join("info").join("exclude")
                } else {
                    git_item.join("info").join("exclude")
                }
            } else {
                git_item.join("info").join("exclude")
            }
        } else {
            git_item.join("info").join("exclude")
        }
    } else {
        git_item.join("info").join("exclude")
    };

    if let Ok(content) = std::fs::read_to_string(&exclude_path)
        && content.lines().any(|line| {
            let t = line.trim();
            t == ".viper" || t == ".viper/" || t == "**/.viper" || t == "**/.viper/"
        })
    {
        return Ok(());
    }

    if let Some(parent) = exclude_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_path)
        .map_err(|e| format!("Couldn't open {}: {e}", exclude_path.display()))?;
    writeln!(file, "\n.viper/\n").map_err(|e| format!("Couldn't write to {}: {e}", exclude_path.display()))?;
    Ok(())
}

/// Creates a new worktree and branch for the given session ID.
///
/// If `branch` is omitted, defaults to `viper/session-{session_id}`.
/// If `base_ref` is omitted, defaults to `HEAD`.
// Primary worktree creation interface for session isolation and tests.
#[allow(dead_code)]
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
        std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't create directory {}: {e}", parent.display()))?;
    }

    let path_str = path.to_string_lossy();
    let output = hidden_command(&git)
        .args(["-c", "core.quotepath=false"])
        .args(["worktree", "add", "-b", &branch, &path_str, base_ref])
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Couldn't run git worktree add: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("already exists") {
            // Branch may already exist from an earlier attempt; checkout existing branch.
            let output_existing = hidden_command(&git)
                .args(["-c", "core.quotepath=false"])
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

/// Removes a worktree and dereferences it from the repository.
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
        .args(["-c", "core.quotepath=false"])
        .args(&args)
        .current_dir(&repo_root)
        .output()
        .map_err(|e| format!("Couldn't run git worktree remove: {e}"))?;

    if !output.status.success() {
        // Fallback: remove directory from disk if git couldn't do it, then prune.
        let _ = std::fs::remove_dir_all(worktree_path);
        let _ = prune_worktrees(&repo_root);
    }
    Ok(())
}

/// Deletes a branch created for a session worktree.
pub fn delete_branch(repo_dir: &Path, branch: &str, force: bool) -> Result<(), String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let flag = if force { "-D" } else { "-d" };
    let output = hidden_command(&git)
        .args(["-c", "core.quotepath=false"])
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

/// Prunes stale worktree administrative references whose paths no longer exist on disk.
pub fn prune_worktrees(repo_dir: &Path) -> Result<(), String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let output = hidden_command(&git)
        .args(["-c", "core.quotepath=false"])
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

/// Parses the output of `git worktree list --porcelain` into structured worktrees.
// Pure parser for porcelain output, used by list_worktrees and unit tests.
#[allow(dead_code)]
pub fn parse_worktree_list(output: &str) -> Vec<WorktreeInfo> {
    let mut worktrees = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut current_head = String::new();
    let mut current_branch: Option<String> = None;
    let mut is_bare = false;
    let mut is_detached = false;
    let mut is_locked = false;
    let mut prunable = false;

    let mut flush = |path: &mut Option<PathBuf>,
                     head: &mut String,
                     branch: &mut Option<String>,
                     bare: &mut bool,
                     detached: &mut bool,
                     locked: &mut bool,
                     prun: &mut bool| {
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
            flush(
                &mut current_path,
                &mut current_head,
                &mut current_branch,
                &mut is_bare,
                &mut is_detached,
                &mut is_locked,
                &mut prunable,
            );
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            flush(
                &mut current_path,
                &mut current_head,
                &mut current_branch,
                &mut is_bare,
                &mut is_detached,
                &mut is_locked,
                &mut prunable,
            );
            current_path = Some(PathBuf::from(path.trim()));
        } else if let Some(head) = line.strip_prefix("HEAD ") {
            current_head = head.trim().to_owned();
        } else if let Some(b) = line.strip_prefix("branch ") {
            let b = b.trim();
            let name = b.strip_prefix("refs/heads/").unwrap_or(b);
            current_branch = Some(name.to_owned());
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
    flush(
        &mut current_path,
        &mut current_head,
        &mut current_branch,
        &mut is_bare,
        &mut is_detached,
        &mut is_locked,
        &mut prunable,
    );
    worktrees
}

/// Lists all active worktrees associated with the repository containing `repo_dir`.
// Inspection function for querying active worktrees.
#[allow(dead_code)]
pub fn list_worktrees(repo_dir: &Path) -> Result<Vec<WorktreeInfo>, String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    let repo_root = repo_root(&git, repo_dir)?;
    let output = hidden_command(&git)
        .args(["-c", "core.quotepath=false"])
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

/// Resolves the root directory of the git repository containing `dir`.
// Helper to discover the toplevel directory of a git repository.
#[allow(dead_code)]
pub fn repo_root(git: &Path, dir: &Path) -> Result<PathBuf, String> {
    let output = hidden_command(git)
        .args(["-c", "core.quotepath=false"])
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("Couldn't run git rev-parse: {e}"))?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(PathBuf::from(text.trim()))
    } else {
        Err(format!("{} isn't inside a git repository.", dir.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_worktree_output() {
        let sample = "\
worktree /path/to/main
HEAD 0123456789abcdef0123456789abcdef01234567
branch refs/heads/main

worktree /path/to/.viper/worktrees/42
HEAD fedcba9876543210fedcba9876543210fedcba98
branch refs/heads/viper/session-42

worktree /path/to/detached
HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
detached

worktree /path/to/locked
HEAD bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
branch refs/heads/locked-branch
locked reason for lock

worktree /path/to/bare
bare
";
        let list = parse_worktree_list(sample);
        assert_eq!(list.len(), 5, "all 5 worktrees should be parsed");

        assert_eq!(list[0].path, PathBuf::from("/path/to/main"));
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert!(!list[0].is_bare);
        assert!(!list[0].is_detached);
        assert!(!list[0].is_locked);

        assert_eq!(list[1].path, PathBuf::from("/path/to/.viper/worktrees/42"));
        assert_eq!(list[1].branch.as_deref(), Some("viper/session-42"));

        assert_eq!(list[2].path, PathBuf::from("/path/to/detached"));
        assert!(list[2].is_detached);
        assert_eq!(list[2].branch, None);

        assert_eq!(list[3].path, PathBuf::from("/path/to/locked"));
        assert!(list[3].is_locked);
        assert_eq!(list[3].branch.as_deref(), Some("locked-branch"));

        assert_eq!(list[4].path, PathBuf::from("/path/to/bare"));
        assert!(list[4].is_bare);
    }

    #[test]
    fn path_and_branch_formatting() {
        let repo = Path::new(r"C:\work\project");
        let path = worktree_path(repo, 7);
        assert_eq!(path, PathBuf::from(r"C:\work\project\.viper\worktrees\7"));

        let branch = worktree_branch(7);
        assert_eq!(branch, "viper/session-7");
    }

    #[test]
    fn parse_worktree_list_handles_empty_and_junk_inputs() {
        assert!(parse_worktree_list("").is_empty(), "empty input yields empty list");
        assert!(parse_worktree_list("not porcelain output\nsome random noise").is_empty(), "junk yields empty list");
    }

    fn init_temp_git_repo(prefix: &str) -> (PathBuf, PathBuf) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("viper-{prefix}-{}-{}", std::process::id(), unique));
        std::fs::create_dir_all(&temp_dir).expect("temp dir creates");

        let git = git_executable().expect("git is installed");
        let run = |args: &[&str]| {
            let out = hidden_command(&git)
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "core.quotepath=false"])
                .args(args)
                .current_dir(&temp_dir)
                .output()
                .expect("git succeeds");
            assert!(out.status.success(), "git command {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
        };

        run(&["init", "-b", "main"]);
        std::fs::write(temp_dir.join("initial.txt"), "hello world\n").expect("write initial file");
        run(&["add", "initial.txt"]);
        run(&["commit", "-m", "Initial commit"]);

        (temp_dir, git)
    }

    #[test]
    fn creates_and_lists_isolated_worktree_in_temp_repo() {
        let (repo_dir, git) = init_temp_git_repo("create");

        let (wt_path, branch) = create_worktree(&repo_dir, 1, None, Some("main"))
            .expect("worktree creation should succeed");

        assert_eq!(branch, "viper/session-1", "branch name matches standard naming");
        assert!(wt_path.exists(), "worktree path exists on disk");
        assert!(wt_path.join("initial.txt").exists(), "worktree contains checkout files");

        let worktrees = list_worktrees(&repo_dir).expect("list worktrees succeeds");
        assert!(worktrees.iter().any(|w| w.branch.as_deref() == Some("viper/session-1")), "new branch listed");

        let status = hidden_command(&git)
            .args(["status", "--porcelain"])
            .current_dir(&repo_dir)
            .output()
            .expect("git status succeeds");
        let status_text = String::from_utf8_lossy(&status.stdout);
        assert!(!status_text.contains(".viper"), "status in root repo must not show .viper: {status_text}");

        let _ = remove_worktree(&repo_dir, &wt_path, true);
        let _ = delete_branch(&repo_dir, &branch, true);
        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn session_worktree_modifications_isolate_from_main_repo() {
        use crate::git_diff::branch_changes;

        let (repo_dir, _) = init_temp_git_repo("isolate");

        let (wt_path, branch) = create_worktree(&repo_dir, 2, None, Some("main"))
            .expect("worktree creation should succeed");

        let worktree_file = wt_path.join("agent_output.txt");
        std::fs::write(&worktree_file, "isolated agent modifications\n").expect("write worktree file");

        assert!(!repo_dir.join("agent_output.txt").exists(), "root repository must remain pristine");

        let diffs = branch_changes(&wt_path, "main").expect("branch changes succeeds");
        let found = diffs.iter().any(|d| d.path == "agent_output.txt");
        assert!(found, "branch diff in worktree detects newly added file");

        let _ = remove_worktree(&repo_dir, &wt_path, true);
        let _ = delete_branch(&repo_dir, &branch, true);
        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn cleans_up_worktree_and_deletes_branch_cleanly() {
        let (repo_dir, _) = init_temp_git_repo("cleanup");

        let (wt_path, branch) = create_worktree(&repo_dir, 3, None, Some("main"))
            .expect("worktree creation should succeed");
        assert!(wt_path.exists());

        remove_worktree(&repo_dir, &wt_path, true).expect("remove worktree succeeds");
        assert!(!wt_path.exists(), "worktree folder removed from disk");

        delete_branch(&repo_dir, &branch, true).expect("delete branch succeeds");

        let worktrees = list_worktrees(&repo_dir).expect("list worktrees succeeds");
        assert!(!worktrees.iter().any(|w| w.branch.as_deref() == Some("viper/session-3")), "worktree pruned from list");

        let _ = std::fs::remove_dir_all(&repo_dir);
    }
}
