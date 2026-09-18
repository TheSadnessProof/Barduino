//! The left-hand panel: the session list grouped by project, with a search filter,
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

/// The operational state of a session.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SessionState {
    Running,
    WaitingForApproval,
    Failed,
    Idle,
}

/// Computes the operational state of a session from its running status and entries.
pub fn session_state(session: &Session) -> SessionState {
    if session.has_pending_approval() {
        SessionState::WaitingForApproval
    } else if session.is_running() {
        SessionState::Running
    } else if matches!(session.entries.last(), Some(Entry::Error(_))) {
        SessionState::Failed
    } else {
        SessionState::Idle
    }
}

/// Compact subtext describing the session provider, model, or turns.
pub fn session_detail(session: &Session) -> String {
    let provider_name = session.provider.short_name();
    if let Some(model) = &session.chosen_model {
        format!("{provider_name} · {model}")
    } else {
        let agent_turns = session
            .entries
            .iter()
            .filter(|e| matches!(e, Entry::Agent(_)))
            .count();
        if agent_turns > 1 {
            format!("{provider_name} · {agent_turns} turns")
        } else if agent_turns == 1 {
            format!("{provider_name} · 1 turn")
        } else {
            provider_name.to_string()
        }
    }
}

/// The background tint for a session card in the sidebar.
pub fn session_card_fill(selected: bool, hovered: bool, dark_mode: bool) -> egui::Color32 {
    if selected {
        if dark_mode {
            egui::Color32::from_white_alpha(20)
        } else {
            egui::Color32::from_black_alpha(16)
        }
    } else if hovered {
        if dark_mode {
            egui::Color32::from_white_alpha(10)
        } else {
            egui::Color32::from_black_alpha(8)
        }
    } else {
        egui::Color32::TRANSPARENT
    }
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
            // Whatever is in flight was read before the agent finished, so its
            // answer is already stale. Dropping the slot here means the next ask
            // replaces it rather than leaving two reads racing.
            summary.loading = None;
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

            let workspaces = group_by_workspace(sessions, "");
            if !workspaces.is_empty() {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);

                for workspace in workspaces {
                    let initial = workspace_initials(&workspace.name);
                    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(26.0), egui::Sense::click());
                    if ui.is_rect_visible(rect) {
                        let has_working = workspace.sessions.iter().any(|s| s.is_running());
                        let has_failed = workspace
                            .sessions
                            .iter()
                            .any(|s| !s.is_running() && matches!(s.entries.last(), Some(Entry::Error(_))));

                        let fill = if response.hovered() {
                            ui.visuals().widgets.hovered.bg_fill
                        } else {
                            ui.visuals().widgets.inactive.bg_fill
                        };
                        ui.painter().rect_filled(rect, 5.0, fill);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            &initial,
                            egui::FontId::proportional(11.5),
                            ui.visuals().text_color(),
                        );
                        if has_failed {
                            let dot = egui::pos2(rect.right() - 2.0, rect.top() + 2.0);
                            ui.painter().circle_filled(dot, 3.5, ui.visuals().error_fg_color);
                        } else if has_working {
                            let dot = egui::pos2(rect.right() - 2.0, rect.top() + 2.0);
                            ui.painter().circle_filled(dot, 3.5, egui::Color32::from_rgb(34, 197, 94));
                        }
                    }
                    let count = workspace.sessions.len();
                    let tip = format!(
                        "{} ({} {})",
                        workspace.name,
                        count,
                        if count == 1 { "session" } else { "sessions" }
                    );
                    if response.on_hover_text(tip).clicked() {
                        action = SidebarAction::Expand;
                    }
                    ui.add_space(4.0);
                }
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

        egui::Panel::bottom(egui::Id::new("sessions_footer")).show_separator_line(true).show(ui, |ui| {
            ui.add_space(5.0);
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
            if !sessions.is_empty() {
                ui.label(egui::RichText::new(sessions.len().to_string()).small().weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Plus, "New session").clicked() {
                    action = SidebarAction::NewSession;
                }
            });
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            icons::paint(ui.painter(), icon_rect, Icon::Search, ui.visuals().weak_text_color());

            let field = egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Search…")
                .desired_width(f32::INFINITY);
            ui.add(field);
            if !self.filter.is_empty() && icons::small_button(ui, Icon::Close, "Clear search").clicked() {
                self.filter.clear();
            }
        });
        ui.add_space(4.0);

        let workspaces = group_by_workspace(sessions, &self.filter);
        // git is asked before the rows are drawn, so the closures below don't have
        // to borrow the sidebar twice.
        let summaries: Vec<Option<RepoSummary>> = workspaces
            .iter()
            .map(|workspace| workspace.dir.clone().and_then(|dir| self.summary(&dir, ui.ctx())))
            .collect();
        let filtering = !self.filter.is_empty();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if workspaces.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    let (icon_rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    icons::paint(ui.painter(), icon_rect, Icon::Search, ui.visuals().weak_text_color());
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("Nothing matches").weak());
                    if filtering && ui.small_button("Clear search").clicked() {
                        self.filter.clear();
                    }
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
                        ui.add_space(2.0);
                        if let Some(row_action) = self.row(ui, session, active == Some(session.id)) {
                            action = row_action;
                        }
                    }
                }
                ui.add_space(6.0);
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
                let hovered = response.hovered();
                let fill = session_card_fill(selected, hovered, ui.visuals().dark_mode);

                let card_frame = egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(9, 5));

                card_frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let state = session_state(session);

                    ui.horizontal(|ui| {
                        // Status dot: ONLY shown for active/pulsing or failed sessions.
                        // Idle sessions do not take up space or draw dots, maintaining Claude-like calm.
                        match state {
                            SessionState::Running => {
                                let (status_rect, _) =
                                    ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                                let center = status_rect.center();
                                let time = ui.input(|i| i.time);
                                let pulse = (time * 4.0).sin() as f32 * 0.5 + 0.5;
                                let outer_r = 3.0 + pulse * 2.0;
                                let alpha = ((1.0 - pulse) * 120.0) as u8;
                                ui.painter().circle_filled(
                                    center,
                                    outer_r,
                                    egui::Color32::from_rgba_unmultiplied(34, 197, 94, alpha),
                                );
                                ui.painter().circle_filled(center, 2.8, egui::Color32::from_rgb(34, 197, 94));
                                ui.ctx().request_repaint();
                            }
                            SessionState::WaitingForApproval => {
                                let (status_rect, _) =
                                    ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                                let center = status_rect.center();
                                let time = ui.input(|i| i.time);
                                let pulse = (time * 4.0).sin() as f32 * 0.5 + 0.5;
                                let outer_r = 3.0 + pulse * 2.0;
                                let alpha = ((1.0 - pulse) * 120.0) as u8;
                                ui.painter().circle_filled(
                                    center,
                                    outer_r,
                                    egui::Color32::from_rgba_unmultiplied(214, 158, 46, alpha),
                                );
                                ui.painter().circle_filled(center, 2.8, egui::Color32::from_rgb(214, 158, 46));
                                ui.ctx().request_repaint();
                            }
                            SessionState::Failed => {
                                let (status_rect, _) =
                                    ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                                let center = status_rect.center();
                                ui.painter().circle_filled(center, 2.8, ui.visuals().error_fg_color);
                            }
                            SessionState::Idle => {}
                        }

                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 1.5;

                            // Title row
                            let title_color = if selected {
                                ui.visuals().strong_text_color()
                            } else {
                                ui.visuals().text_color()
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&session.title)
                                        .color(title_color)
                                        .strong(),
                                )
                                .truncate(),
                            );

                            // Subtext row: provider, model / turns, and action button on hover
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let detail = session_detail(session);
                                let reserved_right = if hovered || selected { 22.0 } else { 0.0 };
                                let detail_width = (ui.available_width() - reserved_right).max(20.0);
                                ui.add_sized(
                                    egui::vec2(detail_width, 14.0),
                                    egui::Label::new(egui::RichText::new(detail).size(11.0).weak()).truncate(),
                                );

                                if hovered || selected {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let more = icons::small_button(ui, Icon::More, "Options");
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
            .inner_margin(egui::Margin::symmetric(9, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(format!("Delete “{}”?", session.title)).truncate());
                ui.add_space(4.0);
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

/// Computes a 1-character monogram for a project avatar on the collapsed rail.
pub fn workspace_initials(name: &str) -> String {
    let mut chars = name.chars().filter(|c| c.is_alphanumeric());
    chars.next().unwrap_or('?').to_uppercase().to_string()
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

/// Whether a session is worth showing while `needle` is typed in the search box.
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
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let chevron = if folded { Icon::ChevronRight } else { Icon::ChevronDown };
        let tip = if folded { "Show these sessions" } else { "Fold these sessions away" };
        if workspace.dir.is_some() && icons::small_button(ui, chevron, tip).clicked() {
            action = Some(Heading::Fold);
        }

        let (folder_rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        icons::paint(ui.painter(), folder_rect, Icon::Folder, ui.visuals().weak_text_color());

        let name = egui::RichText::new(&workspace.name).size(12.0).strong().color(ui.visuals().text_color());
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

    if let Some(summary) = summary {
        ui.add_space(1.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(16.0);

            let (branch_rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            icons::paint(ui.painter(), branch_rect, Icon::Branch, ui.visuals().weak_text_color());

            ui.add(
                egui::Label::new(egui::RichText::new(&summary.branch).size(11.0).weak())
                    .truncate()
                    .selectable(false),
            );

            if summary.changed > 0 {
                let text = if summary.changed == 1 {
                    "· 1 change".to_owned()
                } else {
                    format!("· {} changes", summary.changed)
                };
                let change_color = if ui.visuals().dark_mode {
                    egui::Color32::from_rgb(245, 158, 11)
                } else {
                    egui::Color32::from_rgb(180, 83, 9)
                };
                let changes_label = ui.add(
                    egui::Label::new(egui::RichText::new(text).size(11.0).color(change_color))
                        .selectable(false)
                        .sense(egui::Sense::click()),
                );
                if changes_label.on_hover_text("Show the uncommitted changes").clicked() {
                    action = Some(Heading::OpenChanges(workspace.dir.clone().unwrap_or_default()));
                }
            }
        });
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

    #[test]
    fn workspace_initials_extracts_first_letter() {
        assert_eq!(workspace_initials("Viper"), "V");
        assert_eq!(workspace_initials("frontend-app"), "F");
        assert_eq!(workspace_initials("123-service"), "1");
        assert_eq!(workspace_initials(""), "?");
    }

    #[test]
    fn session_state_reflects_running_and_error() {
        let mut s = session(1, "a");
        assert_eq!(session_state(&s), SessionState::Idle);

        s.entries.push(Entry::Error("failed to run".into()));
        assert_eq!(session_state(&s), SessionState::Failed);
    }

    #[test]
    fn session_state_reflects_waiting_for_approval_and_returns_to_idle() {
        use crate::agent::{ApprovalDecision, ApprovalRequest};
        let mut s = session(1, "a");
        assert_eq!(session_state(&s), SessionState::Idle);

        s.entries.push(Entry::Approval(ApprovalRequest::new("req-1", "bash", "ls", None)));
        assert_eq!(session_state(&s), SessionState::WaitingForApproval);

        assert!(s.resolve_approval("req-1", ApprovalDecision::Approved));
        assert_eq!(session_state(&s), SessionState::Idle);
    }

    #[test]
    fn session_detail_formats_provider_and_turns() {
        let mut s = session(1, "a");
        assert_eq!(session_detail(&s), "Claude");

        s.entries.push(Entry::Agent("hello".into()));
        assert_eq!(session_detail(&s), "Claude · 1 turn");

        s.entries.push(Entry::Agent("world".into()));
        assert_eq!(session_detail(&s), "Claude · 2 turns");

        s.chosen_model = Some("claude-3-7-sonnet".into());
        assert_eq!(session_detail(&s), "Claude · claude-3-7-sonnet");
    }

    #[test]
    fn session_card_fill_is_a_subtle_tint_not_solid_white() {
        // In dark mode, selected and hovered cards must use a subtle alpha tint,
        // never additive solid white.
        let selected_dark = session_card_fill(true, false, true);
        assert_ne!(selected_dark.r(), 255, "selected card must not max out red");
        assert_ne!(selected_dark.g(), 255, "selected card must not max out green");
        assert_ne!(selected_dark.b(), 255, "selected card must not max out blue");
        assert_eq!(selected_dark, egui::Color32::from_white_alpha(20));

        let hovered_dark = session_card_fill(false, true, true);
        assert_ne!(hovered_dark.r(), 255, "hovered card must not max out red");
        assert_eq!(hovered_dark, egui::Color32::from_white_alpha(10));

        let selected_light = session_card_fill(true, false, false);
        assert_eq!(selected_light, egui::Color32::from_black_alpha(16));

        let hovered_light = session_card_fill(false, true, false);
        assert_eq!(hovered_light, egui::Color32::from_black_alpha(8));

        let idle = session_card_fill(false, false, true);
        assert_eq!(idle, egui::Color32::TRANSPARENT);
    }
}
