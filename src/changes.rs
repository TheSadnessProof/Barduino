//! The Changes tab: files changed in the project, and a colored diff of each.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui::{self, Color32, RichText};

use crate::git_diff::{self, FileDiff, FileStatus, LineKind};

/// What the tab compares.
#[derive(Clone, PartialEq)]
pub enum Source {
    /// Uncommitted changes in the project folder's git repository.
    Project(PathBuf),
    /// Branch worktree changes compared to a base ref.
    Branch {
        dir: PathBuf,
        branch: String,
        base: String,
    },
    /// Two files chosen by the user.
    Files { old: PathBuf, new: PathBuf },
}

type Loaded = Arc<Mutex<Option<Result<Vec<FileDiff>, String>>>>;

pub struct Changes {
    pub source: Source,
    files: Option<Result<Vec<FileDiff>, String>>,
    loading: Option<Loaded>,
    selected: Option<String>,
}

impl Changes {
    pub fn new(source: Source, ctx: &egui::Context) -> Self {
        let mut changes = Self { source, files: None, loading: None, selected: None };
        changes.refresh(ctx);
        changes
    }

    pub fn title(&self) -> String {
        match &self.source {
            Source::Project(_) => "Changes".to_owned(),
            Source::Branch { branch, .. } => format!("Changes ({branch})"),
            Source::Files { .. } => "Compare".to_owned(),
        }
    }

    /// Reloads in the background.
    pub fn refresh(&mut self, ctx: &egui::Context) {
        let slot: Loaded = Arc::new(Mutex::new(None));
        let (result, source, ctx) = (Arc::clone(&slot), self.source.clone(), ctx.clone());
        std::thread::spawn(move || {
            let files = match &source {
                Source::Project(dir) => git_diff::working_tree_changes(dir),
                Source::Branch { dir, base, .. } => git_diff::branch_changes(dir, base),
                Source::Files { old, new } => git_diff::compare_files(old, new),
            };
            *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(files);
            ctx.request_repaint();
        });
        self.loading = Some(slot);
    }

    /// Whether this tab shows the project in `dir`, so it should refresh after an agent works there.
    pub fn watches(&self, dir: &Path) -> bool {
        match &self.source {
            Source::Project(project) => project == dir || dir.starts_with(project),
            Source::Branch { dir: worktree, .. } => worktree == dir || worktree.starts_with(dir),
            Source::Files { .. } => false,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let finished = self.loading.as_ref().and_then(|slot| slot.lock().unwrap_or_else(PoisonError::into_inner).take());
        if let Some(files) = finished {
            self.files = Some(files);
            self.loading = None;
        }

        ui.horizontal(|ui| {
            match &self.source {
                Source::Project(dir) => {
                    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    ui.label(RichText::new(format!("Uncommitted changes in {name}")).strong())
                        .on_hover_text(dir.display().to_string());
                }
                Source::Branch { dir, branch, base } => {
                    ui.label(RichText::new(format!("Changes in {branch} vs {base}")).strong())
                        .on_hover_text(dir.display().to_string());
                }
                Source::Files { old, new } => {
                    ui.label(RichText::new("Comparing two files").strong())
                        .on_hover_text(format!("{}\n→ {}", old.display(), new.display()));
                }
            }
            if self.loading.is_some() {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(self.loading.is_none(), egui::Button::new("⟳ Refresh")).clicked() {
                    self.refresh(ui.ctx());
                }
                if let Some(Ok(files)) = &self.files {
                    let (added, removed) =
                        files.iter().fold((0, 0), |(a, r), file| (a + file.added(), r + file.removed()));
                    let noun = if files.len() == 1 { "file" } else { "files" };
                    ui.label(RichText::new(format!("−{removed}")).color(REMOVED_TEXT).monospace());
                    ui.label(RichText::new(format!("+{added}")).color(ADDED_TEXT).monospace());
                    ui.label(RichText::new(format!("{} {noun}", files.len())).weak());
                }
            });
        });
        ui.separator();

        let files = match &self.files {
            None => return,
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().warn_fg_color, error.as_str());
                return;
            }
            Some(Ok(files)) if files.is_empty() => {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("No changes").weak()));
                return;
            }
            Some(Ok(files)) => files,
        };

        // Keep the selection on the same file across refreshes.
        let selected_index = self
            .selected
            .as_ref()
            .and_then(|path| files.iter().position(|file| &file.path == path))
            .unwrap_or(0);

        let tab_id = ui.id();
        let mut clicked = None;
        if files.len() > 1 {
            egui::Panel::top(tab_id.with("file_list"))
                .resizable(true)
                .default_size((files.len() as f32 * 22.0 + 8.0).min(180.0))
                .size_range(44.0..=600.0)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        for (index, file) in files.iter().enumerate() {
                            if file_row(ui, file, index == selected_index).clicked() {
                                clicked = Some(file.path.clone());
                            }
                        }
                    });
                });
        }
        if clicked.is_some() {
            self.selected = clicked;
        }

        let file = &files[selected_index];
        diff_view(ui, file);
    }
}

const ADDED_TEXT: Color32 = Color32::from_rgb(87, 171, 90);
const REMOVED_TEXT: Color32 = Color32::from_rgb(229, 83, 75);

fn status_color(status: FileStatus) -> Color32 {
    match status {
        FileStatus::Added | FileStatus::Untracked => ADDED_TEXT,
        FileStatus::Deleted => REMOVED_TEXT,
        FileStatus::Modified => Color32::from_rgb(210, 160, 60),
        FileStatus::Renamed => Color32::from_rgb(90, 150, 230),
    }
}

fn file_row(ui: &mut egui::Ui, file: &FileDiff, selected: bool) -> egui::Response {
    let response = ui
        .horizontal(|ui| {
            ui.label(RichText::new(file.status.letter()).monospace().strong().color(status_color(file.status)))
                .on_hover_text(file.status.describe());
            let (dir, name) = match file.path.rsplit_once('/') {
                Some((dir, name)) => (format!("{dir}/"), name),
                None => (String::new(), file.path.as_str()),
            };
            let name = if selected { RichText::new(name).strong() } else { RichText::new(name) };
            ui.add(egui::Label::new(name).selectable(false));
            ui.add(egui::Label::new(RichText::new(dir).small().weak()).truncate().selectable(false));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if file.removed() > 0 {
                    ui.label(RichText::new(format!("−{}", file.removed())).small().monospace().color(REMOVED_TEXT));
                }
                if file.added() > 0 {
                    ui.label(RichText::new(format!("+{}", file.added())).small().monospace().color(ADDED_TEXT));
                }
            });
        })
        .response
        .interact(egui::Sense::click());
    if selected || response.hovered() {
        let fill = ui.visuals().selection.bg_fill.gamma_multiply(if selected { 0.35 } else { 0.15 });
        ui.painter().rect_filled(response.rect, 3.0, fill);
    }
    response
}

/// One row of the diff: a hunk header or a line of code.
enum Row<'a> {
    Header(&'a str),
    Line(&'a git_diff::DiffLine),
}

fn diff_view(ui: &mut egui::Ui, file: &FileDiff) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(file.status.letter()).monospace().strong().color(status_color(file.status)));
        let title = match &file.old_path {
            Some(old) if old != &file.path => format!("{old} → {}", file.path),
            _ => file.path.clone(),
        };
        ui.add(egui::Label::new(RichText::new(title).monospace()).truncate());
    });
    if let Some(note) = &file.note {
        ui.label(RichText::new(note).weak());
        return;
    }

    let rows: Vec<Row<'_>> = file
        .hunks
        .iter()
        .flat_map(|hunk| std::iter::once(Row::Header(&hunk.header)).chain(hunk.lines.iter().map(Row::Line)))
        .collect();
    let font = egui::FontId::monospace(12.5);
    let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 2.0;
    let widest = file.hunks.iter().flat_map(|h| &h.lines).map(|l| l.text.chars().count()).max().unwrap_or(0);
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    let gutter = char_width * 5.0;
    let content_width = (gutter * 2.0 + char_width * (widest as f32 + 3.0)).max(ui.available_width());

    let (added_bg, removed_bg) = if ui.visuals().dark_mode {
        (Color32::from_rgba_unmultiplied(46, 160, 67, 40), Color32::from_rgba_unmultiplied(248, 81, 73, 40))
    } else {
        (Color32::from_rgba_unmultiplied(46, 160, 67, 45), Color32::from_rgba_unmultiplied(248, 81, 73, 45))
    };
    let text_color = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();

    // Only the visible rows are drawn, so long diffs stay fast.
    egui::ScrollArea::both().id_salt(("diff", &file.path)).auto_shrink([false, false]).show_rows(
        ui,
        row_height,
        rows.len(),
        |ui, range| {
            ui.set_min_width(content_width);
            let top = ui.min_rect().top();
            let left = ui.min_rect().left();
            for (offset, row) in rows[range.clone()].iter().enumerate() {
                let y = top + (range.start + offset) as f32 * row_height;
                let rect = egui::Rect::from_min_size(egui::pos2(left, y), egui::vec2(content_width, row_height));
                let painter = ui.painter();
                let text_y = rect.center().y;
                match row {
                    Row::Header(header) => {
                        painter.rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                        painter.text(egui::pos2(left + 4.0, text_y), egui::Align2::LEFT_CENTER, *header, font.clone(), weak);
                    }
                    Row::Line(line) => {
                        let (bg, sign, color) = match line.kind {
                            LineKind::Added => (added_bg, "+", text_color),
                            LineKind::Removed => (removed_bg, "−", text_color),
                            LineKind::Context => (Color32::TRANSPARENT, " ", text_color),
                            LineKind::NoNewline => (Color32::TRANSPARENT, " ", weak),
                        };
                        painter.rect_filled(rect, 0.0, bg);
                        let number = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                        painter.text(egui::pos2(left + gutter - 6.0, text_y), egui::Align2::RIGHT_CENTER, number(line.old_number), font.clone(), weak);
                        painter.text(egui::pos2(left + gutter * 2.0 - 6.0, text_y), egui::Align2::RIGHT_CENTER, number(line.new_number), font.clone(), weak);
                        let code = format!("{sign} {}", line.text.replace('\t', "    "));
                        painter.text(egui::pos2(left + gutter * 2.0 + 4.0, text_y), egui::Align2::LEFT_CENTER, code, font.clone(), color);
                    }
                }
            }
            // Reserve the space the rows were painted in, so scrolling works.
            ui.allocate_space(egui::vec2(content_width, row_height * range.len() as f32));
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_source_title_and_watches_match_worktree_and_project() {
        let wt = PathBuf::from(r"C:\work\project\.viper\worktrees\42");
        let project = PathBuf::from(r"C:\work\project");
        let other = PathBuf::from(r"C:\work\other");

        let source = Source::Branch {
            dir: wt.clone(),
            branch: "viper/session-42".into(),
            base: "main".into(),
        };

        let changes = Changes {
            source,
            files: None,
            loading: None,
            selected: None,
        };

        assert_eq!(changes.title(), "Changes (viper/session-42)");
        assert!(changes.watches(&wt), "watches worktree path directly");
        assert!(changes.watches(&project), "watches root project directory enclosing worktree");
        assert!(!changes.watches(&other), "does not watch unrelated directory");
    }

    #[test]
    fn project_source_watches_project_and_nested_paths() {
        let project = PathBuf::from(r"C:\work\project");
        let nested = PathBuf::from(r"C:\work\project\src");
        let other = PathBuf::from(r"C:\work\other");

        let source = Source::Project(project.clone());
        let changes = Changes {
            source,
            files: None,
            loading: None,
            selected: None,
        };

        assert_eq!(changes.title(), "Changes");
        assert!(changes.watches(&project));
        assert!(changes.watches(&nested));
        assert!(!changes.watches(&other));
    }
}
