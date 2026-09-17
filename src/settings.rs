//! App settings and the Settings page shown in the middle column.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::{Provider, hidden_command};
use crate::plan::{self, PlanUsage};
use crate::tools::{BROWSER_SHORTCUT, TERMINAL_SHORTCUT};
use crate::usage::{self, Period, Usage, UsageLog};

/// Colours for states that shouldn't depend on the theme's accent colour.
const GOOD: egui::Color32 = egui::Color32::from_rgb(76, 175, 120);
const WARN: egui::Color32 = egui::Color32::from_rgb(214, 158, 46);
const OVER: egui::Color32 = egui::Color32::from_rgb(214, 92, 84);

/// The page never grows wider than this, so lines stay readable on a big window.
const PAGE_WIDTH: f32 = 880.0;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The provider new sessions start with.
    pub default_provider: Provider,
    /// Providers the user switched off. Stored this way so providers added later start switched on.
    pub disabled_providers: Vec<Provider>,
    /// Executables the user chose instead of the ones Barduino finds itself.
    pub custom_executables: BTreeMap<Provider, PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_provider: Provider::Claude,
            disabled_providers: Vec::new(),
            custom_executables: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn is_enabled(&self, provider: Provider) -> bool {
        !self.disabled_providers.contains(&provider)
    }

    pub fn enabled_providers(&self) -> impl Iterator<Item = Provider> + '_ {
        Provider::ALL.into_iter().filter(|provider| self.is_enabled(*provider))
    }

    pub fn set_enabled(&mut self, provider: Provider, enabled: bool) {
        if enabled {
            self.disabled_providers.retain(|p| *p != provider);
            return;
        }
        // At least one provider has to stay on.
        if self.enabled_providers().any(|p| p != provider) && self.is_enabled(provider) {
            self.disabled_providers.push(provider);
        }
        if !self.is_enabled(self.default_provider) {
            let first = self.enabled_providers().next();
            self.default_provider = first.unwrap_or_default();
        }
    }
}

/// What Barduino knows about one provider's CLI on this computer.
pub struct Installed {
    pub exe: Option<PathBuf>,
    /// True when `exe` is one the user chose in Settings.
    pub custom: bool,
    /// The CLI's `--version` output, filled in by a background thread.
    version: Arc<Mutex<Option<String>>>,
}

impl Installed {
    pub fn version(&self) -> Option<String> {
        self.version.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Where each provider's CLI was found on this computer, if anywhere.
pub struct Detected([Installed; Provider::ALL.len()]);

impl Detected {
    /// Looks for every CLI, preferring executables chosen in Settings. Versions
    /// are read in the background, and `ctx` is asked to redraw when they arrive.
    pub fn scan(settings: &Settings, ctx: &egui::Context) -> Self {
        Self(Provider::ALL.map(|provider| {
            let custom = settings.custom_executables.get(&provider).filter(|exe| exe.is_file()).cloned();
            let (exe, custom) = match custom {
                Some(exe) => (Some(exe), true),
                None => (provider.find(), false),
            };
            let version = Arc::new(Mutex::new(None));
            if let Some(exe) = exe.clone() {
                let (slot, ctx) = (Arc::clone(&version), ctx.clone());
                std::thread::spawn(move || {
                    let found = read_version(&exe);
                    *slot.lock().unwrap_or_else(PoisonError::into_inner) = found;
                    ctx.request_repaint();
                });
            }
            Installed { exe, custom, version }
        }))
    }

    pub fn get(&self, provider: Provider) -> Option<&PathBuf> {
        self.installed(provider).exe.as_ref()
    }

    pub fn installed(&self, provider: Provider) -> &Installed {
        &self.0[provider as usize]
    }
}

/// Runs `<exe> --version` and keeps the first line, e.g. "2.1.271 (Claude Code)".
fn read_version(exe: &Path) -> Option<String> {
    let output = hidden_command(exe).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
}

/// Settings page state that only lasts while the app is open.
pub struct SettingsPage {
    period: Period,
    confirm_reset: bool,
}

impl Default for SettingsPage {
    fn default() -> Self {
        Self { period: Period::Week, confirm_reset: false }
    }
}

pub enum SettingsAction {
    None,
    Close,
    Rescan,
    /// Run the provider's CLI in a terminal tab, e.g. to sign in.
    OpenInTerminal(Provider),
    ChooseExecutable(Provider),
    UseDetectedExecutable(Provider),
    ResetUsage,
}

/// Everything the Settings page shows besides the settings themselves.
pub struct PageContext<'a> {
    pub detected: &'a Detected,
    pub usage: &'a UsageLog,
    /// What each provider last said about its own plan limits.
    pub plan: &'a BTreeMap<Provider, PlanUsage>,
    pub session_counts: BTreeMap<Provider, usize>,
}

impl SettingsPage {
    pub fn ui(&mut self, ui: &mut egui::Ui, settings: &mut Settings, context: &PageContext<'_>) -> SettingsAction {
        let mut action = SettingsAction::None;

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(PAGE_WIDTH);
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.heading("Settings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Back to chat").clicked() {
                        action = SettingsAction::Close;
                    }
                });
            });

            ui.add_space(22.0);
            section_heading(ui, "Agents", |ui| {
                if ui.small_button("Check again").on_hover_text("Look for newly installed or updated CLIs").clicked() {
                    action = SettingsAction::Rescan;
                }
            });
            hint(
                ui,
                "Each session sends its messages to one of these command-line agents. They use their own \
                 accounts and sign-in.",
            );
            ui.add_space(10.0);
            for provider in Provider::ALL {
                if let Some(clicked) = provider_card(ui, settings, provider, context) {
                    action = clicked;
                }
                ui.add_space(10.0);
            }

            ui.add_space(14.0);
            plan_section(ui, settings, context);

            ui.add_space(14.0);
            if let Some(clicked) = self.usage_section(ui, context.usage) {
                action = clicked;
            }

            ui.add_space(18.0);
            section_heading(ui, "Shortcuts", |_ui| {});
            ui.add_space(8.0);
            card(ui, None, |ui| {
                egui::Grid::new("shortcuts").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    for (keys, what) in [
                        (TERMINAL_SHORTCUT, "Show a terminal in the right panel"),
                        (BROWSER_SHORTCUT, "Show the browser in the right panel"),
                        ("Enter", "Send the message"),
                        ("Shift+Enter", "Start a new line instead of sending"),
                    ] {
                        keys_chip(ui, keys);
                        ui.label(egui::RichText::new(what).small());
                        ui.end_row();
                    }
                });
            });
            ui.add_space(24.0);
        });

        action
    }

    fn usage_section(&mut self, ui: &mut egui::Ui, log: &UsageLog) -> Option<SettingsAction> {
        let mut action = None;
        section_heading(ui, "Tokens spent here", |ui| {
            segmented(ui, "usage_period", &mut self.period, &Period::ALL, Period::label);
        });
        hint(ui, "Only messages sent from Barduino. Using the CLIs elsewhere isn't counted here.");
        ui.add_space(10.0);

        let totals: Vec<(Provider, Usage)> =
            Provider::ALL.into_iter().map(|provider| (provider, log.total(provider, self.period))).collect();
        let mut sum = Usage::default();
        for (_, usage) in &totals {
            sum += *usage;
        }

        card(ui, None, |ui| {
            ui.horizontal(|ui| {
                stat(ui, &sum.turns.to_string(), "messages");
                stat(ui, &usage::format_tokens(sum.total_tokens()), "tokens");
                stat(ui, &usage::format_cost(sum.cost_usd), "est. cost");
            });
        });

        ui.add_space(10.0);
        for (provider, total) in &totals {
            card(ui, None, |ui| {
                provider_usage(ui, *provider, total, sum.total_tokens());
            });
            ui.add_space(8.0);
        }

        ui.add_space(4.0);
        egui::CollapsingHeader::new(egui::RichText::new("All the numbers").small().strong())
            .id_salt("usage_details")
            .show(ui, |ui| {
                egui::Grid::new("usage_table").num_columns(8).spacing([18.0, 6.0]).striped(true).show(ui, |ui| {
                    for heading in
                        ["Agent", "Messages", "Input", "Output", "Cache read", "Cache write", "Total", "Est. cost"]
                    {
                        ui.label(egui::RichText::new(heading).small().strong());
                    }
                    ui.end_row();
                    for (provider, usage) in &totals {
                        usage_row(ui, provider.label(), usage, false);
                    }
                    usage_row(ui, "All agents", &sum, true);
                });
                hint(
                    ui,
                    "Est. cost is Claude Code's own estimate at API prices. On a subscription plan you aren't \
                     billed per token, and the other CLIs don't report a cost.",
                );
            });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if self.confirm_reset {
                ui.label("Clear all usage history?");
                let clear = egui::Button::new(egui::RichText::new("Clear").color(OVER));
                if ui.add(clear).clicked() {
                    self.confirm_reset = false;
                    action = Some(SettingsAction::ResetUsage);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm_reset = false;
                }
            } else if ui.add_enabled(!log.is_empty(), egui::Button::new("Reset usage…")).clicked() {
                self.confirm_reset = true;
            }
        });
        action
    }
}

/// One agent's usage: what it spent in the chosen period, and how that sits
/// against the limit for it, if there is one.
fn provider_usage(ui: &mut egui::Ui, provider: Provider, total: &Usage, all_tokens: u64) {
    ui.horizontal(|ui| {
        avatar(ui, provider, 24.0, false);
        ui.label(egui::RichText::new(provider.label()).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if total.cost_usd.is_some() {
                ui.label(egui::RichText::new(format!("est. {}", usage::format_cost(total.cost_usd))).weak());
            }
            ui.label(egui::RichText::new(usage::format_tokens(total.total_tokens())).monospace().strong());
        });
    });

    ui.add_space(6.0);
    let share = if all_tokens == 0 { 0.0 } else { total.total_tokens() as f32 / all_tokens as f32 };
    meter(ui, share, ui.visuals().selection.bg_fill, None);
    ui.add_space(4.0);
    let of_all = format!("{:.0}% of the tokens spent in this period", share * 100.0);
    ui.label(egui::RichText::new(of_all).small().weak());
}

/// What each provider says about its own plan limits. Barduino only passes these
/// figures on: Claude Code sends them with every reply, and Codex saves them with
/// each run, so they also cover work done outside Barduino.
fn plan_section(ui: &mut egui::Ui, settings: &Settings, context: &PageContext<'_>) {
    section_heading(ui, "Plan limits", |_ui| {});
    hint(ui, "Straight from each agent, including usage that didn't come from Barduino.");
    ui.add_space(10.0);

    for provider in Provider::ALL {
        if !settings.is_enabled(provider) {
            continue;
        }
        card(ui, None, |ui| {
            let reported = context.plan.get(&provider);
            ui.horizontal(|ui| {
                avatar(ui, provider, 24.0, false);
                ui.label(egui::RichText::new(provider.label()).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(plan) = reported {
                        ui.label(egui::RichText::new(plan::read_at_text(plan.read_at)).small().weak());
                    }
                });
            });
            match reported {
                Some(plan) => {
                    ui.add_space(8.0);
                    egui::Grid::new(format!("plan_{}", provider.short_name()))
                        .num_columns(4)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            for window in &plan.windows {
                                plan_window_row(ui, window);
                            }
                        });
                    if !plan.notes.is_empty() {
                        ui.add_space(6.0);
                        hint(ui, &plan.notes.join("  ·  "));
                    }
                }
                None => {
                    ui.add_space(4.0);
                    hint(ui, plan::why_missing(provider));
                }
            }
        });
        ui.add_space(8.0);
    }
}

/// One limit window: what it is, how full it is, and when it starts over.
fn plan_window_row(ui: &mut egui::Ui, window: &crate::plan::Window) {
    const METER_WIDTH: f32 = 220.0;
    let colour = if window.used >= 1.0 {
        OVER
    } else if window.used >= 0.8 {
        WARN
    } else {
        GOOD
    };
    ui.label(egui::RichText::new(&window.name).small());
    meter(ui, window.used, colour, Some(METER_WIDTH));
    let percent = format!("{:.0}%", (window.used * 100.0).min(100.0));
    ui.label(egui::RichText::new(percent).monospace().small().color(colour));
    let left = if window.used >= 1.0 { "used up".to_owned() } else { format!("{:.0}% left", (1.0 - window.used) * 100.0) };
    let resets = plan::resets_text(window.resets_at).map(|text| format!(" · {text}")).unwrap_or_default();
    ui.label(egui::RichText::new(format!("{left}{resets}")).small().weak());
    ui.end_row();
}

fn usage_row(ui: &mut egui::Ui, name: &str, usage: &Usage, strong: bool) {
    let cell = |text: String| {
        let text = egui::RichText::new(text).monospace();
        if strong { text.strong() } else { text }
    };
    ui.label(if strong { egui::RichText::new(name).strong() } else { egui::RichText::new(name) });
    ui.label(cell(usage.turns.to_string()));
    for tokens in [usage.input, usage.output, usage.cache_read, usage.cache_write, usage.total_tokens()] {
        ui.label(cell(usage::format_tokens(tokens)));
    }
    ui.label(cell(usage::format_cost(usage.cost_usd)));
    ui.end_row();
}

fn provider_card(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    provider: Provider,
    context: &PageContext<'_>,
) -> Option<SettingsAction> {
    let mut action = None;
    let installed = context.detected.installed(provider);
    let enabled = settings.is_enabled(provider);
    let is_default = settings.default_provider == provider;
    let accent = is_default.then(|| ui.visuals().selection.stroke.color);

    card(ui, accent, |ui| {
        ui.horizontal(|ui| {
            avatar(ui, provider, 38.0, enabled);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let name = egui::RichText::new(provider.label()).strong().size(15.0);
                    ui.label(if enabled { name } else { name.weak() });
                    if is_default {
                        pill(ui, "Default", ui.visuals().selection.bg_fill);
                    }
                    match &installed.exe {
                        Some(_) => pill(ui, "Installed", GOOD.gamma_multiply(0.25)),
                        None => pill(ui, "Not found", WARN.gamma_multiply(0.25)),
                    }
                });
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    let sessions = context.session_counts.get(&provider).copied().unwrap_or(0);
                    let noun = if sessions == 1 { "session" } else { "sessions" };
                    let mut facts = vec![format!("{sessions} {noun}")];
                    if let Some(version) = installed.version() {
                        facts.insert(0, version);
                    }
                    ui.label(egui::RichText::new(facts.join("  ·  ")).small().weak());
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let last_enabled = enabled && settings.enabled_providers().count() == 1;
                let mut on = enabled;
                let response = ui.add_enabled_ui(!last_enabled, |ui| switch(ui, &mut on)).inner;
                if last_enabled {
                    response.on_disabled_hover_text("At least one agent has to stay on.");
                } else {
                    response.on_hover_text(if enabled { "Switch this agent off" } else { "Switch this agent on" });
                    if on != enabled {
                        settings.set_enabled(provider, on);
                    }
                }
            });
        });

        ui.add_space(10.0);
        match &installed.exe {
            Some(exe) => {
                ui.horizontal(|ui| {
                    let path = egui::RichText::new(exe.display().to_string()).monospace().small().weak();
                    ui.add(egui::Label::new(path).truncate());
                    if installed.custom {
                        ui.label(egui::RichText::new("(chosen here)").small().weak());
                    }
                });
            }
            None => {
                ui.label(egui::RichText::new(provider.install_hint()).small().color(WARN));
            }
        }

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if !is_default && ui.add_enabled(enabled, egui::Button::new("Make default")).clicked() {
                settings.default_provider = provider;
            }
            if installed.exe.is_some()
                && ui
                    .button(format!("Open `{}` in a terminal", provider.command()))
                    .on_hover_text("Runs the CLI interactively, where you can sign in or see your plan usage")
                    .clicked()
            {
                action = Some(SettingsAction::OpenInTerminal(provider));
            }
            let more = ui.button("⋯").on_hover_text("More about this agent's executable");
            egui::Popup::menu(&more).show(|ui| {
                if ui.button("Change executable…").clicked() {
                    action = Some(SettingsAction::ChooseExecutable(provider));
                    ui.close();
                }
                if installed.custom && ui.button("Use the one Barduino finds").clicked() {
                    action = Some(SettingsAction::UseDetectedExecutable(provider));
                    ui.close();
                }
            });
        });
    });
    action
}

/// A heading for a group of settings, with room for a control on the right.
fn section_heading(ui: &mut egui::Ui, title: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title.to_uppercase()).small().strong().color(ui.visuals().weak_text_color()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(2.0);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

/// A panel with a soft background, used for every block on the page.
fn card(ui: &mut egui::Ui, accent: Option<egui::Color32>, contents: impl FnOnce(&mut egui::Ui)) {
    let stroke = match accent {
        Some(colour) => egui::Stroke::new(1.0, colour),
        None => ui.visuals().widgets.noninteractive.bg_stroke,
    };
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(stroke)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            contents(ui);
        });
}

/// A rounded square with the agent's initial, so cards and rows are easy to tell apart.
fn avatar(ui: &mut egui::Ui, provider: Provider, size: f32, highlight: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let fill = if highlight {
        ui.visuals().selection.bg_fill.gamma_multiply(0.35)
    } else {
        ui.visuals().widgets.inactive.bg_fill
    };
    ui.painter().rect_filled(rect, size * 0.28, fill);
    let letter = provider.short_name().chars().next().unwrap_or('?').to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        letter,
        egui::FontId::proportional(size * 0.5),
        ui.visuals().strong_text_color(),
    );
}

/// A small coloured label, e.g. "Default" or "Installed".
fn pill(ui: &mut egui::Ui, text: &str, fill: egui::Color32) {
    egui::Frame::new().fill(fill).corner_radius(9.0).inner_margin(egui::Margin::symmetric(7, 1)).show(ui, |ui| {
        ui.label(egui::RichText::new(text).small().color(ui.visuals().strong_text_color()));
    });
}

/// A key combination drawn like a key cap.
fn keys_chip(ui: &mut egui::Ui, keys: &str) {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(5.0)
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(keys).monospace().small());
        });
}

/// A number with its name underneath, for the usage summary.
fn stat(ui: &mut egui::Ui, value: &str, name: &str) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(value).size(22.0).strong());
        ui.label(egui::RichText::new(name).small().weak());
    });
    ui.add_space(28.0);
}

/// A thin bar showing how full something is. `fraction` above 1.0 fills it completely,
/// and `width` defaults to the room that's left.
fn meter(ui: &mut egui::Ui, fraction: f32, colour: egui::Color32, width: Option<f32>) {
    const HEIGHT: f32 = 6.0;
    let width = width.unwrap_or_else(|| ui.available_width());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().rect_filled(rect, HEIGHT / 2.0, ui.visuals().extreme_bg_color);
    let filled = rect.width() * fraction.clamp(0.0, 1.0);
    if filled > 0.0 {
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(filled.max(HEIGHT), HEIGHT));
        ui.painter().rect_filled(bar, HEIGHT / 2.0, colour);
    }
}

/// An on/off switch, which reads more clearly than a checkbox on a card.
fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(36.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_with_time(response.id, *on, 0.1);
        let visuals = ui.style().interact_selectable(&response, *on);
        let track = if ui.is_enabled() {
            ui.visuals().extreme_bg_color.lerp_to_gamma(GOOD, how_on * 0.9)
        } else {
            ui.visuals().widgets.inactive.bg_fill
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, track);
        let radius = rect.height() / 2.0 - 3.0;
        let travel = rect.width() - rect.height();
        let centre = egui::pos2(rect.left() + rect.height() / 2.0 + travel * how_on, rect.center().y);
        ui.painter().circle_filled(centre, radius, visuals.fg_stroke.color);
    }
    response
}

/// A row of choices joined together, used instead of separate radio buttons.
fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    current: &mut T,
    options: &[T],
    label: impl Fn(T) -> &'static str,
) {
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(2))
        .show(ui, |ui| {
            ui.push_id(id, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for option in options {
                        let selected = *current == *option;
                        if ui.selectable_label(selected, egui::RichText::new(label(*option)).small()).clicked() {
                            *current = *option;
                        }
                    }
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_provider_cannot_be_switched_off() {
        let mut settings = Settings::default();
        let (last, others) = Provider::ALL.split_last().unwrap();
        for provider in others {
            settings.set_enabled(*provider, false);
            assert!(!settings.is_enabled(*provider));
        }
        settings.set_enabled(*last, false);
        assert!(settings.is_enabled(*last));
        assert_eq!(settings.default_provider, *last);
    }

    #[test]
    fn switching_off_the_default_picks_another() {
        let mut settings = Settings::default();
        let default = settings.default_provider;
        settings.set_enabled(default, false);
        assert_ne!(settings.default_provider, default);
        assert!(settings.is_enabled(settings.default_provider));
        settings.set_enabled(default, true);
        assert_eq!(settings.enabled_providers().collect::<Vec<_>>(), Provider::ALL);
    }

    #[test]
    fn switching_off_twice_does_not_duplicate() {
        let mut settings = Settings::default();
        let other = Provider::ALL.into_iter().find(|p| *p != settings.default_provider).expect("two providers");
        settings.set_enabled(other, false);
        settings.set_enabled(other, false);
        assert_eq!(settings.disabled_providers, vec![other]);
    }

    #[test]
    fn reads_a_cli_version() {
        // Any executable that prints a version works; cargo is always here when tests run.
        let cargo = std::env::var_os("CARGO").map(PathBuf::from).expect("tests run under cargo");
        let version = read_version(&cargo).expect("cargo --version prints a version");
        assert!(version.starts_with("cargo "), "{version}");
    }
}
