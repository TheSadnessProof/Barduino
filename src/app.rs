use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentEvent, PermissionMode, Provider};
use crate::agent_terminal::{AgentTerminals, TerminalAction};
use crate::browser::{Browser, BrowserState};
use crate::chat::{self, ComposerAction};
use crate::commands::SlashAction;
use crate::icons::{self, Icon};
use crate::models::Catalog;
use crate::plan::{self, PlanUsage};
use crate::session::Session;
use crate::settings::{Detected, PageContext, Settings, SettingsAction, SettingsPage};
use crate::sidebar::{Sidebar, SidebarAction};
use crate::terminal;
use crate::tools::{PanelContext, Tools, ToolsAction};
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

/// Copies a save that couldn't be read somewhere safe, before eframe writes over
/// it. Returns what to tell the user, when there is anything to tell.
fn keep_unreadable_save(raw: String) -> Option<String> {
    let backup = eframe::storage_dir("Barduino")?.join("app.ron.bak");
    std::fs::write(&backup, raw).ok()?;
    Some(format!(
        "Your saved sessions couldn't be read, so Barduino has started empty. The old file was kept at {} — \
         keep hold of it if you want them back.",
        backup.display()
    ))
}

/// Sessions saved before each of them had its own browser take the one address
/// that used to be saved for the whole app, so nobody loses the page they had open.
fn carry_browser_over(sessions: &mut [Session], saved: BrowserState) {
    if saved == BrowserState::default() {
        return;
    }
    for session in sessions.iter_mut().filter(|s| s.browser == BrowserState::default()) {
        session.browser = saved.clone();
    }
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
    /// The right-hand panel for each session, kept by session ID so switching
    /// session swaps the tabs instead of carrying one project's into the next.
    /// Terminals stay alive in here while their session is hidden.
    tools: std::collections::BTreeMap<u64, Tools>,
    /// The system WebView, which all the panels take turns showing.
    browser: Browser,
    /// Each session's agent running in its own terminal, when the middle column is
    /// the CLI's own interface rather than Barduino's chat.
    agent_terminals: AgentTerminals,
    /// The shells this computer offers, found once and after a rescan.
    shells: Vec<terminal::Shell>,
    /// Set until the width egui remembers for the right panel has been forgotten.
    forget_panel_width: bool,
    /// Something the user has to be told, shown across the top until dismissed.
    notice: Option<String>,
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
        let stored = cc.storage.and_then(|storage| eframe::get_value::<SavedState>(storage, eframe::APP_KEY));
        // Nothing saved is an ordinary first launch. Something saved that won't read
        // is not: eframe hands back None either way, and its next autosave — thirty
        // seconds later — would write an empty state over the file. So it is copied
        // aside first, and the user is told where it went.
        let mut notice = None;
        let mut state = stored.unwrap_or_else(|| {
            notice = cc
                .storage
                .and_then(|storage| storage.get_string(eframe::APP_KEY))
                .and_then(keep_unreadable_save);
            SavedState::default()
        });
        let old_address = std::mem::take(&mut state.browser_address);
        if state.browser.address.is_empty() {
            state.browser.address = old_address;
        }
        let apply_panel_defaults = std::mem::take(&mut state.apply_panel_defaults);
        // Settings saved before the agent ran in a terminal are moved over once, so
        // this reaches people who already had Barduino without them going to look.
        if std::mem::take(&mut state.settings.apply_terminal_chat) {
            state.settings.chat_in_terminal = true;
        }
        // The right tools panel starts closed on every launch, new width and all.
        state.show_tools = false;
        // Live terminals and browser pages are keyed by session ID, so a repeated ID
        // would have two sessions sharing one panel and one agent's output going to
        // the other. Whatever the file says, start above every ID already in it.
        let highest = state.sessions.iter().map(|session| session.id).max();
        if let Some(highest) = highest {
            state.next_session_id = state.next_session_id.max(highest + 1);
        }
        carry_browser_over(&mut state.sessions, std::mem::take(&mut state.browser));
        let (events_tx, events_rx) = mpsc::channel();

        let mut app = Self {
            detected: Detected::scan(&state.settings, &cc.egui_ctx),
            state,
            view: View::Chat,
            settings_page: SettingsPage::default(),
            sidebar: Sidebar::default(),
            tools: std::collections::BTreeMap::new(),
            browser: Browser::default(),
            agent_terminals: AgentTerminals::default(),
            shells: terminal::available_shells(),
            markdown: egui_commonmark::CommonMarkCache::default(),
            models: Catalog::default(),
            checked_free_plans: false,
            plan_checks: plan::Checks::default(),
            plan_errors: std::collections::BTreeMap::new(),
            plan_from_disk: plan::read_in_background(&cc.egui_ctx),
            forget_panel_width: apply_panel_defaults,
            notice,
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

    /// The right-hand panel belonging to the active session, started on first use.
    fn active_tools(&mut self) -> &mut Tools {
        self.tools.entry(self.state.active_session).or_default()
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
        // Dropping the session stops its agent if it's still working, and dropping
        // its panel stops any shell it had open.
        let removed = self.state.sessions.remove(index);
        self.tools.remove(&id);
        // And its agent terminal, which is holding a CLI open.
        self.agent_terminals.close(id);

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
            self.active_tools().show_terminal(&cwd);
        }
        if browser {
            self.active_tools().show_browser();
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
        // A path that isn't valid UTF-8 can't be serialized, and eframe only logs
        // that failure — so every save from then on would quietly do nothing.
        if dir.to_str().is_none() {
            self.notice = Some(format!(
                "Barduino can't work in {} — the folder name has characters it can't save.",
                dir.display()
            ));
            return;
        }
        if session.entries.is_empty() {
            let id = session.id;
            session.project_dir = dir;
            // The panel's terminals were started in the old folder — before one was
            // chosen, that is Barduino's own — and its Changes tab watches it. Left
            // alone they would quietly be about the wrong project, so they go.
            self.tools.remove(&id);
            // The agent terminal was started in the old folder too, and an agent
            // reads the folder it was launched in.
            self.agent_terminals.close(id);
        } else {
            // A conversation belongs to its folder, so a different folder gets a new session.
            let permission_mode = session.permission_mode;
            self.new_session(dir, permission_mode);
        }
    }

    fn handle_sidebar(&mut self, action: SidebarAction, ctx: &egui::Context) {
        match action {
            SidebarAction::None => {}
            SidebarAction::Select(id) => {
                self.view = View::Chat;
                self.state.active_session = id;
                self.active_session_mut().focus_composer = true;
                // The terminal chat has no composer to focus, so it takes the
                // keyboard itself — otherwise switching session leaves you typing
                // into nothing. Only asked for when it is the middle column, since
                // asking would otherwise create the terminal it is asking about.
                if self.state.settings.chat_in_terminal {
                    self.agent_terminals.get(id).take_keyboard();
                }
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
            SidebarAction::NewSessionIn(dir) => {
                // Started from a project heading, so the folder is already settled.
                let permission_mode = self.active_session_mut().permission_mode;
                self.new_session(dir, permission_mode);
            }
            SidebarAction::OpenChanges(dir) => {
                self.active_tools().open_changes(&dir, ctx);
                self.state.show_tools = true;
            }
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
            shells: &self.shells,
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
                self.shells = terminal::available_shells();
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
                self.active_tools().open_terminal(&cwd, Some(format!("{}\r", provider.command())));
                self.state.show_tools = true;
            }
        }
    }

    fn chat_area(&mut self, ui: &mut egui::Ui) {
        if self.state.settings.chat_in_terminal {
            self.terminal_chat_area(ui);
            return;
        }
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
            .frame(egui::Frame::NONE.fill(ui.visuals().panel_fill))
            .show(ui, |ui| chat::composer(ui, session, models, installed))
            .inner;
        // Checked again after the composer, which is where the provider can change.
        let installed = self.detected.get(session.provider).is_some();
        let mut open_settings = false;
        let conv_action = egui::CentralPanel::default().show(ui, |ui| {
            if !installed {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{} isn't installed. {}.", session.provider.label(), session.provider.install_hint()),
                    );
                    open_settings = ui.link("Open Settings").clicked();
                });
            }
            chat::conversation(ui, session, settings, markdown)
        }).inner;

        match conv_action {
            chat::ConversationAction::ChangeFolder => self.change_folder(),
            chat::ConversationAction::SelectProvider(p) => {
                self.select_provider(p);
            }
            // Only the terminal chat has a Start button; this one starts an agent
            // when the first message is sent.
            chat::ConversationAction::Start | chat::ConversationAction::None => {}
        }

        match composer_action {
            ComposerAction::Send => self.send(ui.ctx()),
            ComposerAction::Stop => self.active_session_mut().stop(),
            ComposerAction::ChangeFolder => self.change_folder(),
            ComposerAction::Apply(setting) => self.apply_setting(setting),
            ComposerAction::Notice(message) => self.notice = Some(message),
            ComposerAction::None => {}
        }
        if open_settings {
            self.view = View::Settings;
        }
    }

    /// The middle column when the agent runs in its own terminal: no composer and no
    /// transcript, because the CLI draws both of those itself.
    fn terminal_chat_area(&mut self, ui: &mut egui::Ui) {
        let index = self.active_index();
        let session = &self.state.sessions[index];
        let (id, provider) = (session.id, session.provider);
        let shell = self.state.settings.shell(&self.shells);
        let installed = self.detected.get(provider).is_some();
        let settings = &self.state.settings;
        let terminals = &mut self.agent_terminals;
        let mut open_settings = false;

        let action = egui::CentralPanel::default()
            .show(ui, |ui| {
                if !installed {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("{} isn't installed. {}.", provider.label(), provider.install_hint()),
                        );
                        open_settings = ui.link("Open Settings").clicked();
                    });
                }
                terminals.get(id).ui(ui, session, settings, &shell)
            })
            .inner;

        match action {
            TerminalAction::ChangeFolder => self.change_folder(),
            TerminalAction::SelectProvider(provider) => self.select_provider(provider),
            TerminalAction::None => {}
        }
        if open_settings {
            self.view = View::Settings;
        }
    }

    /// Points a session at a different agent. Only before it has said anything: the
    /// conversation so far belongs to the CLI that held it, and the model and effort
    /// are that CLI's own words, so they go back to its defaults.
    fn select_provider(&mut self, provider: Provider) {
        let session = self.active_session_mut();
        if session.can_change_provider() && session.provider != provider {
            session.provider = provider;
            session.chosen_model = None;
            session.effort = None;
        }
    }

    /// Carries out a slash command that belongs to Barduino rather than to the CLI.
    /// The CLIs do these from their own interactive session; a headless run has no
    /// such session, so `/model opus` would otherwise just be words in a prompt.
    fn apply_setting(&mut self, setting: SlashAction) {
        let session = self.active_session_mut();
        match setting {
            SlashAction::Model(model) => {
                session.chosen_model = model;
                // An effort level the new model doesn't offer is dropped by the
                // picker on the next frame, the same as when the model is changed there.
            }
            SlashAction::Effort(effort) => session.effort = effort,
            SlashAction::Permission(mode) => session.permission_mode = mode,
            SlashAction::Clear => {
                let (dir, permission_mode) = (session.project_dir.clone(), session.permission_mode);
                self.new_session(dir, permission_mode);
            }
            SlashAction::OpenSettings => self.view = View::Settings,
            SlashAction::OpenTerminal(command) => {
                let provider = session.provider;
                let cwd = self.tool_cwd();
                self.active_tools().open_terminal(&cwd, Some(format!("{}\r", provider.command())));
                self.state.show_tools = true;
                self.notice = Some(format!(
                    "“/{command}” is one only {} itself can run, so it has been started in a terminal on the \
                     right. Type “/{command}” there.",
                    provider.label()
                ));
            }
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
            if expanded { sidebar.ui(ui, sessions, active, settings_open) } else { sidebar.rail(ui, sessions) }
        })
        .inner;
        self.state.show_sessions = show;
        self.handle_sidebar(action, ui.ctx());
    }

    /// A line across the top for something the user has to see, such as a save that
    /// couldn't be read. It stays until dismissed, because it is usually the only
    /// chance they get to rescue the old file.
    fn notice_banner(&mut self, ui: &mut egui::Ui) {
        let Some(text) = self.notice.clone() else { return };
        let mut dismissed = false;
        egui::Panel::top(egui::Id::new("notice_banner")).show(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(&text).color(ui.visuals().warn_fg_color));
                if ui.small_button("Dismiss").clicked() {
                    dismissed = true;
                }
            });
            ui.add_space(5.0);
        });
        if dismissed {
            self.notice = None;
        }
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
        let shell = self.state.settings.shell(&self.shells);
        let index = self.active_index();
        let id = self.state.active_session;
        let (mut collapse, mut expand) = (false, false);
        let mut show = self.state.show_tools;
        // Three separate fields: this session's panel, the shared WebView, and the
        // page this session wants in it.
        let tools = self.tools.entry(id).or_default();
        let browser = &mut self.browser;
        let page = &mut self.state.sessions[index].browser;
        let action = egui::Panel::show_switched(ui, &mut show, collapsed, expanded, |ui, expanded| {
            if expanded {
                let session = PanelContext { id, cwd: &cwd, shell: &shell, browser, page };
                return tools.ui(ui, frame, session, &mut collapse);
            }
            browser.hide();
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
            self.browser.hide();
        }
        // One WebView is shared between every session, so it outlives any single
        // panel: closing a browser tab in one session must not destroy the page
        // another session still has open. It goes only once nobody wants it.
        if !self.tools.values().any(Tools::wants_browser) {
            self.browser.close();
        }
        if collapse || expand {
            ui.ctx().request_repaint();
        }

        match action {
            // Whatever the middle column is, what was picked in the browser goes into
            // it. In the chat that is a chip above the message box; in a terminal it
            // is a paste into the CLI's own prompt.
            ToolsAction::Attach(elements) => {
                self.view = View::Chat;
                if self.state.settings.chat_in_terminal {
                    let id = self.state.active_session;
                    self.agent_terminals.get(id).attach(&elements, false);
                    return;
                }
                let session = self.active_session_mut();
                for element in elements {
                    session.attach(element);
                }
                session.focus_composer = true;
            }
            ToolsAction::Send(elements) => {
                self.view = View::Chat;
                if self.state.settings.chat_in_terminal {
                    let id = self.state.active_session;
                    self.agent_terminals.get(id).attach(&elements, true);
                    return;
                }
                let session = self.active_session_mut();
                for element in elements {
                    session.attach(element);
                }
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
                    // Kept per session as well as in the daily totals, because its
                    // input side is what the composer shows as the context so far.
                    session.last_usage = Some(*usage);
                }
                // The agent may have edited files, so any diff of its folder is out of date.
                if let AgentEvent::Exited { .. } = &event {
                    for panel in self.tools.values_mut() {
                        panel.refresh_changes(&session.project_dir, ctx);
                    }
                    // The project heading's changed count is out of date too.
                    self.sidebar.invalidate(&session.project_dir);
                }
                session.handle_event(event);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.browser.release_focus_on_click(ui.ctx());
        self.notice_banner(ui);
        self.left_panel(ui);
        self.right_panel(ui, frame);
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
            View::Chat => self.chat_area(ui),
            View::Settings => self.settings_area(ui),
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The state this app actually saves, rather than an empty one. Everything below
    /// goes through RON, which is what eframe writes.
    fn populated_state() -> SavedState {
        let mut session = Session::new(7, PathBuf::from(r"C:\worklpha"), Provider::Codex, PermissionMode::Full);
        session.title = "Fix the login form".into();
        session.entries.push(crate::session::Entry::User(crate::session::UserMessage { text: "hi".into(), elements: Vec::new() }));
        session.entries.push(crate::session::Entry::Agent("hello".into()));
        session.entries.push(crate::session::Entry::Error("boom".into()));
        session.input = "half-written".into();
        session.browser.address = "localhost:5173".into();

        let mut state = SavedState { sessions: vec![session], active_session: 7, next_session_id: 8, ..Default::default() };
        state.settings.shell = Some(PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        state.plan.insert(
            Provider::Claude,
            PlanUsage {
                windows: vec![crate::plan::Window { name: "five-hour".into(), used: 0.25, resets_at: Some(1) }],
                notes: vec!["Max plan".into()],
                read_at: 42,
            },
        );
        state
    }

    #[test]
    fn a_real_state_survives_the_round_trip_eframe_does() {
        let saved = ron::to_string(&populated_state()).expect("should save");
        let back: SavedState = ron::from_str(&saved).expect(&saved);
        assert_eq!(back.sessions.len(), 1);
        assert_eq!(back.sessions[0].entries.len(), 3, "the conversation comes back whole");
        assert_eq!(back.sessions[0].input, "half-written", "including the unsent message");
        assert_eq!(back.sessions[0].browser.address, "localhost:5173");
        assert_eq!(back.sessions[0].permission_mode, PermissionMode::Full);
        assert_eq!(back.settings.shell, populated_state().settings.shell);
        assert_eq!(back.plan[&Provider::Claude].windows[0].used, 0.25);
        assert_eq!(back.next_session_id, 8);
    }

    #[test]
    fn one_missing_field_costs_that_field_and_not_every_session() {
        // Everything in SavedState lives in one RON document, so a struct without
        // defaults used to take the whole file — every session — down with it.
        let with_thin_plan = r#"(sessions: [], plan: {Claude: (windows: [])})"#;
        let state: SavedState = ron::from_str(with_thin_plan).expect("a thin plan entry must still load");
        assert_eq!(state.plan[&Provider::Claude].read_at, 0, "the figure is lost, the file is not");

        let thin_window = r#"(sessions: [], plan: {Claude: (windows: [(name: "five-hour")])})"#;
        let state: SavedState = ron::from_str(thin_window).expect("a thin window must still load");
        assert_eq!(state.plan[&Provider::Claude].windows[0].used, 0.0);

        // A session that has lost a core field keeps the rest of the file readable,
        // and comes back read-only rather than with more access than was chosen.
        let thin_session = r#"(sessions: [(id: 3, title: "kept", entries: [Agent("hi")])])"#;
        let state: SavedState = ron::from_str(thin_session).expect("a thin session must still load");
        assert_eq!(state.sessions[0].title, "kept");
        assert_eq!(state.sessions[0].entries.len(), 1);
        assert_eq!(state.sessions[0].permission_mode, PermissionMode::ReadOnly, "never more than was granted");
    }

    #[test]
    fn a_reused_session_id_cannot_come_back_from_disk() {
        // Live terminals and browser pages are keyed by session ID, so a repeat would
        // have two sessions sharing one panel and misroute an agent's output.
        let saved = r#"(sessions: [(id: 0, title: "a", entries: []), (id: 9, title: "b", entries: [])])"#;
        let mut state: SavedState = ron::from_str(saved).expect("should load");
        assert_eq!(state.next_session_id, 0, "the file says zero, which is already taken");

        let highest = state.sessions.iter().map(|session| session.id).max();
        if let Some(highest) = highest {
            state.next_session_id = state.next_session_id.max(highest + 1);
        }
        assert_eq!(state.next_session_id, 10, "so the next one starts clear of both");
    }

    #[test]
    fn the_one_saved_browser_address_reaches_every_old_session() {
        let session = |id: u64| Session::new(id, PathBuf::new(), Provider::Claude, PermissionMode::ReadOnly);
        let mut sessions = vec![session(1), session(2)];
        // One session had already chosen its own page and keeps it.
        sessions[1].browser.address = "localhost:5173".into();

        let saved = BrowserState { address: "localhost:3000".into(), ..BrowserState::default() };
        carry_browser_over(&mut sessions, saved);
        assert_eq!(sessions[0].browser.address, "localhost:3000", "the old address carries over");
        assert_eq!(sessions[1].browser.address, "localhost:5173", "a session's own page wins");

        // Nothing saved means nothing to carry, so new sessions stay on their default.
        let mut fresh = vec![session(3)];
        carry_browser_over(&mut fresh, BrowserState::default());
        assert_eq!(fresh[0].browser, BrowserState::default());
    }

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
