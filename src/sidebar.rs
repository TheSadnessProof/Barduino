//! The left-hand panel: the session list, with rename and delete.

use eframe::egui;

use crate::icons::{self, Icon};
use crate::session::{Entry, Session};

/// Sidebar state that only lasts while the app is open.
#[derive(Default)]
pub struct Sidebar {
    /// The session being renamed, and the name typed so far.
    renaming: Option<(u64, String)>,
    /// The session waiting for the user to confirm it should be deleted.
    confirm_delete: Option<u64>,
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
}

impl Sidebar {
    /// The thin strip shown while the sidebar is hidden.
    pub fn rail(&mut self, ui: &mut egui::Ui) -> SidebarAction {
        let mut action = SidebarAction::None;
        ui.add_space(6.0);
        ui.vertical_centered(|ui| {
            if icons::button(ui, Icon::SidebarLeft, "Show sessions").clicked() {
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
        ui.add_space(4.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Newest first.
            for session in sessions.iter().rev() {
                if let Some(row_action) = self.row(ui, session, active == Some(session.id)) {
                    action = row_action;
                }
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
                let hovered = response.hovered() || ui.rect_contains_pointer(ui.max_rect());
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
                            let mut detail = format!("{} · {}", session.folder_name(), session.provider.short_name());
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
