use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;

use crate::claude::{self, AgentEvent, PermissionMode, RunningTurn, Turn};

/// Tool output longer than this is cut off in the chat so huge outputs don't slow the UI.
const MAX_TOOL_OUTPUT_CHARS: usize = 4000;

enum Entry {
    User(String),
    Claude(String),
    Tool { name: String, detail: String },
    ToolOutput { text: String, is_error: bool },
    Notice(String),
    Error(String),
}

pub struct BarduinoApp {
    claude_exe: Option<PathBuf>,
    project_dir: PathBuf,
    permission_mode: PermissionMode,
    session_id: Option<String>,
    model: Option<String>,
    entries: Vec<Entry>,
    /// Text Claude is still streaming, shown below the finished entries.
    streaming: String,
    input: String,
    turn: Option<RunningTurn>,
    stop_requested: bool,
    /// Put the cursor in the message box on the next frame.
    focus_composer: bool,
    events_tx: Sender<AgentEvent>,
    events_rx: Receiver<AgentEvent>,
}

impl BarduinoApp {
    pub fn new() -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let claude_exe = claude::find_executable();
        let mut entries = Vec::new();
        if claude_exe.is_none() {
            entries.push(Entry::Error(
                "Couldn't find the Claude Code CLI. Install it from https://claude.com/claude-code, then restart Barduino."
                    .into(),
            ));
        }

        Self {
            claude_exe,
            project_dir: std::env::current_dir().unwrap_or_default(),
            permission_mode: PermissionMode::ReadOnly,
            session_id: None,
            model: None,
            entries,
            streaming: String::new(),
            input: String::new(),
            turn: None,
            stop_requested: false,
            focus_composer: true,
            events_tx,
            events_rx,
        }
    }

    fn is_running(&self) -> bool {
        self.turn.is_some()
    }

    fn send(&mut self, ctx: &egui::Context) {
        let prompt = self.input.trim().to_owned();
        let Some(exe) = self.claude_exe.clone() else { return };
        if prompt.is_empty() || self.is_running() {
            return;
        }

        let turn = Turn {
            prompt: prompt.clone(),
            cwd: self.project_dir.clone(),
            resume_session: self.session_id.clone(),
            permission_mode: self.permission_mode,
        };
        let ctx = ctx.clone();
        match claude::start_turn(&exe, turn, self.events_tx.clone(), move || ctx.request_repaint()) {
            Ok(running) => {
                self.entries.push(Entry::User(prompt));
                self.input.clear();
                self.turn = Some(running);
            }
            Err(err) => self.entries.push(Entry::Error(format!("Couldn't start the Claude CLI: {err}"))),
        }
    }

    fn stop(&mut self) {
        if let Some(turn) = &self.turn {
            self.stop_requested = true;
            turn.stop();
        }
    }

    fn new_chat(&mut self) {
        self.session_id = None;
        self.model = None;
        self.entries.clear();
        self.streaming.clear();
    }

    fn handle_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Started { session_id, model } => {
                self.session_id = Some(session_id);
                self.model = Some(model);
            }
            AgentEvent::TextDelta(text) => self.streaming.push_str(&text),
            AgentEvent::Text(text) => {
                self.streaming.clear();
                if !text.trim().is_empty() {
                    self.entries.push(Entry::Claude(text));
                }
            }
            AgentEvent::ToolUse { name, detail } => {
                self.streaming.clear();
                self.entries.push(Entry::Tool { name, detail });
            }
            AgentEvent::ToolResult { text, is_error } => {
                let cut_off = text.chars().count() > MAX_TOOL_OUTPUT_CHARS;
                let mut text: String = text.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
                if cut_off {
                    text.push_str("\n… (cut off)");
                }
                self.entries.push(Entry::ToolOutput { text, is_error });
            }
            AgentEvent::Finished { session_id, error, denied_tools } => {
                // Resuming can hand back a new session ID; always continue from the latest one.
                if session_id.is_some() {
                    self.session_id = session_id;
                }
                if let Some(error) = error {
                    self.entries.push(Entry::Error(error));
                }
                if !denied_tools.is_empty() {
                    self.entries.push(Entry::Notice(format!(
                        "Claude wasn't allowed to use: {}. Change \"Permissions\" at the top to allow more.",
                        denied_tools.join(", ")
                    )));
                }
            }
            AgentEvent::Exited { error } => {
                // Keep any text that was cut off by Stop.
                let partial = std::mem::take(&mut self.streaming);
                if !partial.trim().is_empty() {
                    self.entries.push(Entry::Claude(partial));
                }
                if self.stop_requested {
                    self.entries.push(Entry::Notice("Stopped.".into()));
                } else if let Some(error) = error {
                    self.entries.push(Entry::Error(error));
                }
                self.turn = None;
                self.stop_requested = false;
                self.focus_composer = true;
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let running = self.is_running();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("Barduino");
            ui.separator();
            ui.label("Project:");
            ui.label(egui::RichText::new(self.project_dir.display().to_string()).monospace());
            if ui.add_enabled(!running, egui::Button::new("Change…")).clicked()
                && let Some(dir) = rfd::FileDialog::new().set_directory(&self.project_dir).pick_folder()
                && dir != self.project_dir
            {
                // Claude sessions belong to a folder, so a new folder means a new chat.
                self.project_dir = dir;
                self.new_chat();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Permissions:");
            egui::ComboBox::from_id_salt("permission_mode")
                .selected_text(self.permission_mode.label())
                .show_ui(ui, |ui| {
                    for mode in PermissionMode::ALL {
                        ui.selectable_value(&mut self.permission_mode, mode, mode.label());
                    }
                });
            if ui.add_enabled(!running, egui::Button::new("New chat")).clicked() {
                self.new_chat();
            }
            if let Some(model) = &self.model {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(model).weak());
                });
            }
        });
        ui.add_space(6.0);
    }

    /// Returns true when the user asked to send the message.
    fn composer(&mut self, ui: &mut egui::Ui) -> bool {
        let composer_id = egui::Id::new("composer");
        // Take Enter before the text box sees it; Shift+Enter still adds a new line.
        let enter_pressed = ui.memory(|m| m.has_focus(composer_id))
            && ui.input_mut(|i| !i.modifiers.shift && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let can_send = self.claude_exe.is_some() && !self.is_running() && !self.input.trim().is_empty();

        ui.add_space(6.0);
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.input)
                .id(composer_id)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Ask Claude…  (Enter to send, Shift+Enter for a new line)"),
        );
        if std::mem::take(&mut self.focus_composer) {
            response.request_focus();
        }

        let mut send = enter_pressed && can_send;
        ui.horizontal(|ui| {
            if self.is_running() {
                ui.spinner();
                ui.label("Claude is working…");
                if ui.button("Stop").clicked() {
                    self.stop();
                }
            } else if ui.add_enabled(can_send, egui::Button::new("Send")).clicked() {
                send = true;
            }
        });
        ui.add_space(6.0);
        send
    }

    fn conversation(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.entries.is_empty() && self.streaming.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Pick a project folder and ask Claude something.").weak());
                    });
                }
                for (index, entry) in self.entries.iter().enumerate() {
                    show_entry(ui, index, entry);
                }
                if !self.streaming.is_empty() {
                    ui.label(&self.streaming);
                }
                ui.add_space(8.0);
            });
    }
}

impl eframe::App for BarduinoApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.events_rx.try_recv() {
            self.handle_event(event);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top(egui::Id::new("toolbar")).show(ui, |ui| self.toolbar(ui));
        let send = egui::Panel::bottom(egui::Id::new("composer_panel"))
            .show(ui, |ui| self.composer(ui))
            .inner;
        egui::CentralPanel::default().show(ui, |ui| self.conversation(ui));

        if send {
            let ctx = ui.ctx().clone();
            self.send(&ctx);
        }
    }
}

fn show_entry(ui: &mut egui::Ui, index: usize, entry: &Entry) {
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
        Entry::Claude(text) => {
            ui.label(text);
        }
        Entry::Tool { name, detail } => {
            ui.label(egui::RichText::new(format!("{name}: {detail}")).monospace().weak());
        }
        Entry::ToolOutput { text, is_error } => {
            let title = if *is_error { "Tool error" } else { "Tool output" };
            egui::CollapsingHeader::new(egui::RichText::new(title).small().weak())
                .id_salt(("tool_output", index))
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
