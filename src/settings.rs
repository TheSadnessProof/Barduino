//! App settings and the Settings page shown in the middle column.

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::agent::{Launcher, Provider};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The provider new sessions start with.
    pub default_provider: Provider,
    /// Providers the user switched off. Stored this way so providers added later start switched on.
    pub disabled_providers: Vec<Provider>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { default_provider: Provider::Claude, disabled_providers: Vec::new() }
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

/// Where each provider's CLI was found on this computer, if anywhere.
pub struct Detected([Option<Launcher>; Provider::ALL.len()]);

impl Detected {
    pub fn scan() -> Self {
        Self(Provider::ALL.map(Provider::find))
    }

    pub fn get(&self, provider: Provider) -> Option<&Launcher> {
        self.0[provider as usize].as_ref()
    }
}

pub enum SettingsAction {
    None,
    Close,
    Rescan,
    /// Run the provider's CLI in the terminal, e.g. to sign in.
    OpenInTerminal(Provider),
}

pub fn page(ui: &mut egui::Ui, settings: &mut Settings, detected: &Detected) -> SettingsAction {
    let mut action = SettingsAction::None;

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.heading("Settings");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Back to chat").clicked() {
                    action = SettingsAction::Close;
                }
            });
        });
        ui.add_space(12.0);

        ui.label(egui::RichText::new("AI providers").strong().size(16.0));
        ui.label(
            egui::RichText::new(
                "Barduino doesn't run AI models itself. Each session is answered by one of these command-line agents, \
                 using its own account and sign-in.",
            )
            .weak(),
        );
        ui.add_space(8.0);

        for provider in Provider::ALL {
            if let Some(clicked) = provider_card(ui, settings, provider, detected.get(provider)) {
                action = clicked;
            }
            ui.add_space(8.0);
        }

        ui.horizontal(|ui| {
            if ui.button("Check again").on_hover_text("Look for newly installed CLIs").clicked() {
                action = SettingsAction::Rescan;
            }
        });
        ui.add_space(16.0);

        ui.label(egui::RichText::new("New sessions").strong().size(16.0));
        ui.horizontal(|ui| {
            ui.label("Start new sessions with:");
            egui::ComboBox::from_id_salt("default_provider")
                .selected_text(settings.default_provider.label())
                .show_ui(ui, |ui| {
                    let enabled: Vec<Provider> = settings.enabled_providers().collect();
                    for provider in enabled {
                        ui.selectable_value(&mut settings.default_provider, provider, provider.label());
                    }
                });
        });
        ui.label(
            egui::RichText::new("You can also pick a provider for each session before sending its first message.")
                .small()
                .weak(),
        );
    });

    action
}

fn provider_card(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    provider: Provider,
    launcher: Option<&Launcher>,
) -> Option<SettingsAction> {
    let mut action = None;
    egui::Frame::group(ui.style()).fill(ui.visuals().faint_bg_color).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(provider.label()).strong());
            match launcher {
                Some(_) => ui.colored_label(egui::Color32::from_rgb(80, 180, 110), "● Installed"),
                None => ui.colored_label(ui.visuals().warn_fg_color, "● Not found"),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut enabled = settings.is_enabled(provider);
                let last_enabled = enabled && settings.enabled_providers().count() == 1;
                let checkbox = ui.add_enabled(!last_enabled, egui::Checkbox::new(&mut enabled, "Use in Barduino"));
                if last_enabled {
                    checkbox.on_disabled_hover_text("At least one provider has to stay on.");
                } else if checkbox.changed() {
                    settings.set_enabled(provider, enabled);
                }
            });
        });

        match launcher {
            Some(launcher) => {
                ui.label(egui::RichText::new(launcher.display()).monospace().small().weak());
                ui.horizontal(|ui| {
                    let command = provider.command();
                    if ui
                        .button(format!("Run `{command}` in the terminal"))
                        .on_hover_text("Opens the CLI in its own interactive mode, where you can sign in or change its settings")
                        .clicked()
                    {
                        action = Some(SettingsAction::OpenInTerminal(provider));
                    }
                    ui.label(
                        egui::RichText::new("If replies fail with a sign-in or API key error, sign in there once.")
                            .small()
                            .weak(),
                    );
                });
            }
            None => {
                ui.label(egui::RichText::new(provider.install_hint()).weak());
            }
        }
    });
    action
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
        assert_eq!(settings.default_provider, Provider::Gemini);
        settings.set_enabled(Provider::Claude, true);
        assert_eq!(settings.enabled_providers().collect::<Vec<_>>(), Provider::ALL);
    }

    #[test]
    fn switching_off_twice_does_not_duplicate() {
        let mut settings = Settings::default();
        settings.set_enabled(Provider::Gemini, false);
        settings.set_enabled(Provider::Gemini, false);
        assert_eq!(settings.disabled_providers, vec![Provider::Gemini]);
    }
}
