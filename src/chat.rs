//! The middle column: the conversation and the message box.

use eframe::egui;

use crate::agent::{PermissionMode, Provider};
use crate::browser::PickedElement;
use crate::commands::{self, CommandSource, SlashCommand};
use crate::git_diff::LineKind;
use crate::line_diff::FileEdit;
use crate::models::{self, Catalog};
use crate::session::{Entry, Session};
use crate::icons::{self, Icon};
use crate::settings::Settings;

/// The colour for full access, which lets the agent run anything.
const RISKY: egui::Color32 = egui::Color32::from_rgb(214, 158, 46);
/// Diff colours, kept close to what the Changes tab uses.
const ADDED: egui::Color32 = egui::Color32::from_rgb(106, 176, 118);
const REMOVED: egui::Color32 = egui::Color32::from_rgb(214, 108, 108);

pub enum ComposerAction {
    None,
    Send,
    Stop,
    ChangeFolder,
}

/// The message box, with the agent, permission and folder pickers along its bottom edge.
pub fn composer(
    ui: &mut egui::Ui,
    session: &mut Session,
    settings: &Settings,
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

    if in_slash {
        if slash_query != session.slash_query {
            session.slash_query = slash_query.clone();
            session.slash_dismissed = false;
            session.slash_selected = 0;
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
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
            session.slash_selected = session.slash_selected.saturating_sub(1);
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
            if show_slash {
                slash_suggestions_ui(ui, session, &slash_matches, &slash_query);
                ui.add_space(6.0);
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

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                provider_picker(ui, session, settings);
                model_picker(ui, session, catalog);
                effort_picker(ui, session, catalog);
                permission_picker(ui, session);
                // Until a folder is chosen this is the one thing the session needs, so it
                // stands out rather than sitting quietly with the other pickers.
                let (label, hover) = if session.has_folder() {
                    (
                        format!("📁 {}", session.folder_name()),
                        format!("{}\nClick to choose another folder", session.project_dir.display()),
                    )
                } else {
                    ("📁 Choose a folder".to_owned(), "Pick the project folder this session works in".to_owned())
                };
                let mut button = egui::Button::new(egui::RichText::new(label).small());
                if !session.has_folder() {
                    button = button.fill(RISKY.gamma_multiply(0.35));
                }
                let folder = ui.add_enabled(!session.is_running(), button).on_hover_text(hover);
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
            // The short name keeps the row of pickers from crowding the Send button.
            .selected_text(session.provider.short_name())
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

pub fn conversation(ui: &mut egui::Ui, session: &Session, markdown: &mut egui_commonmark::CommonMarkCache) {
    egui::ScrollArea::vertical()
        .id_salt(("conversation", session.id))
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            if session.entries.is_empty() && session.streaming.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    let prompt = if session.has_folder() {
                        format!("Ask {} something about {}.", session.provider.short_name(), session.folder_name())
                    } else {
                        "Choose a folder under the message box to get started.".to_owned()
                    };
                    ui.label(egui::RichText::new(prompt).weak());
                });
            }
            for (index, entry) in session.entries.iter().enumerate() {
                show_entry(ui, (session.id, index), entry, markdown);
            }
            // Text still arriving is left plain: half-written markdown would jump about
            // as the rest of it comes in.
            if !session.streaming.is_empty() {
                ui.label(&session.streaming);
            }
            ui.add_space(8.0);
        });
}

fn show_entry(ui: &mut egui::Ui, id: (u64, usize), entry: &Entry, markdown: &mut egui_commonmark::CommonMarkCache) {
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
            tool_row(ui, name, detail);
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

/// What a tool is doing: its name, then what it is working on.
fn tool_row(ui: &mut egui::Ui, name: &str, detail: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        egui::Frame::new()
            .fill(ui.visuals().widgets.inactive.bg_fill)
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(6, 1))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(name).small().strong());
            });
        ui.add(egui::Label::new(egui::RichText::new(detail).monospace().small().weak()).truncate());
    });
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

    egui::Frame::new()
        .fill(egui::Color32::from_rgb(18, 18, 20))
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
                        ui.ctx().copy_text(strip_ansi(text));
                    }
                });
            });
            ui.add_space(4.0);

            egui::ScrollArea::horizontal()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let font_id = egui::FontId::new(12.5, egui::FontFamily::Monospace);
                    let job = parse_ansi_to_layout_job(text, ui.visuals().text_color(), font_id);
                    ui.label(job);
                });
        });
}

/// Parses ANSI escape sequences into an egui LayoutJob with colored text spans,
/// expanding tabs and handling carriage returns.
fn parse_ansi_to_layout_job(text: &str, default_color: egui::Color32, font_id: egui::FontId) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut current_fg = default_color;
    let mut bold = false;
    let mut italic = false;
    let mut underline = false;

    let mut lines = Vec::new();
    for raw_line in text.lines() {
        let effective = raw_line.rsplit('\r').find(|seg| !seg.is_empty()).unwrap_or(raw_line);
        lines.push(effective.replace('\t', "    "));
    }
    let processed = lines.join("\n");

    let mut chars = processed.char_indices().peekable();
    let mut start = 0;

    while let Some(&(idx, ch)) = chars.peek() {
        if ch == '\x1b' {
            if idx > start {
                let segment = &processed[start..idx];
                let color = if bold && current_fg == default_color {
                    egui::Color32::from_rgb(
                        current_fg.r().saturating_add(40),
                        current_fg.g().saturating_add(40),
                        current_fg.b().saturating_add(40),
                    )
                } else {
                    current_fg
                };
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
            if let Some(&(_, '[')) = chars.peek() {
                chars.next();
                let mut params = String::new();
                while let Some(&(_, p_ch)) = chars.peek() {
                    chars.next();
                    if p_ch.is_ascii_alphabetic() {
                        if p_ch == 'm' {
                            apply_sgr_params(&params, &mut current_fg, &mut bold, &mut italic, &mut underline, default_color);
                        }
                        break;
                    } else {
                        params.push(p_ch);
                    }
                }
            }
            start = chars.peek().map(|&(i, _)| i).unwrap_or(processed.len());
        } else {
            chars.next();
        }
    }

    if start < processed.len() {
        let segment = &processed[start..];
        job.append(
            segment,
            0.0,
            egui::TextFormat {
                font_id,
                color: current_fg,
                italics: italic,
                underline: if underline { egui::Stroke::new(1.0, current_fg) } else { egui::Stroke::NONE },
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
    italic: &mut bool,
    underline: &mut bool,
    default_fg: egui::Color32,
) {
    if params.is_empty() {
        *fg = default_fg;
        *bold = false;
        *italic = false;
        *underline = false;
        return;
    }

    let codes: Vec<u32> = params.split(';').filter_map(|s| s.parse().ok()).collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => {
                *fg = default_fg;
                *bold = false;
                *italic = false;
                *underline = false;
            }
            1 => *bold = true,
            2 => *fg = fg.gamma_multiply(0.7),
            3 => *italic = true,
            4 => *underline = true,
            22 => *bold = false,
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
            _ => {}
        }
        i += 1;
    }
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

/// Strips ANSI escape sequences and carriage returns so copied text is clean plain text.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_escape = false;
    for ch in text.chars() {
        if ch == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if ch.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if ch != '\r' {
            out.push(ch);
        }
    }
    out
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
) {
    let card_bg = ui.visuals().panel_fill;
    let border_stroke = egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    let mut chosen = None;

    egui::Frame::new()
        .fill(card_bg)
        .stroke(border_stroke)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} Commands", session.provider.short_name()))
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
                .max_height(180.0)
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
                                        };
                                        egui::Frame::new()
                                            .fill(badge_bg)
                                            .corner_radius(4.0)
                                            .inner_margin(egui::Margin::symmetric(5, 1))
                                            .show(ui, |ui| {
                                                ui.label(egui::RichText::new(cmd.source.badge()).small());
                                            });

                                        ui.add_space(4.0);
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&cmd.description).weak().small(),
                                            )
                                            .truncate(),
                                        );
                                    });
                                });
                            },
                        );

                        if row.response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            if !is_selected {
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

    #[test]
    fn strip_ansi_removes_color_codes_and_carriage_returns() {
        let raw = "\x1b[32mSuccess\x1b[0m: built in 1.2s\rDone\x1b[K";
        assert_eq!(strip_ansi(raw), "Success: built in 1.2sDone");
    }

    #[test]
    fn ansi_parser_expands_tabs_and_handles_colors() {
        let text = "\x1b[31mError\x1b[0m:\tfailed";
        let job = parse_ansi_to_layout_job(text, egui::Color32::WHITE, egui::FontId::monospace(12.0));
        assert_eq!(job.text, "Error:    failed");
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[0].format.color, egui::Color32::from_rgb(220, 60, 60));
        assert_eq!(job.sections[1].format.color, egui::Color32::WHITE);
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
}
