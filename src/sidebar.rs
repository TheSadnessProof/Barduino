//! The left-hand panel: the session list grouped by project, with a filter,
//! rename and delete, and a count on the rail of what the closed panel hides.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;

use crate::git_diff::{self, RepoSummary};
use crate::icons::{self, Icon};
use crate::session::{Entry, Session};

type Loaded = Arc<Mutex<Option<Result<RepoSummary, String>>>>;

/// What git says about one project, asked for in the background.
#[derive(Default)]
struct Summary {
    /// Set while git is being asked.
    loading: Option<Loaded>,
    /// The last answer. None for a folder that isn't a repository.
    value: Option<RepoSummary>,
    /// Set once git has been asked, so a folder it can't answer for isn't asked
    /// again on every frame.
    asked: bool,
}

/// Sidebar state that only lasts while the app is open.
#[derive(Default)]
pub struct Sidebar {
    /// The session being renamed, and the name typed so far.
    renaming: Option<(u64, String)>,
    /// The session waiting for the user to confirm it should be deleted.
    confirm_delete: Option<u64>,
    /// What the user typed in the filter box.
    filter: String,
    /// Projects whose sessions are folded away.
    folded: BTreeSet<PathBuf>,
    /// Each project's branch and changed-file count.
    summaries: BTreeMap<PathBuf, Summary>,
}

pub enum SidebarAction {
    None,
    Select(u64),
    NewSession,
    Rename(u64, String),
    Delete(u64),
    OpenSettings,
    Collapse,
    Expand,
    /// Start a session in this project's folder.
    NewSessionIn(PathBuf),
    /// Show this project's uncommitted changes.
    OpenChanges(PathBuf),
}

/// How many sessions are busy, and how many stopped and want looking at. The
/// rail shows these because a closed panel hides the rows that would say so.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Attention {
    pub working: usize,
    pub failed: usize,
}

/// Counts the sessions the user would want to know about without looking.
pub fn attention(sessions: &[Session]) -> Attention {
    let mut counts = Attention::default();
    for session in sessions {
        if session.is_running() {
            counts.working += 1;
        } else if matches!(session.entries.last(), Some(Entry::Error(_))) {
            counts.failed += 1;
        }
    }
    counts
}

impl Sidebar {
    /// Forgets what git said about `dir`, so it is asked again. Called after an
    /// agent has been working there.
    pub fn invalidate(&mut self, dir: &Path) {
        if let Some(summary) = self.summaries.get_mut(dir) {
            summary.asked = false;
        }
    }

    /// This project's branch and changed count, asking git the first time it's needed.
    fn summary(&mut self, dir: &Path, ctx: &egui::Context) -> Option<RepoSummary> {
        let entry = self.summaries.entry(dir.to_owned()).or_default();
        let finished =
            entry.loading.as_ref().and_then(|slot| slot.lock().unwrap_or_else(PoisonError::into_inner).take());
        if let Some(answer) = finished {
            entry.value = answer.ok();
            entry.loading = None;
        }
        if !entry.asked {
            entry.asked = true;
            let slot: Loaded = Arc::new(Mutex::new(None));
            let (result, dir, ctx) = (Arc::clone(&slot), dir.to_owned(), ctx.clone());
            std::thread::spawn(move || {
                *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(git_diff::summary(&dir));
                ctx.request_repaint();
            });
            entry.loading = Some(slot);
        }
        entry.value.clone()
    }

    /// The thin strip shown while the sidebar is hidden.
    pub fn rail(&mut self, ui: &mut egui::Ui, sessions: &[Session]) -> SidebarAction {
        let mut action = SidebarAction::None;
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            let show = icons::button(ui, Icon::SidebarLeft, "Show sessions");
            // The rows that would show a spinner or an error are hidden, so the
            // count goes on the button that brings them back.
            let counts = attention(sessions);
            if counts.failed > 0 {
                badge(ui, show.rect, counts.failed, ui.visuals().error_fg_color);
            } else {
                badge(ui, show.rect, counts.working, ui.visuals().selection.bg_fill);
            }
            if show.clicked() {
                action = SidebarAction::Expand;
            }
            if icons::button(ui, Icon::Plus, "New session").clicked() {
                action = SidebarAction::NewSession;
            }
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(6.0);
            if icons::button(ui, Icon::Settings, "Settings").clicked() {
                action = SidebarAction::OpenSettings;
            }
        });
        action
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        sessions: &[Session],
        active: Option<u64>,
        settings_open: bool,
    ) -> SidebarAction {
        let mut action = SidebarAction::None;

        egui::Panel::bottom(egui::Id::new("sessions_footer")).show_separator_line(false).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if icons::toggle(ui, Icon::Settings, "Settings", settings_open).clicked() {
                    action = SidebarAction::OpenSettings;
                }
                if ui.selectable_label(settings_open, "Settings").clicked() {
                    action = SidebarAction::OpenSettings;
                }
            });
            ui.add_space(4.0);
        });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if icons::button(ui, Icon::SidebarLeft, "Hide sessions").clicked() {
                action = SidebarAction::Collapse;
            }
            ui.label(egui::RichText::new("Sessions").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Plus, "New session").clicked() {
                    action = SidebarAction::NewSession;
                }
            });
        });

        // Worth the room once there are more sessions than fit on screen.
        if sessions.len() > 6 || !self.filter.is_empty() {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field = egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter")
                    .desired_width(f32::INFINITY);
                ui.add(field);
                if !self.filter.is_empty() && icons::small_button(ui, Icon::Close, "Clear the filter").clicked() {
                    self.filter.clear();
                }
            });
        }
        ui.add_space(4.0);

        let workspaces = group_by_workspace(sessions, &self.filter);
        // git is asked before the rows are drawn, so the closures below don't have
        // to borrow the sidebar twice.
        let summaries: Vec<Option<RepoSummary>> = workspaces
            .iter()
            .map(|workspace| workspace.dir.clone().and_then(|dir| self.summary(&dir, ui.ctx())))
            .collect();
        // Folding away a project would hide what the filter just found.
        let filtering = !self.filter.is_empty();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if workspaces.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Nothing matches").weak());
                });
            }
            for (workspace, summary) in workspaces.into_iter().zip(summaries) {
                let folded = !filtering && workspace.dir.as_ref().is_some_and(|dir| self.folded.contains(dir));
                if let Some(heading) = workspace_heading(ui, &workspace, summary.as_ref(), folded) {
                    match heading {
                        Heading::Fold => {
                            if let Some(dir) = &workspace.dir {
                                if folded {
                                    self.folded.remove(dir);
                                } else {
                                    self.folded.insert(dir.clone());
                                }
                            }
                        }
                        Heading::NewSession(dir) => action = SidebarAction::NewSessionIn(dir),
                        Heading::OpenChanges(dir) => action = SidebarAction::OpenChanges(dir),
                    }
                }
                if !folded {
                    for session in workspace.sessions {
                        if let Some(row_action) = self.row(ui, session, active == Some(session.id)) {
                            action = row_action;
                        }
                    }
                }
                ui.add_space(8.0);
            }
        });
        action
    }

    fn row(&mut self, ui: &mut egui::Ui, session: &Session, selected: bool) -> Option<SidebarAction> {
        if let Some((id, name)) = &mut self.renaming
            && *id == session.id
        {
            return rename_row(ui, session.id, name).map(|done| {
                self.renaming = None;
                match done {
                    Some(name) => SidebarAction::Rename(session.id, name),
                    None => SidebarAction::None,
                }
            });
        }
        if self.confirm_delete == Some(session.id) {
            return self.confirm_delete_row(ui, session);
        }

        let mut action = None;
        let row = ui.scope_builder(
            egui::UiBuilder::new().id_salt(("session_row", session.id)).sense(egui::Sense::click()),
            |ui| {
                let response = ui.response();
                let visuals = ui.style().interact_selectable(&response, selected);
                // `ui.response()` already reports this row's own rect from the last pass.
                // Falling back to `max_rect` would cover everything below it, so hovering
                // one row revealed the menu button on every row above it too.
                let hovered = response.hovered();
                let fill = if selected || hovered { visuals.weak_bg_fill } else { egui::Color32::TRANSPARENT };

                egui::Frame::new().fill(fill).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if session.is_running() {
                            ui.spinner();
                        }
                        ui.vertical(|ui| {
                            ui.set_width((ui.available_width() - 30.0).max(40.0));
                            ui.add(
                                egui::Label::new(egui::RichText::new(&session.title).color(visuals.text_color()))
                                    .truncate()
                                    .selectable(false),
                            );
                            let failed = !session.is_running() && matches!(session.entries.last(), Some(Entry::Error(_)));
                            let mut detail = session.provider.short_name().to_owned();
                            if failed {
                                detail.push_str(" · error");
                            }
                            ui.add(egui::Label::new(egui::RichText::new(detail).small().weak()).truncate().selectable(false));
                        });
                        // The menu button only shows while the row is hovered or selected, like Claude Code.
                        if hovered || selected {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let more = icons::button(ui, Icon::More, "Rename or delete");
                                egui::Popup::menu(&more).show(|ui| {
                                    if ui.button("Rename").clicked() {
                                        self.renaming = Some((session.id, session.title.clone()));
                                    }
                                    if ui.button("Delete").clicked() {
                                        self.confirm_delete = Some(session.id);
                                    }
                                });
                            });
                        }
                    });
                });
            },
        );

        let response = row.response;
        if response.double_clicked() {
            self.renaming = Some((session.id, session.title.clone()));
        } else if response.clicked() {
            action = Some(SidebarAction::Select(session.id));
        }
        egui::Popup::context_menu(&response).show(|ui| {
            if ui.button("Rename").clicked() {
                self.renaming = Some((session.id, session.title.clone()));
            }
            if ui.button("Delete").clicked() {
                self.confirm_delete = Some(session.id);
            }
        });
        action
    }

    fn confirm_delete_row(&mut self, ui: &mut egui::Ui, session: &Session) -> Option<SidebarAction> {
        let mut action = None;
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(format!("Delete “{}”?", session.title)).truncate());
                ui.horizontal(|ui| {
                    let delete = egui::Button::new(egui::RichText::new("Delete").color(ui.visuals().error_fg_color));
                    if ui.add(delete).clicked() {
                        self.confirm_delete = None;
                        action = Some(SidebarAction::Delete(session.id));
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_delete = None;
                    }
                });
            });
        action
    }
}

/// The sessions that share one project folder, newest first.
pub struct Workspace<'a> {
    /// The folder's name, or a stand-in for sessions that have none yet.
    pub name: String,
    /// The full path, for the tooltip. None while no folder has been chosen.
    pub path: Option<String>,
    /// The folder itself, for git and for starting a session in it.
    pub dir: Option<PathBuf>,
    pub sessions: Vec<&'a Session>,
}

/// Whether a session is worth showing while `needle` is typed in the filter box.
/// An empty needle keeps everything.
fn matches(session: &Session, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let needle = needle.to_lowercase();
    session.title.to_lowercase().contains(&needle)
        || session.folder_name().to_lowercase().contains(&needle)
        || session.project_dir.display().to_string().to_lowercase().contains(&needle)
        || session.provider.short_name().to_lowercase().contains(&needle)
}

/// Groups sessions by the folder they work in, so one project's work stays together.
/// Workspaces are ordered by their newest session, and so are the sessions inside them.
/// Only sessions matching `filter` are kept, and a workspace with none is dropped.
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

/// What the user did on a project's heading.
enum Heading {
    /// Fold the project away, or open it again.
    Fold,
    NewSession(PathBuf),
    OpenChanges(PathBuf),
}

/// The project above a group of sessions: its name, its branch and how much is
/// uncommitted, and buttons to start a session or show the changes.
fn workspace_heading(
    ui: &mut egui::Ui,
    workspace: &Workspace<'_>,
    summary: Option<&RepoSummary>,
    folded: bool,
) -> Option<Heading> {
    let mut action = None;
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let chevron = if folded { Icon::ChevronRight } else { Icon::ChevronDown };
        let tip = if folded { "Show these sessions" } else { "Fold these sessions away" };
        if workspace.dir.is_some() && icons::small_button(ui, chevron, tip).clicked() {
            action = Some(Heading::Fold);
        }

        let name = egui::RichText::new(&workspace.name).small().strong().color(ui.visuals().weak_text_color());
        let heading = ui.add(egui::Label::new(name).truncate().selectable(false).sense(egui::Sense::click()));
        match &workspace.path {
            Some(path) => {
                if heading.on_hover_text(path).clicked() {
                    action = Some(Heading::Fold);
                }
            }
            None => {
                heading.on_hover_text("These sessions still need a folder");
            }
        }

        if let Some(dir) = &workspace.dir {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::small_button(ui, Icon::Plus, "New session in this project").clicked() {
                    action = Some(Heading::NewSession(dir.clone()));
                }
            });
        }
    });

    // The branch and changed count go on their own line, which still reads at the
    // narrowest the panel gets.
    if let Some(summary) = summary {
        let changed = match summary.changed {
            0 => "no changes".to_owned(),
            1 => "1 change".to_owned(),
            n => format!("{n} changes"),
        };
        let text = format!("{} · {changed}", summary.branch);
        let label = egui::Label::new(egui::RichText::new(text).small().weak()).truncate().selectable(false);
        let response = ui.add(if summary.changed > 0 { label.sense(egui::Sense::click()) } else { label });
        if summary.changed > 0 && response.on_hover_text("Show the uncommitted changes").clicked() {
            action = Some(Heading::OpenChanges(workspace.dir.clone().unwrap_or_default()));
        }
    }
    ui.add_space(2.0);
    action
}

/// A small count over the corner of a button, for what a closed panel hides.
fn badge(ui: &egui::Ui, over: egui::Rect, count: usize, fill: egui::Color32) {
    if count == 0 {
        return;
    }
    let text = if count > 9 { "9+".to_owned() } else { count.to_string() };
    let centre = egui::pos2(over.right() - 4.0, over.top() + 4.0);
    ui.painter().circle_filled(centre, 7.0, fill);
    ui.painter().text(
        centre,
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(9.0),
        egui::Color32::WHITE,
    );
}

/// A text box for renaming. Returns `Some(Some(name))` when saved, `Some(None)`
/// when cancelled, and `None` while the user is still typing.
fn rename_row(ui: &mut egui::Ui, id: u64, name: &mut String) -> Option<Option<String>> {
    let edit_id = egui::Id::new(("rename_session", id));
    let response = ui.add(egui::TextEdit::singleline(name).id(edit_id).desired_width(f32::INFINITY));
    if !response.has_focus() && !response.lost_focus() {
        response.request_focus();
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Some(None);
    }
    if response.lost_focus() {
        let name = name.trim().to_owned();
        return Some((!name.is_empty()).then_some(name));
    }
    None
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::agent::{PermissionMode, Provider};

    fn session(id: u64, folder: &str) -> Session {
        Session::new(id, PathBuf::from(folder), Provider::Claude, PermissionMode::ReadOnly)
    }

    #[test]
    fn sessions_are_grouped_by_their_folder() {
        let sessions = vec![
            session(1, "C:\\work\\alpha"),
            session(2, "C:\\work\\beta"),
            session(3, "C:\\work\\alpha"),
        ];
        let workspaces = group_by_workspace(&sessions, "");
        // Beta holds the newest session that isn't alpha's, but alpha's newest is newer.
        let names: Vec<&str> = workspaces.iter().map(|workspace| workspace.name.as_str()).collect();
        assert_eq!(names, ["alpha", "beta"]);
        let alpha_ids: Vec<u64> = workspaces[0].sessions.iter().map(|session| session.id).collect();
        assert_eq!(alpha_ids, [3, 1], "newest first inside a workspace");
        assert_eq!(workspaces[0].path.as_deref(), Some("C:\\work\\alpha"));
        assert_eq!(workspaces[0].dir, Some(PathBuf::from("C:\\work\\alpha")));
    }

    #[test]
    fn sessions_without_a_folder_share_one_group() {
        let sessions = vec![session(1, ""), session(2, "C:\\work\\alpha"), session(3, "")];
        let workspaces = group_by_workspace(&sessions, "");
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].path, None, "the newest session has no folder yet");
        assert_eq!(workspaces[0].sessions.len(), 2);
        assert_eq!(workspaces[0].name, crate::session::NO_FOLDER);
        assert_eq!(workspaces[0].dir, None, "so there is no folder for git or a new session");
    }

    #[test]
    fn the_filter_keeps_matching_sessions_and_drops_empty_projects() {
        let mut sessions = vec![session(1, "C:\\work\\alpha"), session(2, "C:\\work\\beta")];
        sessions[0].title = "Fix the login form".into();
        sessions[1].title = "Rename a column".into();

        // The title matches, whatever the case.
        let found = group_by_workspace(&sessions, "LOGIN");
        assert_eq!(found.len(), 1, "only the project holding the match is left");
        assert_eq!(found[0].sessions[0].id, 1);

        // So does the folder, which is how you narrow to one project.
        let by_folder = group_by_workspace(&sessions, "beta");
        assert_eq!(by_folder.len(), 1);
        assert_eq!(by_folder[0].sessions[0].id, 2);

        assert!(group_by_workspace(&sessions, "nothing here").is_empty());
        assert_eq!(group_by_workspace(&sessions, "").len(), 2, "an empty filter keeps everything");
    }

    #[test]
    fn the_rail_counts_what_the_closed_panel_hides() {
        let mut sessions = vec![session(1, "a"), session(2, "a"), session(3, "a")];
        sessions[1].entries.push(Entry::Error("boom".into()));
        // A reply that finished cleanly needs nothing from the user.
        sessions[2].entries.push(Entry::Agent("done".into()));

        let counts = attention(&sessions);
        assert_eq!(counts, Attention { working: 0, failed: 1 });
        assert_eq!(attention(&[]), Attention::default(), "nothing to say about no sessions");
    }
}
