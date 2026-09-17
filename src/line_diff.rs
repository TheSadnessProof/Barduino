//! Works out what an agent's edit changed, so the chat can show it as a diff.
//! The agents hand over the text before and after rather than a diff, and the
//! files may not be on disk yet, so the comparison happens here.

use serde::{Deserialize, Serialize};

use crate::git_diff::{DiffLine, LineKind};

/// Lines of unchanged text kept either side of a change, so an edit reads in context.
const CONTEXT: usize = 2;

/// A file an agent's tool is about to change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEdit {
    pub path: String,
    /// The text being replaced, empty when the file is being written from scratch.
    pub old: String,
    pub new: String,
}

impl FileEdit {
    /// The change as diff lines, with a few lines of context around each edit.
    pub fn lines(&self) -> Vec<DiffLine> {
        let all = compare(&split(&self.old), &split(&self.new));
        trim_to_context(all)
    }

    /// How many lines the edit adds and removes.
    pub fn counts(&self) -> (usize, usize) {
        let lines = compare(&split(&self.old), &split(&self.new));
        let added = lines.iter().filter(|line| line.kind == LineKind::Added).count();
        let removed = lines.iter().filter(|line| line.kind == LineKind::Removed).count();
        (added, removed)
    }
}

/// Splits text into lines, treating "" as no lines rather than one empty one.
fn split(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect()
}

/// Lines up the two versions on their longest common subsequence, which is what
/// makes untouched lines show as context instead of a removal and an addition.
fn compare(old: &[&str], new: &[&str]) -> Vec<DiffLine> {
    let table = common_lengths(old, new);
    let mut lines = Vec::new();
    let (mut o, mut n) = (0, 0);
    while o < old.len() && n < new.len() {
        if old[o] == new[n] {
            lines.push(line(LineKind::Context, Some(o), Some(n), old[o]));
            o += 1;
            n += 1;
        } else if table[o + 1][n] >= table[o][n + 1] {
            lines.push(line(LineKind::Removed, Some(o), None, old[o]));
            o += 1;
        } else {
            lines.push(line(LineKind::Added, None, Some(n), new[n]));
            n += 1;
        }
    }
    for (index, text) in old[o..].iter().enumerate() {
        lines.push(line(LineKind::Removed, Some(o + index), None, text));
    }
    for (index, text) in new[n..].iter().enumerate() {
        lines.push(line(LineKind::Added, None, Some(n + index), text));
    }
    lines
}

/// `table[o][n]` is the length of the longest common subsequence of the lines
/// from `o` and `n` onwards, which is the usual way of walking a diff backwards.
fn common_lengths(old: &[&str], new: &[&str]) -> Vec<Vec<usize>> {
    let mut table = vec![vec![0; new.len() + 1]; old.len() + 1];
    for o in (0..old.len()).rev() {
        for n in (0..new.len()).rev() {
            table[o][n] = if old[o] == new[n] {
                table[o + 1][n + 1] + 1
            } else {
                table[o + 1][n].max(table[o][n + 1])
            };
        }
    }
    table
}

fn line(kind: LineKind, old_index: Option<usize>, new_index: Option<usize>, text: &str) -> DiffLine {
    DiffLine {
        kind,
        old_number: old_index.map(|index| index as u32 + 1),
        new_number: new_index.map(|index| index as u32 + 1),
        text: text.to_owned(),
    }
}

/// Drops the runs of context far from any change, leaving a marker in their place,
/// so replacing two lines of a long file doesn't print the whole file.
fn trim_to_context(lines: Vec<DiffLine>) -> Vec<DiffLine> {
    let changed: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.kind != LineKind::Context)
        .map(|(index, _)| index)
        .collect();
    if changed.is_empty() {
        return Vec::new();
    }

    let mut kept = Vec::new();
    let mut skipped = false;
    for (index, line) in lines.into_iter().enumerate() {
        let near_change = changed.iter().any(|at| index.abs_diff(*at) <= CONTEXT);
        if near_change {
            if skipped {
                kept.push(DiffLine {
                    kind: LineKind::NoNewline,
                    old_number: None,
                    new_number: None,
                    text: "…".to_owned(),
                });
                skipped = false;
            }
            kept.push(line);
        } else {
            skipped = true;
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str) -> FileEdit {
        FileEdit { path: "src/main.rs".into(), old: old.into(), new: new.into() }
    }

    fn shown(edit: &FileEdit) -> Vec<(LineKind, String)> {
        edit.lines().into_iter().map(|line| (line.kind, line.text)).collect()
    }

    #[test]
    fn unchanged_lines_stay_as_context() {
        let edit = edit("fn main() {\n    old();\n}", "fn main() {\n    new();\n}");
        assert_eq!(
            shown(&edit),
            vec![
                (LineKind::Context, "fn main() {".to_owned()),
                (LineKind::Removed, "    old();".to_owned()),
                (LineKind::Added, "    new();".to_owned()),
                (LineKind::Context, "}".to_owned()),
            ]
        );
        assert_eq!(edit.counts(), (1, 1));
    }

    #[test]
    fn a_new_file_is_all_additions() {
        let edit = edit("", "one\ntwo");
        assert_eq!(shown(&edit), vec![
            (LineKind::Added, "one".to_owned()),
            (LineKind::Added, "two".to_owned())
        ]);
        assert_eq!(edit.counts(), (2, 0));
    }

    #[test]
    fn line_numbers_follow_each_side() {
        let lines = edit("a\nb\nc", "a\nB\nc").lines();
        let removed = lines.iter().find(|line| line.kind == LineKind::Removed).expect("one removal");
        let added = lines.iter().find(|line| line.kind == LineKind::Added).expect("one addition");
        assert_eq!((removed.old_number, removed.new_number), (Some(2), None));
        assert_eq!((added.old_number, added.new_number), (None, Some(2)));
    }

    #[test]
    fn far_off_lines_are_left_out() {
        let old: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let new = old.replace("line 10", "line ten");
        let edit = edit(old.trim_end(), new.trim_end());
        let shown = shown(&edit);
        assert!(shown.iter().any(|(_, text)| text == "line 8"), "context is kept: {shown:?}");
        assert!(!shown.iter().any(|(_, text)| text == "line 2"), "distant lines are dropped: {shown:?}");
        assert!(shown.iter().any(|(kind, text)| *kind == LineKind::NoNewline && text == "…"));
        assert_eq!(edit.counts(), (1, 1));
    }

    #[test]
    fn an_edit_that_changes_nothing_shows_nothing() {
        assert!(edit("same\n", "same\n").lines().is_empty());
    }

    #[test]
    fn windows_line_endings_are_not_a_change() {
        assert!(edit("a\r\nb", "a\nb").lines().is_empty());
    }
}
