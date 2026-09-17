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
    },
    Changes(Changes),
    /// There is at most one, because it shares the single browser window.
    Browser,
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

pub struct Tools {
    tabs: Vec<Tab>,
    active: usize,
    next_terminal_number: u64,
    browser: Browser,
}

impl Tools {
    pub fn new(browser_state: BrowserState) -> Self {
        Self { tabs: Vec::new(), active: 0, next_terminal_number: 1, browser: Browser::new(browser_state) }
    }

    pub fn browser_state(&self) -> &BrowserState {
        &self.browser.state
    }

    /// Opens a new terminal in `cwd`, optionally typing a command into it.
    pub fn open_terminal(&mut self, cwd: &Path, typed: Option<String>) {
        let number = self.next_terminal_number;
        self.next_terminal_number += 1;
        self.tabs.push(Tab::Terminal { number, cwd: cwd.to_owned(), terminal: None, typed });
        self.active = self.tabs.len() - 1;
    }

    /// Shows a terminal in `cwd`: the most recent one that's open, or a new one.
    pub fn show_terminal(&mut self, cwd: &Path) {
        match self.tabs.iter().rposition(|tab| matches!(tab, Tab::Terminal { .. })) {
            Some(index) => self.active = index,
            None => self.open_terminal(cwd, None),
        }
    }

    /// Shows the uncommitted changes in `dir`, reusing a tab that already does.
    pub fn open_changes(&mut self, dir: &Path, ctx: &egui::Context) {
        let existing = self.tabs.iter().position(|tab| matches!(tab, Tab::Changes(changes) if changes.watches(dir)));
        match existing {
            Some(index) => self.active = index,
            None => {
                self.tabs.push(Tab::Changes(Changes::new(Source::Project(dir.to_owned()), ctx)));
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

    /// Shows the browser tab, opening it if needed.
    pub fn open_browser(&mut self) {
        match self.tabs.iter().position(|tab| matches!(tab, Tab::Browser)) {
            Some(index) => self.active = index,
            None => {
                self.tabs.push(Tab::Browser);
                self.active = self.tabs.len() - 1;
            }
        }
    }

    fn close(&mut self, index: usize) {
        // Dropping a terminal tab stops its shell.
        if let Tab::Browser = self.tabs.remove(index) {
            self.browser.close();
        }
        if self.active >= index && self.active > 0 {
            self.active -= 1;
        }
    }

    /// Hides the browser page; call this whenever the panel itself isn't shown.
    pub fn hide_browser(&mut self) {
        self.browser.hide();
    }

    pub fn release_focus_on_click(&self, ctx: &egui::Context) {
        self.browser.release_focus_on_click(ctx);
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
    pub fn ui(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, cwd: &Path, collapse: &mut bool) -> ToolsAction {
        let mut close = None;
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
                            Source::Files { old, new } => format!("{}\n→ {}", old.display(), new.display()),
                        };
                        (changes.title(), hover)
                    }
                    Tab::Browser => ("Browser".to_owned(), "Built-in browser".to_owned()),
                };

                let is_active = index == self.active;
                let mut tab_closed = false;
                let tab_resp = ui
                    .scope_builder(
                        egui::UiBuilder::new().id_salt(("tool_tab", index)).sense(egui::Sense::click()),
                        |ui| {
                            let response = ui.response();
                            let hovered = response.hovered() || ui.rect_contains_pointer(ui.max_rect());
                            let visuals = ui.style().interact_selectable(&response, is_active);
                            let fill = if is_active {
                                visuals.weak_bg_fill
                            } else if hovered {
                                ui.visuals().faint_bg_color
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            let stroke = if is_active {
                                visuals.bg_stroke
                            } else {
                                egui::Stroke::NONE
                            };

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
                }
                tab_resp.on_hover_text(hover);
            }
            self.add_menu(ui, cwd);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, Icon::Close, "Close panel").clicked() {
                    *collapse = true;
                }
            });
        });
        ui.separator();
        if let Some(index) = close {
            self.close(index);
            if self.tabs.is_empty() {
                *collapse = true;
            }
        }

        let browser_shown = matches!(self.tabs.get(self.active), Some(Tab::Browser));
        if !browser_shown {
            self.browser.hide();
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
            Some(Tab::Terminal { number, cwd, terminal, typed }) => {
                ui.push_id(("terminal", *number), |ui| terminal::show(ui, terminal, cwd, typed.take()));
            }
            Some(Tab::Changes(changes)) => changes.ui(ui),
            Some(Tab::Browser) => {
                // The page is a native window drawn over the app, so it has to get out
                // of the way whenever a menu or popup needs to draw on top of it.
                let page_visible = !egui::Popup::is_any_open(ui.ctx());
                match self.browser.ui(ui, frame, page_visible) {
                    BrowserAction::Attach(elements) => action = ToolsAction::Attach(elements),
                    BrowserAction::Send(elements) => action = ToolsAction::Send(elements),
                    BrowserAction::None => {}
                }
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
        let tools = Tools::new(BrowserState::default());
        assert!(tools.tabs.is_empty());
    }

    #[test]
    fn closing_tab_removes_it() {
        let mut tools = Tools::new(BrowserState::default());
        tools.open_terminal(Path::new("."), None);
        assert_eq!(tools.tabs.len(), 1);
        tools.close(0);
        assert!(tools.tabs.is_empty());
    }
}
