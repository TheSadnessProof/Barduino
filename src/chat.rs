//! The middle column: the conversation and the message box.

use std::path::Path;

use eframe::egui;

use crate::agent::{PermissionMode, Provider};
use crate::browser::PickedElement;
use crate::commands::{self, CommandSource, Handling, SlashAction, SlashCommand};
use crate::git_diff::LineKind;
use crate::line_diff::FileEdit;
use crate::models::{self, Catalog};
use crate::session::{Entry, Session};
use crate::icons::{self, Icon};
use crate::settings::Settings;
use crate::tool_call::{self, ToolKind};
use crate::usage;

/// The colour for full access, which lets the agent run anything.
const RISKY: egui::Color32 = egui::Color32::from_rgb(214, 158, 46);
/// How tall the message box may grow before it scrolls instead, in rows. Beyond
/// this a long message would start pushing the conversation off the screen.
const MAX_COMPOSER_ROWS: f32 = 12.0;
/// How long a label on one of the chips under the message box may be.
const MAX_CHIP_CHARS: usize = 18;
/// How long a path in a tool's row may be before the front of it is cut away.
const MAX_DETAIL_CHARS: usize = 60;
/// Diff colours, kept close to what the Changes tab uses.
const ADDED: egui::Color32 = egui::Color32::from_rgb(106, 176, 118);
const REMOVED: egui::Color32 = egui::Color32::from_rgb(214, 108, 108);

pub enum ComposerAction {
    None,
    Send,
    Stop,
    ChangeFolder,
    /// A slash command Barduino carries out itself instead of sending, because a
    /// headless CLI has no interactive session for it to change.
    Apply(SlashAction),
    /// Why a slash command couldn't be applied, in words for the banner.
    Notice(String),
}

/// Actions returned from the conversation area (e.g. empty session controls).
pub enum ConversationAction {
    None,
    ChangeFolder,
    SelectProvider(Provider),
    /// Start the agent. Only the terminal chat asks for this: Barduino's own chat
    /// starts an agent when the first message is sent, so it has nothing to press.
    Start,
}

/// Claude signature terracotta/coral accent for primary actions and focus states.
const CLAUDE_CORAL: egui::Color32 = egui::Color32::from_rgb(217, 119, 87);

/// The message box, styled with Claude's signature aesthetics, hosting prompt input,
/// autocomplete popups, and turn execution controls.
pub fn composer(
    ui: &mut egui::Ui,
    session: &mut Session,
    catalog: &Catalog,
    agent_installed: bool,
) -> ComposerAction {
    let composer_id = egui::Id::new(("composer", session.id));
    let has_focus = ui.memory(|m| m.has_focus(composer_id));

    // Check if the user is currently typing a slash command (e.g. "/" or "/go" or "/compact").
    let (in_slash, slash_query) = {
        let trimmed = session.input.trim_start();
        if trimmed.starts_with('/') && !trimmed.contains(char::is_whitespace) {
            (true, trimmed[1..].to_owned())
        } else {
            (false, String::new())
        }
    };

    // The list follows the keyboard and never the mouse. Scrolling to the highlight
    // moves a different row under the pointer, which highlights that one, which
    // scrolls again — so the menu chased the cursor around as soon as it was moved.
    let mut scroll_to_selected = false;

    if in_slash {
        if slash_query != session.slash_query {
            session.slash_query = slash_query.clone();
            session.slash_dismissed = false;
            session.slash_selected = 0;
            // A narrower query starts at the top rather than wherever the last one left off.
            scroll_to_selected = true;
        }
    } else {
        session.slash_query.clear();
        session.slash_dismissed = false;
        session.slash_selected = 0;
    }

    let show_slash = in_slash && !session.slash_dismissed;
    let slash_commands = if show_slash {
        commands::discover(session.provider, &session.project_dir)
    } else {
        Vec::new()
    };
    let slash_matches = if show_slash {
        commands::filter(&slash_commands, &slash_query)
    } else {
        Vec::new()
    };

    let mut slash_completed = None;
    if show_slash && has_focus {
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
            session.slash_selected = (session.slash_selected + 1).min(slash_matches.len().saturating_sub(1));
            scroll_to_selected = true;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            session.slash_selected = session.slash_selected.saturating_sub(1);
            scroll_to_selected = true;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            session.slash_dismissed = true;
        }
        let complete_key = ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                || (!i.modifiers.shift && !slash_matches.is_empty() && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        });
        if complete_key && let Some(cmd) = slash_matches.get(session.slash_selected).or_else(|| slash_matches.first()) {
            slash_completed = Some(cmd.name.clone());
        }
    }

    if let Some(name) = slash_completed {
        session.input = format!("/{name} ");
        session.slash_selected = 0;
        session.slash_dismissed = false;
        session.focus_composer = true;
    }

    // Take Enter before the text box sees it; Shift+Enter still adds a new line.
    let enter_pressed = has_focus
        && ui.input_mut(|i| !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let can_send = agent_installed && !session.is_running() && session.has_message() && session.has_folder();
    let mut action =
        if enter_pressed && can_send { send_or_apply(session, catalog) } else { ComposerAction::None };

    // Warm, sophisticated palette inspired by Claude's signature desktop and web interface.
    let (card_bg, border_stroke, hint_color) = if ui.visuals().dark_mode {
        let border = if has_focus {
            CLAUDE_CORAL.gamma_multiply(0.85)
        } else {
            egui::Color32::from_rgb(58, 56, 52)
        };
        (
            egui::Color32::from_rgb(36, 35, 33),
            egui::Stroke::new(if has_focus { 1.5 } else { 1.0 }, border),
            egui::Color32::from_rgb(148, 142, 134),
        )
    } else {
        let border = if has_focus {
            CLAUDE_CORAL
        } else {
            egui::Color32::from_rgb(222, 218, 212)
        };
        (
            egui::Color32::WHITE,
            egui::Stroke::new(if has_focus { 1.5 } else { 1.0 }, border),
            egui::Color32::from_rgb(150, 145, 138),
        )
    };

    let available_w = ui.available_width();
    let max_w = 820.0_f32.min((available_w - 32.0).max(280.0));

    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(max_w);
        egui::Frame::new()
            .fill(card_bg)
            .stroke(border_stroke)
            .corner_radius(16.0)
            .inner_margin(egui::Margin { left: 16, right: 12, top: 12, bottom: 10 })
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
                if show_slash {
                    slash_suggestions_ui(ui, session, &slash_matches, &slash_query, scroll_to_selected);
                    ui.add_space(6.0);
                }
                let hint = format!("Message {}…   ·   / for commands", session.provider.short_name());
                // The box grows with what is typed and then scrolls, rather than
                // pushing the conversation off the top of the screen.
                let row_height = ui.text_style_height(&egui::TextStyle::Body);
                let response = egui::ScrollArea::vertical()
                    .id_salt(("composer_scroll", session.id))
                    .max_height(row_height * MAX_COMPOSER_ROWS)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut session.input)
                                .id(composer_id)
                                .frame(egui::Frame::NONE)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY)
                                .hint_text(egui::RichText::new(hint).color(hint_color)),
                        )
                    })
                    .inner;
                // Only when nothing else holds the keyboard. A turn finishing sets this,
                // and it used to pull the cursor out of a terminal mid-command.
                if std::mem::take(&mut session.focus_composer) && ui.memory(|m| m.focused().is_none()) {
                    response.request_focus();
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    // Send and the context take their width from the right first, so
                    // a long model name crowds the chips rather than the controls.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let running = session.is_running();
                        if running {
                            let stop_btn = egui::Button::new(
                                egui::RichText::new("■").size(13.0).color(egui::Color32::WHITE),
                            )
                            .fill(egui::Color32::from_rgb(205, 65, 65))
                            .corner_radius(16.0)
                            .min_size(egui::vec2(32.0, 32.0));

                            if ui.add(stop_btn).on_hover_text("Stop generating").clicked() {
                                action = ComposerAction::Stop;
                            }
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(format!("{} is working…", session.provider.short_name())).weak().small());
                            ui.spinner();
                        } else {
                            let (send_bg, send_fg) = if can_send {
                                (CLAUDE_CORAL, egui::Color32::WHITE)
                            } else if ui.visuals().dark_mode {
                                (egui::Color32::from_rgb(48, 46, 43), egui::Color32::from_rgb(110, 105, 98))
                            } else {
                                (egui::Color32::from_rgb(230, 226, 220), egui::Color32::from_rgb(160, 155, 148))
                            };

                            let send_btn = egui::Button::new(
                                egui::RichText::new("↑").size(17.0).strong().color(send_fg),
                            )
                            .fill(send_bg)
                            .corner_radius(16.0)
                            .min_size(egui::vec2(32.0, 32.0));

                            let tooltip = if !agent_installed {
                                format!("{} is not installed on this machine", session.provider.label())
                            } else if !session.has_folder() {
                                "Choose a workspace folder above to start".to_owned()
                            } else if !session.has_message() {
                                "Type a message or command to send".to_owned()
                            } else {
                                "Send message (Enter, Shift+Enter for newline)".to_owned()
                            };

                            if ui.add_enabled(can_send, send_btn).on_hover_text(tooltip).clicked() {
                                action = send_or_apply(session, catalog);
                            }
                            ui.add_space(4.0);
                            context_chip(ui, session);
                        }

                        // Whatever is left over, filled from the left as usual.
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if settings_row(ui, session, catalog) {
                                action = ComposerAction::ChangeFolder;
                            }
                        });
                    });
                });
            });
    });
    ui.add_space(12.0);
    action
}

/// What pressing send does with what is in the box. A command Barduino owns is
/// carried out here and never reaches the CLI; anything else is a prompt.
fn send_or_apply(session: &mut Session, catalog: &Catalog) -> ComposerAction {
    let models = catalog.models(session.provider);
    let efforts = catalog.efforts(session.provider, session.chosen_model.as_deref());
    match commands::intercept(&session.input, &models, &efforts) {
        Some(Ok(setting)) => {
            session.input.clear();
            ComposerAction::Apply(setting)
        }
        // The words stay in the box, so a near miss can be corrected rather than retyped.
        Some(Err(message)) => ComposerAction::Notice(message),
        None => ComposerAction::Send,
    }
}

/// The row under the message box: what the next turn will run as, and how much of
/// the conversation the model is already carrying. Returns true if the folder chip
/// was clicked.
fn settings_row(ui: &mut egui::Ui, session: &mut Session, catalog: &Catalog) -> bool {
    // Scoped, because the restyling below would otherwise reach the send button
    // drawn after it in the same row.
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // Quiet chips rather than form controls, so the row stays out of the way
        // until it is wanted. Only the fill changes, so nothing moves on hover.
        let quiet = if ui.visuals().dark_mode {
            egui::Color32::from_rgb(46, 44, 41)
        } else {
            egui::Color32::from_rgb(243, 240, 235)
        };
        ui.visuals_mut().widgets.inactive.weak_bg_fill = quiet;
        ui.visuals_mut().widgets.inactive.bg_fill = quiet;

        model_picker(ui, session, catalog);
        effort_picker(ui, session, catalog);
        permission_picker(ui, session);
        folder_chip(ui, session)
    })
    .inner
}

/// A picker label short enough that four of them still fit on one row. Model names
/// come from the CLI and can be long — "gemini-3-pro-preview-high" — and an
/// unbounded one would push the send button off the card on a narrow window.
fn short_label(label: &str, max: usize) -> String {
    if label.chars().count() <= max {
        return label.to_owned();
    }
    // Cut on a char boundary, not a byte one: these names are not all ASCII.
    label.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
}

/// Which model answers in this session. The list comes from the CLI itself, so it
/// may still be loading the first time it's opened.
fn model_picker(ui: &mut egui::Ui, session: &mut Session, catalog: &Catalog) {
    let provider = session.provider;
    let selected = match &session.chosen_model {
        Some(id) => short_label(&catalog.label_for(provider, id), MAX_CHIP_CHARS),
        None => "Default model".to_owned(),
    };
    // "Default" doesn't say which, so the one the CLI actually answered with goes
    // in the tooltip rather than being guessed at in the label.
    let answering = match &session.model {
        Some(model) if session.chosen_model.is_none() => format!("\n\nThe CLI last answered with {model}."),
        _ => String::new(),
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
        .on_hover_text(format!("Which model this session uses. Applies from the next message.{answering}"));
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

/// The colour each mode is marked with, from the most cautious to the one that
/// gives up the safety net.
fn mode_dot(mode: PermissionMode) -> egui::Color32 {
    match mode {
        PermissionMode::Plan => egui::Color32::from_rgb(150, 110, 210),
        PermissionMode::ReadOnly => egui::Color32::from_rgb(140, 160, 180),
        PermissionMode::AcceptEdits => egui::Color32::from_rgb(70, 165, 120),
        PermissionMode::Full => RISKY,
    }
}

/// What the agent may do without being asked.
fn permission_picker(ui: &mut egui::Ui, session: &mut Session) {
    let current = session.permission_mode;
    let label = egui::RichText::new(format!("● {}", current.label())).small().color(mode_dot(current));
    egui::ComboBox::from_id_salt(("permission_mode", session.id))
        .selected_text(label)
        .show_ui(ui, |ui| {
            for mode in PermissionMode::ALL {
                let label = egui::RichText::new(format!("● {}", mode.label())).color(mode_dot(mode));
                ui.selectable_value(&mut session.permission_mode, mode, label).on_hover_text(mode.description());
            }
        })
        .response
        .on_hover_text(format!(
            "What the agent may do without asking. Applies from the next message.\n\n{}",
            current.description()
        ));
}

/// The folder this session works in, and a way to change it without leaving the
/// message box. Returns true when it is clicked.
fn folder_chip(ui: &mut egui::Ui, session: &Session) -> bool {
    if !session.has_folder() {
        return false;
    }
    let name = short_label(&session.folder_name(), MAX_CHIP_CHARS);
    let chip = egui::Button::new(egui::RichText::new(format!("📁 {name}")).small().weak())
        .fill(egui::Color32::TRANSPARENT)
        .corner_radius(6.0);
    ui.add(chip)
        .on_hover_text(format!("Working in {}\nClick to choose another folder", session.project_dir.display()))
        .clicked()
}

/// How much of the conversation the model is carrying, once a turn has finished
/// and said so. No CLI reports the size of its context window, so this is the
/// count on its own rather than a share of a number we'd have to invent.
fn context_chip(ui: &mut egui::Ui, session: &Session) {
    let Some(last) = session.last_usage else { return };
    let tokens = last.context_tokens();
    if tokens == 0 {
        return;
    }
    ui.label(egui::RichText::new(format!("{} context", usage::short_count(tokens))).small().weak()).on_hover_text(
        format!(
            "The conversation so far, as the model read it on the last turn: {} tokens in, {} out.\n\
             /clear starts a fresh session in the same folder.",
            usage::short_count(tokens),
            usage::short_count(last.output)
        ),
    );
}

pub fn conversation(
    ui: &mut egui::Ui,
    session: &Session,
    settings: &Settings,
    markdown: &mut egui_commonmark::CommonMarkCache,
) -> ConversationAction {
    if session.entries.is_empty() && session.streaming.is_empty() {
        let mut action = ConversationAction::None;
        egui::ScrollArea::vertical()
            .id_salt(("empty_scroll", session.id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                action = empty_session_ui(ui, session, settings, false);
            });
        return action;
    }

    egui::ScrollArea::vertical()
        .id_salt(("conversation", session.id))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for (index, entry) in session.entries.iter().enumerate() {
                show_entry(ui, (session.id, index), entry, markdown, &session.project_dir);
            }
            // Text still arriving is left plain: half-written markdown would jump about
            // as the rest of it comes in.
            if !session.streaming.is_empty() {
                ui.label(&session.streaming);
            }
            ui.add_space(8.0);
        });

    ConversationAction::None
}

/// The welcome setup screen displayed in the center of an empty session.
/// Houses agent selection and project workspace setup before the conversation begins.
pub fn empty_session_ui(
    ui: &mut egui::Ui,
    session: &Session,
    settings: &Settings,
    show_start: bool,
) -> ConversationAction {
    let mut action = ConversationAction::None;

    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.1).clamp(20.0, 60.0));

        ui.label(egui::RichText::new("What would you like to build?").size(22.0).strong());
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Select an agent and project workspace to get started.").weak());

        ui.add_space(28.0);

        let max_w: f32 = 560.0_f32.min((ui.available_width() - 32.0).max(280.0));
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, 0.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.set_max_width(max_w);

                // --- 1. Agent Selection ---
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("SELECT AGENT").size(11.0).strong().weak());
                });
                ui.add_space(8.0);

                let providers: Vec<Provider> = settings.enabled_providers().collect();
                let spacing = 8.0;
                let num_cards = providers.len().max(1) as f32;
                let card_w = ((max_w - (num_cards - 1.0) * spacing) / num_cards).floor();

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = spacing;
                    for provider in &providers {
                        let selected = session.provider == *provider;
                        let (desc, icon) = match provider {
                            Provider::Claude => ("Anthropic agentic CLI", "🟣"),
                            Provider::Antigravity => ("Google autonomous CLI", "🔵"),
                            Provider::Codex => ("OpenAI coding CLI", "🟢"),
                        };

                        let item_id = egui::Id::new(("empty_agent_card", session.id, provider.short_name()));
                        let row = ui.scope_builder(
                            egui::UiBuilder::new().id_salt(item_id).sense(egui::Sense::click()),
                            |ui| {
                                let resp = ui.response();
                                let hovered = resp.hovered();

                                let fill = if selected {
                                    CLAUDE_CORAL.gamma_multiply(0.15)
                                } else if hovered {
                                    if ui.visuals().dark_mode { egui::Color32::from_rgb(46, 44, 41) } else { ui.visuals().faint_bg_color }
                                } else if ui.visuals().dark_mode {
                                    egui::Color32::from_rgb(36, 35, 33)
                                } else {
                                    egui::Color32::WHITE
                                };

                                let stroke = if selected {
                                    egui::Stroke::new(1.5, CLAUDE_CORAL)
                                } else if hovered {
                                    egui::Stroke::new(1.0, ui.visuals().widgets.hovered.bg_stroke.color)
                                } else if ui.visuals().dark_mode {
                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 56, 52))
                                } else {
                                    egui::Stroke::new(1.0, egui::Color32::from_rgb(222, 218, 212))
                                };

                                egui::Frame::new()
                                    .fill(fill)
                                    .stroke(stroke)
                                    .corner_radius(10.0)
                                    .inner_margin(egui::Margin::symmetric(12, 10))
                                    .show(ui, |ui| {
                                        ui.set_width(card_w);
                                        ui.horizontal(|ui| {
                                            ui.label(egui::RichText::new(icon).size(15.0));
                                            let mut title = egui::RichText::new(provider.short_name()).strong();
                                            if selected {
                                                title = title.color(CLAUDE_CORAL);
                                            }
                                            ui.label(title);
                                        });
                                        ui.add_space(4.0);
                                        ui.add(egui::Label::new(egui::RichText::new(desc).small().weak()).truncate());
                                    });
                            },
                        );

                        if row.response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if row.response.clicked() {
                            action = ConversationAction::SelectProvider(*provider);
                        }
                    }
                });

                ui.add_space(22.0);

                // --- 2. Workspace Folder Selection ---
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("WORKSPACE FOLDER").size(11.0).strong().weak());
                });
                ui.add_space(8.0);

                let folder_id = egui::Id::new(("empty_folder_card", session.id));
                let folder_resp = ui.scope_builder(
                    egui::UiBuilder::new().id_salt(folder_id).sense(egui::Sense::click()),
                    |ui| {
                        let resp = ui.response();
                        let hovered = resp.hovered();

                        let (fill, stroke) = if !session.has_folder() {
                            let fill = if hovered {
                                RISKY.gamma_multiply(0.2)
                            } else {
                                RISKY.gamma_multiply(0.12)
                            };
                            let stroke = egui::Stroke::new(1.2, RISKY.gamma_multiply(0.65));
                            (fill, stroke)
                        } else {
                            let fill = if hovered {
                                if ui.visuals().dark_mode { egui::Color32::from_rgb(46, 44, 41) } else { ui.visuals().faint_bg_color }
                            } else if ui.visuals().dark_mode {
                                egui::Color32::from_rgb(36, 35, 33)
                            } else {
                                egui::Color32::WHITE
                            };
                            let stroke = if ui.visuals().dark_mode {
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(58, 56, 52))
                            } else {
                                egui::Stroke::new(1.0, egui::Color32::from_rgb(222, 218, 212))
                            };
                            (fill, stroke)
                        };

                        egui::Frame::new()
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(10.0)
                            .inner_margin(egui::Margin::symmetric(14, 12))
                            .show(ui, |ui| {
                                ui.set_width(max_w);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("📁").size(20.0));
                                    ui.add_space(4.0);
                                    ui.vertical(|ui| {
                                        if session.has_folder() {
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new(session.folder_name()).strong());
                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    ui.label(egui::RichText::new("Change folder").small().weak());
                                                });
                                            });
                                            ui.add(egui::Label::new(egui::RichText::new(session.project_dir.display().to_string()).small().weak()).truncate());
                                        } else {
                                            ui.label(egui::RichText::new("Choose a project folder").strong().color(RISKY));
                                            ui.label(egui::RichText::new("Click to select the repository or directory for this session").small().weak());
                                        }
                                    });
                                });
                            });
                    },
                );

                if folder_resp.response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if folder_resp.response.clicked() {
                    action = ConversationAction::ChangeFolder;
                }

                if show_start {
                    ui.add_space(22.0);
                    if start_button(ui, session) {
                        action = ConversationAction::Start;
                    }
                    // A session held in Barduino's own chat before the agent moved
                    // into a terminal still has its messages. They aren't lost, but
                    // they aren't here either, so say where they went.
                    if !session.entries.is_empty() {
                        ui.add_space(10.0);
                        let count = session.entries.len();
                        ui.label(
                            egui::RichText::new(format!(
                                "This session has {count} earlier messages in Barduino's own chat. \
                                 Turn off “Run the agent in a terminal” in Settings to read them."
                            ))
                            .small()
                            .weak(),
                        );
                    }
                }
            },
        );
    });

    action
}

/// The button that runs the agent, for the terminal chat — which has no message box
/// to send the first message from, so starting has to be something you press.
/// Returns true when it is clicked.
fn start_button(ui: &mut egui::Ui, session: &Session) -> bool {
    let ready = session.has_folder();
    let (fill, text_colour) = if ready {
        (CLAUDE_CORAL, egui::Color32::WHITE)
    } else if ui.visuals().dark_mode {
        (egui::Color32::from_rgb(48, 46, 43), egui::Color32::from_rgb(110, 105, 98))
    } else {
        (egui::Color32::from_rgb(230, 226, 220), egui::Color32::from_rgb(160, 155, 148))
    };
    let label = format!("Start {}", session.provider.short_name());
    let button = egui::Button::new(egui::RichText::new(label).strong().color(text_colour))
        .fill(fill)
        .corner_radius(10.0)
        .min_size(egui::vec2(180.0, 38.0));

    let tooltip = if ready {
        format!("Opens a terminal in {} and runs {} in it", session.folder_name(), session.provider.command())
    } else {
        "Choose a project folder first — an agent always runs inside one".to_owned()
    };
    let clicked = ui.add_enabled(ready, button).on_hover_text(tooltip).clicked();
    ui.add_space(8.0);
    ui.label(egui::RichText::new("The agent's own interface opens here, as if you had run it yourself.").small().weak());
    clicked
}

fn show_entry(
    ui: &mut egui::Ui,
    id: (u64, usize),
    entry: &Entry,
    markdown: &mut egui_commonmark::CommonMarkCache,
    project_dir: &Path,
) {
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
        // Agents write in markdown, so headings, lists and code blocks are shown as such.
        Entry::Agent(text) => {
            egui_commonmark::CommonMarkViewer::new().show(ui, markdown, text);
        }
        Entry::Tool { name, detail, edit } => {
            tool_row(ui, name, detail, project_dir);
            if let Some(edit) = edit {
                edit_view(ui, id, edit);
            }
        }
        Entry::ToolOutput { text, is_error } => {
            let (title, colour) = if *is_error {
                ("Failed", ui.visuals().error_fg_color)
            } else {
                ("Output", ui.visuals().weak_text_color())
            };
            let lines = text.lines().count();
            let header = format!("{title} · {lines} {}", if lines == 1 { "line" } else { "lines" });
            indented(ui, colour, |ui| {
                egui::CollapsingHeader::new(egui::RichText::new(header).small().color(colour))
                    .id_salt(("tool_output", id))
                    // What failed is the thing you came to read. Ordinary output
                    // stays folded away so the transcript reads as a conversation.
                    .default_open(*is_error)
                    .show(ui, |ui| output_text(ui, text));
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

/// The colour each family of tool is marked with: cool for the ones that only
/// look, warmer for the ones that change something or reach outside the project.
fn kind_colour(kind: ToolKind) -> egui::Color32 {
    match kind {
        ToolKind::Read => egui::Color32::from_rgb(120, 145, 175),
        ToolKind::Edit => ADDED,
        ToolKind::Run => egui::Color32::from_rgb(158, 124, 208),
        ToolKind::Search => egui::Color32::from_rgb(92, 163, 158),
        ToolKind::Web => egui::Color32::from_rgb(96, 142, 200),
        ToolKind::Plan => RISKY,
        ToolKind::Delegate => CLAUDE_CORAL,
    }
}

/// A tool's detail with the project's own paths shortened.
///
/// Only the families whose detail holds a path are touched, and `short_path`
/// leaves anything that isn't one alone — so a command is shown exactly as it was
/// run, and a regex keeps its backslashes.
fn readable_detail(kind: Option<ToolKind>, detail: &str, project_dir: &Path) -> String {
    if !matches!(kind, Some(ToolKind::Read | ToolKind::Edit | ToolKind::Search)) {
        return detail.to_owned();
    }
    // A detail can carry more than one part — "src/app.rs · lines 40–90" — and the
    // path is not always the first of them.
    detail
        .split(" · ")
        .map(|part| tool_call::short_path(part, project_dir, MAX_DETAIL_CHARS))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// What a tool did: the verb it did it with, then what it worked on. A tool that
/// reported a list — a plan, a to-do list — has it underneath.
fn tool_row(ui: &mut egui::Ui, name: &str, detail: &str, project_dir: &Path) {
    let described = tool_call::describe(name);
    // An unrecognised tool keeps its own name. A wrong verb would be worse than
    // the CLI's own word for it, and MCP servers bring names nobody can predict.
    let (label, colour) = match described {
        Some((kind, verb)) => (verb, kind_colour(kind)),
        None => (name, ui.visuals().weak_text_color()),
    };
    let (phrase, body) = tool_call::split_detail(detail);
    let phrase = readable_detail(described.map(|(kind, _)| kind), phrase, project_dir);

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        egui::Frame::new()
            .fill(colour.gamma_multiply(0.22))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 1))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(label).small().strong().color(colour));
            });
        ui.add(egui::Label::new(egui::RichText::new(&phrase).monospace().small().weak()).truncate())
            .on_hover_text(&phrase);
    });
    if !body.is_empty() {
        indented(ui, colour.gamma_multiply(0.5), |ui| {
            for line in body {
                ui.add(egui::Label::new(egui::RichText::new(line).small().weak()).truncate());
            }
        });
    }
}

/// The change an editing tool is about to make, as a diff.
fn edit_view(ui: &mut egui::Ui, id: (u64, usize), edit: &FileEdit) {
    let lines = edit.lines();
    if lines.is_empty() {
        return;
    }
    let (added, removed) = edit.counts();
    let summary = format!("{}  +{added} −{removed}", edit.path);
    indented(ui, ui.visuals().selection.bg_fill, |ui| {
        egui::CollapsingHeader::new(egui::RichText::new(summary).small().monospace())
            .id_salt(("edit", id))
            .default_open(true)
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(ui.visuals().extreme_bg_color)
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for line in &lines {
                            diff_line(ui, line.kind, &line.text);
                        }
                    });
            });
    });
}

/// One line of a diff: a sign in the margin and the line itself, tinted to match.
fn diff_line(ui: &mut egui::Ui, kind: LineKind, text: &str) {
    let (sign, colour) = match kind {
        LineKind::Added => ("+", ADDED),
        LineKind::Removed => ("−", REMOVED),
        LineKind::Context => (" ", ui.visuals().text_color()),
        // Stands in for the lines left out between changes.
        LineKind::NoNewline => ("", ui.visuals().weak_text_color()),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new(sign).monospace().color(colour));
        ui.label(egui::RichText::new(text).monospace().color(colour));
    });
}

/// Tool output, formatted as a diff when a command printed a diff, or as a dark
/// terminal card with parsed ANSI colors, tabs expanded, and a copy button.
fn output_text(ui: &mut egui::Ui, text: &str) {
    let looks_like_diff = text.lines().any(|line| line.starts_with("@@ ") || line.starts_with("diff --git"));
    if looks_like_diff {
        for line in text.lines() {
            let colour = match line.chars().next() {
                Some('+') if !line.starts_with("+++") => ADDED,
                Some('-') if !line.starts_with("---") => REMOVED,
                Some('@') => ui.visuals().selection.stroke.color,
                _ => ui.visuals().weak_text_color(),
            };
            ui.label(egui::RichText::new(line).monospace().small().color(colour));
        }
        return;
    }

    // The house background for output, rather than a hardcoded dark one: Windows
    // defaults to a light theme, where a near-black card with theme-coloured text
    // on it came out at about 2.3:1 — unreadable.
    let background = ui.visuals().extreme_bg_color;
    egui::Frame::new()
        .fill(background)
        .stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color.gamma_multiply(0.4)))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let lines = text.lines().count();
                ui.label(
                    egui::RichText::new(format!("{lines} {}", if lines == 1 { "line" } else { "lines" }))
                        .monospace()
                        .small()
                        .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Copy").on_hover_text("Copy output to clipboard").clicked() {
                        ui.ctx().copy_text(visible_text(text));
                    }
                });
            });
            ui.add_space(4.0);

            // Hug the content vertically. Nested in the conversation's own scroll area,
            // `false` on the cross axis resolves to the *available* height, which
            // changes as the user scrolls — so the card grew to fill the viewport and
            // shifted everything under it on every frame.
            egui::ScrollArea::horizontal()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let font_id = egui::FontId::new(12.5, egui::FontFamily::Monospace);
                    let job = parse_ansi_to_layout_job(text, ui.visuals().text_color(), background, font_id);
                    // Extend, or `Label` wraps to the visible width and the scroll
                    // area it sits in can never scroll.
                    ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
                });
        });
}

/// Parses ANSI escape sequences into an egui LayoutJob with colored text spans,
/// expanding tabs and handling carriage returns.
fn parse_ansi_to_layout_job(
    text: &str,
    default_color: egui::Color32,
    background: egui::Color32,
    font_id: egui::FontId,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut current_fg = default_color;
    let mut bold = false;
    let mut dim = false;
    let mut italic = false;
    let mut underline = false;

    let processed = flatten_lines(text);

    let mut chars = processed.char_indices().peekable();
    let mut start = 0;

    while let Some(&(idx, ch)) = chars.peek() {
        if ch == '\x1b' {
            if idx > start {
                let segment = &processed[start..idx];
                let color = run_colour(current_fg, default_color, background, bold, dim);
                job.append(
                    segment,
                    0.0,
                    egui::TextFormat {
                        font_id: font_id.clone(),
                        color,
                        italics: italic,
                        underline: if underline { egui::Stroke::new(1.0, color) } else { egui::Stroke::NONE },
                        ..Default::default()
                    },
                );
            }

            chars.next();
            match chars.peek() {
                Some(&(_, '[')) => {
                    chars.next();
                    let mut params = String::new();
                    while let Some(&(_, p_ch)) = chars.peek() {
                        chars.next();
                        if p_ch.is_ascii_alphabetic() {
                            if p_ch == 'm' {
                                apply_sgr_params(
                                    &params,
                                    &mut current_fg,
                                    &mut bold,
                                    &mut dim,
                                    &mut italic,
                                    &mut underline,
                                    default_color,
                                );
                            }
                            break;
                        }
                        params.push(p_ch);
                    }
                }
                // An OSC — a window title, or a hyperlink — ends at BEL or at ESC
                // backslash, not at the first letter: its payload is full of them.
                Some(&(_, ']')) => {
                    chars.next();
                    while let Some(&(_, c)) = chars.peek() {
                        chars.next();
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' {
                            if let Some(&(_, '\\')) = chars.peek() {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                // A character-set selector, such as the ESC ( B `tput sgr0` emits.
                Some(&(_, '(' | ')' | '*' | '+' | '%' | '#')) => {
                    chars.next();
                    chars.next();
                }
                // A single-character escape, such as ESC 7 or ESC =.
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            start = chars.peek().map(|&(i, _)| i).unwrap_or(processed.len());
        } else {
            chars.next();
        }
    }

    if start < processed.len() {
        let segment = &processed[start..];
        // Through the same helper as the runs above: output ending without a reset
        // — a CLI killed mid-write — used to lose its bold and dim right here.
        let color = run_colour(current_fg, default_color, background, bold, dim);
        job.append(
            segment,
            0.0,
            egui::TextFormat {
                font_id,
                color,
                italics: italic,
                underline: if underline { egui::Stroke::new(1.0, color) } else { egui::Stroke::NONE },
                ..Default::default()
            },
        );
    }

    job
}

fn apply_sgr_params(
    params: &str,
    fg: &mut egui::Color32,
    bold: &mut bool,
    dim: &mut bool,
    italic: &mut bool,
    underline: &mut bool,
    default_fg: egui::Color32,
) {
    if params.is_empty() {
        *fg = default_fg;
        *bold = false;
        *dim = false;
        *italic = false;
        *underline = false;
        return;
    }

    // An empty parameter means zero, per ECMA-48, so `ESC[;31m` resets and then goes
    // red. Anything that isn't a number at all becomes a code nothing matches, which
    // keeps the arguments of a 38 or a 48 in the right places.
    let codes: Vec<u32> = params
        .split(';')
        .map(|part| if part.is_empty() { 0 } else { part.parse().unwrap_or(u32::MAX) })
        .collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => {
                *fg = default_fg;
                *bold = false;
                *dim = false;
                *italic = false;
                *underline = false;
            }
            1 => *bold = true,
            // A flag rather than a change to the colour. Dimming the colour itself
            // was cumulative and 22 never put it back, so the repeated dim spans
            // that npm, jest and eslint emit faded the output away to nothing.
            2 => *dim = true,
            3 => *italic = true,
            4 => *underline = true,
            22 => {
                *bold = false;
                *dim = false;
            }
            23 => *italic = false,
            24 => *underline = false,
            39 => *fg = default_fg,
            30 => *fg = egui::Color32::from_rgb(0, 0, 0),
            31 => *fg = egui::Color32::from_rgb(220, 60, 60),
            32 => *fg = egui::Color32::from_rgb(50, 190, 100),
            33 => *fg = egui::Color32::from_rgb(220, 180, 40),
            34 => *fg = egui::Color32::from_rgb(70, 130, 230),
            35 => *fg = egui::Color32::from_rgb(180, 80, 190),
            36 => *fg = egui::Color32::from_rgb(40, 180, 200),
            37 => *fg = egui::Color32::from_rgb(210, 210, 210),
            90 => *fg = egui::Color32::from_rgb(120, 120, 120),
            91 => *fg = egui::Color32::from_rgb(240, 90, 90),
            92 => *fg = egui::Color32::from_rgb(70, 220, 130),
            93 => *fg = egui::Color32::from_rgb(245, 210, 60),
            94 => *fg = egui::Color32::from_rgb(100, 160, 255),
            95 => *fg = egui::Color32::from_rgb(215, 110, 225),
            96 => *fg = egui::Color32::from_rgb(70, 210, 230),
            97 => *fg = egui::Color32::from_rgb(255, 255, 255),
            38 => {
                if i + 2 < codes.len() && codes[i + 1] == 5 {
                    *fg = ansi_256_color(codes[i + 2] as u8);
                    i += 2;
                } else if i + 4 < codes.len() && codes[i + 1] == 2 {
                    *fg = egui::Color32::from_rgb(codes[i + 2] as u8, codes[i + 3] as u8, codes[i + 4] as u8);
                    i += 4;
                }
            }
            48 => {
                // The background isn't painted, but its arguments still have to be
                // stepped over. Unread, `38;5;208;48;5;0` went on to read the
                // trailing 0 as "reset everything" and wiped the colour just set.
                if i + 2 < codes.len() && codes[i + 1] == 5 {
                    i += 2;
                } else if i + 4 < codes.len() && codes[i + 1] == 2 {
                    i += 4;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// What a run of text is drawn in, once bold and dim have had their say and the
/// result has been kept clear of the card behind it.
fn run_colour(
    fg: egui::Color32,
    default_fg: egui::Color32,
    background: egui::Color32,
    bold: bool,
    dim: bool,
) -> egui::Color32 {
    let mut colour = if bold && fg == default_fg {
        egui::Color32::from_rgb(fg.r().saturating_add(40), fg.g().saturating_add(40), fg.b().saturating_add(40))
    } else {
        fg
    };
    if dim {
        colour = colour.gamma_multiply(0.65);
    }
    readable_on(colour, background, default_fg)
}

/// Keeps a colour the output asked for legible on the card. A tool that assumes a
/// dark terminal asks for black and one that assumes a light terminal asks for
/// white; whichever way the theme goes, one of those would be invisible.
fn readable_on(colour: egui::Color32, background: egui::Color32, fallback: egui::Color32) -> egui::Color32 {
    let luma =
        |c: egui::Color32| 0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b());
    if (luma(colour) - luma(background)).abs() < 40.0 { fallback } else { colour }
}

/// What a line looks like on screen: a progress bar that rewrote itself with a
/// carriage return shows only its last state, and tabs become spaces. Shared with
/// [`visible_text`] so what Copy hands over is what was on the screen.
fn flatten_lines(text: &str) -> String {
    text.lines()
        .map(|raw| {
            let effective = raw.rsplit('\r').find(|segment| !segment.is_empty()).unwrap_or(raw);
            effective.replace('\t', "    ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The text the card is showing, for the Copy button.
fn visible_text(text: &str) -> String {
    strip_escapes(&flatten_lines(text))
}

/// Removes escape sequences and leaves the text they were dressing up.
fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            // A CSI runs up to and including its first letter.
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            // An OSC — a window title, or a hyperlink — ends at BEL or at ESC
            // backslash. Its payload is full of letters, so it can't end at the
            // first one: that is how the h of https was eaten out of the clipboard.
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        chars.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            // A character-set selector, such as the ESC ( B that `tput sgr0` emits.
            Some('(' | ')' | '*' | '+' | '%' | '#') => {
                chars.next();
                chars.next();
            }
            // A single-character escape, such as ESC 7 or ESC =.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

fn ansi_256_color(idx: u8) -> egui::Color32 {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0), (205, 49, 49), (13, 188, 121), (229, 229, 16),
        (36, 114, 200), (188, 63, 188), (17, 168, 205), (229, 229, 229),
        (102, 102, 102), (241, 76, 76), (35, 209, 139), (245, 245, 67),
        (59, 142, 234), (214, 112, 214), (41, 184, 219), (255, 255, 255),
    ];
    match idx {
        0..16 => {
            let (r, g, b) = ANSI[idx as usize];
            egui::Color32::from_rgb(r, g, b)
        }
        16..232 => {
            let i = idx - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            egui::Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..=255 => {
            let gray = 8 + (idx - 232) * 10;
            egui::Color32::from_rgb(gray, gray, gray)
        }
    }
}

/// Puts a block under the tool it belongs to, behind a coloured line down the left.
fn indented(ui: &mut egui::Ui, colour: egui::Color32, contents: impl FnOnce(&mut egui::Ui)) {
    const INDENT: f32 = 10.0;
    ui.horizontal(|ui| {
        ui.add_space(INDENT);
        let line = ui.cursor().min;
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            contents(ui);
        });
        // Drawn after the contents, so it can be exactly as tall as they turned out.
        let bottom = ui.min_rect().bottom();
        ui.painter().vline(line.x - 4.0, line.y..=bottom, egui::Stroke::new(1.5, colour.gamma_multiply(0.7)));
    });
}

/// The autocomplete popover for slash commands (`/`), listing real-time options
/// offered by the active provider, including built-in commands and discovered skills.
fn slash_suggestions_ui(
    ui: &mut egui::Ui,
    session: &mut Session,
    matches: &[&SlashCommand],
    query: &str,
    scroll_to_selected: bool,
) {
    // Only a pointer that actually moved this frame may take the highlight. Resting
    // the mouse over the list would otherwise drag it back every frame, and the
    // arrow keys would appear to do nothing at all.
    let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    let (card_bg, border_stroke) = if ui.visuals().dark_mode {
        (egui::Color32::from_rgb(40, 38, 35), egui::Stroke::new(1.0, egui::Color32::from_rgb(60, 58, 53)))
    } else {
        (egui::Color32::from_rgb(252, 250, 247), egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 216, 210)))
    };
    let mut chosen = None;

    egui::Frame::new()
        .fill(card_bg)
        .stroke(border_stroke)
        .corner_radius(12.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} Commands ({})",
                        session.provider.short_name(),
                        matches.len()
                    ))
                    .strong()
                    .small(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new("↑↓ navigate · Tab/Enter complete · Esc dismiss")
                            .weak()
                            .small(),
                    );
                });
            });
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);

            if matches.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!(
                        "No commands matching “/{query}” for {}",
                        session.provider.short_name()
                    ))
                    .weak()
                    .small(),
                );
                ui.add_space(4.0);
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt(("slash_scroll", session.id))
                .max_height(240.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for (index, cmd) in matches.iter().enumerate() {
                        let is_selected = index == session.slash_selected;
                        let item_id = egui::Id::new(("slash_row", session.id, &cmd.name));

                        let row = ui.scope_builder(
                            egui::UiBuilder::new().id_salt(item_id).sense(egui::Sense::click()),
                            |ui| {
                                let response = ui.response();
                                let hovered = response.hovered();
                                let visuals = ui.style().interact_selectable(&response, is_selected);
                                let fill = if is_selected {
                                    ui.visuals().selection.bg_fill.gamma_multiply(0.22)
                                } else if hovered {
                                    visuals.weak_bg_fill
                                } else {
                                    egui::Color32::TRANSPARENT
                                };

                                let mut frame = egui::Frame::new()
                                    .fill(fill)
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::symmetric(8, 5));
                                if is_selected {
                                    frame = frame.stroke(egui::Stroke::new(1.0, ui.visuals().selection.bg_fill));
                                }

                                frame.show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let mut name_text = egui::RichText::new(format!("/{}", cmd.name)).strong();
                                        if is_selected {
                                            name_text = name_text.color(ui.visuals().selection.stroke.color);
                                        }
                                        ui.label(name_text);

                                        let badge_bg = match cmd.source {
                                            CommandSource::Builtin => {
                                                ui.visuals().widgets.noninteractive.bg_fill
                                            }
                                            CommandSource::Skill => {
                                                egui::Color32::from_rgb(45, 120, 110).gamma_multiply(0.4)
                                            }
                                            CommandSource::Project => {
                                                egui::Color32::from_rgb(110, 70, 160).gamma_multiply(0.4)
                                            }
                                            CommandSource::Barduino => CLAUDE_CORAL.gamma_multiply(0.35),
                                        };
                                        egui::Frame::new()
                                            .fill(badge_bg)
                                            .corner_radius(4.0)
                                            .inner_margin(egui::Margin::symmetric(5, 1))
                                            .show(ui, |ui| {
                                                ui.label(egui::RichText::new(cmd.source.badge()).small());
                                            });
                                        handling_badge(ui, cmd);

                                        ui.add_space(4.0);
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&cmd.description).weak().small(),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(&cmd.description);
                                    });
                                });
                            },
                        );

                        if is_selected && scroll_to_selected {
                            // None scrolls as little as it takes to bring the row
                            // into view, so the list only moves at the edges rather
                            // than re-centring on every keypress.
                            row.response.scroll_to_me(None);
                        }

                        if row.response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            if !is_selected && pointer_moved {
                                session.slash_selected = index;
                            }
                        }
                        if row.response.clicked() {
                            chosen = Some(cmd.name.clone());
                        }
                    }
                });
        });

    if let Some(name) = chosen {
        session.input = format!("/{name} ");
        session.slash_selected = 0;
        session.slash_dismissed = false;
        session.focus_composer = true;
    }
}

/// Warns about a command the CLI can only run from its own interface. Nothing is
/// said about the others: the "Barduino" badge already means it takes effect here,
/// and everything else is a prompt, which is what the menu implies anyway.
fn handling_badge(ui: &mut egui::Ui, cmd: &SlashCommand) {
    if commands::handling(&cmd.name, cmd.source) != Handling::Terminal {
        return;
    }
    ui.label(egui::RichText::new("opens a terminal").small().color(RISKY.gamma_multiply(0.9)))
        .on_hover_text("Only the CLI's own interface can run this, so Barduino will start it in a terminal for you.");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::PermissionMode;

    /// The card and the Copy button must never disagree: whatever the parser puts on
    /// screen is exactly what the clipboard gets. The old stripper concatenated every
    /// progress frame a `\r` had overwritten, so Copy produced text nobody had seen.
    #[test]
    fn copied_output_is_exactly_what_is_on_screen() {
        let shown = |raw: &str| {
            parse_ansi_to_layout_job(raw, egui::Color32::WHITE, egui::Color32::BLACK, egui::FontId::monospace(12.0))
                .text
        };
        for raw in [
            "\x1b[32mSuccess\x1b[0m: built in 1.2s\rDone\x1b[K",
            "plain",
            "tab\tseparated",
            "\x1b[31mred\x1b[0m and \x1b[2mdim\x1b[22m",
            "downloading 10%\rdownloading 50%\rdownloading 100%",
            "\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\",
            "",
        ] {
            assert_eq!(visible_text(raw), shown(raw), "copy and display disagree on {raw:?}");
        }
        // And specifically: only the last thing written to the line survives.
        assert_eq!(visible_text("\x1b[32mSuccess\x1b[0m: built in 1.2s\rDone\x1b[K"), "Done");
    }

    #[test]
    fn the_output_parser_survives_junk() {
        let job = |raw: &str| {
            parse_ansi_to_layout_job(raw, egui::Color32::WHITE, egui::Color32::BLACK, egui::FontId::monospace(12.0))
        };
        assert_eq!(job("").text, "");
        assert_eq!(job("\x1b").text, "", "a lone escape at the very end");
        assert_eq!(job("\x1b[").text, "", "a CSI that never finishes");
        assert_eq!(job("\x1b[38;5").text, "", "a colour whose arguments are cut off");
        assert_eq!(job("\x1b[999mstill here").text, "still here", "an unknown code");
        assert_eq!(job("\x1b[99999999999mstill here").text, "still here", "a parameter too big to parse");
        assert_eq!(job("\x1b[;;mstill here").text, "still here", "nothing but empty parameters");

        // Escapes that aren't CSI have to be consumed too, or their bodies show up
        // as literal text: a window title, and the ESC ( B that `tput sgr0` emits.
        assert_eq!(job("\x1b]0;my window\x07after").text, "after");
        assert_eq!(job("\x1b(Bafter").text, "after");
        assert_eq!(visible_text("\x1b]8;;https://example.com\x1b\\click\x1b]8;;\x1b\\"), "click");
    }

    #[test]
    fn a_background_colour_does_not_wipe_the_foreground() {
        // What bat, delta, fzf and rg emit: orange on black, in one sequence. The
        // background's arguments used to be read as more foreground codes, so the
        // trailing 0 reset the orange that the same sequence had just set.
        let job = parse_ansi_to_layout_job(
            "\x1b[38;5;208;48;5;0morange",
            egui::Color32::WHITE,
            egui::Color32::BLACK,
            egui::FontId::monospace(12.0),
        );
        assert_eq!(job.text, "orange");
        assert_eq!(job.sections[0].format.color, egui::Color32::from_rgb(255, 135, 0));
    }

    #[test]
    fn dim_does_not_pile_up_and_bold_survives_a_missing_reset() {
        let job = |raw: &str| {
            parse_ansi_to_layout_job(raw, egui::Color32::WHITE, egui::Color32::BLACK, egui::FontId::monospace(12.0))
        };
        // Repeated dim spans — npm, jest and eslint emit them constantly — used to
        // multiply, fading the bottom of the card away to nothing.
        let repeated = job("\x1b[2ma\x1b[22m\x1b[2mb\x1b[22m\x1b[2mc\x1b[22m");
        let dimmed: Vec<egui::Color32> = repeated.sections.iter().map(|s| s.format.color).collect();
        assert!(dimmed.windows(2).all(|pair| pair[0] == pair[1]), "each dim span is equally dim: {dimmed:?}");
        assert!(dimmed[0] != egui::Color32::WHITE, "and dimmer than plain text");

        // 22 puts the colour back, rather than leaving it dimmed for good.
        let restored = job("\x1b[2mdim\x1b[22mplain");
        assert_eq!(restored.sections.last().expect("two runs").format.color, egui::Color32::WHITE);

        // Output cut off before its reset still gets its bold; the trailing run used
        // to skip the styling the runs before it had. Grey, because bold brightens by
        // adding to each channel and white has nowhere left to go.
        let grey = egui::Color32::from_gray(180);
        let unreset = parse_ansi_to_layout_job(
            "plain\x1b[1mbold",
            grey,
            egui::Color32::BLACK,
            egui::FontId::monospace(12.0),
        );
        assert_ne!(
            unreset.sections[0].format.color,
            unreset.sections.last().expect("two runs").format.color,
            "bold reads brighter even with no reset at the end"
        );
    }

    #[test]
    fn an_empty_parameter_counts_as_zero() {
        // ECMA-48: `ESC[;31m` is a reset followed by red.
        let job = parse_ansi_to_layout_job(
            "\x1b[1m\x1b[;31mred",
            egui::Color32::WHITE,
            egui::Color32::BLACK,
            egui::FontId::monospace(12.0),
        );
        assert_eq!(job.sections.last().expect("a run").format.color, egui::Color32::from_rgb(220, 60, 60));
    }

    #[test]
    fn a_colour_that_would_vanish_into_the_card_is_replaced() {
        // A tool assuming a dark terminal asks for black; on a dark card that is
        // invisible, so it falls back to the theme's own text colour.
        let on_dark = readable_on(egui::Color32::BLACK, egui::Color32::from_gray(16), egui::Color32::WHITE);
        assert_eq!(on_dark, egui::Color32::WHITE);
        // The same the other way round, which is what a light theme gets.
        let on_light = readable_on(egui::Color32::WHITE, egui::Color32::from_gray(248), egui::Color32::BLACK);
        assert_eq!(on_light, egui::Color32::BLACK);
        // A colour with room around it is left exactly as the output asked.
        let red = egui::Color32::from_rgb(220, 60, 60);
        assert_eq!(readable_on(red, egui::Color32::from_gray(16), egui::Color32::WHITE), red);
    }

    #[test]
    fn ansi_parser_expands_tabs_and_handles_colors() {
        let text = "\x1b[31mError\x1b[0m:\tfailed";
        let job = parse_ansi_to_layout_job(
            text,
            egui::Color32::WHITE,
            egui::Color32::BLACK,
            egui::FontId::monospace(12.0),
        );
        assert_eq!(job.text, "Error:    failed");
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[0].format.color, egui::Color32::from_rgb(220, 60, 60));
        assert_eq!(job.sections[1].format.color, egui::Color32::WHITE);
    }

    #[test]
    fn only_the_details_that_hold_a_path_get_shortened() {
        let project = Path::new(r"C:\work\barduino");
        let shorten = |kind, detail| readable_detail(kind, detail, project);

        assert_eq!(shorten(Some(ToolKind::Read), r"C:\work\barduino\src\app.rs"), "src/app.rs");
        // A read of part of a file carries the range alongside the path.
        assert_eq!(
            shorten(Some(ToolKind::Read), r"C:\work\barduino\src\app.rs · lines 40–90"),
            "src/app.rs · lines 40–90"
        );
        // A search carries the pattern first and the folder second.
        assert_eq!(shorten(Some(ToolKind::Search), r"note.txt · C:\work\barduino\src"), "note.txt · src");

        // A command is never touched: backslashes in it are the command's own, and
        // rewriting them would change what the row says was run.
        let command = r"cargo run -- --path C:\work\barduino\src";
        assert_eq!(shorten(Some(ToolKind::Run), command), command);
        // Nor is a tool we don't recognise, whose detail could be anything.
        assert_eq!(shorten(None, command), command);
        // And a regex keeps its escapes even in a family that does hold paths.
        assert_eq!(shorten(Some(ToolKind::Search), r"\bfn\s+main\b"), r"\bfn\s+main\b");
    }

    #[test]
    fn a_chip_label_is_cut_to_fit_the_row() {
        // Claude's own names are short enough to leave alone.
        assert_eq!(short_label("Opus", MAX_CHIP_CHARS), "Opus");
        // A model name read from a CLI can be far longer than the row has space for.
        assert_eq!(short_label("gemini-3-pro-preview-high", 12), "gemini-3-pr…");
        // Exactly the limit is still left alone, so nothing is cut for one character.
        assert_eq!(short_label("123456789012", 12), "123456789012");
        // Cut on characters, not bytes: a folder name is not always ASCII. The
        // ellipsis counts towards the limit, so what comes back is never wider.
        assert_eq!(short_label("ბარდუინოს-საქაღალდე", 6), "ბარდუ…");
        assert_eq!(short_label("gemini-3-pro-preview-high", 12).chars().count(), 12);
    }

    #[test]
    fn slash_command_trigger_detects_prompt_prefix() {
        let is_slash = |input: &str| {
            let trimmed = input.trim_start();
            trimmed.starts_with('/') && !trimmed.contains(char::is_whitespace)
        };
        assert!(is_slash("/"), "bare slash opens suggestions");
        assert!(is_slash("/goal"), "typing command name keeps suggestions open");
        assert!(is_slash("/co"), "prefix query keeps suggestions open");
        assert!(!is_slash("/goal solve this"), "space closes autocomplete for argument typing");
        assert!(!is_slash("please look at /path/to/file"), "normal prompt with slash does not trigger autocomplete");
        assert!(!is_slash(""), "empty input does not trigger autocomplete");
    }

    #[test]
    fn empty_session_without_folder_is_detected() {
        let session = Session::new(1, std::path::PathBuf::new(), Provider::Claude, PermissionMode::ReadOnly);
        assert!(!session.has_folder(), "new session with empty path has no folder");
    }
}
