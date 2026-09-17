//! The middle column: a slim top bar, the conversation and the message box.

use eframe::egui;

use crate::agent::{PermissionMode, Provider};
use crate::session::{Entry, Session};
use crate::settings::Settings;

pub enum ComposerAction {
    None,
    Send,
    Stop,
    ChangeFolder,
}

pub fn top_bar(ui: &mut egui::Ui, session: &Session, show_sessions: &mut bool, show_tools: &mut bool) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.toggle_value(show_sessions, "Sessions").on_hover_text("Show or hide the session list");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(show_tools, "Tools").on_hover_text("Show or hide the terminal and browser");
            if let Some(model) = &session.model {
                ui.label(egui::RichText::new(model).small().weak());
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(&session.title).strong()).truncate());
            });
        });
    });
    ui.add_space(4.0);
}

/// The message box, with the agent, permission and folder pickers along its bottom edge.
pub fn composer(ui: &mut egui::Ui, session: &mut Session, settings: &Settings, agent_installed: bool) -> ComposerAction {
    let composer_id = egui::Id::new(("composer", session.id));
    // Take Enter before the text box sees it; Shift+Enter still adds a new line.
    let enter_pressed = ui.memory(|m| m.has_focus(composer_id))
        && ui.input_mut(|i| !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let can_send = agent_installed && !session.is_running() && !session.input.trim().is_empty();
    let mut action = if enter_pressed && can_send { ComposerAction::Send } else { ComposerAction::None };

    ui.add_space(8.0);
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            let response = ui.add(
                egui::TextEdit::multiline(&mut session.input)
                    .id(composer_id)
                    .frame(egui::Frame::NONE)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY)
                    .hint_text(format!(
                        "Ask {}…  (Enter to send, Shift+Enter for a new line)",
                        session.provider.short_name()
                    )),
            );
            if std::mem::take(&mut session.focus_composer) {
                response.request_focus();
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                provider_picker(ui, session, settings);
                permission_picker(ui, session);
                let folder = ui
                    .add_enabled(!session.is_running(), egui::Button::new(format!("📁 {}", session.folder_name())).small())
                    .on_hover_text(format!("{}\nClick to choose another folder", session.project_dir.display()));
                if folder.clicked() {
                    action = ComposerAction::ChangeFolder;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if session.is_running() {
                        if ui.button("Stop").clicked() {
                            action = ComposerAction::Stop;
                        }
                        ui.label(egui::RichText::new(format!("{} is working…", session.provider.short_name())).weak());
                        ui.spinner();
                    } else if ui.add_enabled(can_send, egui::Button::new("Send")).clicked() {
                        action = ComposerAction::Send;
                    }
                });
            });
        });
    ui.add_space(8.0);
    action
}

fn provider_picker(ui: &mut egui::Ui, session: &mut Session, settings: &Settings) {
    ui.add_enabled_ui(session.can_change_provider(), |ui| {
        egui::ComboBox::from_id_salt(("provider", session.id))
            .selected_text(session.provider.label())
            .show_ui(ui, |ui| {
                let mut choices: Vec<Provider> = settings.enabled_providers().collect();
                if !choices.contains(&session.provider) {
                    choices.push(session.provider);
                }
                for provider in choices {
                    ui.selectable_value(&mut session.provider, provider, provider.label());
                }
            });
    })
    .response
    .on_hover_text("Which agent answers in this session")
    .on_disabled_hover_text("A conversation stays with one agent. Start a new session to use another.");
}

fn permission_picker(ui: &mut egui::Ui, session: &mut Session) {
    egui::ComboBox::from_id_salt(("permission_mode", session.id))
        .selected_text(session.permission_mode.label())
        .show_ui(ui, |ui| {
            for mode in PermissionMode::ALL {
                ui.selectable_value(&mut session.permission_mode, mode, mode.label());
            }
        })
        .response
        .on_hover_text("What the agent may do without asking. Applies from the next message.");
}

pub fn conversation(ui: &mut egui::Ui, session: &Session) {
    egui::ScrollArea::vertical()
        .id_salt(("conversation", session.id))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if session.entries.is_empty() && session.streaming.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Ask {} something about {}.",
                            session.provider.short_name(),
                            session.folder_name()
                        ))
                        .weak(),
                    );
                });
            }
            for (index, entry) in session.entries.iter().enumerate() {
                show_entry(ui, (session.id, index), entry);
            }
            if !session.streaming.is_empty() {
                ui.label(&session.streaming);
            }
            ui.add_space(8.0);
        });
}

fn show_entry(ui: &mut egui::Ui, id: (u64, usize), entry: &Entry) {
    match entry {
        Entry::User(text) => {
            ui.add_space(10.0);
            egui::Frame::group(ui.style())
                .fill(ui.visuals().faint_bg_color)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("You").strong());
                    ui.label(text);
                });
            ui.add_space(4.0);
        }
        Entry::Agent(text) => {
            ui.label(text);
        }
        Entry::Tool { name, detail } => {
            ui.label(egui::RichText::new(format!("{name}: {detail}")).monospace().weak());
        }
        Entry::ToolOutput { text, is_error } => {
            let title = if *is_error { "Tool error" } else { "Tool output" };
            egui::CollapsingHeader::new(egui::RichText::new(title).small().weak())
                .id_salt(("tool_output", id))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(text).monospace().small());
                });
        }
        Entry::Notice(text) => {
            ui.label(egui::RichText::new(text).italics().weak());
        }
        Entry::Error(text) => {
            ui.colored_label(ui.visuals().error_fg_color, text);
        }
    }
}
