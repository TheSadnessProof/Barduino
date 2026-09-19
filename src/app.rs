use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentEvent, PermissionMode, Provider};
use crate::browser::{Browser, BrowserState};
use crate::commands::SlashAction;
use crate::icons::{self, Icon};
use crate::models::Catalog;
use crate::plan::{self, PlanUsage};
use crate::preview;
use crate::session::Session;
use crate::settings::{Detected, PageContext, Settings, SettingsAction, SettingsPage};
use crate::sidebar::{Sidebar, SidebarAction};
use crate::terminal;
use crate::tools::{PanelContext, Tools, ToolsAction};
use crate::usage::UsageLog;

/// What the middle column shows.
#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Chat,
    Settings,
}

/// What the central area displays for the active session.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CentralState {
    /// The active session does not have a project folder chosen yet.
    NeedsFolder,
    /// The active session has a folder, but its provider CLI is not installed.
    MissingCli(Provider),
    /// The session has a folder and detected CLI executable.
    TerminalReady,
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

fn write_backup_to_dir(dir: &std::path::Path, state: &SavedState) -> bool {
    let Ok(serialized) = ron::to_string(state) else { return false };
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let temp_path = dir.join("app.ron.tmp");
    let backup_path = dir.join("app.ron.bak");

    if std::fs::write(&temp_path, serialized.as_bytes()).is_ok() {
        if std::fs::rename(&temp_path, &backup_path).is_err() {
            let _ = std::fs::remove_file(&backup_path);
            if std::fs::rename(&temp_path, &backup_path).is_err() {
                let _ = std::fs::remove_file(&temp_path);
                return false;
            }
        }
        return true;
    }
    false
}

/// Atomically writes a backup of the saved state to disk, so unexpected crashes
/// or corrupted writes never destroy the user's session history.
fn atomic_backup_state(state: &SavedState) {
    let Some(dir) = eframe::storage_dir("Viper") else { return };
    write_backup_to_dir(&dir, state);
}

/// Copies a save that couldn't be read somewhere safe, before eframe writes over
/// it. Returns what to tell the user, when there is anything to tell.
fn keep_unreadable_save(raw: String) -> Option<String> {
    let backup = eframe::storage_dir("Viper")?.join("app.ron.corrupt");
    std::fs::write(&backup, raw).ok()?;
    Some(format!(
        "Your saved sessions couldn't be read, so Viper has started empty. The old file was kept at {} — \
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

pub struct ViperApp {
    detected: Detected,
    state: SavedState,
    view: View,
    settings_page: SettingsPage,
    sidebar: Sidebar,
    /// The interactive provider CLI terminal for each session, kept by session ID
    /// so switching sessions swaps the terminal buffer and keeps the CLI running.
    provider_terminals: std::collections::BTreeMap<u64, Result<terminal::Terminal, String>>,
    /// The right-hand panel for each session, kept by session ID so switching
    /// session swaps the tabs instead of carrying one project's into the next.
    /// Terminals stay alive in here while their session is hidden.
    tools: std::collections::BTreeMap<u64, Tools>,
    /// The system WebView, which all the panels take turns showing.
    browser: Browser,
    /// The shells this computer offers, found once and after a rescan.
    shells: Vec<terminal::Shell>,
    /// Set until the width egui remembers for the right panel has been forgotten.
    forget_panel_width: bool,
    /// Something the user has to be told, shown across the top until dismissed.
    notice: Option<String>,
    /// Markdown the conversation has already laid out, kept so it isn't redone each frame.
    #[allow(dead_code)]
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

impl ViperApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let stored = cc.storage.and_then(|storage| eframe::get_value::<SavedState>(storage, eframe::APP_KEY));
        // Nothing saved is an ordinary first launch. Something saved that won't read
        // is not: eframe hands back None either way, and its next autosave — thirty
        // seconds later — would write an empty state over the file. So it is copied
        // aside first, and the user is told where it went.
        let mut notice = None;
        let mut state = stored.unwrap_or_else(|| {
            if let Some(storage_dir) = eframe::storage_dir("Viper") {
                let backup = storage_dir.join("app.ron.bak");
                if let Ok(raw) = std::fs::read_to_string(&backup)
                    && let Ok(recovered) = ron::from_str::<SavedState>(&raw)
                {
                    if let Some(unreadable) = cc.storage.and_then(|storage| storage.get_string(eframe::APP_KEY)) {
                        let _ = keep_unreadable_save(unreadable);
                    }
                    notice = Some("Recovered your saved sessions from the backup copy.".into());
                    return recovered;
                }
                let primary = storage_dir.join("app.ron");
                if let Ok(raw) = std::fs::read_to_string(&primary)
                    && let Ok(recovered) = ron::from_str::<SavedState>(&raw)
                {
                    return recovered;
                }
            }
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
            provider_terminals: std::collections::BTreeMap::new(),
            tools: std::collections::BTreeMap::new(),
            browser: Browser::default(),
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

    #[cfg(test)]
    fn test_app(state: SavedState) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        Self {
            detected: Detected::default(),
            state,
            view: View::Chat,
            settings_page: SettingsPage::default(),
            sidebar: Sidebar::default(),
            provider_terminals: std::collections::BTreeMap::new(),
            tools: std::collections::BTreeMap::new(),
            browser: Browser::default(),
            shells: Vec::new(),
            markdown: egui_commonmark::CommonMarkCache::default(),
            models: Catalog::default(),
            checked_free_plans: true,
            plan_checks: plan::Checks::default(),
            plan_errors: std::collections::BTreeMap::new(),
            plan_from_disk: std::sync::Arc::new(std::sync::Mutex::new(None)),
            forget_panel_width: false,
            notice: None,
            events_tx,
            events_rx,
        }
    }

    fn active_index(&self) -> usize {
        let id = self.state.active_session;
        self.state.sessions.iter().position(|s| s.id == id).unwrap_or(0)
    }

    /// The folder the tools panel works in: the session's, or where Viper was
    /// started from while the session still has none.
    fn tool_cwd(&self) -> PathBuf {
        let session = &self.state.sessions[self.active_index()];
        if session.has_folder() {
            session.working_dir().to_path_buf()
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

    /// Determines the display state of the central view for the active session.
    #[cfg(test)]
    pub(crate) fn central_state(&self) -> CentralState {
        let index = self.active_index();
        let session = &self.state.sessions[index];
        if !session.has_folder() {
            CentralState::NeedsFolder
        } else if self.detected.get(session.provider).is_none() {
            CentralState::MissingCli(session.provider)
        } else {
            CentralState::TerminalReady
        }
    }

    #[cfg(test)]
    fn active_session(&self) -> &Session {
        let index = self.active_index();
        &self.state.sessions[index]
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

    /// Sets up an isolated git worktree for a session, checking out a dedicated branch.
    // Public session setup API for worktree isolation.
    #[allow(dead_code)]
    pub fn setup_session_worktree(&mut self, session_id: u64, base_ref: Option<&str>) -> Result<PathBuf, String> {
        let session = self
            .state
            .sessions
            .iter_mut()
            .find(|s| s.id == session_id)
            .ok_or_else(|| format!("Session {session_id} not found."))?;

        if !session.has_folder() {
            return Err("Session has no project folder configured.".to_owned());
        }

        let (path, branch) =
            crate::worktree::create_worktree(&session.project_dir, session_id, None, base_ref)?;
        session.worktree_dir = Some(path.clone());
        session.worktree_branch = Some(branch);
        session.worktree_base = Some(base_ref.unwrap_or("HEAD").to_owned());
        Ok(path)
    }

    fn delete_session(&mut self, id: u64) {
        let Some(index) = self.state.sessions.iter().position(|s| s.id == id) else { return };
        // Dropping the session stops its agent if it's still working, and dropping
        // its panel stops any shell it had open.
        let removed = self.state.sessions.remove(index);
        self.tools.remove(&id);
        self.provider_terminals.remove(&id);

        if let Some(ref path) = removed.worktree_dir {
            let _ = crate::worktree::remove_worktree(&removed.project_dir, path, true);
            if let Some(ref branch) = removed.worktree_branch {
                let _ = crate::worktree::delete_branch(&removed.project_dir, branch, true);
            }
        }

        if self.state.sessions.is_empty() {
            self.new_session(removed.project_dir, removed.permission_mode);
        } else if self.state.active_session == id {
            let next = &mut self.state.sessions[index.saturating_sub(1)];
            next.focus_composer = true;
            self.state.active_session = next.id;
        }
    }

    #[allow(dead_code)] // Retained for compatibility with headless agent turns.
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
                "Viper can't work in {} — the folder name has characters it can't save.",
                dir.display()
            ));
            return;
        }
        if session.entries.is_empty() {
            let id = session.id;
            session.project_dir = dir;
            // The panel's terminals were started in the old folder — before one was
            // chosen, that is Viper's own — and its Changes tab watches it. Left
            // alone they would quietly be about the wrong project, so they go.
            self.tools.remove(&id);
            self.provider_terminals.remove(&id);
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
                let branch_info = {
                    let session = &self.state.sessions[self.active_index()];
                    if (session.project_dir == dir || session.worktree_dir.as_ref().is_some_and(|wt| wt.starts_with(&dir)))
                        && let (Some(wt_dir), Some(branch)) = (&session.worktree_dir, &session.worktree_branch)
                    {
                        Some((wt_dir.clone(), branch.clone(), session.worktree_base.clone().unwrap_or_else(|| "HEAD".to_owned())))
                    } else {
                        None
                    }
                };
                if let Some((wt_dir, branch, base)) = branch_info {
                    self.active_tools().open_branch_changes(&wt_dir, &branch, &base, ctx);
                } else {
                    self.active_tools().open_changes(&dir, ctx);
                }
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
}

/// The operational state of the middle terminal area for the active session.
#[derive(Debug, PartialEq, Eq)]
pub enum TerminalState {
    /// No folder has been selected yet.
    NeedsFolder { provider: Provider },
    /// The selected provider executable was not found on this computer.
    MissingExecutable { provider: Provider, hint: &'static str },
    /// The session is configured and the CLI executable is ready to run.
    Ready {
        session_id: u64,
        provider: Provider,
        cwd: PathBuf,
        exe: PathBuf,
        model: Option<String>,
        effort: Option<String>,
        resume_id: Option<String>,
        permission_mode: PermissionMode,
        take_keyboard: bool,
    },
}

/// Resolves the operational state for the active session's middle terminal view.
/// Consumes `session.focus_composer` when transitioning to `Ready`.
pub fn resolve_terminal_state(session: &mut Session, detected: &Detected) -> TerminalState {
    let provider = session.provider;
    if !session.has_folder() {
        return TerminalState::NeedsFolder { provider };
    }
    let Some(exe) = detected.get(provider).cloned() else {
        return TerminalState::MissingExecutable {
            provider,
            hint: provider.install_hint(),
        };
    };
    let take_keyboard = std::mem::take(&mut session.focus_composer);
    TerminalState::Ready {
        session_id: session.id,
        provider,
        cwd: session.working_dir().to_path_buf(),
        exe,
        model: session.chosen_model.clone(),
        effort: session.effort.clone(),
        resume_id: session.agent_session_id.clone(),
        permission_mode: session.permission_mode,
        take_keyboard,
    }
}

impl ViperApp {
    fn terminal_area(&mut self, ui: &mut egui::Ui) {
        if self.state.sessions.is_empty() {
            return;
        }
        let index = self.active_index();
        let provider = self.state.sessions[index].provider;
        if let Some(exe) = self.detected.get(provider).cloned() {
            self.models.start(provider, exe, ui.ctx());
        }

        let state = resolve_terminal_state(&mut self.state.sessions[index], &self.detected);

        match state {
            TerminalState::NeedsFolder { provider } => {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.label(egui::RichText::new("Choose a project folder to start").strong().size(18.0));
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Select a project directory to launch an interactive {} session.",
                                provider.label()
                            ))
                            .weak(),
                        );
                        ui.add_space(16.0);
                        if ui.button("Choose Folder…").clicked() {
                            self.change_folder();
                        }
                    });
                });
            }
            TerminalState::MissingExecutable { provider, hint } => {
                let mut open_settings = false;
                ui.vertical(|ui| {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(
                                    ui.visuals().error_fg_color,
                                    format!("{} isn't installed. {}.", provider.label(), hint),
                                );
                                if ui.button("Open Settings").clicked() {
                                    open_settings = true;
                                }
                            });
                        });
                });
                if open_settings {
                    self.view = View::Settings;
                }
            }
            TerminalState::Ready {
                session_id,
                provider,
                cwd,
                exe,
                model,
                effort,
                resume_id,
                permission_mode,
                take_keyboard,
            } => {
                let terminal_entry = self.provider_terminals.entry(session_id).or_insert_with(|| {
                    let (prog, args) = crate::agent::build_interactive_command(
                        provider,
                        &exe,
                        &cwd,
                        model.as_deref(),
                        effort.as_deref(),
                        resume_id.as_deref(),
                        permission_mode,
                    );
                    terminal::Terminal::start_command(&cwd, &prog, &args, ui.ctx().clone())
                });

                let mut restart = false;
                ui.scope_builder(
                    egui::UiBuilder::new().id_salt(("session_terminal", session_id)),
                    |ui| match terminal_entry {
                        Ok(terminal) => {
                            restart = terminal.ui(ui, take_keyboard);
                        }
                        Err(err) => {
                            ui.centered_and_justified(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.colored_label(ui.visuals().error_fg_color, err.as_str());
                                    ui.add_space(8.0);
                                    if ui.button("Try again").clicked() {
                                        restart = true;
                                    }
                                });
                            });
                        }
                    },
                );

                if restart {
                    self.provider_terminals.remove(&session_id);
                    ui.ctx().request_repaint();
                }
            }
        }
    }

    /// Carries out a slash command that belongs to Viper rather than to the CLI.
    /// The CLIs do these from their own interactive session; a headless run has no
    /// such session, so `/model opus` would otherwise just be words in a prompt.
    #[allow(dead_code)] // Retained for compatibility with programmatic slash commands.
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

    /// Pulls finished turns and events from background agent threads.
    fn poll_events(&mut self, ctx: &egui::Context) {
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
                        panel.refresh_changes(session.working_dir(), ctx);
                        if session.worktree_dir.is_some() {
                            panel.refresh_changes(&session.project_dir, ctx);
                        }
                    }
                    // The project heading's changed count is out of date too.
                    self.sidebar.invalidate(&session.project_dir);

                    // Check if active browser tab is displaying a file:// URL inside the session folder.
                    if let Some(panel) = self.tools.get(&id)
                        && panel.active_browser_auto_refresh()
                        && let Some(url) = panel.active_browser_url()
                        && let Some(path) = preview::file_url_to_path(url)
                    {
                        let is_artifact = session.previewable_artifacts().contains(&path);
                        if is_artifact || path.starts_with(session.working_dir()) || path.starts_with(&session.project_dir) {
                            self.browser.reload();
                            ctx.request_repaint();
                        }
                    }
                }
                session.handle_event(event);
            }
        }
    }
    /// Preserved transitional wiring for interactive agent control and approval hooks.
    #[inline(never)]
    fn transitional_agent_hooks(&mut self, id: &str) {
        if false {
            let session = self.active_session_mut();
            let _ = session.can_change_provider();
            session.stop();
            session.resolve_approval(id, crate::agent::ApprovalDecision::Approved);
            session.resolve_approval(id, crate::agent::ApprovalDecision::Denied);
            self.active_tools().mount_preview("", false);
        }
    }
}

impl eframe::App for ViperApp {
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
        self.poll_events(ctx);
        if false {
            self.transitional_agent_hooks("");
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.browser.release_focus_on_click(ui.ctx());
        self.notice_banner(ui);
        self.left_panel(ui);
        self.right_panel(ui, frame);
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| match self.view {
            View::Chat => self.terminal_area(ui),
            View::Settings => self.settings_area(ui),
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.state);
        atomic_backup_state(&self.state);
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

    #[test]
    fn a_state_saved_while_the_agent_ran_in_a_terminal_still_loads() {
        // The two settings that chose between the chat and a terminal are gone, and
        // the file on disk still has them. They have to be skipped rather than take
        // every session down with them.
        let saved = r#"(sessions: [(id: 4, title: "kept", entries: [Agent("hi")])],
                        settings: (chat_in_terminal: true, apply_terminal_chat: false))"#;
        let state: SavedState = ron::from_str(saved).expect("a save from the terminal chat must still load");
        assert_eq!(state.sessions[0].title, "kept");
        assert_eq!(state.sessions[0].entries.len(), 1, "the conversation is still there to read");
    }

    #[test]
    fn atomic_backup_writes_and_replaces_cleanly_on_disk() {
        let temp = std::env::temp_dir().join(format!("viper_test_backup_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);

        let mut state = populated_state();
        assert!(write_backup_to_dir(&temp, &state), "backup write should succeed");
        let backup_file = temp.join("app.ron.bak");
        assert!(backup_file.is_file(), "backup file must exist");
        assert!(!temp.join("app.ron.tmp").exists(), "temp file must be cleaned up");

        let content = std::fs::read_to_string(&backup_file).expect("readable backup");
        let restored: SavedState = ron::from_str(&content).expect("deserializable backup");
        assert_eq!(restored.sessions[0].title, "Fix the login form");

        // Second write atomically replaces existing backup
        state.sessions[0].title = "Updated title".into();
        assert!(write_backup_to_dir(&temp, &state), "subsequent backup write should succeed");
        let updated_content = std::fs::read_to_string(&backup_file).expect("readable updated backup");
        let updated_restored: SavedState = ron::from_str(&updated_content).expect("deserializable updated");
        assert_eq!(updated_restored.sessions[0].title, "Updated title");

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn saved_state_with_interactive_approvals_and_legacy_sessions_survives_ron_round_trip() {
        use crate::agent::{ApprovalDecision, ApprovalRequest, ApprovalStatus};
        use crate::line_diff::FileEdit;
        use crate::session::Entry;

        let mut state = populated_state();
        let mut session_with_approvals = Session::new(8, PathBuf::from(r"C:\work\beta"), Provider::Claude, PermissionMode::ReadOnly);
        session_with_approvals.title = "Approval session".into();

        // 1. Pending approval with diff
        let edit = FileEdit::new("src/main.rs", "fn main() {}\n", "fn main() {\n    println!(\"hi\");\n}\n");
        let req1 = ApprovalRequest::new("req-p1", "write_file", "src/main.rs", Some(edit));
        session_with_approvals.entries.push(Entry::Approval(req1));

        // 2. Approved approval without diff
        let mut req2 = ApprovalRequest::new("req-a2", "bash", "cargo build", None);
        req2.resolve(ApprovalDecision::Approved);
        session_with_approvals.entries.push(Entry::Approval(req2));

        // 3. Denied approval
        let mut req3 = ApprovalRequest::new("req-d3", "bash", "rm -rf .", None);
        req3.resolve(ApprovalDecision::Denied);
        session_with_approvals.entries.push(Entry::Approval(req3));

        state.sessions.push(session_with_approvals);
        state.next_session_id = 9;

        let saved = ron::to_string(&state).expect("state with approvals must serialize to RON");
        let restored: SavedState = ron::from_str(&saved).expect("state with approvals must deserialize from RON");

        assert_eq!(restored.sessions.len(), 2);
        let app_session = &restored.sessions[1];
        assert_eq!(app_session.id, 8);
        assert_eq!(app_session.entries.len(), 3);
        assert!(app_session.has_pending_approval());

        let Entry::Approval(r1) = &app_session.entries[0] else { panic!("expected approval") };
        assert_eq!(r1.id, "req-p1");
        assert_eq!(r1.status, ApprovalStatus::Pending);
        assert!(r1.edit.is_some());

        let Entry::Approval(r2) = &app_session.entries[1] else { panic!("expected approval") };
        assert_eq!(r2.id, "req-a2");
        assert_eq!(r2.status, ApprovalStatus::Approved);

        let Entry::Approval(r3) = &app_session.entries[2] else { panic!("expected approval") };
        assert_eq!(r3.id, "req-d3");
        assert_eq!(r3.status, ApprovalStatus::Denied);
    }

    #[test]
    fn session_with_worktree_fields_serializes_and_deserializes_in_saved_state() {
        use std::path::Path;

        let mut state = populated_state();
        let mut session = Session::new(12, PathBuf::from(r"C:\work\gamma"), Provider::Claude, PermissionMode::Plan);
        session.worktree_dir = Some(PathBuf::from(r"C:\work\gamma\.viper\worktrees\12"));
        session.worktree_branch = Some("viper/session-12".into());
        session.worktree_base = Some("main".into());
        state.sessions.push(session);

        let saved = ron::to_string(&state).expect("state with worktree fields serializes");
        let restored: SavedState = ron::from_str(&saved).expect("state with worktree fields deserializes");

        let wt_session = restored.sessions.iter().find(|s| s.id == 12).expect("session 12 found");
        assert_eq!(wt_session.worktree_dir, Some(PathBuf::from(r"C:\work\gamma\.viper\worktrees\12")));
        assert_eq!(wt_session.worktree_branch.as_deref(), Some("viper/session-12"));
        assert_eq!(wt_session.worktree_base.as_deref(), Some("main"));
        assert_eq!(wt_session.working_dir(), Path::new(r"C:\work\gamma\.viper\worktrees\12"));
    }

    #[test]
    fn app_setup_session_worktree_and_delete_session_cleans_up_on_disk() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("viper-app-wt-{}-{}", std::process::id(), unique));
        std::fs::create_dir_all(&temp_dir).expect("temp repo dir creates");

        let git = crate::git_diff::git_executable().expect("git must be installed");
        let run = |args: &[&str]| {
            let out = crate::agent::hidden_command(&git)
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "core.quotepath=false"])
                .args(args)
                .current_dir(&temp_dir)
                .output()
                .expect("git command succeeds");
            assert!(out.status.success(), "git {:?} failed", args);
        };
        run(&["init", "-b", "main"]);
        std::fs::write(temp_dir.join("root.txt"), "root repo file\n").unwrap();
        run(&["add", "root.txt"]);
        run(&["commit", "-m", "Initial commit"]);

        let mut state = SavedState::default();
        let session = Session::new(55, temp_dir.clone(), Provider::Claude, PermissionMode::Full);
        state.sessions = vec![session];
        state.active_session = 55;
        state.next_session_id = 56;

        let mut app = ViperApp::test_app(state);

        // Before worktree setup: tool_cwd is root project dir
        assert_eq!(app.tool_cwd(), temp_dir);

        // Setup session worktree
        let wt_path = app.setup_session_worktree(55, Some("main")).expect("setup worktree succeeds");
        assert!(wt_path.exists(), "worktree path was created on disk");

        // After worktree setup: tool_cwd switches to isolated worktree path
        assert_eq!(app.tool_cwd(), wt_path);

        let active_session = &app.state.sessions[0];
        assert_eq!(active_session.worktree_dir, Some(wt_path.clone()));
        assert_eq!(active_session.worktree_branch.as_deref(), Some("viper/session-55"));
        assert_eq!(active_session.worktree_base.as_deref(), Some("main"));

        // Delete session cleans up worktree on disk
        app.delete_session(55);
        assert!(!wt_path.exists(), "worktree directory was removed on session deletion");

        // Verify dedicated branch was deleted from git
        let branch_out = crate::agent::hidden_command(&git)
            .args(["branch", "--list", "viper/session-55"])
            .current_dir(&temp_dir)
            .output()
            .expect("git branch --list succeeds");
        assert!(
            String::from_utf8_lossy(&branch_out.stdout).trim().is_empty(),
            "branch viper/session-55 must be deleted from git on session deletion"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn app_delete_session_cleans_up_uncommitted_and_unmerged_worktree_and_handles_already_deleted() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("viper-app-cleanup-stress-{}-{}", std::process::id(), unique));
        std::fs::create_dir_all(&temp_dir).expect("temp repo dir creates");

        let git = crate::git_diff::git_executable().expect("git must be installed");
        let run = |dir: &std::path::Path, args: &[&str]| {
            let out = crate::agent::hidden_command(&git)
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "core.quotepath=false"])
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git command succeeds");
            assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
        };
        run(&temp_dir, &["init", "-b", "main"]);
        std::fs::write(temp_dir.join("root.txt"), "main branch content\n").unwrap();
        run(&temp_dir, &["add", "root.txt"]);
        run(&temp_dir, &["commit", "-m", "Initial commit"]);

        let mut state = SavedState::default();
        let s1 = Session::new(60, temp_dir.clone(), Provider::Claude, PermissionMode::Full);
        let s2 = Session::new(61, temp_dir.clone(), Provider::Codex, PermissionMode::Plan);
        state.sessions = vec![s1, s2];
        state.active_session = 60;
        state.next_session_id = 62;

        let mut app = ViperApp::test_app(state);

        // Setup worktree for s1
        let wt1 = app.setup_session_worktree(60, Some("main")).expect("wt1 creates");
        assert!(wt1.exists());

        // Make an unmerged commit on s1's branch
        std::fs::write(wt1.join("isolated.txt"), "feature commit\n").unwrap();
        run(&wt1, &["add", "isolated.txt"]);
        run(&wt1, &["commit", "-m", "Unmerged feature commit"]);

        // Make uncommitted, dirty modifications in wt1
        std::fs::write(wt1.join("isolated.txt"), "uncommitted dirty edits\n").unwrap();
        std::fs::write(wt1.join("untracked.txt"), "untracked file\n").unwrap();

        // Delete session 60: must cleanly remove worktree folder and delete unmerged branch (-D force)
        app.delete_session(60);
        assert!(!wt1.exists(), "worktree folder wt1 must be removed despite dirty uncommitted files");

        let b60 = crate::agent::hidden_command(&git)
            .args(["branch", "--list", "viper/session-60"])
            .current_dir(&temp_dir)
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&b60.stdout).trim().is_empty(), "branch viper/session-60 must be deleted despite unmerged commits");

        // Now test deleting a session whose worktree folder was already deleted externally
        let wt2 = app.setup_session_worktree(61, Some("main")).expect("wt2 creates");
        assert!(wt2.exists());
        let _ = std::fs::remove_dir_all(&wt2); // externally removed
        assert!(!wt2.exists());

        // Deleting session 61 must not panic or crash
        app.delete_session(61);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn app_mounts_preview_and_reloads_on_turn_exit() {
        let s = Session::new(70, PathBuf::from(r"C:\work\demo"), Provider::Claude, PermissionMode::Full);
        let state = SavedState {
            sessions: vec![s],
            active_session: 70,
            ..SavedState::default()
        };
        let mut app = ViperApp::test_app(state);

        // Mount preview
        let html_file = PathBuf::from(r"C:\work\demo\index.html");
        let url = preview::path_to_file_url(&html_file);
        app.active_tools().mount_preview(&url, true);
        app.state.show_tools = true;

        assert_eq!(app.active_tools().active_browser_url(), Some("file:///C:/work/demo/index.html"));
        assert!(app.state.show_tools);

        // Simulate agent turn exit
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send((70, AgentEvent::Exited { error: None })).unwrap();
        app.events_rx = rx;
        app.poll_events(&ctx);

        // Browser should have reload pending
        assert!(app.browser.is_reload_pending(), "auto_refresh reloads the browser when preview file is in project");
    }

    #[test]
    fn app_turn_exit_auto_reload_triggers_for_isolated_worktree_artifacts() {
        let mut s = Session::new(71, PathBuf::from(r"C:\work\project"), Provider::Claude, PermissionMode::Full);
        let wt = PathBuf::from(r"C:\work\project\.viper\worktrees\71");
        s.worktree_dir = Some(wt.clone());

        let state = SavedState {
            sessions: vec![s],
            active_session: 71,
            ..SavedState::default()
        };
        let mut app = ViperApp::test_app(state);

        // Mount preview for file located in the worktree
        let wt_file = wt.join("dist").join("app.html");
        let url = preview::path_to_file_url(&wt_file);
        app.active_tools().mount_preview(&url, true);

        // Turn exit
        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send((71, AgentEvent::Exited { error: None })).unwrap();
        app.events_rx = rx;
        app.poll_events(&ctx);

        assert!(app.browser.is_reload_pending(), "turn exit triggers reload for file in worktree");
    }

    #[test]
    fn app_turn_exit_auto_reload_suppressed_when_auto_refresh_is_false() {
        let s = Session::new(72, PathBuf::from(r"C:\work\demo"), Provider::Claude, PermissionMode::Full);
        let state = SavedState {
            sessions: vec![s],
            active_session: 72,
            ..SavedState::default()
        };
        let mut app = ViperApp::test_app(state);

        let html_file = PathBuf::from(r"C:\work\demo\index.html");
        let url = preview::path_to_file_url(&html_file);
        app.active_tools().mount_preview(&url, true);

        // Disable auto-refresh on active browser tab
        app.active_tools().set_active_browser_auto_refresh(false);

        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send((72, AgentEvent::Exited { error: None })).unwrap();
        app.events_rx = rx;
        app.poll_events(&ctx);

        assert!(!app.browser.is_reload_pending(), "auto_refresh false prevents reload on turn exit");
    }

    #[test]
    fn app_turn_exit_auto_reload_suppressed_for_external_web_and_unrelated_file_urls() {
        let s = Session::new(73, PathBuf::from(r"C:\work\demo"), Provider::Claude, PermissionMode::Full);
        let state = SavedState {
            sessions: vec![s],
            active_session: 73,
            ..SavedState::default()
        };
        let mut app = ViperApp::test_app(state);

        // Mount unrelated file from a completely different directory
        let other_file = PathBuf::from(r"D:\other_work\unrelated.html");
        let url = preview::path_to_file_url(&other_file);
        app.active_tools().mount_preview(&url, true);

        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send((73, AgentEvent::Exited { error: None })).unwrap();
        app.events_rx = rx;
        app.poll_events(&ctx);

        assert!(!app.browser.is_reload_pending(), "unrelated file url does not trigger auto-reload");

        // Now test external https:// URL
        app.active_tools().mount_preview("https://example.com", true);
        let (tx2, rx2) = mpsc::channel();
        tx2.send((73, AgentEvent::Exited { error: None })).unwrap();
        app.events_rx = rx2;
        app.poll_events(&ctx);

        assert!(!app.browser.is_reload_pending(), "web https url does not trigger auto-reload");
    }

    #[test]
    fn app_turn_exit_auto_reload_suppressed_when_active_tab_is_not_browser() {
        let s = Session::new(74, PathBuf::from(r"C:\work\demo"), Provider::Claude, PermissionMode::Full);
        let state = SavedState {
            sessions: vec![s],
            active_session: 74,
            ..SavedState::default()
        };
        let mut app = ViperApp::test_app(state);

        // Mount preview first
        let html_file = PathBuf::from(r"C:\work\demo\index.html");
        let url = preview::path_to_file_url(&html_file);
        app.active_tools().mount_preview(&url, true);
        assert!(app.active_tools().active_browser_url().is_some());

        // Open terminal, making terminal the active tab
        app.active_tools().open_terminal(std::path::Path::new("."), None);
        assert_eq!(app.active_tools().active_browser_url(), None);

        let ctx = egui::Context::default();
        let (tx, rx) = mpsc::channel();
        tx.send((74, AgentEvent::Exited { error: None })).unwrap();
        app.events_rx = rx;
        app.poll_events(&ctx);

        assert!(!app.browser.is_reload_pending(), "when frontmost tab is not browser, reload is suppressed");
    }

    #[test]
    fn app_turn_exit_auto_reload_only_triggers_on_exited_event_not_stream_or_finish() {
        let s = Session::new(75, PathBuf::from(r"C:\work\demo"), Provider::Claude, PermissionMode::Full);
        let state = SavedState {
            sessions: vec![s],
            active_session: 75,
            ..SavedState::default()
        };
        let mut app = ViperApp::test_app(state);

        let html_file = PathBuf::from(r"C:\work\demo\index.html");
        let url = preview::path_to_file_url(&html_file);
        app.active_tools().mount_preview(&url, true);

        let ctx = egui::Context::default();

        // 1. Stream text
        let (tx1, rx1) = mpsc::channel();
        tx1.send((75, AgentEvent::TextDelta("Working on layout...".into()))).unwrap();
        app.events_rx = rx1;
        app.poll_events(&ctx);
        assert!(!app.browser.is_reload_pending(), "streaming text delta must not trigger reload");

        // 2. Finished event
        let (tx2, rx2) = mpsc::channel();
        tx2.send((75, AgentEvent::Finished { session_id: None, error: None, denied_tools: Vec::new(), usage: None })).unwrap();
        app.events_rx = rx2;
        app.poll_events(&ctx);
        assert!(!app.browser.is_reload_pending(), "finished event must not trigger reload");

        // 3. Exited event
        let (tx3, rx3) = mpsc::channel();
        tx3.send((75, AgentEvent::Exited { error: None })).unwrap();
        app.events_rx = rx3;
        app.poll_events(&ctx);
        assert!(app.browser.is_reload_pending(), "exited event triggers reload");
    }

    #[test]
    fn legacy_saved_state_ron_without_auto_refresh_deserializes_and_defaults_to_true() {
        // Pre-M3 SavedState representation without auto_refresh in app browser or session browser
        let legacy_ron = r#"(
            sessions: [
                (
                    id: 10,
                    title: "Legacy Session",
                    project_dir: "C:\\work\\project",
                    provider: Claude,
                    permission_mode: Full,
                    entries: [],
                    browser: (
                        address: "http://localhost:3000",
                        viewport: Desktop,
                        custom_size: (1280, 800),
                    ),
                ),
            ],
            active_session: 10,
            next_session_id: 11,
            show_sessions: true,
            show_tools: false,
            browser: (
                address: "https://example.com",
                viewport: Mobile,
                custom_size: (430, 932),
            ),
        )"#;

        let restored: SavedState = ron::from_str(legacy_ron).expect("legacy SavedState without auto_refresh must deserialize");
        assert!(restored.browser.auto_refresh, "top-level browser defaults auto_refresh to true");
        assert_eq!(restored.browser.address, "https://example.com");

        let session = &restored.sessions[0];
        assert!(session.browser.auto_refresh, "session browser defaults auto_refresh to true");
        assert_eq!(session.browser.address, "http://localhost:3000");

        // Roundtrip with explicit auto_refresh = false
        let mut modified = restored;
        modified.browser.auto_refresh = false;
        modified.sessions[0].browser.auto_refresh = false;

        let serialized = ron::to_string(&modified).expect("SavedState with auto_refresh=false serializes");
        let restored_again: SavedState = ron::from_str(&serialized).expect("SavedState with auto_refresh=false deserializes");
        assert!(!restored_again.browser.auto_refresh, "explicit auto_refresh false roundtrips at top level");
        assert!(!restored_again.sessions[0].browser.auto_refresh, "explicit auto_refresh false roundtrips in session");
    }

    #[test]
    fn session_without_folder_resolves_to_needs_folder_state() {
        let mut session = Session::new(1, PathBuf::new(), Provider::Claude, PermissionMode::ReadOnly);
        let detected = Detected::default();
        let state = resolve_terminal_state(&mut session, &detected);
        assert_eq!(state, TerminalState::NeedsFolder { provider: Provider::Claude });
    }

    #[test]
    fn session_with_missing_cli_resolves_to_missing_executable_state() {
        let mut session = Session::new(2, PathBuf::from(r"C:\work\project"), Provider::Codex, PermissionMode::Full);
        let detected = Detected::default();
        let state = resolve_terminal_state(&mut session, &detected);
        assert_eq!(
            state,
            TerminalState::MissingExecutable {
                provider: Provider::Codex,
                hint: Provider::Codex.install_hint(),
            }
        );
    }

    #[test]
    fn ready_session_resolves_to_ready_terminal_state_and_consumes_focus() {
        let mut session = Session::new(3, PathBuf::from(r"C:\work\project"), Provider::Antigravity, PermissionMode::Full);
        session.chosen_model = Some("gemini-2.5-flash".into());
        session.effort = Some("high".into());
        session.agent_session_id = Some("conv-12345".into());
        assert!(session.focus_composer, "session starts with focus requested");

        let temp = std::env::temp_dir();
        let fake_exe = temp.join("viper_test_agy.exe");
        let _ = std::fs::write(&fake_exe, b"");
        let mut settings = Settings::default();
        settings.custom_executables.insert(Provider::Antigravity, fake_exe.clone());
        let detected = Detected::scan(&settings, &egui::Context::default());

        let state = resolve_terminal_state(&mut session, &detected);
        assert_eq!(
            state,
            TerminalState::Ready {
                session_id: 3,
                provider: Provider::Antigravity,
                cwd: PathBuf::from(r"C:\work\project"),
                exe: fake_exe,
                model: Some("gemini-2.5-flash".into()),
                effort: Some("high".into()),
                resume_id: Some("conv-12345".into()),
                permission_mode: PermissionMode::Full,
                take_keyboard: true,
            }
        );
        assert!(!session.focus_composer, "first resolution must consume focus_composer");

        // Subsequent call does not request keyboard focus again
        let state_subsequent = resolve_terminal_state(&mut session, &detected);
        let TerminalState::Ready { take_keyboard, .. } = state_subsequent else {
            panic!("expected Ready state");
        };
        assert!(!take_keyboard, "subsequent frame must not take keyboard without new trigger");
    }

    #[test]
    fn deleting_session_cleans_up_provider_terminal_map() {
        let state = populated_state();
        let mut app = ViperApp::test_app(state);
        // Simulate a terminal entry in provider_terminals
        app.provider_terminals.insert(7, Err("test error".to_owned()));
        assert!(app.provider_terminals.contains_key(&7));

        app.delete_session(7);
        assert!(!app.provider_terminals.contains_key(&7), "deleting session must purge provider terminal entry");
    }

    fn run_ui_test(ctx: &egui::Context, mut run: impl FnMut(&mut egui::Ui)) {
        let mut output = ctx.run_ui(egui::RawInput::default(), &mut run);
        output.textures_delta.clear();
    }

    #[test]
    fn central_view_renders_terminal_area_without_chat_composer() {
        let mut state = SavedState::default();
        let session = Session::new(10, PathBuf::from(r"C:\work\project"), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 10;
        state.next_session_id = 11;

        let mut app = ViperApp::test_app(state);
        assert_eq!(app.view, View::Chat, "central view defaults to chat/terminal view rather than settings");

        // Execute a headless egui frame rendering the central panel
        let ctx = egui::Context::default();
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        // Verify that the legacy chat composer panel ("composer_panel") is never created in central view
        let composer_id = egui::Id::new("composer_panel");
        assert!(
            !ctx.memory(|m| m.has_focus(composer_id)),
            "chat composer panel should not be created or focused in central view"
        );
    }

    #[test]
    fn unconfigured_session_without_folder_shows_guidance_and_does_not_spawn_terminal() {
        let mut state = SavedState::default();
        // A session created without a folder (empty project_dir)
        let session = Session::new(20, PathBuf::new(), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 20;
        state.next_session_id = 21;

        let mut app = ViperApp::test_app(state);
        assert!(!app.active_session().has_folder(), "session starts without a project folder");
        assert_eq!(
            app.central_state(),
            CentralState::NeedsFolder,
            "central state identifies unconfigured session needing a folder"
        );

        // Render a headless egui pass
        let ctx = egui::Context::default();
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        // Verify that no terminal process was spawned into provider_terminals
        assert!(
            app.provider_terminals.is_empty(),
            "unconfigured session must not attempt to spawn a terminal process in provider_terminals"
        );
        assert!(
            !app.provider_terminals.contains_key(&20),
            "session 20 must have no terminal entry"
        );
    }

    #[test]
    fn session_with_missing_executable_displays_warning_without_panicking() {
        let mut state = SavedState::default();
        let session = Session::new(30, PathBuf::from(r"C:\work\project"), Provider::Codex, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 30;
        state.next_session_id = 31;

        let mut app = ViperApp::test_app(state);
        assert!(app.detected.get(Provider::Codex).is_none(), "Codex executable is not detected");

        assert_eq!(
            app.central_state(),
            CentralState::MissingCli(Provider::Codex),
            "central state reports missing executable for Codex"
        );

        // Render headless egui pass: must execute cleanly without panicking
        let ctx = egui::Context::default();
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        // Verify that no terminal process was spawned into provider_terminals
        assert!(
            app.provider_terminals.is_empty(),
            "missing executable must not attempt terminal spawn in provider_terminals"
        );
    }

    #[test]
    fn switching_sessions_primes_keyboard_focus_flag_and_drawing_consumes_it() {
        let mut state = SavedState::default();
        let s1 = Session::new(41, PathBuf::from(r"C:\work\alpha"), Provider::Claude, PermissionMode::ReadOnly);
        let s2 = Session::new(42, PathBuf::from(r"C:\work\beta"), Provider::Codex, PermissionMode::Plan);
        state.sessions = vec![s1, s2];
        state.active_session = 41;
        state.next_session_id = 43;

        let mut app = ViperApp::test_app(state);
        let ctx = egui::Context::default();
        let temp = std::env::temp_dir();
        let fake_exe = temp.join("viper_test_codex.exe");
        let _ = std::fs::write(&fake_exe, b"");
        app.state.settings.custom_executables.insert(Provider::Codex, fake_exe);
        app.detected = Detected::scan(&app.state.settings, &ctx);

        let [session_1, session_2] = &mut app.state.sessions[..] else {
            panic!("expected two sessions in test app")
        };
        // Reset initial focus flags to simulate steady state
        session_1.focus_composer = false;
        session_2.focus_composer = false;

        // Switch to session 42 via sidebar action
        app.handle_sidebar(SidebarAction::Select(42), &ctx);

        assert_eq!(app.state.active_session, 42, "sidebar select updates active session to 42");
        assert!(
            app.state.sessions[1].focus_composer,
            "switching to session 42 primes its keyboard focus flag"
        );
        assert!(
            !app.state.sessions[0].focus_composer,
            "inactive session 41 does not request keyboard focus"
        );

        // Rendering terminal_area consumes the focus flag
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        assert!(
            !app.state.sessions[1].focus_composer,
            "drawing the terminal hands the focus flag over and resets it to false"
        );

        // Subsequent frame without session switch keeps focus flag false
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        assert!(
            !app.state.sessions[1].focus_composer,
            "subsequent drawing frame does not re-prime focus flag"
        );
    }

    #[test]
    fn new_session_initializes_with_keyboard_focus_requested() {
        let state = populated_state();
        let mut app = ViperApp::test_app(state);
        let ctx = egui::Context::default();

        // Create a new session via sidebar
        app.handle_sidebar(SidebarAction::NewSession, &ctx);

        let active = app.active_session();
        assert!(
            active.focus_composer,
            "newly created session initializes with keyboard focus requested"
        );
        assert!(
            !active.has_folder(),
            "new session starts without a project folder"
        );
    }

    #[test]
    fn terminal_area_spawns_process_when_session_and_cli_are_ready() {
        let temp = std::env::temp_dir();
        let mut state = SavedState::default();
        let session = Session::new(50, temp.clone(), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 50;
        state.next_session_id = 51;

        let mut app = ViperApp::test_app(state);
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        app.state.settings.custom_executables.insert(Provider::Claude, exe.clone());
        let ctx = egui::Context::default();
        app.detected = Detected::scan(&app.state.settings, &ctx);

        assert_eq!(
            app.central_state(),
            CentralState::TerminalReady,
            "session with folder and detected executable is ready for terminal"
        );

        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        assert!(
            app.provider_terminals.contains_key(&50),
            "ready session spawns and registers a terminal in provider_terminals"
        );

        // Verify focus flag was consumed
        assert!(
            !app.state.sessions[0].focus_composer,
            "focus flag was transferred to the newly spawned terminal"
        );
    }

    #[test]
    fn multi_session_switching_preserves_terminals_in_map_and_transfers_focus() {
        let temp = std::env::temp_dir();
        let mut state = SavedState::default();
        let s1 = Session::new(1, temp.clone(), Provider::Claude, PermissionMode::ReadOnly);
        let s2 = Session::new(2, temp.clone(), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![s1, s2];
        state.active_session = 1;
        state.next_session_id = 3;

        let mut app = ViperApp::test_app(state);
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        app.state.settings.custom_executables.insert(Provider::Claude, exe);
        let ctx = egui::Context::default();
        app.detected = Detected::scan(&app.state.settings, &ctx);

        // Verify initial state: session 1 active and focus requested
        assert_eq!(app.state.active_session, 1);
        assert!(app.state.sessions[0].focus_composer, "session 1 starts with focus requested");

        // 1. Render session 1
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        // Verify session 1 terminal spawned and focus consumed
        assert!(app.provider_terminals.contains_key(&1), "session 1 terminal must be spawned");
        assert!(app.provider_terminals.get(&1).unwrap().is_ok());
        assert!(!app.state.sessions[0].focus_composer, "session 1 focus flag must be consumed on render");

        let term1_addr = app.provider_terminals.get(&1).unwrap().as_ref().unwrap() as *const terminal::Terminal;

        // 2. Switch to session 2 via sidebar action
        app.handle_sidebar(SidebarAction::Select(2), &ctx);
        assert_eq!(app.state.active_session, 2);
        assert!(app.state.sessions[1].focus_composer, "switching to session 2 primes focus");

        // Render session 2
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        // Verify session 2 spawned and focus consumed while session 1 remains alive in provider_terminals
        assert!(app.provider_terminals.contains_key(&2), "session 2 terminal must be spawned");
        assert!(app.provider_terminals.get(&2).unwrap().is_ok());
        assert!(!app.state.sessions[1].focus_composer, "session 2 focus flag must be consumed on render");
        assert!(app.provider_terminals.contains_key(&1), "session 1 terminal remains alive in map");
        let term1_addr_during_s2 = app.provider_terminals.get(&1).unwrap().as_ref().unwrap() as *const terminal::Terminal;
        assert_eq!(term1_addr, term1_addr_during_s2, "session 1 terminal was preserved in place while session 2 active");

        // 3. Switch back to session 1
        app.handle_sidebar(SidebarAction::Select(1), &ctx);
        assert_eq!(app.state.active_session, 1);
        assert!(app.state.sessions[0].focus_composer, "switching back to session 1 primes focus flag");

        // Render session 1 again
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        // Verify session 1 terminal was preserved (not re-spawned) and focus flag was transferred
        let term1_addr_after = app.provider_terminals.get(&1).unwrap().as_ref().unwrap() as *const terminal::Terminal;
        assert_eq!(term1_addr, term1_addr_after, "session 1 terminal was preserved and not re-spawned");
        assert!(!app.state.sessions[0].focus_composer, "session 1 focus flag consumed after switch back");
        assert_eq!(app.provider_terminals.len(), 2, "both session terminals remain alive in map");
    }

    #[test]
    fn deleting_active_session_transfers_focus_to_neighbor_and_cleans_terminal() {
        let temp = std::env::temp_dir();
        let mut state = SavedState::default();
        let s1 = Session::new(1, temp.clone(), Provider::Claude, PermissionMode::ReadOnly);
        let s2 = Session::new(2, temp.clone(), Provider::Claude, PermissionMode::ReadOnly);
        let s3 = Session::new(3, temp.clone(), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![s1, s2, s3];
        state.active_session = 1;
        state.next_session_id = 4;

        let mut app = ViperApp::test_app(state);
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        app.state.settings.custom_executables.insert(Provider::Claude, exe);
        let ctx = egui::Context::default();
        app.detected = Detected::scan(&app.state.settings, &ctx);

        // Spawn terminals for all 3 sessions
        for id in [1, 2, 3] {
            app.handle_sidebar(SidebarAction::Select(id), &ctx);
            run_ui_test(&ctx, |ui| {
                app.terminal_area(ui);
            });
            assert!(app.provider_terminals.contains_key(&id), "terminal for session {id} spawned");
            assert!(app.provider_terminals.get(&id).unwrap().is_ok());
        }
        assert_eq!(app.provider_terminals.len(), 3);

        // Make session 2 active
        app.handle_sidebar(SidebarAction::Select(2), &ctx);
        assert_eq!(app.state.active_session, 2);

        // Delete active session 2 via sidebar action
        app.handle_sidebar(SidebarAction::Delete(2), &ctx);

        // Verify session 2 is removed from state.sessions and provider_terminals
        assert!(!app.state.sessions.iter().any(|s| s.id == 2), "session 2 removed from sessions list");
        assert!(!app.provider_terminals.contains_key(&2), "session 2 terminal removed from provider_terminals");

        // Verify neighbor session 1 becomes active with focus_composer == true
        assert_eq!(app.state.active_session, 1, "active session transferred to neighbor session 1");
        assert!(
            app.state.sessions.iter().find(|s| s.id == 1).unwrap().focus_composer,
            "neighbor session 1 received focus_composer flag"
        );

        // Verify session 3 terminal was preserved and remains running
        assert!(app.provider_terminals.contains_key(&3), "session 3 terminal preserved in map");
        assert!(app.provider_terminals.get(&3).unwrap().is_ok(), "session 3 terminal is Ok");
        assert!(
            !app.provider_terminals.get_mut(&3).unwrap().as_mut().unwrap().has_exited(),
            "session 3 process remains alive and running"
        );
        assert!(app.provider_terminals.contains_key(&1), "session 1 terminal preserved in map");
        assert_eq!(app.provider_terminals.len(), 2, "map contains remaining sessions 1 and 3");
    }

    #[test]
    fn folder_change_clears_terminal_and_subsequent_frame_respawns_in_new_cwd() {
        let temp = std::env::temp_dir();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir1 = temp.join(format!("viper_test_dir1_{}", unique));
        let dir2 = temp.join(format!("viper_test_dir2_{}", unique));
        std::fs::create_dir_all(&dir1).expect("create dir1");
        std::fs::create_dir_all(&dir2).expect("create dir2");

        let mut state = SavedState::default();
        let session = Session::new(10, dir1.clone(), Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 10;
        state.next_session_id = 11;

        let mut app = ViperApp::test_app(state);
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        app.state.settings.custom_executables.insert(Provider::Claude, exe);
        let ctx = egui::Context::default();
        app.detected = Detected::scan(&app.state.settings, &ctx);

        // Initial frame: spawns terminal in dir1
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        assert!(app.provider_terminals.contains_key(&10), "terminal spawned in dir1");
        assert!(app.provider_terminals.get(&10).unwrap().is_ok());

        // Verify resolved terminal state had cwd = dir1
        let state1 = resolve_terminal_state(&mut app.state.sessions[0], &app.detected);
        let TerminalState::Ready { cwd: cwd1, .. } = state1 else {
            panic!("expected Ready state for dir1");
        };
        assert_eq!(cwd1, dir1);

        // Change project_dir to dir2 and clear provider_terminals[&id] (as change_folder does)
        app.state.sessions[0].project_dir = dir2.clone();
        app.provider_terminals.remove(&10);
        app.state.sessions[0].focus_composer = true;

        assert!(!app.provider_terminals.contains_key(&10), "terminal map cleared for session 10");

        // Subsequent frame: re-resolves state with new cwd and re-spawns fresh terminal in dir2
        let state2 = resolve_terminal_state(&mut app.state.sessions[0], &app.detected);
        let TerminalState::Ready { cwd: cwd2, take_keyboard, .. } = state2 else {
            panic!("expected Ready state for dir2");
        };
        assert_eq!(cwd2, dir2, "re-resolved state has new cwd dir2");
        assert!(take_keyboard, "focus requested for new directory");

        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        assert!(app.provider_terminals.contains_key(&10), "fresh terminal spawned in dir2");
        assert!(app.provider_terminals.get(&10).unwrap().is_ok());

        let _ = std::fs::remove_dir_all(&dir1);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn failed_terminal_spawn_retry_action_clears_error_and_allows_respawn() {
        let temp = std::env::temp_dir();
        let mut state = SavedState::default();
        let session = Session::new(51, temp, Provider::Claude, PermissionMode::ReadOnly);
        state.sessions = vec![session];
        state.active_session = 51;

        let mut app = ViperApp::test_app(state);
        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        app.state.settings.custom_executables.insert(Provider::Claude, exe);
        let ctx = egui::Context::default();
        app.detected = Detected::scan(&app.state.settings, &ctx);

        // Simulate a spawn error in provider_terminals
        app.provider_terminals.insert(51, Err("simulated failure".into()));
        assert!(app.provider_terminals.get(&51).unwrap().is_err());

        let click_pos = egui::pos2(400.0, 43.0);
        let screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0)));

        // Frame 1: Render terminal_area to lay out the error message and "Try again" button
        let mut warm_input = egui::RawInput { screen_rect, ..Default::default() };
        warm_input.events.push(egui::Event::PointerMoved(click_pos));
        let mut out0 = ctx.run_ui(warm_input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.terminal_area(ui);
            });
        });
        out0.textures_delta.clear();
        assert!(app.provider_terminals.contains_key(&51), "error entry still present after warm-up");

        // Frame 2: Pointer press down on the "Try again" button
        let mut press_input = egui::RawInput { screen_rect, ..Default::default() };
        press_input.events.push(egui::Event::PointerMoved(click_pos));
        press_input.events.push(egui::Event::PointerButton {
            pos: click_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out1 = ctx.run_ui(press_input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.terminal_area(ui);
            });
        });
        out1.textures_delta.clear();

        // Frame 3: Pointer release triggers "Try again" clicked(), which sets restart = true and removes error
        let mut release_input = egui::RawInput { screen_rect, ..Default::default() };
        release_input.events.push(egui::Event::PointerMoved(click_pos));
        release_input.events.push(egui::Event::PointerButton {
            pos: click_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut out2 = ctx.run_ui(release_input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.terminal_area(ui);
            });
        });
        out2.textures_delta.clear();

        assert!(!app.provider_terminals.contains_key(&51), "'Try again' action must clear error entry from provider_terminals");

        // Subsequent frame with valid command spawns cleanly
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        assert!(app.provider_terminals.contains_key(&51), "subsequent frame spawns clean terminal");
        assert!(app.provider_terminals.get(&51).unwrap().is_ok(), "newly spawned terminal is Ok");
    }

    #[test]
    fn saved_state_ron_serialization_omits_provider_terminals() {
        let temp = std::env::temp_dir();
        let mut state = populated_state();
        let s2 = Session::new(20, temp, Provider::Codex, PermissionMode::Full);
        state.sessions.push(s2);
        state.active_session = 20;

        let mut app = ViperApp::test_app(state);
        // Simulate active terminals and errors in provider_terminals
        app.provider_terminals.insert(7, Err("some spawn error".into()));
        app.provider_terminals.insert(20, Err("another error".into()));

        // Serialize app.state to RON
        let ron_str = ron::to_string(&app.state).expect("app.state must serialize to RON cleanly");

        // Verify zero occurrences of provider_terminals
        assert!(
            !ron_str.contains("provider_terminals"),
            "serialized RON must never contain provider_terminals field: {ron_str}"
        );

        // Verify deserializing it back roundtrips cleanly without error
        let restored: SavedState = ron::from_str(&ron_str).expect("serialized state must deserialize back from RON");
        assert_eq!(restored.sessions.len(), app.state.sessions.len());
        assert_eq!(restored.active_session, app.state.active_session);
        assert_eq!(restored.next_session_id, app.state.next_session_id);
    }

    #[test]
    fn comprehensive_legacy_ron_state_loads_and_carries_defaults() {
        // Construct a legacy RON state string representing an older Viper build:
        // - Uses legacy "Gemini" provider (aliased to Antigravity)
        // - Uses legacy "claude_session_id" field (aliased to agent_session_id)
        // - Obsolete settings fields (chat_in_terminal, apply_terminal_chat)
        // - Missing modern fields (worktree_dir, worktree_branch, worktree_base, elements, chosen_model, effort, auto_refresh)
        // - Missing apply_panel_defaults (must default to true)
        let legacy_ron = r#"(
            sessions: [
                (
                    id: 101,
                    title: "Legacy Gemini Session",
                    project_dir: "C:\\legacy\\project",
                    provider: Gemini,
                    permission_mode: Full,
                    claude_session_id: Some("legacy-session-999"),
                    entries: [
                        Agent("Legacy agent greeting"),
                    ],
                    browser: (
                        address: "http://localhost:8000",
                        viewport: Desktop,
                        custom_size: (1024, 768),
                    ),
                ),
                (
                    id: 102,
                    title: "Legacy Claude Session",
                    project_dir: "C:\\legacy\\claude_proj",
                    provider: Claude,
                    permission_mode: ReadOnly,
                    entries: [],
                ),
            ],
            active_session: 101,
            next_session_id: 103,
            show_sessions: true,
            show_tools: true,
            settings: (
                default_provider: Gemini,
                chat_in_terminal: true,
                apply_terminal_chat: false,
            ),
        )"#;

        let restored: SavedState = ron::from_str(legacy_ron).expect("legacy RON state must deserialize cleanly");

        // Verify sessions count and active session
        assert_eq!(restored.sessions.len(), 2);
        assert_eq!(restored.active_session, 101);
        assert_eq!(restored.next_session_id, 103);

        // 1. Verify "Gemini" maps to Provider::Antigravity
        assert_eq!(
            restored.sessions[0].provider,
            Provider::Antigravity,
            "legacy Gemini provider must map to Provider::Antigravity"
        );
        assert_eq!(
            restored.settings.default_provider,
            Provider::Antigravity,
            "legacy default_provider Gemini must map to Provider::Antigravity"
        );

        // 2. Verify legacy "claude_session_id" maps to agent_session_id
        assert_eq!(
            restored.sessions[0].agent_session_id.as_deref(),
            Some("legacy-session-999"),
            "legacy claude_session_id must carry over to agent_session_id"
        );

        // 3. Verify defaults for missing modern fields
        assert_eq!(restored.sessions[0].worktree_dir, None, "missing worktree_dir defaults to None");
        assert_eq!(restored.sessions[0].worktree_branch, None, "missing worktree_branch defaults to None");
        assert_eq!(restored.sessions[0].worktree_base, None, "missing worktree_base defaults to None");
        assert_eq!(restored.sessions[0].chosen_model, None, "missing chosen_model defaults to None");
        assert_eq!(restored.sessions[0].effort, None, "missing effort defaults to None");
        assert!(restored.sessions[0].browser.auto_refresh, "missing auto_refresh defaults to true");

        // 4. Verify apply_panel_defaults defaults to true for old saves
        assert!(
            restored.apply_panel_defaults,
            "legacy save without apply_panel_defaults must default to true"
        );
    }

    #[test]
    fn restart_restores_session_and_spawns_cli_lazily_in_session_folder() {
        let temp = std::env::temp_dir();
        // Construct a ViperApp simulating restart from a loaded SavedState
        let mut state = SavedState::default();
        let session = Session::new(88, temp.clone(), Provider::Claude, PermissionMode::Full);
        state.sessions = vec![session];
        state.active_session = 88;
        state.next_session_id = 89;

        let exe = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        state.settings.custom_executables.insert(Provider::Claude, exe);

        let mut app = ViperApp::test_app(state);
        let ctx = egui::Context::default();
        app.detected = Detected::scan(&app.state.settings, &ctx);

        // Verify provider_terminals starts completely empty on restart
        assert!(
            app.provider_terminals.is_empty(),
            "provider_terminals must start empty on application restart"
        );
        assert!(!app.provider_terminals.contains_key(&88));

        // First render of terminal_area lazily spawns the CLI in the session's folder
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });

        assert!(
            app.provider_terminals.contains_key(&88),
            "first render of terminal_area lazily spawns the CLI in provider_terminals"
        );
        assert!(
            app.provider_terminals.get(&88).unwrap().is_ok(),
            "lazily spawned terminal is Ok and running"
        );
        assert!(
            !app.state.sessions[0].focus_composer,
            "focus flag was consumed during first render"
        );
    }

    #[test]
    fn middle_provider_terminal_and_tools_panel_coexist_without_interference() {
        let temp = std::env::temp_dir();
        let mut state = SavedState::default();
        let session = Session::new(99, temp.clone(), Provider::Claude, PermissionMode::Full);
        state.sessions = vec![session];
        state.active_session = 99;
        state.next_session_id = 100;
        state.show_tools = true;

        let shell = if cfg!(windows) {
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        } else {
            PathBuf::from("/bin/sh")
        };
        state.settings.shell = Some(shell.clone());
        state.settings.custom_executables.insert(Provider::Claude, shell.clone());

        let mut app = ViperApp::test_app(state);
        app.shells = vec![terminal::Shell { name: "default", path: shell }];
        let ctx = egui::Context::default();
        app.detected = Detected::scan(&app.state.settings, &ctx);

        // 1. First render middle provider terminal
        run_ui_test(&ctx, |ui| {
            app.terminal_area(ui);
        });
        assert!(app.provider_terminals.contains_key(&99), "middle terminal spawned");
        assert!(app.provider_terminals.get(&99).unwrap().is_ok());
        let middle_term_addr = app.provider_terminals.get(&99).unwrap().as_ref().unwrap() as *const terminal::Terminal;

        // 2. Open secondary terminal tab in tools panel
        let cwd = app.tool_cwd();
        app.tools.entry(99).or_default().open_terminal(&cwd, None);
        assert!(app.tools.contains_key(&99));

        // 3. Render both middle terminal area and tools panel simultaneously
        let frame = eframe::Frame::_new_kittest();
        run_ui_test(&ctx, |ui| {
            app.right_panel(ui, &frame);
            app.terminal_area(ui);
        });

        // Verify middle terminal is intact and not re-spawned
        assert!(app.provider_terminals.contains_key(&99));
        let middle_term_addr_after_both = app.provider_terminals.get(&99).unwrap().as_ref().unwrap() as *const terminal::Terminal;
        assert_eq!(
            middle_term_addr, middle_term_addr_after_both,
            "middle session terminal remains intact after rendering alongside tools panel"
        );

        // 4. Open changes diff tab and switch active tab in tools panel
        app.tools.entry(99).or_default().open_changes(&temp, &ctx);

        // 5. Render both again with active tab changed to Changes diff
        run_ui_test(&ctx, |ui| {
            app.right_panel(ui, &frame);
            app.terminal_area(ui);
        });

        // 6. Verify switching tool tabs leaves the middle session's provider_terminals entry completely intact
        assert!(app.provider_terminals.contains_key(&99));
        let middle_term_addr_after_tab_switch = app.provider_terminals.get(&99).unwrap().as_ref().unwrap() as *const terminal::Terminal;
        assert_eq!(
            middle_term_addr, middle_term_addr_after_tab_switch,
            "switching tools tabs left middle session terminal intact without interference"
        );
    }
}

