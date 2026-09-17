use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::browser::Browser;
use crate::chat::{self, ComposerAction, HeaderAction};
use crate::claude::{self, AgentEvent, PermissionMode};
use crate::session::{Entry, Session};
use crate::terminal;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
enum ToolsTab {
    Terminal,
    Browser,
}

/// Everything that is saved between launches.
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct SavedState {
    sessions: Vec<Session>,
    active_session: u64,
    next_session_id: u64,
    show_sessions: bool,
    show_tools: bool,
    tools_tab: ToolsTab,
    browser_address: String,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            active_session: 0,
            next_session_id: 0,
            show_sessions: true,
            show_tools: true,
            tools_tab: ToolsTab::Terminal,
            browser_address: String::new(),
        }
    }
}

pub struct BarduinoApp {
    claude_exe: Option<PathBuf>,
    state: SavedState,
    browser: Browser,
    /// Agent events, tagged with the ID of the session they belong to.
    events_tx: Sender<(u64, AgentEvent)>,
    events_rx: Receiver<(u64, AgentEvent)>,
}

impl BarduinoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state: SavedState =
            cc.storage.and_then(|storage| eframe::get_value(storage, eframe::APP_KEY)).unwrap_or_default();
        let browser = Browser::new(std::mem::take(&mut state.browser_address));
        let (events_tx, events_rx) = mpsc::channel();

        let mut app = Self { claude_exe: claude::find_executable(), state, browser, events_tx, events_rx };
        if app.state.sessions.is_empty() {
            let project_dir = std::env::current_dir().unwrap_or_default();
            app.new_session(project_dir, PermissionMode::ReadOnly);
        }
        if !app.state.sessions.iter().any(|s| s.id == app.state.active_session) {
            app.state.active_session = app.state.sessions[0].id;
        }
        app.active_session_mut().focus_composer = true;
        app
    }

    fn active_session_mut(&mut self) -> &mut Session {
        let id = self.state.active_session;
        let index = self.state.sessions.iter().position(|s| s.id == id).unwrap_or(0);
        &mut self.state.sessions[index]
    }

    fn new_session(&mut self, project_dir: PathBuf, permission_mode: PermissionMode) {
        let id = self.state.next_session_id;
        self.state.next_session_id += 1;
        self.state.sessions.push(Session::new(id, project_dir, permission_mode));
        self.state.active_session = id;
    }

    fn delete_session(&mut self, id: u64) {
        let Some(index) = self.state.sessions.iter().position(|s| s.id == id) else { return };
        // Dropping the session stops its Claude turn and its terminal.
        let removed = self.state.sessions.remove(index);

        if self.state.sessions.is_empty() {
            self.new_session(removed.project_dir, removed.permission_mode);
        } else if self.state.active_session == id {
            let next = &mut self.state.sessions[index.saturating_sub(1)];
            next.focus_composer = true;
            self.state.active_session = next.id;
        }
    }

    fn send(&mut self, ctx: &egui::Context) {
        let Some(exe) = self.claude_exe.clone() else { return };
        let tx = self.events_tx.clone();
        let ctx = ctx.clone();
        let session = self.active_session_mut();
        let id = session.id;
        session.send(&exe, move |event| {
            let _ = tx.send((id, event));
            ctx.request_repaint();
        });
    }

    fn change_folder(&mut self) {
        let session = self.active_session_mut();
        let Some(dir) = rfd::FileDialog::new().set_directory(&session.project_dir).pick_folder() else {
            return;
        };
        if dir == session.project_dir {
            return;
        }
        if session.entries.is_empty() {
            session.project_dir = dir;
            // Start the terminal again in the new folder.
            session.terminal = None;
        } else {
            // A conversation belongs to its folder, so a different folder gets a new session.
            let permission_mode = session.permission_mode;
            self.new_session(dir, permission_mode);
        }
    }

    fn sessions_panel(&mut self, ui: &mut egui::Ui) {
        let mut select = None;
        let mut delete = None;

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading("Sessions");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ New").on_hover_text("Start a new session in the same folder").clicked() {
                    let session = self.active_session_mut();
                    let (project_dir, permission_mode) = (session.project_dir.clone(), session.permission_mode);
                    self.new_session(project_dir, permission_mode);
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Newest first.
            for session in self.state.sessions.iter().rev() {
                let selected = session.id == self.state.active_session;
                let response = session_row(ui, session, selected);
                if response.clicked() {
                    select = Some(session.id);
                }
                response.context_menu(|ui| {
                    if ui.button("Delete session").clicked() {
                        delete = Some(session.id);
                        ui.close();
                    }
                });
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Right-click a session to delete it.").small().weak());
        });

        if let Some(id) = select
            && id != self.state.active_session
        {
            self.state.active_session = id;
            self.active_session_mut().focus_composer = true;
        }
        if let Some(id) = delete {
            self.delete_session(id);
        }
    }

    fn tools_panel(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.state.tools_tab, ToolsTab::Terminal, "Terminal");
            ui.selectable_value(&mut self.state.tools_tab, ToolsTab::Browser, "Browser");
        });
        ui.separator();

        match self.state.tools_tab {
            ToolsTab::Terminal => {
                let session = self.active_session_mut();
                ui.push_id(("terminal", session.id), |ui| {
                    terminal::show(ui, &mut session.terminal, &session.project_dir);
                });
            }
            ToolsTab::Browser => self.browser.ui(ui, frame),
        }
    }

    fn chat_area(&mut self, ui: &mut egui::Ui) {
        let claude_available = self.claude_exe.is_some();
        let (show_sessions, show_tools) = (&mut self.state.show_sessions, &mut self.state.show_tools);
        let id = self.state.active_session;
        let index = self.state.sessions.iter().position(|s| s.id == id).unwrap_or(0);
        let session = &mut self.state.sessions[index];

        let header_action = egui::Panel::top(egui::Id::new("chat_header"))
            .show(ui, |ui| chat::header(ui, session, show_sessions, show_tools))
            .inner;
        let composer_action = egui::Panel::bottom(egui::Id::new("composer_panel"))
            .show(ui, |ui| chat::composer(ui, session, claude_available))
            .inner;
        egui::CentralPanel::default().show(ui, |ui| {
            if !claude_available {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Couldn't find the Claude Code CLI. Install it from https://claude.com/claude-code, then restart Barduino.",
                );
            }
            chat::conversation(ui, session);
        });

        match composer_action {
            ComposerAction::Send => self.send(ui.ctx()),
            ComposerAction::Stop => self.active_session_mut().stop(),
            ComposerAction::None => {}
        }
        if let HeaderAction::ChangeFolder = header_action {
            self.change_folder();
        }
    }
}

impl eframe::App for BarduinoApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok((id, event)) = self.events_rx.try_recv() {
            // Events for a deleted session are dropped.
            if let Some(session) = self.state.sessions.iter_mut().find(|s| s.id == id) {
                session.handle_event(event);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.browser.release_focus_on_click(ui.ctx());

        if self.state.show_sessions {
            egui::Panel::left(egui::Id::new("sessions_panel"))
                .resizable(true)
                .default_size(240.0)
                .size_range(160.0..=420.0)
                .show(ui, |ui| self.sessions_panel(ui));
        }

        let browser_showing = self.state.show_tools && self.state.tools_tab == ToolsTab::Browser;
        if self.state.show_tools {
            egui::Panel::right(egui::Id::new("tools_panel"))
                .resizable(true)
                .default_size(460.0)
                .size_range(280.0..=1000.0)
                .show(ui, |ui| self.tools_panel(ui, frame));
        }
        if !browser_showing {
            self.browser.hide();
        }

        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.chat_area(ui));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.state.browser_address = self.browser.address().to_owned();
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
    }
}

fn session_row(ui: &mut egui::Ui, session: &Session, selected: bool) -> egui::Response {
    ui.scope_builder(egui::UiBuilder::new().id_salt(("session_row", session.id)).sense(egui::Sense::click()), |ui| {
        let response = ui.response();
        let visuals = ui.style().interact_selectable(&response, selected);
        let fill = if selected || response.hovered() { visuals.weak_bg_fill } else { egui::Color32::TRANSPARENT };

        egui::Frame::new().fill(fill).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if session.is_running() {
                    ui.spinner();
                }
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&session.title).color(visuals.text_color())).truncate());
                    let failed = !session.is_running() && matches!(session.entries.last(), Some(Entry::Error(_)));
                    let detail =
                        if failed { format!("{} · error", session.folder_name()) } else { session.folder_name() };
                    ui.add(egui::Label::new(egui::RichText::new(detail).small().weak()).truncate());
                });
            });
        });
    })
    .response
}
