//! Works out what an agent's edit changed, so the chat can show it as a diff.
//! The agents hand over the text before and after rather than a diff, and the
//! files may not be on disk yet, so the comparison happens here.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::git_diff::{DiffLine, LineKind};

/// Lines of unchanged text kept either side of a change, so an edit reads in context.
const CONTEXT: usize = 2;

/// A file an agent's tool is about to change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEdit {
    pub path: String,
    /// The text being replaced, empty when the file is being written from scratch.
    pub old: String,
    pub new: String,
    /// Memoized diff lines and line addition/removal counts, so redrawing the UI
    /// does not recompute quadratic diff matrices every frame.
    #[serde(skip)]
    diff: OnceLock<(Vec<DiffLine>, (usize, usize))>,
}

impl PartialEq for FileEdit {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.old == other.old && self.new == other.new
    }
}

impl FileEdit {
    pub fn new(path: impl Into<String>, old: impl Into<String>, new: impl Into<String>) -> Self {
        let edit = Self {
            path: path.into(),
            old: old.into(),
            new: new.into(),
            diff: OnceLock::new(),
        };
        edit.compute();
        edit
    }

    fn compute(&self) -> &(Vec<DiffLine>, (usize, usize)) {
        self.diff.get_or_init(|| {
            let all = compare(&split(&self.old), &split(&self.new));
            let added = all.iter().filter(|line| line.kind == LineKind::Added).count();
            let removed = all.iter().filter(|line| line.kind == LineKind::Removed).count();
            let lines = trim_to_context(all);
            (lines, (added, removed))
        })
    }

    /// The change as diff lines, with a few lines of context around each edit.
    pub fn lines(&self) -> &[DiffLine] {
        &self.compute().0
    }

    /// How many lines the edit adds and removes.
    pub fn counts(&self) -> (usize, usize) {
        self.compute().1
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
///
/// Trims identical prefix and suffix lines before building the dynamic programming table,
/// reducing the matrix from the whole file to only the edited region.
fn compare(old: &[&str], new: &[&str]) -> Vec<DiffLine> {
    let mut prefix_len = 0;
    while prefix_len < old.len() && prefix_len < new.len() && old[prefix_len] == new[prefix_len] {
        prefix_len += 1;
    }

    if prefix_len == old.len() && prefix_len == new.len() {
        return (0..old.len())
            .map(|i| line(LineKind::Context, Some(i), Some(i), old[i]))
            .collect();
    }

    let mut suffix_len = 0;
    while suffix_len < old.len() - prefix_len
        && suffix_len < new.len() - prefix_len
        && old[old.len() - 1 - suffix_len] == new[new.len() - 1 - suffix_len]
    {
        suffix_len += 1;
    }

    let mut lines = Vec::with_capacity(old.len() + new.len());

    // 1. Context before the edit
    for (i, text) in old.iter().take(prefix_len).enumerate() {
        lines.push(line(LineKind::Context, Some(i), Some(i), text));
    }

    // 2. The edited region via dynamic programming LCS
    let old_mid = &old[prefix_len..old.len() - suffix_len];
    let new_mid = &new[prefix_len..new.len() - suffix_len];
    let table = common_lengths(old_mid, new_mid);
    let (mut o, mut n) = (0, 0);
    while o < old_mid.len() && n < new_mid.len() {
        if old_mid[o] == new_mid[n] {
            lines.push(line(
                LineKind::Context,
                Some(prefix_len + o),
                Some(prefix_len + n),
                old_mid[o],
            ));
            o += 1;
            n += 1;
        } else if table[o + 1][n] >= table[o][n + 1] {
            lines.push(line(LineKind::Removed, Some(prefix_len + o), None, old_mid[o]));
            o += 1;
        } else {
            lines.push(line(LineKind::Added, None, Some(prefix_len + n), new_mid[n]));
            n += 1;
        }
    }
    for (index, text) in old_mid[o..].iter().enumerate() {
        lines.push(line(LineKind::Removed, Some(prefix_len + o + index), None, text));
    }
    for (index, text) in new_mid[n..].iter().enumerate() {
        lines.push(line(LineKind::Added, None, Some(prefix_len + n + index), text));
    }

    // 3. Context after the edit
    let old_suffix_start = old.len() - suffix_len;
    let new_suffix_start = new.len() - suffix_len;
    for (i, text) in old[old_suffix_start..].iter().enumerate() {
        let old_idx = old_suffix_start + i;
        let new_idx = new_suffix_start + i;
        lines.push(line(LineKind::Context, Some(old_idx), Some(new_idx), text));
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
        FileEdit::new("src/main.rs", old, new)
    }

    fn shown(edit: &FileEdit) -> Vec<(LineKind, String)> {
        edit.lines().iter().map(|line| (line.kind, line.text.clone())).collect()
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
        let edit = edit("a\nb\nc", "a\nB\nc");
        let lines = edit.lines();
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

    #[test]
    fn large_files_with_small_changes_trim_prefix_and_suffix_instantly() {
        let old: String = (1..=3000).map(|n| format!("line {n}\n")).collect();
        let new = old.replace("line 1500", "line fifteen hundred");
        let edit = edit(&old, &new);
        assert_eq!(edit.counts(), (1, 1));
        let lines = edit.lines();
        let changed = lines.iter().find(|l| l.kind == LineKind::Added).expect("has addition");
        assert_eq!(changed.text, "line fifteen hundred");
    }

    #[test]
    fn diff_memoization_caches_lines_and_counts() {
        let edit = edit("old\n", "new\n");
        let counts1 = edit.counts();
        let counts2 = edit.counts();
        assert_eq!(counts1, counts2);
        assert_eq!(edit.lines().len(), edit.lines().len());
    }

    #[test]
    fn pure_insertions_and_deletions_with_identical_context_preserve_line_numbers() {
        // Insertion in the middle
        let edit_ins = edit("head\ntail", "head\ninserted\ntail");
        assert_eq!(edit_ins.counts(), (1, 0));
        let ins_lines = edit_ins.lines();
        let added = ins_lines.iter().find(|l| l.kind == LineKind::Added).expect("has addition");
        assert_eq!(added.text, "inserted");
        assert_eq!(added.new_number, Some(2));
        assert_eq!(added.old_number, None);

        // Deletion in the middle
        let edit_del = edit("head\ndeleted\ntail", "head\ntail");
        assert_eq!(edit_del.counts(), (0, 1));
        let del_lines = edit_del.lines();
        let removed = del_lines.iter().find(|l| l.kind == LineKind::Removed).expect("has removal");
        assert_eq!(removed.text, "deleted");
        assert_eq!(removed.old_number, Some(2));
        assert_eq!(removed.new_number, None);
    }

    #[test]
    fn prefix_and_suffix_trimming_handles_multibyte_unicode() {
        let old = "ქართული ტექსტი 1\nძველი ხაზი\nქართული ტექსტი 2";
        let new = "ქართული ტექსტი 1\nახალი ხაზი\nქართული ტექსტი 2";
        let edit = edit(old, new);
        assert_eq!(edit.counts(), (1, 1));
        let changed = edit.lines().iter().find(|l| l.kind == LineKind::Added).expect("added line");
        assert_eq!(changed.text, "ახალი ხაზი");
    }

    #[test]
    fn diff_deserialized_from_saved_state_computes_on_demand() {
        let json = r#"{"path":"test.rs","old":"one\ntwo","new":"one\nTWO"}"#;
        let edit: FileEdit = serde_json::from_str(json).expect("deserializes without diff field");
        assert_eq!(edit.counts(), (1, 1));
        assert_eq!(edit.lines().len(), 3);
    }
}
