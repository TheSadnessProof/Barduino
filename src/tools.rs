//! The right-hand panel: tabs for terminals, code changes and the browser, opened with "+".

use std::path::{Path, PathBuf};

use eframe::egui;

use crate::browser::{Browser, BrowserAction, BrowserState, PickedElement};
use crate::changes::{Changes, Source};
use crate::icons::{self, Icon};
use crate::terminal::{self, Terminal};

enum Tab {
    Terminal {
        number: u64,
        cwd: PathBuf,
        /// Started the first time the tab is drawn.
        terminal: Option<Result<Terminal, String>>,
        /// Typed into the shell once it has started.
        typed: Option<String>,
        /// Set while this terminal should take the keyboard the next time it is
        /// drawn, so a terminal you just opened can be typed into right away.
        focus: bool,
    },
    Changes(Changes),
    /// A page. Every tab remembers its own address, but they take turns in the one
    /// WebView the app owns, so switching to one loads the page it was showing.
    Browser {
        number: u64,
        state: BrowserState,
    },
}

/// Shown next to the menu items and in Settings; the keys are handled in app.rs.
pub const TERMINAL_SHORTCUT: &str = "Ctrl+`";
pub const BROWSER_SHORTCUT: &str = "Ctrl+Shift+B";

pub enum ToolsAction {
    None,
    /// Attach these page elements to the active session's message.
    Attach(Vec<PickedElement>),
    /// Send these page elements directly to the active session.
    Send(Vec<PickedElement>),
}

/// What the panel needs from the session it belongs to.
pub struct PanelContext<'a> {
    /// Which session this panel belongs to.
    pub id: u64,
    /// The folder terminals start in and changes are read from.
    pub cwd: &'a Path,
    /// The shell a new terminal runs.
    pub shell: &'a Path,
    /// The one WebView every session takes turns showing.
    pub browser: &'a mut Browser,
    /// What this session wants shown in it.
    pub page: &'a mut BrowserState,
}

pub struct Tools {
    tabs: Vec<Tab>,
    active: usize,
    next_terminal_number: u64,
    next_browser_number: u64,
}

impl Default for Tools {
    fn default() -> Self {
        Self { tabs: Vec::new(), active: 0, next_terminal_number: 1, next_browser_number: 1 }
    }
}

impl Tools {

    /// Whether this panel is the one showing the shared WebView.
    pub fn shows_browser(&self) -> bool {
        matches!(self.tabs.get(self.active), Some(Tab::Browser { .. }))
    }

    /// Whether the tab in front is a terminal.
    fn shows_terminal(&self) -> bool {
        matches!(self.tabs.get(self.active), Some(Tab::Terminal { .. }))
    }

    /// Opens a new terminal in `cwd`, optionally typing a command into it.
    pub fn open_terminal(&mut self, cwd: &Path, typed: Option<String>) {
        let number = self.next_terminal_number;
        self.next_terminal_number += 1;
        self.tabs.push(Tab::Terminal { number, cwd: cwd.to_owned(), terminal: None, typed, focus: true });
        self.active = self.tabs.len() - 1;
    }

    /// Shows a terminal in `cwd`: the most recent one that's open, or a new one.
    pub fn show_terminal(&mut self, cwd: &Path) {
        // The one already in front, so the shortcut returns you to the terminal you
        // were using rather than always to the newest.
        let showing = self.shows_terminal().then_some(self.active);
        match showing.or_else(|| self.tabs.iter().rposition(|tab| matches!(tab, Tab::Terminal { .. }))) {
            Some(index) => {
                self.active = index;
                self.focus_terminal(index);
            }
            None => self.open_terminal(cwd, None),
        }
    }

    /// Puts the cursor in this tab's terminal the next time it's drawn. Anything
    /// else in that tab ignores it.
    fn focus_terminal(&mut self, index: usize) {
        if let Some(Tab::Terminal { focus, .. }) = self.tabs.get_mut(index) {
            *focus = true;
        }
    }

    /// Shows the uncommitted changes in `dir`, reusing a tab that already does.
    pub fn open_changes(&mut self, dir: &Path, ctx: &egui::Context) {
        let existing = self.tabs.iter().position(|tab| match tab {
            Tab::Changes(changes) => matches!(&changes.source, Source::Project(p) if p == dir),
            _ => false,
        });
        match existing {
            Some(index) => self.active = index,
            None => {
                self.tabs.push(Tab::Changes(Changes::new(Source::Project(dir.to_owned()), ctx)));
                self.active = self.tabs.len() - 1;
            }
        }
    }

    /// Shows branch changes in `dir` compared to `base`, reusing a tab that already watches it.
    pub fn open_branch_changes(&mut self, dir: &Path, branch: &str, base: &str, ctx: &egui::Context) {
        let existing = self.tabs.iter().position(|tab| match tab {
            Tab::Changes(changes) => matches!(&changes.source, Source::Branch { dir: d, branch: b, .. } if d == dir && b == branch),
            _ => false,
        });
        match existing {
            Some(index) => self.active = index,
            None => {
                self.tabs.push(Tab::Changes(Changes::new(
                    Source::Branch {
                        dir: dir.to_owned(),
                        branch: branch.to_owned(),
                        base: base.to_owned(),
                    },
                    ctx,
                )));
                self.active = self.tabs.len() - 1;
            }
        }
    }

    /// Asks for two files and shows how they differ. Returns false if the user cancelled.
    fn compare_files(&mut self, dir: &Path, ctx: &egui::Context) -> bool {
        let pick = |title: &str| rfd::FileDialog::new().set_title(title).set_directory(dir).pick_file();
        let Some(old) = pick("Choose the original file") else { return false };
        let Some(new) = pick("Choose the file to compare it with") else { return false };
        self.tabs.push(Tab::Changes(Changes::new(Source::Files { old, new }, ctx)));
        self.active = self.tabs.len() - 1;
        true
    }

    /// Reloads Changes tabs for `dir`, e.g. after an agent finished working there.
    pub fn refresh_changes(&mut self, dir: &Path, ctx: &egui::Context) {
        for tab in &mut self.tabs {
            if let Tab::Changes(changes) = tab
                && changes.watches(dir)
            {
                changes.refresh(ctx);
            }
        }
    }

    /// Mounts a web artifact preview URL in the browser: reusing an existing browser
    /// tab if open, or creating a new browser tab. Activates the tab.
    pub fn mount_preview(&mut self, url: &str, _reload_if_loaded: bool) {
        let normalized = crate::browser::normalize_url(url);
        if let Some(index) = self.tabs.iter().position(|tab| matches!(tab, Tab::Browser { .. })) {
            if let Tab::Browser { state, .. } = &mut self.tabs[index] {
                state.address = normalized;
            }
            self.active = index;
        } else {
            let number = self.next_browser_number;
            self.next_browser_number += 1;
            let mut state = self.browser_states().last().cloned().unwrap_or_default();
            state.address = normalized;
            self.tabs.push(Tab::Browser { number, state });
            self.active = self.tabs.len() - 1;
        }
    }

    /// The URL currently shown in the frontmost tab, if that tab is a browser.
    pub fn active_browser_url(&self) -> Option<&str> {
        match self.tabs.get(self.active) {
            Some(Tab::Browser { state, .. }) => Some(&state.address),
            _ => None,
        }
    }

    /// Whether the frontmost tab is a browser with auto-refresh enabled.
    pub fn active_browser_auto_refresh(&self) -> bool {
        match self.tabs.get(self.active) {
            Some(Tab::Browser { state, .. }) => state.auto_refresh,
            _ => false,
        }
    }

    /// Sets auto-refresh on the active browser tab.
    #[cfg(test)]
    pub fn set_active_browser_auto_refresh(&mut self, enabled: bool) {
        if let Some(Tab::Browser { state, .. }) = self.tabs.get_mut(self.active) {
            state.auto_refresh = enabled;
        }
    }

    /// Shows the browser tab, opening it if needed.
    pub fn open_browser(&mut self) {
        // A new one each time, starting where the last one was looking: opening a
        // second browser is nearly always to compare it with the first.
        let number = self.next_browser_number;
        self.next_browser_number += 1;
        let state = self.browser_states().last().cloned().unwrap_or_default();
        self.tabs.push(Tab::Browser { number, state });
        self.active = self.tabs.len() - 1;
    }

    /// Shows a page: the browser tab already in front, or the most recent one, or a
    /// new one. The shortcut returns you to the page you were on rather than opening
    /// another every time it is pressed.
    pub fn show_browser(&mut self) {
        if self.shows_browser() {
            return;
        }
        match self.tabs.iter().rposition(|tab| matches!(tab, Tab::Browser { .. })) {
            Some(index) => self.active = index,
            None => self.open_browser(),
        }
    }

    fn browser_states(&self) -> impl Iterator<Item = &BrowserState> {
        self.tabs.iter().filter_map(|tab| match tab {
            Tab::Browser { state, .. } => Some(state),
            _ => None,
        })
    }

    /// Closes tab `index`. Dropping a terminal tab stops its shell. The WebView is
    /// shared, so it is left alone here — app.rs closes it once no session wants it.
    fn close(&mut self, index: usize) {
        self.tabs.remove(index);
        if self.active >= index && self.active > 0 {
            self.active -= 1;
        }
    }

    /// Whether this panel has a browser tab open at all, in front or behind. The
    /// WebView is shared, so it may only be closed once this is false everywhere.
    pub fn wants_browser(&self) -> bool {
        self.tabs.iter().any(|tab| matches!(tab, Tab::Browser { .. }))
    }

    /// The "+" menu. `cwd` is where a new terminal starts. Returns true when a tab was opened.
    pub fn add_menu(&mut self, ui: &mut egui::Ui, cwd: &Path) -> bool {
        let mut opened = false;
        let response = icons::button(ui, Icon::Plus, "Open a terminal, changes or the browser");
        egui::Popup::menu(&response).show(|ui| {
            if ui.add(egui::Button::new("Terminal").shortcut_text(TERMINAL_SHORTCUT)).clicked() {
                self.open_terminal(cwd, None);
                opened = true;
            }
            if ui.button("Changes").on_hover_text("Uncommitted changes in this project").clicked() {
                self.open_changes(cwd, ui.ctx());
                opened = true;
            }
            if ui.button("Compare two files…").clicked() {
                ui.close();
                opened |= self.compare_files(cwd, ui.ctx());
            }
            if ui.add(egui::Button::new("Browser").shortcut_text(BROWSER_SHORTCUT)).clicked() {
                self.open_browser();
                opened = true;
            }
        });
        opened
    }

    /// Draws the expanded panel. `collapse` is set when the user hides it.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        session: PanelContext<'_>,
        collapse: &mut bool,
    ) -> ToolsAction {
        let PanelContext { id, cwd, shell, browser, page } = session;
        let mut close = None;
        // Both are acted on after the strip, which is iterating the tabs.
        let mut clicked = None;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for (index, tab) in self.tabs.iter().enumerate() {
                let (title, hover) = match tab {
                    Tab::Terminal { number, cwd, .. } => {
                        let title = if *number == 1 { "Terminal".to_owned() } else { format!("Terminal {number}") };
                        (title, cwd.display().to_string())
                    }
                    Tab::Changes(changes) => {
                        let hover = match &changes.source {
                            Source::Project(dir) => format!("Uncommitted changes in {}", dir.display()),
                            Source::Branch { dir, branch, base } => {
                                format!("Changes in {branch} vs {base}\n{}", dir.display())
                            }
                            Source::Files { old, new } => format!("{}\n→ {}", old.display(), new.display()),
                        };
                        (changes.title(), hover)
                    }
                                    Tab::Browser { number, state } => {
                        let title = if *number == 1 { "Browser".to_owned() } else { format!("Browser {number}") };
                        let hover = if state.address.is_empty() {
                            "Built-in browser".to_owned()
                        } else {
                            state.address.clone()
                        };
                        (title, hover)
                    }
                };

                let is_active = index == self.active;
                let mut tab_closed = false;
                let tab_resp = ui
                    .scope_builder(
                        egui::UiBuilder::new().id_salt(("tool_tab", index)).sense(egui::Sense::click()),
                        |ui| {
                            let response = ui.response();
                            // `ui.response()` already reports the tab's own rect from the last
                            // pass. Falling back to `max_rect` would cover the rest of the strip,
                            // so hovering one tab lit up every tab before it.
                            let hovered = response.hovered();
                            let visuals = ui.style().interact_selectable(&response, is_active);
                            let fill = if is_active {
                                visuals.weak_bg_fill
                            } else if hovered {
                                ui.visuals().faint_bg_color
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            // A Frame counts its stroke width as margin, so a stroke that
                            // appeared on hover would resize this tab and shift every tab
                            // after it. The width stays fixed; only the colour changes.
                            let stroke = egui::Stroke::new(
                                1.0,
                                if is_active {
                                    ui.visuals().selection.stroke.color
                                } else {
                                    egui::Color32::TRANSPARENT
                                },
                            );

                            egui::Frame::new()
                                .fill(fill)
                                .stroke(stroke)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(8, 4))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(title)
                                                    .color(if is_active {
                                                        visuals.fg_stroke.color
                                                    } else {
                                                        visuals.text_color()
                                                    })
                                                    .small(),
                                            )
                                            .selectable(false),
                                        );
                                        if icons::small_button(ui, Icon::Close, "Close tab").clicked() {
                                            tab_closed = true;
                                        }
                                    });
                                });
                        },
                    )
                    .response;

                if tab_closed || tab_resp.middle_clicked() {
                    close = Some(index);
                } else if tab_resp.clicked() {
                    self.active = index;
                    clicked = Some(index);
                }
                tab_resp.on_hover_text(hover);
            }
            self.add_menu(ui, cwd);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::SidebarRight, "Hide panel").clicked() {
                    *collapse = true;
                }
            });
        });
        ui.separator();
        // Clicking a terminal's tab is asking to type in it, not just to look.
        if let Some(index) = clicked {
            self.focus_terminal(index);
        }
        if let Some(index) = close {
            self.close(index);
            if self.tabs.is_empty() {
                *collapse = true;
            }
        }

        if !self.shows_browser() {
            browser.hide();
        }

        let mut action = ToolsAction::None;
        match self.tabs.get_mut(self.active) {
            None => {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("Nothing open").weak());
                    ui.label(egui::RichText::new("Click + to open a terminal, changes or the browser.").small().weak());
                    ui.add_space(6.0);
                    let keys = format!("{TERMINAL_SHORTCUT} terminal  ·  {BROWSER_SHORTCUT} browser");
                    ui.label(egui::RichText::new(keys).small().weak());
                });
            }
            Some(Tab::Terminal { number, cwd, terminal, typed, focus }) => {
                let take_keyboard = std::mem::take(focus);
                let restarted = ui
                    .push_id(("terminal", *number), |ui| {
                        terminal::show(ui, terminal, cwd, shell, typed, take_keyboard)
                    })
                    .inner;
                // The restarted shell is a new terminal, so it wants the cursor as well.
                *focus = restarted;
            }
            Some(Tab::Changes(changes)) => changes.ui(ui),
            Some(Tab::Browser { number, state }) => {
                // The first browser of a session opens on the page it was last looking
                // at, which is what was saved for it.
                if state.address.is_empty() && !page.address.is_empty() {
                    *state = page.clone();
                }
                // The page is a native window drawn over the app, so it has to get out
                // of the way whenever a menu or popup needs to draw on top of it.
                let page_visible = !egui::Popup::is_any_open(ui.ctx());
                match browser.ui((id, *number), state, ui, frame, page_visible) {
                    BrowserAction::Attach(elements) => action = ToolsAction::Attach(elements),
                    BrowserAction::Send(elements) => action = ToolsAction::Send(elements),
                    BrowserAction::None => {}
                }
                // The session remembers the page in front of it, so that the one you
                // were looking at is still there after a restart.
                *page = state.clone();
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_starts_empty() {
        assert!(Tools::default().tabs.is_empty());
    }

    #[test]
    fn closing_tab_removes_it() {
        let mut tools = Tools::default();
        tools.open_terminal(Path::new("."), None);
        assert_eq!(tools.tabs.len(), 1);
        tools.close(0);
        assert!(tools.tabs.is_empty());
    }

    /// True while this tab's terminal is still waiting to be given the keyboard.
    fn waiting_for_keyboard(tools: &Tools, index: usize) -> bool {
        matches!(tools.tabs.get(index), Some(Tab::Terminal { focus: true, .. }))
    }

    /// Stands in for drawing the tab, which is what hands the request over.
    fn draw(tools: &mut Tools, index: usize) {
        if let Some(Tab::Terminal { focus, .. }) = tools.tabs.get_mut(index) {
            *focus = false;
        }
    }

    #[test]
    fn a_terminal_asks_for_the_keyboard_when_it_opens_or_is_shown_again() {
        let mut tools = Tools::default();
        tools.open_terminal(Path::new("."), None);
        assert!(waiting_for_keyboard(&tools, 0), "a terminal you just opened takes the cursor");

        draw(&mut tools, 0);
        assert!(!waiting_for_keyboard(&tools, 0), "and doesn't keep asking for it afterwards");

        // Ctrl+` while a terminal is already open is asking to type in that one.
        tools.open_browser();
        assert_eq!(tools.active, 1, "the browser is in front now");
        tools.show_terminal(Path::new("."));
        assert_eq!(tools.active, 0, "back to the terminal that was already open");
        assert!(waiting_for_keyboard(&tools, 0));
        assert_eq!(tools.tabs.len(), 2, "without opening a second one");

        // Nothing to focus in a browser tab, and asking anyway is harmless.
        tools.focus_terminal(1);
        assert!(matches!(tools.tabs.get(1), Some(Tab::Browser { .. })));
    }

    #[test]
    fn a_second_browser_is_a_second_tab_not_the_same_one_again() {
        let mut tools = Tools::default();
        tools.open_browser();
        if let Some(Tab::Browser { state, .. }) = tools.tabs.get_mut(0) {
            state.address = "localhost:3000".to_owned();
        }

        // Opening another gives you another, rather than putting you back on the one
        // you already had — which is what comparing two pages needs.
        tools.open_browser();
        assert_eq!(tools.tabs.len(), 2, "two browsers");
        assert_eq!(tools.active, 1, "and the new one is in front");

        // It starts where the last one was looking, since a second browser is almost
        // always opened to compare it with the first.
        let addresses: Vec<&str> = tools.browser_states().map(|state| state.address.as_str()).collect();
        assert_eq!(addresses, ["localhost:3000", "localhost:3000"]);

        // But they are their own from then on.
        if let Some(Tab::Browser { state, .. }) = tools.tabs.get_mut(1) {
            state.address = "localhost:8080".to_owned();
        }
        let addresses: Vec<&str> = tools.browser_states().map(|state| state.address.as_str()).collect();
        assert_eq!(addresses, ["localhost:3000", "localhost:8080"], "each tab keeps its own page");

        // The shortcut returns you to a browser rather than opening yet another.
        tools.open_terminal(Path::new("."), None);
        tools.show_browser();
        assert_eq!(tools.tabs.len(), 3, "no fourth tab");
        assert!(tools.shows_browser());
        tools.show_browser();
        assert_eq!(tools.tabs.len(), 3, "and pressing it again stays put");
    }

    #[test]
    fn each_session_keeps_its_own_tabs() {
        // What app.rs does: one panel per session, looked up by session id.
        let mut panels: std::collections::BTreeMap<u64, Tools> = std::collections::BTreeMap::new();
        panels.entry(1).or_default().open_terminal(Path::new("."), None);
        panels.entry(1).or_default().open_browser();
        panels.entry(2).or_default().open_terminal(Path::new("."), None);

        assert_eq!(panels[&1].tabs.len(), 2, "the first session kept both of its tabs");
        assert_eq!(panels[&2].tabs.len(), 1, "the second session started fresh");
        assert!(panels[&1].shows_browser(), "the browser is the tab the first session is on");
        assert!(!panels[&2].shows_browser(), "so the second session isn't showing the page");

        // Terminal numbering is per session, so both call their first one "Terminal".
        let numbers: Vec<u64> = panels
            .values()
            .flat_map(|panel| panel.tabs.iter())
            .filter_map(|tab| if let Tab::Terminal { number, .. } = tab { Some(*number) } else { None })
            .collect();
        assert_eq!(numbers, [1, 1]);
    }

    #[test]
    fn open_branch_changes_creates_branch_tab_and_reuses_it() {
        let mut tools = Tools::default();
        let ctx = egui::Context::default();
        let wt = Path::new(r"C:\work\project\.viper\worktrees\1");

        tools.open_branch_changes(wt, "viper/session-1", "main", &ctx);
        assert_eq!(tools.tabs.len(), 1);
        assert_eq!(tools.active, 0);

        // Reopening reuses existing tab rather than creating duplicate
        tools.open_branch_changes(wt, "viper/session-1", "main", &ctx);
        assert_eq!(tools.tabs.len(), 1);
        assert_eq!(tools.active, 0);
    }

    #[test]
    fn mounting_preview_opens_or_switches_to_browser_tab() {
        let mut tools = Tools::default();
        assert_eq!(tools.active_browser_url(), None);

        // Mounting a preview opens a new browser tab
        tools.mount_preview("file:///C:/test/index.html", true);
        assert_eq!(tools.tabs.len(), 1);
        assert_eq!(tools.active, 0);
        assert_eq!(tools.active_browser_url(), Some("file:///C:/test/index.html"));
        assert!(tools.active_browser_auto_refresh());

        // Opening a terminal makes it active
        tools.open_terminal(Path::new("."), None);
        assert_eq!(tools.tabs.len(), 2);
        assert_eq!(tools.active, 1);
        assert_eq!(tools.active_browser_url(), None);

        // Mounting another preview switches back to the existing browser tab and updates URL
        tools.mount_preview("file:///C:/test/about.html", true);
        assert_eq!(tools.tabs.len(), 2, "reuses existing browser tab without creating another");
        assert_eq!(tools.active, 0);
        assert_eq!(tools.active_browser_url(), Some("file:///C:/test/about.html"));
    }

    #[test]
    fn project_changes_and_branch_changes_do_not_collide() {
        let mut tools = Tools::default();
        let ctx = egui::Context::default();
        let project_dir = Path::new(r"C:\work\project");
        let wt = Path::new(r"C:\work\project\.viper\worktrees\1");

        tools.open_changes(project_dir, &ctx);
        assert_eq!(tools.tabs.len(), 1);
        assert_eq!(tools.active, 0);

        // Opening branch changes opens a separate tab
        tools.open_branch_changes(wt, "viper/session-1", "main", &ctx);
        assert_eq!(tools.tabs.len(), 2);
        assert_eq!(tools.active, 1);

        // Reopening project changes switches back to tab 0 rather than creating a third tab
        tools.open_changes(project_dir, &ctx);
        assert_eq!(tools.tabs.len(), 2);
        assert_eq!(tools.active, 0);
    }

    #[test]
    fn mounting_preview_preserves_viewport_and_size_while_updating_address() {
        let mut tools = Tools::default();
        tools.mount_preview("file:///C:/test/first.html", true);
        assert_eq!(tools.tabs.len(), 1);
        assert_eq!(tools.active, 0);

        // Customize viewport, size, and auto_refresh
        if let Tab::Browser { state, .. } = &mut tools.tabs[0] {
            state.viewport = crate::browser::Viewport::Fixed;
            state.size = [800, 600];
            state.auto_refresh = false;
        }

        // Mount new preview URL
        tools.mount_preview("file:///C:/test/second.html", true);
        assert_eq!(tools.tabs.len(), 1, "reuses existing browser tab");
        assert_eq!(tools.active, 0);
        assert_eq!(tools.active_browser_url(), Some("file:///C:/test/second.html"));
        assert!(!tools.active_browser_auto_refresh(), "custom auto_refresh setting preserved");

        if let Tab::Browser { state, .. } = &tools.tabs[0] {
            assert_eq!(state.viewport, crate::browser::Viewport::Fixed);
            assert_eq!(state.size, [800, 600]);
        } else {
            panic!("tab 0 must be a browser");
        }
    }

    #[test]
    fn mounting_preview_normalizes_raw_windows_path_and_reuses_tab_among_mixed_tabs() {
        let mut tools = Tools::default();
        let ctx = egui::Context::default();
        tools.open_terminal(Path::new("."), None);
        tools.open_changes(Path::new(r"C:\work\project"), &ctx);
        assert_eq!(tools.tabs.len(), 2);
        assert_eq!(tools.active, 1);

        // Mount preview with raw Windows path containing spaces and special characters
        tools.mount_preview(r"C:\work\my app\page #1.html", true);
        assert_eq!(tools.tabs.len(), 3);
        assert_eq!(tools.active, 2);
        assert_eq!(tools.active_browser_url(), Some("file:///C:/work/my%20app/page%20%231.html"));

        // Switch to terminal tab
        tools.active = 0;
        assert_eq!(tools.active_browser_url(), None);
        assert!(!tools.active_browser_auto_refresh());

        // Mount another preview with file:// URL
        tools.mount_preview("file:///C:/work/my%20app/about.html", true);
        assert_eq!(tools.tabs.len(), 3, "does not duplicate browser tab among mixed tabs");
        assert_eq!(tools.active, 2, "switches active back to the browser tab");
        assert_eq!(tools.active_browser_url(), Some("file:///C:/work/my%20app/about.html"));
    }

    #[test]
    fn multiple_branch_changes_and_project_changes_tabs_coexist_without_collision() {
        let mut tools = Tools::default();
        let ctx = egui::Context::default();
        let project_a = Path::new(r"C:\work\project_a");
        let project_b = Path::new(r"C:\work\project_b");
        let wt_1 = Path::new(r"C:\work\project_a\.viper\worktrees\1");
        let wt_2 = Path::new(r"C:\work\project_a\.viper\worktrees\2");

        // Open project changes for project A
        tools.open_changes(project_a, &ctx);
        assert_eq!(tools.tabs.len(), 1);
        assert_eq!(tools.active, 0);

        // Open branch changes for worktree 1 (branch 1)
        tools.open_branch_changes(wt_1, "viper/session-1", "main", &ctx);
        assert_eq!(tools.tabs.len(), 2);
        assert_eq!(tools.active, 1);

        // Open branch changes for worktree 2 (branch 2)
        tools.open_branch_changes(wt_2, "viper/session-2", "main", &ctx);
        assert_eq!(tools.tabs.len(), 3);
        assert_eq!(tools.active, 2);

        // Open project changes for project B
        tools.open_changes(project_b, &ctx);
        assert_eq!(tools.tabs.len(), 4);
        assert_eq!(tools.active, 3);

        // Reopen worktree 1 branch changes: must reuse tab 1
        tools.open_branch_changes(wt_1, "viper/session-1", "main", &ctx);
        assert_eq!(tools.tabs.len(), 4, "reuses existing branch tab 1");
        assert_eq!(tools.active, 1);

        // Reopen project A changes: must reuse tab 0
        tools.open_changes(project_a, &ctx);
        assert_eq!(tools.tabs.len(), 4, "reuses existing project tab 0");
        assert_eq!(tools.active, 0);

        // Reopen worktree 2 branch changes: must reuse tab 2
        tools.open_branch_changes(wt_2, "viper/session-2", "main", &ctx);
        assert_eq!(tools.tabs.len(), 4, "reuses existing branch tab 2");
        assert_eq!(tools.active, 2);
    }
}

