//! The middle column: session header, conversation and message box.

use eframe::egui;

use crate::agent::{PermissionMode, Provider};
use crate::session::{Entry, Session};
use crate::settings::Settings;

pub enum HeaderAction {
    None,
    ChangeFolder,
}

pub enum ComposerAction {
    None,
    Send,
    Stop,
}

pub fn header(
    ui: &mut egui::Ui,
    session: &mut Session,
    settings: &Settings,
    show_sessions: &mut bool,
    show_tools: &mut bool,
) -> HeaderAction {
    let mut action = HeaderAction::None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.toggle_value(show_sessions, "Sessions").on_hover_text("Show or hide the session list");
        ui.separator();
        ui.label("Project:");
        ui.add(egui::Label::new(egui::RichText::new(session.project_dir.display().to_string()).monospace()).truncate());
        if ui.add_enabled(!session.is_running(), egui::Button::new("Change…")).clicked() {
            action = HeaderAction::ChangeFolder;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.toggle_value(show_tools, "Tools").on_hover_text("Show or hide the terminal and browser");
        });
    });
    ui.horizontal(|ui| {
        ui.label("Agent:");
        let can_change = session.can_change_provider();
        ui.add_enabled_ui(can_change, |ui| {
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
        .on_disabled_hover_text("A conversation stays with one agent. Start a new session to use another.");
        ui.separator();
        ui.label("Permissions:");
        egui::ComboBox::from_id_salt("permission_mode")
            .selected_text(session.permission_mode.label())
            .show_ui(ui, |ui| {
                for mode in PermissionMode::ALL {
                    ui.selectable_value(&mut session.permission_mode, mode, mode.label());
                }
            });
        if let Some(model) = &session.model {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(model).weak());
            });
        }
    });
    ui.add_space(6.0);
    action
}

pub fn composer(ui: &mut egui::Ui, session: &mut Session, agent_installed: bool) -> ComposerAction {
    let composer_id = egui::Id::new(("composer", session.id));
    // Take Enter before the text box sees it; Shift+Enter still adds a new line.
    let enter_pressed = ui.memory(|m| m.has_focus(composer_id))
        && ui.input_mut(|i| !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let can_send = agent_installed && !session.is_running() && !session.input.trim().is_empty();

    ui.add_space(6.0);
    let response = ui.add(
        egui::TextEdit::multiline(&mut session.input)
            .id(composer_id)
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text(format!("Ask {}…  (Enter to send, Shift+Enter for a new line)", session.provider.short_name())),
    );
    if std::mem::take(&mut session.focus_composer) {
        response.request_focus();
    }

    let mut action = if enter_pressed && can_send { ComposerAction::Send } else { ComposerAction::None };
    ui.horizontal(|ui| {
        if session.is_running() {
            ui.spinner();
            ui.label(format!("{} is working…", session.provider.short_name()));
            if ui.button("Stop").clicked() {
                action = ComposerAction::Stop;
            }
        } else if ui.add_enabled(can_send, egui::Button::new("Send")).clicked() {
            action = ComposerAction::Send;
        }
    });
    ui.add_space(6.0);
    action
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
                            "Pick a project folder and ask {} something.",
                            session.provider.short_name()
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
