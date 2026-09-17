use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentEvent, PermissionMode, Provider};
use crate::browser::BrowserState;
use crate::chat::{self, ComposerAction};
use crate::icons::{self, Icon};
use crate::models::Catalog;
use crate::plan::{self, PlanUsage};
use crate::session::Session;
use crate::settings::{Detected, PageContext, Settings, SettingsAction, SettingsPage};
use crate::sidebar::{Sidebar, SidebarAction};
use crate::tools::{Tools, ToolsAction};
use crate::usage::UsageLog;

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
    usage: UsageLog,
    /// The plan limits each provider last reported.
    plan: std::collections::BTreeMap<Provider, PlanUsage>,
    browser: BrowserState,
    /// Where older versions saved the browser address. Only read, to carry it over.
    #[serde(skip_serializing)]
    browser_address: String,
    /// True in state saved before the right panel started closed and a fifth wide,
    /// so those settings reach people who already had the old ones.
    #[serde(default = "saved_before_panel_defaults")]
    apply_panel_defaults: bool,
}

fn saved_before_panel_defaults() -> bool {
    true
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            active_session: 0,
            next_session_id: 0,
            show_sessions: true,
            show_tools: false,
            settings: Settings::default(),
            usage: UsageLog::default(),
            plan: std::collections::BTreeMap::new(),
            browser: BrowserState::default(),
            browser_address: String::new(),
            apply_panel_defaults: false,
        }
    }
}

pub struct BarduinoApp {
    detected: Detected,
    state: SavedState,
    view: View,
    settings_page: SettingsPage,
    sidebar: Sidebar,
    tools: Tools,
    /// Set until the width egui remembers for the right panel has been forgotten.
    forget_panel_width: bool,
    /// Markdown the conversation has already laid out, kept so it isn't redone each frame.
    markdown: egui_commonmark::CommonMarkCache,
    /// The models each CLI offers, read in the background when first needed.
    models: Catalog,
    /// Set once this launch has asked the agents that answer for free.
    checked_free_plans: bool,
    /// Plan limit checks running in the background, and what they said.
    plan_checks: plan::Checks,
    plan_errors: std::collections::BTreeMap<Provider, String>,
    /// Plan limits being read from disk, for CLIs that only write them there.
    plan_from_disk: std::sync::Arc<std::sync::Mutex<Option<std::collections::BTreeMap<Provider, PlanUsage>>>>,
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
        // Someone who had the panel open keeps it closed from now on, at the new width.
        let apply_panel_defaults = std::mem::take(&mut state.apply_panel_defaults);
        if apply_panel_defaults {
            state.show_tools = false;
        }
        // The right tools panel starts closed by default on launch.
        state.show_tools = false;
        let tools = Tools::new(state.browser.clone());
        let (events_tx, events_rx) = mpsc::channel();

        let mut app = Self {
            detected: Detected::scan(&state.settings, &cc.egui_ctx),
            state,
            view: View::Chat,
            settings_page: SettingsPage::default(),
            sidebar: Sidebar::default(),
            tools,
            markdown: egui_commonmark::CommonMarkCache::default(),
            models: Catalog::default(),
            checked_free_plans: false,
            plan_checks: plan::Checks::default(),
            plan_errors: std::collections::BTreeMap::new(),
            plan_from_disk: plan::read_in_background(&cc.egui_ctx),
            forget_panel_width: apply_panel_defaults,
            events_tx,
            events_rx,
        };
        if app.state.sessions.is_empty() {
            app.new_session(PathBuf::new(), PermissionMode::ReadOnly);
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

    /// The folder the tools panel works in: the session's, or where Barduino was
    /// started from while the session still has none.
    fn tool_cwd(&self) -> PathBuf {
        let session = &self.state.sessions[self.active_index()];
        if session.has_folder() {
            session.project_dir.clone()
        } else {
            std::env::current_dir().unwrap_or_default()
        }
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

    /// Keys that open the tools panel. They're taken before anything is drawn, so
    /// a focused terminal or message box never swallows them.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        // COMMAND is Ctrl on Windows and Linux, and Cmd on macOS.
        const TERMINAL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Backtick);
        const BROWSER: KeyboardShortcut =
            KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::B);

        let (terminal, browser) =
            ctx.input_mut(|i| (i.consume_shortcut(&TERMINAL), i.consume_shortcut(&BROWSER)));
        if !terminal && !browser {
            return;
        }
        let cwd = self.tool_cwd();
        if terminal {
            self.tools.show_terminal(&cwd);
        }
        if browser {
            self.tools.open_browser();
        }
        self.state.show_tools = true;
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
                // A new session starts without a folder, so its first step is choosing one.
                let permission_mode = self.active_session_mut().permission_mode;
                self.new_session(PathBuf::new(), permission_mode);
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
        // The agents that answer for free are asked once per launch, so the page
        // has something to show without the user pressing anything.
        if !self.checked_free_plans {
            self.checked_free_plans = true;
            for provider in Provider::ALL.into_iter().filter(|provider| plan::is_free(*provider)) {
                if let Some(exe) = self.detected.get(provider).cloned() {
                    self.plan_checks.start(provider, exe, ui.ctx());
                }
            }
        }

        let mut session_counts = std::collections::BTreeMap::new();
        for session in &self.state.sessions {
            *session_counts.entry(session.provider).or_default() += 1;
        }
        let context = PageContext {
            detected: &self.detected,
            usage: &self.state.usage,
            plan: &self.state.plan,
            plan_checks: &self.plan_checks,
            plan_errors: &self.plan_errors,
            session_counts,
        };
        let (page, settings) = (&mut self.settings_page, &mut self.state.settings);
        let action = egui::CentralPanel::default().show(ui, |ui| page.ui(ui, settings, &context)).inner;

        match action {
            SettingsAction::None => {}
            SettingsAction::Close => self.view = View::Chat,
            SettingsAction::CheckPlan(providers) => {
                for provider in providers {
                    if let Some(exe) = self.detected.get(provider).cloned() {
                        self.plan_errors.remove(&provider);
                        self.plan_checks.start(provider, exe, ui.ctx());
                    }
                }
            }
            SettingsAction::Rescan => {
                self.detected = Detected::scan(&self.state.settings, ui.ctx());
                self.plan_from_disk = plan::read_in_background(ui.ctx());
            }
            SettingsAction::ChooseExecutable(provider) => {
                let mut dialog = rfd::FileDialog::new().set_title(format!("Choose the {} executable", provider.label()));
                if let Some(dir) = self.detected.get(provider).and_then(|exe| exe.parent()) {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(exe) = dialog.pick_file() {
                    self.state.settings.custom_executables.insert(provider, exe);
                    self.detected = Detected::scan(&self.state.settings, ui.ctx());
                }
            }
            SettingsAction::UseDetectedExecutable(provider) => {
                self.state.settings.custom_executables.remove(&provider);
                self.detected = Detected::scan(&self.state.settings, ui.ctx());
            }
            SettingsAction::ResetUsage => self.state.usage.clear(),
            SettingsAction::OpenInTerminal(provider) => {
                let cwd = self.tool_cwd();
                self.tools.open_terminal(&cwd, Some(format!("{}\r", provider.command())));
                self.state.show_tools = true;
            }
        }
    }

    fn chat_area(&mut self, ui: &mut egui::Ui) {
        let index = self.active_index();
        let provider = self.state.sessions[index].provider;
        let installed = self.detected.get(provider).is_some();
        // A provider's model list is read once per launch, when a session first shows it.
        if let Some(exe) = self.detected.get(provider).cloned() {
            self.models.start(provider, exe, ui.ctx());
        }
        let session = &mut self.state.sessions[index];
        let settings = &self.state.settings;
        let models = &self.models;
        let markdown = &mut self.markdown;

        let composer_action = egui::Panel::bottom(egui::Id::new("composer_panel"))
            .show_separator_line(false)
            .show(ui, |ui| chat::composer(ui, session, settings, models, installed))
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
            chat::conversation(ui, session, markdown);
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
        // A fifth of the window to start with, which the user can then drag wider.
        let default_width = (ui.ctx().viewport_rect().width() * 0.2).clamp(240.0, 900.0);
        let expanded = egui::Panel::right(egui::Id::new("tools_panel"))
            .resizable(true)
            .default_size(default_width)
            .size_range(240.0..=1600.0);
        let cwd = self.tool_cwd();
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
        if collapse || expand {
            ui.ctx().request_repaint();
        }

        match action {
            ToolsAction::Attach(elements) => {
                let session = self.active_session_mut();
                for element in elements {
                    session.attach(element);
                }
                session.focus_composer = true;
                self.view = View::Chat;
            }
            ToolsAction::Send(elements) => {
                let session = self.active_session_mut();
                for element in elements {
                    session.attach(element);
                }
                self.view = View::Chat;
                self.send(ui.ctx());
            }
            ToolsAction::None => {}
        }
    }
}

impl eframe::App for BarduinoApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if std::mem::take(&mut self.forget_panel_width) {
            // egui remembers a panel's width, which would otherwise win over the new default.
            ctx.data_mut(|d| d.remove::<egui::containers::PanelState>(egui::Id::new("tools_panel")));
        }
        for (provider, result) in self.plan_checks.take_finished() {
            match result {
                Ok(plan) => {
                    self.state.plan.insert(provider, plan);
                    self.plan_errors.remove(&provider);
                }
                Err(problem) => {
                    self.plan_errors.insert(provider, problem);
                }
            }
        }
        self.handle_shortcuts(ctx);
        if let Some(found) = self.plan_from_disk.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            self.state.plan.extend(found);
        }
        while let Ok((id, event)) = self.events_rx.try_recv() {
            // Events for a deleted session are dropped.
            if let Some(session) = self.state.sessions.iter_mut().find(|s| s.id == id) {
                if let AgentEvent::Plan(plan) = &event {
                    self.state.plan.insert(session.provider, plan.clone());
                }
                if let AgentEvent::Finished { usage: Some(usage), .. } = &event {
                    self.state.usage.record(session.provider, *usage);
                }
                // The agent may have edited files, so any diff of its folder is out of date.
                if let AgentEvent::Exited { .. } = &event {
                    self.tools.refresh_changes(&session.project_dir, ctx);
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_saved_before_the_panel_defaults_asks_for_them() {
        let old: SavedState = ron::from_str("(show_tools: true)").expect("old state should load");
        assert!(old.apply_panel_defaults, "an older save asks for the new panel defaults");
        assert!(!SavedState::default().apply_panel_defaults, "a new one already has them");

        let saved = ron::to_string(&SavedState::default()).expect("state should save");
        let again: SavedState = ron::from_str(&saved).expect("saved state should load");
        assert!(!again.apply_panel_defaults, "the defaults are only applied once");
        assert!(!again.show_tools, "the right panel starts closed");
    }
}
