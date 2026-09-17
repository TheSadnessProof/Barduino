//! The middle column: the conversation and the message box.

use eframe::egui;

use crate::agent::{PermissionMode, Provider};
use crate::browser::PickedElement;
use crate::models::{self, Catalog};
use crate::session::{Entry, Session};
use crate::icons::{self, Icon};
use crate::settings::Settings;
use crate::voice::VoiceError;

/// The colour for full access, which lets the agent run anything.
const RISKY: egui::Color32 = egui::Color32::from_rgb(214, 158, 46);

pub enum ComposerAction {
    None,
    Send,
    Stop,
    ChangeFolder,
    ToggleVoice,
    OpenSpeechSettings,
}

/// Voice input state for the message box.
pub struct Voice<'a> {
    pub listening: bool,
    /// Words heard so far that aren't final yet.
    pub partial: &'a str,
    pub error: Option<&'a VoiceError>,
}

/// The message box, with the agent, permission and folder pickers along its bottom edge.
pub fn composer(
    ui: &mut egui::Ui,
    session: &mut Session,
    settings: &Settings,
    catalog: &Catalog,
    agent_installed: bool,
    voice: Voice<'_>,
) -> ComposerAction {
    let composer_id = egui::Id::new(("composer", session.id));
    // Take Enter before the text box sees it; Shift+Enter still adds a new line.
    let enter_pressed = ui.memory(|m| m.has_focus(composer_id))
        && ui.input_mut(|i| !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let can_send = agent_installed && !session.is_running() && session.has_message();
    let mut action = if enter_pressed && can_send { ComposerAction::Send } else { ComposerAction::None };

    ui.add_space(8.0);
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            if !session.elements.is_empty() {
                let mut remove = None;
                ui.horizontal_wrapped(|ui| {
                    for (index, element) in session.elements.iter().enumerate() {
                        if element_chip(ui, element, true) {
                            remove = Some(index);
                        }
                    }
                });
                if let Some(index) = remove {
                    session.elements.remove(index);
                }
                ui.add_space(4.0);
            }
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
            if voice.listening {
                let heard = if voice.partial.is_empty() { "Listening…" } else { voice.partial };
                ui.add(egui::Label::new(egui::RichText::new(heard).italics().weak()).truncate());
            }
            if let Some(error) = voice.error {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(error.message()).small().color(ui.visuals().warn_fg_color));
                    if *error == VoiceError::SpeechPrivacyOff && ui.link("Open speech settings").clicked() {
                        action = ComposerAction::OpenSpeechSettings;
                    }
                });
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                provider_picker(ui, session, settings);
                model_picker(ui, session, catalog);
                effort_picker(ui, session, catalog);
                permission_picker(ui, session);
                let folder = ui
                    .add_enabled(!session.is_running(), egui::Button::new(format!("📁 {}", session.folder_name())).small())
                    .on_hover_text(format!("{}\nClick to choose another folder", session.project_dir.display()));
                if folder.clicked() {
                    action = ComposerAction::ChangeFolder;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let running = session.is_running();
                    if running {
                        if ui.button("Stop").clicked() {
                            action = ComposerAction::Stop;
                        }
                        ui.label(egui::RichText::new(format!("{} is working…", session.provider.short_name())).weak());
                        ui.spinner();
                    } else if ui.add_enabled(can_send, egui::Button::new("Send")).clicked() {
                        action = ComposerAction::Send;
                    }
                    let mic_tip = if voice.listening { "Stop voice input" } else { "Voice input: speak to type" };
                    let mic = icons::toggle(ui, Icon::Microphone, mic_tip, voice.listening);
                    if voice.listening {
                        // A red ring while the microphone is on.
                        ui.painter().circle_stroke(mic.rect.center(), 12.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(229, 83, 75)));
                    }
                    if mic.clicked() {
                        action = ComposerAction::ToggleVoice;
                    }
                    if !running
                        && session.chosen_model.is_none()
                        && let Some(model) = &session.model
                    {
                        ui.label(egui::RichText::new(model).small().weak());
                    }
                });
            });
        });
    ui.add_space(8.0);
    action
}

fn provider_picker(ui: &mut egui::Ui, session: &mut Session, settings: &Settings) {
    let was = session.provider;
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

    // Another agent has its own models, so a choice made for the old one can't stand.
    if session.provider != was {
        session.chosen_model = None;
        session.effort = None;
    }
}

/// Which model answers in this session. The list comes from the CLI itself, so
/// it may still be loading the first time it's opened.
fn model_picker(ui: &mut egui::Ui, session: &mut Session, catalog: &Catalog) {
    let provider = session.provider;
    let selected = match &session.chosen_model {
        Some(id) => catalog.label_for(provider, id),
        None => "Default model".to_owned(),
    };
    egui::ComboBox::from_id_salt(("model", session.id))
        .selected_text(egui::RichText::new(selected).small())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut session.chosen_model, None, "Default model")
                .on_hover_text("Whichever model the CLI is set to use");
            let models = catalog.models(provider);
            if models.is_empty() {
                ui.horizontal(|ui| {
                    if catalog.is_loading(provider) {
                        ui.spinner();
                        ui.label(egui::RichText::new("Reading the model list…").small().weak());
                    } else {
                        ui.label(egui::RichText::new("No other models reported.").small().weak());
                    }
                });
            }
            for model in models {
                ui.selectable_value(&mut session.chosen_model, Some(model.id.clone()), &model.label)
                    .on_hover_text(&model.id);
            }
        })
        .response
        .on_hover_text("Which model this session uses. Applies from the next message.");
}

/// How hard the model should work. The levels are the provider's own.
fn effort_picker(ui: &mut egui::Ui, session: &mut Session, catalog: &Catalog) {
    let levels = catalog.efforts(session.provider, session.chosen_model.as_deref());
    // A level the provider no longer offers would otherwise be stuck in the session.
    if let Some(effort) = &session.effort
        && !levels.iter().any(|level| level == effort)
    {
        session.effort = None;
    }
    let selected = match &session.effort {
        Some(effort) => models::effort_label(effort),
        None => "Default effort".to_owned(),
    };
    egui::ComboBox::from_id_salt(("effort", session.id))
        .selected_text(egui::RichText::new(selected).small())
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut session.effort, None, "Default effort");
            for level in levels {
                let label = models::effort_label(&level);
                ui.selectable_value(&mut session.effort, Some(level), label);
            }
        })
        .response
        .on_hover_text(
            "How much thinking the model puts in. More effort means slower, more thorough \
             answers that use more of your plan.",
        );
}

fn permission_picker(ui: &mut egui::Ui, session: &mut Session) {
    let current = session.permission_mode;
    let selected = if current.is_risky() {
        egui::RichText::new(current.label()).color(RISKY)
    } else {
        egui::RichText::new(current.label())
    };
    egui::ComboBox::from_id_salt(("permission_mode", session.id))
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for mode in PermissionMode::ALL {
                let label = if mode.is_risky() {
                    egui::RichText::new(mode.label()).color(RISKY)
                } else {
                    egui::RichText::new(mode.label())
                };
                ui.selectable_value(&mut session.permission_mode, mode, label).on_hover_text(mode.description());
            }
        })
        .response
        .on_hover_text(format!(
            "What the agent may do without asking. Applies from the next message.

{}",
            current.description()
        ));
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
        Entry::User(message) => {
            ui.add_space(10.0);
            egui::Frame::group(ui.style())
                .fill(ui.visuals().faint_bg_color)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new("You").strong());
                    if !message.text.is_empty() {
                        ui.label(&message.text);
                    }
                    if !message.elements.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for element in &message.elements {
                                element_chip(ui, element, false);
                            }
                        });
                    }
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

/// A small card for a page element attached to a message, with an × to remove
/// it when `removable`. Returns true when the × is clicked.
fn element_chip(ui: &mut egui::Ui, element: &PickedElement, removable: bool) -> bool {
    let mut removed = false;
    let chip = egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (icon, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                // A comment leads with what the user wrote; a plain pick with the element.
                match element.note() {
                    Some(note) => {
                        icons::paint(ui.painter(), icon, Icon::Comment, ui.visuals().weak_text_color());
                        ui.label(egui::RichText::new(shorten_note(note)).small());
                        ui.label(egui::RichText::new(element.short_label()).monospace().small().weak());
                    }
                    None => {
                        icons::paint(ui.painter(), icon, Icon::Pick, ui.visuals().weak_text_color());
                        ui.label(egui::RichText::new(element.short_label()).monospace().small());
                        if let Some(text) = element.short_text() {
                            ui.label(egui::RichText::new(text).small().weak());
                        }
                    }
                }
                if removable {
                    let (rect, close) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                    let color = if close.hovered() {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    icons::paint(ui.painter(), rect.shrink(2.0), Icon::Close, color);
                    removed = close.on_hover_text("Remove").clicked();
                }
            });
        });
    let mut hover = String::new();
    if let Some(note) = element.note() {
        hover.push_str(note);
        hover.push('\n');
    }
    hover.push_str(&format!("{}\n{} × {} px on {}", element.selector, element.width, element.height, element.url));
    chip.response.on_hover_text(hover);
    removed
}

/// Keeps a chip narrow: enough of the note to recognise it, not all of it.
fn shorten_note(note: &str) -> String {
    const MAX: usize = 40;
    let one_line = note.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MAX {
        return one_line;
    }
    one_line.chars().take(MAX - 1).chain(['…']).collect()
}
