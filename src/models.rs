//! Which models each agent CLI offers, and how hard they can be asked to work.
//! The lists come from the CLIs themselves where they can be asked, so they stay
//! right as the providers add models.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;

use crate::agent::{self, Provider};

/// One model a session can be set to.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// What the CLI is given, e.g. "opus" or "gpt-6-astra".
    pub id: String,
    pub label: String,
    /// The effort levels this model takes, when the CLI says which.
    pub efforts: Vec<String>,
}

/// The effort levels a provider takes when a model doesn't name its own.
fn default_efforts(provider: Provider) -> Vec<String> {
    let levels: &[&str] = match provider {
        // Claude Code takes the whole range on any model.
        Provider::Claude => &["low", "medium", "high", "xhigh", "max"],
        Provider::Codex => &["low", "medium", "high", "xhigh", "max"],
        // agy only has three, and most of its models bake the level into their name.
        Provider::Antigravity => &["low", "medium", "high"],
    };
    levels.iter().map(|level| (*level).to_owned()).collect()
}

/// How an effort level is written in the picker.
pub fn effort_label(effort: &str) -> String {
    match effort {
        "xhigh" => "Extra high".to_owned(),
        "max" => "Maximum".to_owned(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

/// The models a provider offers, asked of the CLI where that is possible.
fn discover(provider: Provider, exe: &Path) -> Vec<Model> {
    match provider {
        // Claude Code has no command that lists models, so these are its own aliases,
        // which always point at the current version of each model.
        Provider::Claude => ["Opus", "Sonnet", "Haiku", "Fable"]
            .into_iter()
            .map(|name| Model {
                id: name.to_lowercase(),
                label: name.to_owned(),
                efforts: default_efforts(provider),
            })
            .collect(),
        Provider::Codex => codex_models(exe),
        Provider::Antigravity => agy_models(exe),
    }
}

/// `codex debug models` prints the catalog, including the effort levels each model takes.
fn codex_models(exe: &Path) -> Vec<Model> {
    let Ok(output) = agent::hidden_command(exe).args(["debug", "models"]).output() else {
        return Vec::new();
    };
    let Ok(catalog) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return Vec::new();
    };
    catalog["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model["slug"].as_str()?.to_owned();
            let label = model["display_name"].as_str().unwrap_or(&id).to_owned();
            let efforts: Vec<String> = model["supported_reasoning_levels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|level| level["effort"].as_str().map(str::to_owned))
                .collect();
            Some(Model { id, label, efforts })
        })
        .collect()
}

/// `agy models` prints one model per line, as an id and a name separated by a tab.
fn agy_models(exe: &Path) -> Vec<Model> {
    let Ok(output) = agent::hidden_command(exe).arg("models").output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (id, label) = line.split_once('\t')?;
            let (id, label) = (id.trim(), label.trim());
            if id.is_empty() || id.contains(' ') {
                return None;
            }
            Some(Model {
                id: id.to_owned(),
                label: label.to_owned(),
                efforts: default_efforts(Provider::Antigravity),
            })
        })
        .collect()
}

/// The model lists, filled in by background threads the first time each provider is asked for.
#[derive(Default)]
pub struct Catalog {
    inner: Arc<Mutex<BTreeMap<Provider, Vec<Model>>>>,
    /// Providers whose list is on its way.
    loading: Arc<Mutex<BTreeMap<Provider, bool>>>,
}

impl Catalog {
    /// The models known for a provider. Empty while the list is still being read,
    /// which `start` sets going.
    pub fn models(&self, provider: Provider) -> Vec<Model> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).get(&provider).cloned().unwrap_or_default()
    }

    pub fn is_loading(&self, provider: Provider) -> bool {
        *self.loading.lock().unwrap_or_else(PoisonError::into_inner).get(&provider).unwrap_or(&false)
    }

    /// Reads a provider's models once per launch.
    pub fn start(&self, provider: Provider, exe: PathBuf, ctx: &egui::Context) {
        {
            let mut loading = self.loading.lock().unwrap_or_else(PoisonError::into_inner);
            let known = self.inner.lock().unwrap_or_else(PoisonError::into_inner).contains_key(&provider);
            if known || loading.get(&provider).copied().unwrap_or(false) {
                return;
            }
            loading.insert(provider, true);
        }
        let (inner, loading, ctx) = (Arc::clone(&self.inner), Arc::clone(&self.loading), ctx.clone());
        std::thread::spawn(move || {
            let models = discover(provider, &exe);
            inner.lock().unwrap_or_else(PoisonError::into_inner).insert(provider, models);
            loading.lock().unwrap_or_else(PoisonError::into_inner).insert(provider, false);
            ctx.request_repaint();
        });
    }

    /// The effort levels to offer for a session, which depend on the model when
    /// the provider says so.
    pub fn efforts(&self, provider: Provider, model_id: Option<&str>) -> Vec<String> {
        let models = self.models(provider);
        let chosen = model_id.and_then(|id| models.iter().find(|model| model.id == id));
        match chosen {
            Some(model) if !model.efforts.is_empty() => model.efforts.clone(),
            _ => default_efforts(provider),
        }
    }

    /// How a session's model is named in the picker.
    pub fn label_for(&self, provider: Provider, model_id: &str) -> String {
        self.models(provider)
            .into_iter()
            .find(|model| model.id == model_id)
            .map(|model| model.label)
            .unwrap_or_else(|| model_id.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_effort_levels_for_people() {
        assert_eq!(effort_label("low"), "Low");
        assert_eq!(effort_label("xhigh"), "Extra high");
        assert_eq!(effort_label("max"), "Maximum");
        assert_eq!(effort_label(""), "");
    }

    #[test]
    fn claude_offers_its_aliases() {
        let models = discover(Provider::Claude, Path::new("claude"));
        assert!(models.iter().any(|model| model.id == "opus" && model.label == "Opus"), "{models:?}");
        assert!(models.iter().all(|model| model.efforts.contains(&"max".to_owned())));
    }

    #[test]
    fn falls_back_to_the_providers_own_levels() {
        let catalog = Catalog::default();
        // Nothing is loaded yet, so a session still gets something to choose from.
        assert_eq!(catalog.efforts(Provider::Antigravity, None), ["low", "medium", "high"]);
        assert_eq!(catalog.label_for(Provider::Codex, "gpt-6-astra"), "gpt-6-astra");
        assert!(!catalog.is_loading(Provider::Claude));
    }

    /// Asks the installed CLIs for their models. Depends on them being installed and
    /// signed in, so it only runs when asked for:
    /// `cargo test -- --ignored real_models --nocapture`
    #[test]
    #[ignore]
    fn real_models_come_from_the_clis() {
        for provider in [Provider::Codex, Provider::Antigravity] {
            let exe = provider.find().expect("the CLI should be installed");
            let models = discover(provider, &exe);
            println!("{}: {} models", provider.label(), models.len());
            for model in models.iter().take(4) {
                println!("  {} — {} (efforts: {:?})", model.id, model.label, model.efforts);
            }
            assert!(!models.is_empty(), "{} should list models", provider.label());
            assert!(models.iter().all(|model| !model.id.is_empty() && !model.label.is_empty()));
        }
    }
}
