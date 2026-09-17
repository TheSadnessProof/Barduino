//! Reads changes from git: uncommitted changes in a project, or the differences
//! between any two files.

use std::path::{Path, PathBuf};

use crate::agent::{self, hidden_command};

/// Files bigger than this are listed but not shown line by line.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// The id git uses for an empty tree, for comparing against a repo with no commits yet.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    /// A new file git doesn't track yet.
    Untracked,
}

impl FileStatus {
    pub fn letter(self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "U",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Modified => "Modified",
            Self::Added => "Added",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::Untracked => "New, not tracked by git yet",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// Git's "\ No newline at end of file" marker.
    NoNewline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_number: Option<u32>,
    pub new_number: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hunk {
    /// The "@@ -1,4 +1,5 @@ fn main()" line.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileDiff {
    pub path: String,
    /// The previous path, for renamed files.
    pub old_path: Option<String>,
    pub status: FileStatus,
    /// Why the file has no line-by-line diff, e.g. because it's binary.
    pub note: Option<String>,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    fn new(path: String, status: FileStatus) -> Self {
        Self { path, old_path: None, status, note: None, hunks: Vec::new() }
    }

    pub fn added(&self) -> usize {
        self.count(LineKind::Added)
    }

    pub fn removed(&self) -> usize {
        self.count(LineKind::Removed)
    }

    fn count(&self, kind: LineKind) -> usize {
        self.hunks.iter().flat_map(|hunk| &hunk.lines).filter(|line| line.kind == kind).count()
    }
}

/// Uncommitted changes (staged or not) in the git repository containing `dir`,
/// plus files git doesn't track yet.
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

/// A project at a glance, for the heading above its sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSummary {
    /// The branch name, or "detached" when HEAD isn't on one.
    pub branch: String,
    /// How many files differ from the last commit, untracked ones included.
    pub changed: usize,
}

/// Reads the branch and changed-file count of the repository containing `dir`.
/// This shells out to git twice, so it belongs on a background thread.
pub fn summary(dir: &Path) -> Result<RepoSummary, String> {
    let git = git_executable().ok_or("Git isn't installed.")?;
    run_git(&git, dir, &["rev-parse", "--is-inside-work-tree"])
        .map_err(|_| format!("{} isn't inside a git repository.", dir.display()))?;
    let branch = run_git(&git, dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let status = run_git(&git, dir, &["status", "--porcelain"])?;
    Ok(parse_summary(&branch, &status))
}

/// Splits the two git answers into a summary. Separate so it can be tested
/// without a repository on disk.
fn parse_summary(branch: &str, status: &str) -> RepoSummary {
    let branch = branch.trim();
    // A detached HEAD answers "HEAD", which says nothing useful on its own.
    let branch = if branch.is_empty() || branch == "HEAD" { "detached" } else { branch };
    RepoSummary { branch: branch.to_owned(), changed: status.lines().filter(|line| !line.trim().is_empty()).count() }
}

/// The differences between two files anywhere on disk.
pub fn compare_files(old: &Path, new: &Path) -> Result<Vec<FileDiff>, String> {
    let git = git_executable().ok_or("Git isn't installed, so files can't be compared.")?;
    let (old_text, new_text) = (old.display().to_string(), new.display().to_string());
    // `git diff --no-index` exits with 1 when the files differ, so both 0 and 1 are fine.
    let output = hidden_command(&git)
        .args(["-c", "core.quotepath=false", "diff", "--no-index", "--no-color", "--no-ext-diff", "--"])
        .args([&old_text, &new_text])
        .output()
        .map_err(|err| format!("Couldn't run git: {err}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let mut files = parse(&String::from_utf8_lossy(&output.stdout));
    // Show the paths as chosen, not git's a/ and b/ versions of them.
    for file in &mut files {
        file.path = new_text.clone();
        file.old_path = Some(old_text.clone());
        file.status = FileStatus::Modified;
    }
    Ok(files)
}

fn git_executable() -> Option<PathBuf> {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    agent::find_on_path(&[name]).or_else(|| {
        let program_files = std::env::var_os("ProgramFiles")?;
        Some(PathBuf::from(program_files).join("Git").join("cmd").join(name)).filter(|git| git.is_file())
    })
}

fn run_git(git: &Path, dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = hidden_command(git)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|err| format!("Couldn't run git: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// An untracked file shown as if every line were added.
fn untracked_file(root: &Path, path: &str) -> FileDiff {
    let mut file = FileDiff::new(path.to_owned(), FileStatus::Untracked);
    let full = root.join(path);
    match std::fs::metadata(&full) {
        Ok(meta) if meta.len() > MAX_FILE_BYTES => {
            file.note = Some(format!("Too large to show ({} KB).", meta.len() / 1024));
        }
        Ok(_) => match std::fs::read(&full) {
            Ok(bytes) if bytes.contains(&0) => file.note = Some("Binary file.".to_owned()),
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                let lines: Vec<DiffLine> = text
                    .lines()
                    .zip(1..)
                    .map(|(line, number)| DiffLine {
                        kind: LineKind::Added,
                        old_number: None,
                        new_number: Some(number),
                        text: line.to_owned(),
                    })
                    .collect();
                if !lines.is_empty() {
                    let header = format!("@@ -0,0 +1,{} @@", lines.len());
                    file.hunks.push(Hunk { header, lines });
                }
            }
            Err(err) => file.note = Some(format!("Couldn't read the file: {err}")),
        },
        Err(err) => file.note = Some(format!("Couldn't read the file: {err}")),
    }
    file
}

/// Parses `git diff` output into files, hunks and numbered lines.
pub fn parse(diff: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    // Lines still expected in the current hunk, so content like "--- x" isn't mistaken for a header.
    let (mut old_left, mut new_left) = (0u32, 0u32);
    let (mut old_number, mut new_number) = (0u32, 0u32);

    for line in diff.lines() {
        let in_hunk = old_left > 0 || new_left > 0;
        let Some(file) = files.last_mut().filter(|_| !line.starts_with("diff --git ")) else {
            if let Some(rest) = line.strip_prefix("diff --git ") {
                files.push(FileDiff::new(path_from_header(rest), FileStatus::Modified));
                (old_left, new_left) = (0, 0);
            }
            continue;
        };

        // Git adds this marker after a hunk's last line, when the hunk is already complete.
        if line.starts_with("\\ ") {
            if let Some(hunk) = file.hunks.last_mut() {
                hunk.lines.push(DiffLine { kind: LineKind::NoNewline, old_number: None, new_number: None, text: line.to_owned() });
            }
            continue;
        }

        if in_hunk {
            let (kind, text) = match line.chars().next() {
                Some('+') => (LineKind::Added, &line[1..]),
                Some('-') => (LineKind::Removed, &line[1..]),
                Some(' ') => (LineKind::Context, &line[1..]),
                // An empty context line can lose its leading space.
                _ => (LineKind::Context, line),
            };
            let (old, new) = match kind {
                LineKind::Added => (None, Some(new_number)),
                LineKind::Removed => (Some(old_number), None),
                LineKind::Context => (Some(old_number), Some(new_number)),
                LineKind::NoNewline => (None, None),
            };
            match kind {
                LineKind::Added => (new_number, new_left) = (new_number + 1, new_left.saturating_sub(1)),
                LineKind::Removed => (old_number, old_left) = (old_number + 1, old_left.saturating_sub(1)),
                LineKind::Context => {
                    (old_number, old_left) = (old_number + 1, old_left.saturating_sub(1));
                    (new_number, new_left) = (new_number + 1, new_left.saturating_sub(1));
                }
                LineKind::NoNewline => {}
            }
            if let Some(hunk) = file.hunks.last_mut() {
                hunk.lines.push(DiffLine { kind, old_number: old, new_number: new, text: text.to_owned() });
            }
            continue;
        }

        if let Some((old_start, old_len, new_start, new_len)) = parse_hunk_header(line) {
            (old_number, old_left, new_number, new_left) = (old_start, old_len, new_start, new_len);
            file.hunks.push(Hunk { header: line.to_owned(), lines: Vec::new() });
        } else if line.starts_with("new file mode") {
            file.status = FileStatus::Added;
        } else if line.starts_with("deleted file mode") {
            file.status = FileStatus::Deleted;
        } else if let Some(from) = line.strip_prefix("rename from ") {
            file.status = FileStatus::Renamed;
            file.old_path = Some(from.to_owned());
        } else if let Some(to) = line.strip_prefix("rename to ") {
            file.path = to.to_owned();
        } else if let Some(path) = line.strip_prefix("+++ b/") {
            file.path = path.to_owned();
        } else if line.starts_with("--- a/") && file.status == FileStatus::Deleted {
            file.path = line["--- a/".len()..].to_owned();
        } else if line.starts_with("Binary files ") {
            file.note = Some("Binary file.".to_owned());
        }
    }
    files
}

/// The new path from "a/old b/new". Paths with spaces are fixed up later by the
/// "+++ b/" line when there is one.
fn path_from_header(rest: &str) -> String {
    match rest.rfind(" b/") {
        Some(index) => rest[index + 3..].to_owned(),
        None => rest.to_owned(),
    }
}

/// Reads "@@ -12,5 +12,7 @@ …" into (old start, old length, new start, new length).
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    let ranges = line.strip_prefix("@@ -")?.split(" @@").next()?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |text: &str| -> Option<(u32, u32)> {
        match text.split_once(',') {
            Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
            None => Some((text.parse().ok()?, 1)),
        }
    };
    let ((old_start, old_len), (new_start, new_len)) = (range(old)?, range(new)?);
    Some((old_start, old_len, new_start, new_len))
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    #[test]
    fn a_summary_counts_changed_files_and_names_the_branch() {
        let status = " M src/app.rs\n?? notes.txt\nA  src/new.rs\n";
        assert_eq!(parse_summary("prod\n", status), RepoSummary { branch: "prod".into(), changed: 3 });

        let clean = parse_summary("feature/icons\n", "");
        assert_eq!(clean, RepoSummary { branch: "feature/icons".into(), changed: 0 });

        // A detached HEAD answers with the word HEAD, which is no use as a label.
        assert_eq!(parse_summary("HEAD\n", "").branch, "detached");
        assert_eq!(parse_summary("", "").branch, "detached", "and so is no answer at all");

        // Trailing blank lines in git's output aren't files.
        assert_eq!(parse_summary("prod", " M a\n\n").changed, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
diff --git a/src/main.rs b/src/main.rs
index 1111111..2222222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,4 +1,5 @@ fn main() {
 use std::io;
--- old comment
+// new comment
+let x = 1;

 fn main() {}
diff --git a/old name.txt b/new name.txt
similarity index 90%
rename from old name.txt
rename to new name.txt
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 3333333..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
\\ No newline at end of file
diff --git a/logo.png b/logo.png
new file mode 100644
index 0000000..4444444
Binary files /dev/null and b/logo.png differ
";

    #[test]
    fn parses_modified_renamed_deleted_and_binary_files() {
        let files = parse(SAMPLE);
        assert_eq!(files.len(), 4);

        let main = &files[0];
        assert_eq!((main.path.as_str(), main.status), ("src/main.rs", FileStatus::Modified));
        assert_eq!((main.added(), main.removed()), (2, 1));
        let lines = &main.hunks[0].lines;
        // A removed line whose text starts with "-- " must not be read as a header.
        assert_eq!(lines[1], DiffLine { kind: LineKind::Removed, old_number: Some(2), new_number: None, text: "-- old comment".into() });
        assert_eq!(lines[3].new_number, Some(3));
        assert_eq!(lines[4], DiffLine { kind: LineKind::Context, old_number: Some(3), new_number: Some(4), text: String::new() });

        let renamed = &files[1];
        assert_eq!((renamed.path.as_str(), renamed.old_path.as_deref()), ("new name.txt", Some("old name.txt")));
        assert_eq!(renamed.status, FileStatus::Renamed);

        let gone = &files[2];
        assert_eq!((gone.path.as_str(), gone.status, gone.removed()), ("gone.txt", FileStatus::Deleted, 1));
        assert_eq!(gone.hunks[0].lines[1].kind, LineKind::NoNewline);

        let logo = &files[3];
        assert_eq!((logo.status, logo.note.as_deref()), (FileStatus::Added, Some("Binary file.")));
    }

    #[test]
    fn reads_hunk_headers() {
        assert_eq!(parse_hunk_header("@@ -12,5 +12,7 @@ fn x()"), Some((12, 5, 12, 7)));
        assert_eq!(parse_hunk_header("@@ -1 +0,0 @@"), Some((1, 1, 0, 0)));
        assert_eq!(parse_hunk_header("not a hunk"), None);
    }

    /// Runs the real git in a throwaway repository.
    #[test]
    fn reads_changes_from_a_real_repository() {
        let Some(git) = git_executable() else {
            eprintln!("git isn't installed; skipping");
            return;
        };
        let dir = std::env::temp_dir().join(format!("barduino-git-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        let git_in = |args: &[&str]| run_git(&git, &dir, args).unwrap();
        git_in(&["init", "--quiet"]);

        // Before the first commit there's no HEAD to compare against.
        std::fs::write(dir.join("src/app.txt"), "one\ntwo\n").unwrap();
        git_in(&["add", "."]);
        let before_commit = working_tree_changes(&dir).unwrap();
        assert_eq!(before_commit.len(), 1);
        assert_eq!((before_commit[0].status, before_commit[0].added()), (FileStatus::Added, 2));

        git_in(&["-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "--quiet", "-m", "first"]);
        std::fs::write(dir.join("src/app.txt"), "one\n2\nthree\n").unwrap();
        std::fs::write(dir.join("notes new.md"), "hello\n").unwrap();

        // Works from a subfolder, and paths stay relative to the repository root.
        let changes = working_tree_changes(&dir.join("src")).unwrap();
        let paths: Vec<(&str, FileStatus)> = changes.iter().map(|f| (f.path.as_str(), f.status)).collect();
        assert_eq!(paths, [("notes new.md", FileStatus::Untracked), ("src/app.txt", FileStatus::Modified)]);
        assert_eq!((changes[1].added(), changes[1].removed()), (2, 1));

        let compared = compare_files(&dir.join("notes new.md"), &dir.join("src/app.txt")).unwrap();
        assert_eq!(compared.len(), 1);
        assert!(compared[0].added() == 3 && compared[0].removed() == 1, "{compared:?}");

        let outside = std::env::temp_dir().join(format!("barduino-no-repo-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        // The temp folder itself could be inside a repo on some machines, so only check it doesn't panic.
        let _ = working_tree_changes(&outside);

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
