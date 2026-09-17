//! App settings and the Settings page shown in the middle column.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::Provider;
use crate::usage::{self, Period, Usage, UsageLog};

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
    let mut cmd = Command::new(exe);
    cmd.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().ok()?;
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
    pub session_counts: BTreeMap<Provider, usize>,
}

impl SettingsPage {
    pub fn ui(&mut self, ui: &mut egui::Ui, settings: &mut Settings, context: &PageContext<'_>) -> SettingsAction {
        let mut action = SettingsAction::None;

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(900.0);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.heading("Settings");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Back to chat").clicked() {
                        action = SettingsAction::Close;
                    }
                });
            });
            ui.add_space(16.0);

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Providers").strong().size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Check again").on_hover_text("Look for newly installed or updated CLIs").clicked() {
                        action = SettingsAction::Rescan;
                    }
                });
            });
            ui.label(
                egui::RichText::new(
                    "Barduino sends each session's messages to one of these command-line agents. \
                     They use their own accounts and sign-in.",
                )
                .weak(),
            );
            ui.add_space(8.0);
            for provider in Provider::ALL {
                if let Some(clicked) = provider_card(ui, settings, provider, context) {
                    action = clicked;
                }
                ui.add_space(8.0);
            }

            ui.add_space(12.0);
            if let Some(clicked) = self.usage_section(ui, context.usage) {
                action = clicked;
            }
            ui.add_space(16.0);
        });

        action
    }

    fn usage_section(&mut self, ui: &mut egui::Ui, log: &UsageLog) -> Option<SettingsAction> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Token usage").strong().size(16.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for period in Period::ALL.into_iter().rev() {
                    ui.selectable_value(&mut self.period, period, period.label());
                }
            });
        });
        ui.label(
            egui::RichText::new(
                "Counts messages sent from Barduino. Using the CLIs outside Barduino isn't included.",
            )
            .weak(),
        );
        ui.add_space(6.0);

        let totals: Vec<(Provider, Usage)> =
            Provider::ALL.into_iter().map(|provider| (provider, log.total(provider, self.period))).collect();
        let mut sum = Usage::default();
        for (_, usage) in &totals {
            sum += *usage;
        }

        egui::Frame::group(ui.style()).fill(ui.visuals().faint_bg_color).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("usage_table").num_columns(8).spacing([18.0, 6.0]).striped(true).show(ui, |ui| {
                for heading in ["Provider", "Messages", "Input", "Output", "Cache read", "Cache write", "Total", "Est. cost"] {
                    ui.label(egui::RichText::new(heading).small().strong());
                }
                ui.end_row();
                for (provider, usage) in &totals {
                    usage_row(ui, provider.label(), usage, false);
                }
                usage_row(ui, "All providers", &sum, true);
            });
        });
        ui.label(
            egui::RichText::new(
                "Est. cost is Claude Code's own estimate at API prices. With a subscription plan you aren't billed \
                 per token. Antigravity doesn't report a cost.",
            )
            .small()
            .weak(),
        );

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if self.confirm_reset {
                ui.label("Clear all usage history?");
                let clear = egui::Button::new(egui::RichText::new("Clear").color(ui.visuals().error_fg_color));
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

    let stroke = if is_default {
        egui::Stroke::new(1.0, ui.visuals().selection.stroke.color)
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke
    };
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(stroke)
        .corner_radius(8.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                let name = egui::RichText::new(provider.label()).strong().size(15.0);
                ui.label(if enabled { name } else { name.weak() });
                if is_default {
                    badge(ui, "Default", ui.visuals().selection.bg_fill);
                }
                if !enabled {
                    badge(ui, "Off", ui.visuals().widgets.inactive.bg_fill);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = enabled;
                    let last_enabled = enabled && settings.enabled_providers().count() == 1;
                    let switch = ui.add_enabled(!last_enabled, egui::Checkbox::new(&mut on, "Use in Barduino"));
                    if last_enabled {
                        switch.on_disabled_hover_text("At least one provider has to stay on.");
                    } else if switch.changed() {
                        settings.set_enabled(provider, on);
                    }
                });
            });

            ui.horizontal_wrapped(|ui| {
                match &installed.exe {
                    Some(_) => ui.colored_label(egui::Color32::from_rgb(80, 180, 110), "● Installed"),
                    None => ui.colored_label(ui.visuals().warn_fg_color, "● Not found"),
                };
                if let Some(version) = installed.version() {
                    ui.label(egui::RichText::new(format!("· {version}")).weak());
                }
                let sessions = context.session_counts.get(&provider).copied().unwrap_or(0);
                let noun = if sessions == 1 { "session" } else { "sessions" };
                ui.label(egui::RichText::new(format!("· {sessions} {noun}")).weak());
            });

            match &installed.exe {
                Some(exe) => {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(exe.display().to_string()).monospace().small().weak())
                                .truncate(),
                        );
                        if installed.custom {
                            ui.label(egui::RichText::new("(chosen in Settings)").small().weak());
                        }
                    });
                }
                None => {
                    ui.label(egui::RichText::new(provider.install_hint()).weak());
                }
            }

            let today = context.usage.total(provider, Period::Today);
            let week = context.usage.total(provider, Period::Week);
            let mut summary = format!(
                "Today {} tokens · Last 7 days {} tokens",
                usage::format_tokens(today.total_tokens()),
                usage::format_tokens(week.total_tokens())
            );
            if week.cost_usd.is_some() {
                summary.push_str(&format!(" · est. {}", usage::format_cost(week.cost_usd)));
            }
            ui.label(egui::RichText::new(summary).small());

            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if !is_default && ui.add_enabled(enabled, egui::Button::new("Make default")).clicked() {
                    settings.default_provider = provider;
                }
                if ui
                    .button("Change executable…")
                    .on_hover_text("Use a different copy of this CLI, for example a specific version")
                    .clicked()
                {
                    action = Some(SettingsAction::ChooseExecutable(provider));
                }
                if installed.custom && ui.button("Use detected").clicked() {
                    action = Some(SettingsAction::UseDetectedExecutable(provider));
                }
                if installed.exe.is_some()
                    && ui
                        .button(format!("Open `{}` in a terminal", provider.command()))
                        .on_hover_text("Runs the CLI interactively, where you can sign in or change its own settings")
                        .clicked()
                {
                    action = Some(SettingsAction::OpenInTerminal(provider));
                }
            });
        });
    action
}

fn badge(ui: &mut egui::Ui, text: &str, fill: egui::Color32) {
    egui::Frame::new().fill(fill).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 1)).show(ui, |ui| {
        ui.label(egui::RichText::new(text).small().color(ui.visuals().strong_text_color()));
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
        settings.set_enabled(Provider::Claude, false);
        assert_eq!(settings.default_provider, Provider::Antigravity);
        settings.set_enabled(Provider::Claude, true);
        assert_eq!(settings.enabled_providers().collect::<Vec<_>>(), Provider::ALL);
    }

    #[test]
    fn switching_off_twice_does_not_duplicate() {
        let mut settings = Settings::default();
        settings.set_enabled(Provider::Antigravity, false);
        settings.set_enabled(Provider::Antigravity, false);
        assert_eq!(settings.disabled_providers, vec![Provider::Antigravity]);
    }

    #[test]
    fn reads_a_cli_version() {
        // Any executable that prints a version works; cargo is always here when tests run.
        let cargo = std::env::var_os("CARGO").map(PathBuf::from).expect("tests run under cargo");
        let version = read_version(&cargo).expect("cargo --version prints a version");
        assert!(version.starts_with("cargo "), "{version}");
    }
}
