use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentEvent, PermissionMode, Provider};
use crate::browser::Browser;
use crate::chat::{self, ComposerAction};
use crate::session::{Entry, Session};
use crate::settings::{self, Detected, Settings, SettingsAction};
use crate::terminal;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
enum ToolsTab {
    Terminal,
    Browser,
}

/// What the middle column shows.
#[derive(Clone, Copy, PartialEq)]
enum View {
    Chat,
    Settings,
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
    settings: Settings,
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
            settings: Settings::default(),
        }
    }
}

pub struct BarduinoApp {
    detected: Detected,
    state: SavedState,
    view: View,
    browser: Browser,
    /// Text to type into the active session's terminal on the next frame.
    terminal_input: Option<String>,
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

        let mut app = Self {
            detected: Detected::scan(),
            state,
            view: View::Chat,
            browser,
            terminal_input: None,
            events_tx,
            events_rx,
        };
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

    /// Starts a session with the default provider from Settings.
    fn new_session(&mut self, project_dir: PathBuf, permission_mode: PermissionMode) {
        let id = self.state.next_session_id;
        self.state.next_session_id += 1;
        let provider = self.state.settings.default_provider;
        self.state.sessions.push(Session::new(id, project_dir, provider, permission_mode));
        self.state.active_session = id;
    }

    fn delete_session(&mut self, id: u64) {
        let Some(index) = self.state.sessions.iter().position(|s| s.id == id) else { return };
        // Dropping the session stops its agent turn and its terminal.
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
        let provider = self.active_session_mut().provider;
        let Some(launcher) = self.detected.get(provider).cloned() else { return };
        let tx = self.events_tx.clone();
        let ctx = ctx.clone();
        let session = self.active_session_mut();
        let id = session.id;
        session.send(&launcher, move |event| {
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

        egui::Panel::bottom(egui::Id::new("sessions_footer")).show(ui, |ui| {
            ui.add_space(6.0);
            let open = self.view == View::Settings;
            if ui.selectable_label(open, "⚙ Settings").clicked() {
                self.view = if open { View::Chat } else { View::Settings };
            }
            ui.add_space(6.0);
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading("Sessions");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let default = self.state.settings.default_provider.label();
                if ui.button("+ New").on_hover_text(format!("Start a new {default} session in the same folder")).clicked() {
                    let session = self.active_session_mut();
                    let (project_dir, permission_mode) = (session.project_dir.clone(), session.permission_mode);
                    self.new_session(project_dir, permission_mode);
                    self.view = View::Chat;
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Newest first.
            for session in self.state.sessions.iter().rev() {
                let selected = self.view == View::Chat && session.id == self.state.active_session;
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

        if let Some(id) = select {
            self.view = View::Chat;
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
                let typed = self.terminal_input.take();
                let session = self.active_session_mut();
                ui.push_id(("terminal", session.id), |ui| {
                    terminal::show(ui, &mut session.terminal, &session.project_dir, typed);
                });
            }
            ToolsTab::Browser => self.browser.ui(ui, frame),
        }
    }

    fn settings_area(&mut self, ui: &mut egui::Ui) {
        let action = egui::CentralPanel::default()
            .show(ui, |ui| settings::page(ui, &mut self.state.settings, &self.detected))
            .inner;
        match action {
            SettingsAction::None => {}
            SettingsAction::Close => self.view = View::Chat,
            SettingsAction::Rescan => self.detected = Detected::scan(),
            SettingsAction::OpenInTerminal(provider) => self.open_in_terminal(provider),
        }
    }

    /// Shows the terminal and starts the provider's CLI in it.
    fn open_in_terminal(&mut self, provider: Provider) {
        self.state.show_tools = true;
        self.state.tools_tab = ToolsTab::Terminal;
        self.terminal_input = Some(format!("{}\r", provider.command()));
    }

    fn chat_area(&mut self, ui: &mut egui::Ui) {
        let (show_sessions, show_tools) = (&mut self.state.show_sessions, &mut self.state.show_tools);
        let id = self.state.active_session;
        let index = self.state.sessions.iter().position(|s| s.id == id).unwrap_or(0);
        let session = &mut self.state.sessions[index];
        let settings = &self.state.settings;

        egui::Panel::top(egui::Id::new("chat_top_bar"))
            .show(ui, |ui| chat::top_bar(ui, session, show_sessions, show_tools));
        let composer_action = egui::Panel::bottom(egui::Id::new("composer_panel"))
            .show_separator_line(false)
            .show(ui, |ui| {
                let installed = self.detected.get(session.provider).is_some();
                chat::composer(ui, session, settings, installed)
            })
            .inner;
        // Checked after the composer, which is where the provider can change.
        let installed = self.detected.get(session.provider).is_some();
        let mut open_settings = false;
        egui::CentralPanel::default().show(ui, |ui| {
            if !installed {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{} isn't installed. {}.", session.provider.label(), session.provider.install_hint()),
                    );
                    open_settings = ui.link("Open Settings").clicked();
                });
            }
            chat::conversation(ui, session);
        });

        match composer_action {
            ComposerAction::Send => self.send(ui.ctx()),
            ComposerAction::Stop => self.active_session_mut().stop(),
            ComposerAction::ChangeFolder => self.change_folder(),
            ComposerAction::None => {}
        }
        if open_settings {
            self.view = View::Settings;
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

        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
            View::Chat => self.chat_area(ui),
            View::Settings => self.settings_area(ui),
        });
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
                    let mut detail = format!("{} · {}", session.folder_name(), session.provider.short_name());
                    if failed {
                        detail.push_str(" · error");
                    }
                    ui.add(egui::Label::new(egui::RichText::new(detail).small().weak()).truncate());
                });
            });
        });
    })
    .response
}
