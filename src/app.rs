use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentEvent, PermissionMode};
use crate::browser::BrowserState;
use crate::chat::{self, ComposerAction};
use crate::icons::{self, Icon};
use crate::session::Session;
use crate::settings::{self, Detected, Settings, SettingsAction};
use crate::sidebar::{Sidebar, SidebarAction};
use crate::tools::{Tools, ToolsAction};

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
    settings: Settings,
    browser: BrowserState,
    /// Where older versions saved the browser address. Only read, to carry it over.
    #[serde(skip_serializing)]
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
            settings: Settings::default(),
            browser: BrowserState::default(),
            browser_address: String::new(),
        }
    }
}

pub struct BarduinoApp {
    detected: Detected,
    state: SavedState,
    view: View,
    sidebar: Sidebar,
    tools: Tools,
    /// Agent events, tagged with the ID of the session they belong to.
    events_tx: Sender<(u64, AgentEvent)>,
    events_rx: Receiver<(u64, AgentEvent)>,
}

impl BarduinoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state: SavedState =
            cc.storage.and_then(|storage| eframe::get_value(storage, eframe::APP_KEY)).unwrap_or_default();
        let old_address = std::mem::take(&mut state.browser_address);
        if state.browser.address.is_empty() {
            state.browser.address = old_address;
        }
        let tools = Tools::new(state.browser.clone());
        let (events_tx, events_rx) = mpsc::channel();

        let mut app = Self {
            detected: Detected::scan(),
            state,
            view: View::Chat,
            sidebar: Sidebar::default(),
            tools,
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

    fn active_index(&self) -> usize {
        let id = self.state.active_session;
        self.state.sessions.iter().position(|s| s.id == id).unwrap_or(0)
    }

    fn active_session_mut(&mut self) -> &mut Session {
        let index = self.active_index();
        &mut self.state.sessions[index]
    }

    /// Starts a session with the default provider from Settings.
    fn new_session(&mut self, project_dir: PathBuf, permission_mode: PermissionMode) {
        let id = self.state.next_session_id;
        self.state.next_session_id += 1;
        let provider = self.state.settings.default_provider;
        self.state.sessions.push(Session::new(id, project_dir, provider, permission_mode));
        self.state.active_session = id;
        self.view = View::Chat;
    }

    fn delete_session(&mut self, id: u64) {
        let Some(index) = self.state.sessions.iter().position(|s| s.id == id) else { return };
        // Dropping the session stops its agent if it's still working.
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
        let Some(exe) = self.detected.get(provider).cloned() else { return };
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
        } else {
            // A conversation belongs to its folder, so a different folder gets a new session.
            let permission_mode = session.permission_mode;
            self.new_session(dir, permission_mode);
        }
    }

    fn handle_sidebar(&mut self, action: SidebarAction) {
        match action {
            SidebarAction::None => {}
            SidebarAction::Select(id) => {
                self.view = View::Chat;
                self.state.active_session = id;
                self.active_session_mut().focus_composer = true;
            }
            SidebarAction::NewSession => {
                let session = self.active_session_mut();
                let (project_dir, permission_mode) = (session.project_dir.clone(), session.permission_mode);
                self.new_session(project_dir, permission_mode);
            }
            SidebarAction::Rename(id, title) => {
                if let Some(session) = self.state.sessions.iter_mut().find(|s| s.id == id) {
                    session.title = title;
                }
            }
            SidebarAction::Delete(id) => self.delete_session(id),
            SidebarAction::OpenSettings => {
                self.view = if self.view == View::Settings { View::Chat } else { View::Settings };
            }
            SidebarAction::Collapse => self.state.show_sessions = false,
            SidebarAction::Expand => self.state.show_sessions = true,
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
            SettingsAction::OpenInTerminal(provider) => {
                let cwd = self.active_session_mut().project_dir.clone();
                self.tools.open_terminal(&cwd, Some(format!("{}\r", provider.command())));
                self.state.show_tools = true;
            }
        }
    }

    fn chat_area(&mut self, ui: &mut egui::Ui) {
        let index = self.active_index();
        let session = &mut self.state.sessions[index];
        let settings = &self.state.settings;
        let installed = self.detected.get(session.provider).is_some();

        let composer_action = egui::Panel::bottom(egui::Id::new("composer_panel"))
            .show_separator_line(false)
            .show(ui, |ui| chat::composer(ui, session, settings, installed))
            .inner;
        // Checked again after the composer, which is where the provider can change.
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

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        let collapsed = egui::Panel::left(egui::Id::new("sessions_rail")).resizable(false).exact_size(44.0);
        let expanded = egui::Panel::left(egui::Id::new("sessions_panel"))
            .resizable(true)
            .default_size(260.0)
            .size_range(180.0..=420.0);
        let (sidebar, sessions) = (&mut self.sidebar, &self.state.sessions);
        let active = (self.view == View::Chat).then_some(self.state.active_session);
        let settings_open = self.view == View::Settings;
        let mut show = self.state.show_sessions;
        let action = egui::Panel::show_switched(ui, &mut show, collapsed, expanded, |ui, expanded| {
            if expanded { sidebar.ui(ui, sessions, active, settings_open) } else { sidebar.rail(ui) }
        })
        .inner;
        self.state.show_sessions = show;
        self.handle_sidebar(action);
    }

    fn right_panel(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let collapsed = egui::Panel::right(egui::Id::new("tools_rail")).resizable(false).exact_size(44.0);
        let expanded = egui::Panel::right(egui::Id::new("tools_panel"))
            .resizable(true)
            .default_size(560.0)
            .size_range(300.0..=1600.0);
        let cwd = self.active_session_mut().project_dir.clone();
        let tools = &mut self.tools;
        let (mut collapse, mut expand) = (false, false);
        let mut show = self.state.show_tools;
        let action = egui::Panel::show_switched(ui, &mut show, collapsed, expanded, |ui, expanded| {
            if expanded {
                return tools.ui(ui, frame, &cwd, &mut collapse);
            }
            tools.hide_browser();
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                expand |= icons::button(ui, Icon::SidebarRight, "Show terminal and browser").clicked();
                expand |= tools.add_menu(ui, &cwd);
            });
            ToolsAction::None
        })
        .inner;
        self.state.show_tools = (show || expand) && !collapse;
        if !self.state.show_tools {
            self.tools.hide_browser();
        }

        if let ToolsAction::AddToMessage(text) = action {
            let session = self.active_session_mut();
            if !session.input.trim().is_empty() {
                session.input.push_str("\n\n");
            }
            session.input.push_str(&text);
            session.focus_composer = true;
            self.view = View::Chat;
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
        self.tools.release_focus_on_click(ui.ctx());
        self.left_panel(ui);
        self.right_panel(ui, frame);
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
            View::Chat => self.chat_area(ui),
            View::Settings => self.settings_area(ui),
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.state.browser = self.tools.browser_state().clone();
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
    }
}
