//! Provider slash commands (`/`): built-in actions, custom project commands,
//! and agent skills discovered in real time as the user types.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::agent::{self, PermissionMode, Provider};
use crate::models::Model;

/// How long a scan is reused before the folders are read again: long enough that
/// typing doesn't re-read the disk on every frame, short enough that a skill added
/// while the menu is open still turns up.
const RESCAN_AFTER: Duration = Duration::from_millis(1500);

/// The last scan, kept because [`discover`] is called from the composer on every
/// frame the menu is open, and reading several directories plus a `SKILL.md` each
/// at frame rate is a lot of disk for a list that hardly ever changes.
static LAST_SCAN: Mutex<Option<Scan>> = Mutex::new(None);

struct Scan {
    provider: Provider,
    project_dir: PathBuf,
    at: Instant,
    commands: Vec<SlashCommand>,
}

/// The origin of a slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSource {
    /// Native command built into the CLI provider itself.
    Builtin,
    /// An agent skill defined in `.agents/skills` or `.claude/skills`.
    Skill,
    /// A custom command defined in `.claude/commands` or workspace config.
    Project,
    /// Viper's own. These change how the next turn runs, which is the CLIs' own
    /// job in their interactive interfaces — but Viper runs them headless, so it
    /// carries them out itself, the same way for every provider.
    Viper,
}

impl CommandSource {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Builtin => "Built-in",
            Self::Skill => "Skill",
            Self::Project => "Project",
            Self::Viper => "Viper",
        }
    }
}

/// Viper's own commands, offered whichever CLI is answering. They are the same
/// settings the row under the message box shows.
const VIPER_COMMANDS: &[(&str, &str)] = &[
    ("model", "Choose the model for this session, e.g. /model opus"),
    ("effort", "Choose how hard the model works, e.g. /effort high"),
    ("permission", "Choose what the agent may do without asking, e.g. /permission full"),
    ("clear", "Start a fresh session in the same folder"),
    ("settings", "Open Viper's settings"),
];

/// The other spellings the CLIs use for those same commands. They work when typed,
/// but they don't each get a row of their own in the menu.
const ALSO_OURS: &[&str] = &["permissions", "mode", "config"];

/// One slash command that can be suggested when typing `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommand {
    /// Command name without the leading `/`, e.g. "goal" or "compact".
    pub name: String,
    /// Plain English explanation of what this command does.
    pub description: String,
    pub source: CommandSource,
}

/// Native commands built into each CLI provider.
pub fn builtin_commands(provider: Provider) -> &'static [(&'static str, &'static str)] {
    match provider {
        Provider::Claude => &[
            ("help", "Show help and available commands"),
            ("compact", "Free up context by summarizing the conversation so far"),
            ("clear", "Start a new session with empty context; previous session stays on disk"),
            ("cost", "Show token usage and estimated cost for this session"),
            ("context", "Visualize current context usage as a colored grid"),
            ("memory", "Edit CLAUDE.md files and memory settings"),
            ("review", "Review recent changes and diffs in the project"),
            ("init", "Initialize CLAUDE.md guidelines and project config"),
            ("doctor", "Check system health and tool installations"),
            ("add-dir", "Add a new working directory"),
            ("artifacts", "Browse your published and shared artifacts"),
            ("btw", "Ask a quick side question without interrupting the main conversation"),
            ("cd", "Move this session to a new working directory"),
            ("ide", "Manage IDE integrations and show status"),
            ("install-github-app", "Set up Claude GitHub Actions for a repository"),
            ("install-slack-app", "Install the Claude Slack app"),
            ("mcp", "Manage MCP servers"),
            ("resume", "Resume a previous conversation"),
            ("skills", "List available skills"),
            ("tasks", "View and manage everything running in the background"),
            ("skill-doctor", "Show which loaded skills are unused and costing context"),
            ("permissions", "Manage allow and deny tool permission rules"),
            ("branch", "Create a branch of the current conversation at this point"),
            ("fork", "Spawn a background agent that inherits the full conversation"),
            ("subtask", "Send a subagent off with your full context; its result comes back here"),
            ("reload-plugins", "Activate pending plugin changes in the current session"),
            ("reload-skills", "Pick up skills added or changed on disk during this session"),
            ("ultraplan", "Claude Code on the web drafts a plan you can edit and approve"),
            ("ultrareview", "Find and verify bugs in your branch using Claude Code on the web"),
            ("teleport", "Send this session to the cloud, or resume one from claude.ai"),
            ("schedule", "Create and manage scheduled remote Claude Code agents"),
            ("autofix-pr", "Monitor and autofix any issues with the current PR"),
            ("model", "Set model for this session"),
            ("effort", "Set effort level for model usage"),
            ("plan", "Enable plan mode or view the current session plan"),
            ("theme", "Change the theme"),
            ("tui", "Set the terminal UI renderer (default | fullscreen)"),
            ("config", "Open settings"),
            ("pr-comments", "Fetch and review GitHub pull request comments"),
            ("bug", "Report a bug or share your conversation"),
            ("feedback", "Send feedback to Anthropic or report a bug"),
            ("login", "Sign in to your Anthropic account"),
            ("logout", "Sign out from your Anthropic account"),
            ("terminal-setup", "Configure terminal integration and Shift+Enter bindings"),
            ("copy", "Copy Claude's last response to clipboard"),
            ("autocompact", "Set how full the context gets before auto-summarizing"),
            ("status", "Show Claude Code status including version, model, account, and tools"),
            ("voice", "Toggle voice mode"),
            ("powerup", "Discover Claude Code features through quick interactive lessons"),
            ("loops", "List, create, and delete loops"),
            ("hooks", "View hook configurations for tool events"),
            ("export", "Export the current conversation to a file or clipboard"),
            ("usage-credits", "Configure usage credits or request them from your admin"),
            ("recap", "Generate a one-line session recap now"),
            ("goal", "Set a goal Claude checks before stopping"),
        ],
        Provider::Antigravity => &[
            ("help", "Show available commands and keybindings"),
            ("agents", "List available custom agents"),
            ("changelog", "Show release notes and changes"),
            ("config", "Open settings panel"),
            ("credits", "Show remaining G1 credits and purchase link"),
            ("effort", "Set the reasoning effort"),
            ("hooks", "Manage hook configurations for tool events"),
            ("model", "Set a model, or run a single prompt on another model"),
            ("permissions", "Manage tool permissions"),
            ("skills", "List available skills"),
            ("usage", "View model quota usage"),
            ("mode", "Set the agent execution mode (accept-edits, plan)"),
            ("plugin", "Manage plugins (install, uninstall, list, enable, disable)"),
            ("goal", "Run an autonomous long-running task thoroughly until completion"),
            ("schedule", "Schedule an instruction on a recurring cron schedule or timer"),
            ("grill-me", "Align on a plan through an interactive interview to resolve decisions"),
            ("learn", "Extract and persist learnings, patterns, and conventions from this session"),
            ("remote-control", "Manage the remote-control background daemon"),
            ("clear", "Start a new session with empty context"),
            ("compact", "Summarize conversation history to conserve context"),
        ],
        Provider::Codex => &[
            ("help", "View Codex help and available commands"),
            ("review", "Review changes in the repository for bugs and style"),
            ("fix", "Diagnose and fix errors in code or failing tests"),
            ("explain", "Explain code structure, architecture, or logic"),
            ("test", "Generate comprehensive unit tests for the current code"),
            ("plan", "Create an implementation plan before making modifications"),
            ("goal", "Set or adjust objective for the current session"),
            ("diff", "Show git diff of uncommitted changes"),
            ("clear", "Reset conversation context and start fresh"),
            ("compact", "Compact context by summarizing conversation history"),
            ("model", "View or change the active model"),
            ("mcp", "Manage external MCP servers and tools"),
            ("skills", "List and manage installed Codex skills"),
            ("apps", "Manage connected ChatGPT apps and plugins"),
            ("usage", "View quota, token usage, and limits"),
            ("recap", "Generate a summary of the current session"),
            ("raw", "Toggle raw output mode without formatting"),
            ("keymap", "Display keyboard shortcuts and key bindings"),
            ("status", "Show session metadata and agent status"),
            ("settings", "Open configuration and preferences"),
            ("feedback", "Submit feedback or report an issue"),
            ("logout", "Remove stored authentication credentials"),
            ("exit", "End the current session"),
        ],
    }
}

/// Discovers all available slash commands for a provider in real time,
/// including built-in commands, project skills, and custom commands.
pub fn discover(provider: Provider, project_dir: &Path) -> Vec<SlashCommand> {
    let mut last = LAST_SCAN.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(scan) = last.as_ref()
        && scan.provider == provider
        && scan.project_dir == project_dir
        && scan.at.elapsed() < RESCAN_AFTER
    {
        return scan.commands.clone();
    }
    let commands = scan_everything(provider, project_dir);
    *last = Some(Scan {
        provider,
        project_dir: project_dir.to_owned(),
        at: Instant::now(),
        commands: commands.clone(),
    });
    commands
}

/// Reads the folders. Separate from [`discover`] so that it can be cached.
fn scan_everything(provider: Provider, project_dir: &Path) -> Vec<SlashCommand> {
    let mut commands = Vec::new();
    let mut seen = HashSet::new();

    // 0. Viper's own, first so that where a CLI has a command of the same name
    // these are the ones offered — they are the ones that actually take effect here.
    for &(name, desc) in VIPER_COMMANDS {
        if seen.insert(name.to_lowercase()) {
            commands.push(SlashCommand {
                name: name.to_owned(),
                description: desc.to_owned(),
                source: CommandSource::Viper,
            });
        }
    }
    seen.extend(ALSO_OURS.iter().map(|alias| (*alias).to_owned()));

    // 1. Built-in provider commands.
    for &(name, desc) in builtin_commands(provider) {
        if seen.insert(name.to_lowercase()) {
            commands.push(SlashCommand {
                name: name.to_owned(),
                description: desc.to_owned(),
                source: CommandSource::Builtin,
            });
        }
    }

    // 2. Project-level commands and skills.
    if project_dir.is_dir() {
        match provider {
            Provider::Claude => {
                scan_claude_commands(&project_dir.join(".claude").join("commands"), &mut commands, &mut seen);
                scan_skills(&project_dir.join(".claude").join("skills"), &mut commands, &mut seen);
                scan_skills(&project_dir.join(".agents").join("skills"), &mut commands, &mut seen);
            }
            Provider::Antigravity => {
                scan_skills(&project_dir.join(".agents").join("skills"), &mut commands, &mut seen);
                scan_skills(&project_dir.join(".gemini").join("skills"), &mut commands, &mut seen);
            }
            Provider::Codex => {
                scan_skills(&project_dir.join(".codex").join("skills"), &mut commands, &mut seen);
                scan_skills(&project_dir.join(".agents").join("skills"), &mut commands, &mut seen);
            }
        }
    }

    // 3. User global commands and skills.
    if let Some(home) = agent::home_dir() {
        match provider {
            Provider::Claude => {
                scan_claude_commands(&home.join(".claude").join("commands"), &mut commands, &mut seen);
                scan_skills(&home.join(".claude").join("skills"), &mut commands, &mut seen);
                scan_claude_plugin_marketplaces(
                    &home.join(".claude").join("plugins").join("marketplaces"),
                    &mut commands,
                    &mut seen,
                );
            }
            Provider::Antigravity => {
                scan_skills(&home.join(".gemini").join("config").join("skills"), &mut commands, &mut seen);
                scan_skills(
                    &home.join(".gemini").join("antigravity-ide").join("builtin").join("skills"),
                    &mut commands,
                    &mut seen,
                );
            }
            Provider::Codex => {
                scan_skills(&home.join(".codex").join("skills"), &mut commands, &mut seen);
                scan_skills(&home.join(".codex").join("skills").join(".system"), &mut commands, &mut seen);
            }
        }
    }

    commands
}

/// Scans marketplace plugins for packaged commands and skills (`<marketplace>/plugins/*`).
fn scan_claude_plugin_marketplaces(dir: &Path, commands: &mut Vec<SlashCommand>, seen: &mut HashSet<String>) {
    let Ok(marketplaces) = std::fs::read_dir(dir) else { return };
    for marketplace in marketplaces.flatten() {
        let plugins = marketplace.path().join("plugins");
        let Ok(entries) = std::fs::read_dir(&plugins) else { continue };
        for entry in entries.flatten() {
            let plugin_dir = entry.path();
            if !plugin_dir.is_dir() {
                continue;
            }
            scan_claude_commands(&plugin_dir.join("commands"), commands, seen);
            scan_skills(&plugin_dir.join("skills"), commands, seen);
        }
    }
}

/// Scans a directory of skills (`<dir>/<skill_name>/SKILL.md`).
fn scan_skills(skills_dir: &Path, commands: &mut Vec<SlashCommand>, seen: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(skills_dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_file = path.join("SKILL.md");
        if !skill_file.is_file() {
            continue;
        }
        let folder_name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let Ok(content) = std::fs::read_to_string(&skill_file) else { continue };
        let (name, description) = parse_skill_frontmatter(&content, folder_name);
        if seen.insert(name.to_lowercase()) {
            commands.push(SlashCommand {
                name,
                description,
                source: CommandSource::Skill,
            });
        }
    }
}

/// Scans a directory of Claude markdown commands (`<dir>/*.md`).
fn scan_claude_commands(dir: &Path, commands: &mut Vec<SlashCommand>, seen: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let description = extract_command_description(&content).unwrap_or_else(|| "Custom project command".to_owned());
        if seen.insert(stem.to_lowercase()) {
            commands.push(SlashCommand {
                name: stem.to_owned(),
                description,
                source: CommandSource::Project,
            });
        }
    }
}

/// Tolerantly extracts `name:` and `description:` from YAML frontmatter in a `SKILL.md`.
pub fn parse_skill_frontmatter(content: &str, default_name: &str) -> (String, String) {
    let mut name = default_name.to_owned();
    let mut description = String::new();

    let trimmed = content.trim_start();
    if let Some(rest) = trimmed.strip_prefix("---")
        && let Some(end) = rest.find("\n---")
    {
        let frontmatter = &rest[..end];
            for line in frontmatter.lines() {
                let line = line.trim();
                if let Some(val) = line.strip_prefix("name:").map(str::trim) {
                    let unquoted = val.trim_matches(|c| c == '"' || c == '\'');
                    if !unquoted.is_empty() {
                        name = unquoted.to_owned();
                    }
                } else if let Some(val) = line.strip_prefix("description:").map(str::trim) {
                    let unquoted = val.trim_matches(|c| c == '"' || c == '\'');
                    if !unquoted.is_empty() {
                        description = unquoted.to_owned();
                    }
                }
            }
    }

    if description.is_empty() {
        description = extract_command_description(content).unwrap_or_else(|| "Agent skill".to_owned());
    }

    (name, description)
}

/// Extracts the first heading or informative line from a command or skill markdown file.
fn extract_command_description(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("<!--") || line == "---" {
            continue;
        }
        if let Some(heading) = line.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim();
            if !heading.is_empty() {
                return Some(heading.to_owned());
            }
        }
        // Take first plain line up to 100 characters.
        let mut clean = line.to_owned();
        if clean.len() > 100 {
            clean.truncate(100);
            clean.push('…');
        }
        return Some(clean);
    }
    None
}

/// Filters commands matching a query string, prioritizing exact prefix matches on
/// the command name, followed by substring matches, then description matches.
pub fn filter<'a>(commands: &'a [SlashCommand], query: &str) -> Vec<&'a SlashCommand> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return commands.iter().collect();
    }

    let mut prefix_matches = Vec::new();
    let mut name_matches = Vec::new();
    let mut desc_matches = Vec::new();

    for cmd in commands {
        let name_lower = cmd.name.to_lowercase();
        if name_lower.starts_with(&query) {
            prefix_matches.push(cmd);
        } else if name_lower.contains(&query) {
            name_matches.push(cmd);
        } else if cmd.description.to_lowercase().contains(&query) {
            desc_matches.push(cmd);
        }
    }

    prefix_matches.extend(name_matches);
    prefix_matches.extend(desc_matches);
    prefix_matches
}

/// What sending a slash command actually does.
///
/// The CLIs' built-in commands belong to their own interactive session. Viper
/// runs them headless, where there is no such session — so a command that changes
/// how the next turn runs has to be carried out here, and a few can't be reached
/// at all without a real terminal. Saying which is which is the difference between
/// a menu that works and a menu that quietly does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handling {
    /// Viper applies it to this session itself.
    Here,
    /// Goes to the CLI as the prompt it is: a skill, a project command, or one of
    /// the CLI's own commands that expands into a prompt.
    Cli,
    /// Only the CLI's own terminal can do it — signing in, themes, key bindings.
    Terminal,
}

/// Commands that need the CLI running in a terminal, because they change how its
/// own interface behaves or ask something a one-shot run can't be asked.
const NEEDS_TERMINAL: &[&str] =
    &["login", "logout", "terminal-setup", "theme", "tui", "voice", "keymap", "ide", "exit", "raw", "resume"];

/// What happens when this command is sent, so the menu can say so before it is.
pub fn handling(name: &str, source: CommandSource) -> Handling {
    match source {
        CommandSource::Viper => Handling::Here,
        // A skill or a project command is a prompt expansion whoever runs it.
        CommandSource::Skill | CommandSource::Project => Handling::Cli,
        CommandSource::Builtin if NEEDS_TERMINAL.contains(&name) => Handling::Terminal,
        CommandSource::Builtin => Handling::Cli,
    }
}

/// A change to the session that a slash command is asking for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashAction {
    /// `None` hands the choice back to whatever the CLI defaults to.
    Model(Option<String>),
    Effort(Option<String>),
    Permission(PermissionMode),
    /// A fresh session in the same folder, which is what the CLIs' `/clear` does.
    Clear,
    OpenSettings,
    /// Start the CLI in a terminal, for a command only its own interface can run.
    /// Carries the command name so the user can be told what to type there.
    OpenTerminal(String),
}

/// What the CLIs themselves call these modes, so a habit picked up in one of them
/// works here too. The labels already cover "read only", "full access" and
/// "plan only", which is why those aren't repeated.
const PERMISSION_ALIASES: &[(&str, PermissionMode)] =
    &[("acceptedits", PermissionMode::AcceptEdits), ("bypasspermissions", PermissionMode::Full)];

/// Reads a typed message as a command Viper should carry out rather than send.
///
/// Returns `None` when it isn't one of ours and should go to the CLI as written,
/// and `Err` with a sentence for the user when it is ours but the argument isn't.
/// Nothing here reaches the CLI, which matters: a turn spent asking an agent to
/// change its own model is a turn the user pays for and gets nothing from.
pub fn intercept(input: &str, models: &[Model], efforts: &[String]) -> Option<Result<SlashAction, String>> {
    let rest = input.trim().strip_prefix('/')?;
    let (name, argument) = match rest.split_once(char::is_whitespace) {
        Some((name, argument)) => (name, argument.trim()),
        None => (rest, ""),
    };
    match name.to_lowercase().as_str() {
        "model" => Some(pick_model(argument, models)),
        "effort" => Some(pick_effort(argument, efforts)),
        "permission" | "permissions" | "mode" => Some(pick_permission(argument)),
        "clear" => Some(Ok(SlashAction::Clear)),
        "config" | "settings" => Some(Ok(SlashAction::OpenSettings)),
        // Signing in, themes and key bindings belong to the CLI's own interface.
        // Sent as a message they would just be words in front of the model, so the
        // CLI is started in a terminal where the command actually works instead.
        other if NEEDS_TERMINAL.contains(&other) => Some(Ok(SlashAction::OpenTerminal(other.to_owned()))),
        _ => None,
    }
}

fn pick_model(argument: &str, models: &[Model]) -> Result<SlashAction, String> {
    if models.is_empty() {
        return Err("The model list is still being read from the CLI. Try again in a moment.".to_owned());
    }
    let offered = choices(models.iter().map(|model| model.id.as_str()));
    if argument.is_empty() {
        return Err(format!("Which model? Try {offered}, or “default” — or pick one from the row under the message box."));
    }
    if means_default(argument) {
        return Ok(SlashAction::Model(None));
    }
    // Matched on the label as well, since that is the name the picker shows.
    let wanted = argument.to_lowercase();
    match models.iter().find(|m| m.id.to_lowercase() == wanted || m.label.to_lowercase() == wanted) {
        Some(model) => Ok(SlashAction::Model(Some(model.id.clone()))),
        None => Err(format!("There's no model called “{argument}”. Try {offered}.")),
    }
}

fn pick_effort(argument: &str, efforts: &[String]) -> Result<SlashAction, String> {
    let offered = choices(efforts.iter().map(String::as_str));
    if argument.is_empty() {
        return Err(format!("How much effort? Try {offered}, or “default” — or pick one from the row under the message box."));
    }
    if means_default(argument) {
        return Ok(SlashAction::Effort(None));
    }
    match efforts.iter().find(|level| level.eq_ignore_ascii_case(argument)) {
        Some(level) => Ok(SlashAction::Effort(Some(level.clone()))),
        None => Err(format!("There's no effort level called “{argument}”. Try {offered}.")),
    }
}

fn pick_permission(argument: &str) -> Result<SlashAction, String> {
    let offered = choices(PermissionMode::ALL.iter().map(|mode| mode.keyword()));
    if argument.is_empty() {
        return Err(format!("Which mode? Try {offered} — or pick one from the row under the message box."));
    }
    // So that "edit", "Can edit files" and "accept-edits" all reach the same mode.
    let wanted: String = argument.to_lowercase().chars().filter(|ch| ch.is_alphanumeric()).collect();
    let by_name = PermissionMode::ALL.into_iter().find(|mode| {
        let label: String = mode.label().to_lowercase().chars().filter(|ch| ch.is_alphanumeric()).collect();
        wanted == mode.keyword() || wanted == label
    });
    let found = by_name.or_else(|| {
        PERMISSION_ALIASES.iter().find(|(alias, _)| *alias == wanted).map(|(_, mode)| *mode)
    });
    match found {
        Some(mode) => Ok(SlashAction::Permission(mode)),
        None => Err(format!("There's no mode called “{argument}”. Try {offered}.")),
    }
}

/// True for the words that mean "stop choosing and let the CLI decide".
fn means_default(argument: &str) -> bool {
    matches!(argument.to_lowercase().as_str(), "default" | "auto" | "none" | "reset")
}

/// The options in a sentence: "low, medium or high".
fn choices<'a>(options: impl Iterator<Item = &'a str>) -> String {
    let options: Vec<&str> = options.collect();
    match options.split_last() {
        None => "one of the CLI's own".to_owned(),
        Some((last, [])) => format!("“{last}”"),
        Some((last, rest)) => format!("{} or “{last}”", rest.iter().map(|o| format!("“{o}”")).collect::<Vec<_>>().join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_commands_exist_for_every_provider() {
        for &provider in &Provider::ALL {
            let builtins = builtin_commands(provider);
            assert!(!builtins.is_empty(), "{:?} should have built-in commands", provider);
            for &(name, desc) in builtins {
                assert!(!name.is_empty(), "command name should not be empty");
                assert!(!desc.is_empty(), "command description should explain what it does");
            }
        }
    }

    #[test]
    fn providers_builtins_match_their_clis() {
        let claude_names: HashSet<&str> = builtin_commands(Provider::Claude).iter().map(|(n, _)| *n).collect();
        assert!(claude_names.contains("compact"), "claude has compact");
        assert!(claude_names.contains("context"), "claude has context");
        assert!(claude_names.contains("cost"), "claude has cost");
        assert!(claude_names.contains("memory"), "claude has memory");

        let agy_names: HashSet<&str> = builtin_commands(Provider::Antigravity).iter().map(|(n, _)| *n).collect();
        assert!(agy_names.contains("agents"), "agy has agents");
        assert!(agy_names.contains("changelog"), "agy has changelog");
        assert!(agy_names.contains("credits"), "agy has credits");
        assert!(agy_names.contains("skills"), "agy has skills");

        let codex_names: HashSet<&str> = builtin_commands(Provider::Codex).iter().map(|(n, _)| *n).collect();
        assert!(codex_names.contains("diff"), "codex has diff");
        assert!(codex_names.contains("goal"), "codex has goal");
        assert!(codex_names.contains("apps"), "codex has apps");
        assert!(codex_names.contains("review"), "codex has review");
    }

    #[test]
    fn skills_are_discovered_from_directory() {
        let temp = std::env::temp_dir().join("viper_test_skills");
        let skill_dir = temp.join(".agents").join("skills").join("my-skill");
        let _ = std::fs::create_dir_all(&skill_dir);
        let skill_content = "---\nname: my-skill\ndescription: A helpful skill for automated workflows\n---\n# My Skill\n";
        let _ = std::fs::write(skill_dir.join("SKILL.md"), skill_content);

        let commands = discover(Provider::Antigravity, &temp);
        let found = commands.iter().find(|c| c.name == "my-skill");
        assert!(found.is_some(), "skill should be discovered");
        let cmd = found.unwrap();
        assert_eq!(cmd.description, "A helpful skill for automated workflows");
        assert_eq!(cmd.source, CommandSource::Skill);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn claude_custom_commands_are_discovered() {
        let temp = std::env::temp_dir().join("viper_test_claude_cmds");
        let cmd_dir = temp.join(".claude").join("commands");
        let _ = std::fs::create_dir_all(&cmd_dir);
        let cmd_content = "# Run all unit tests and lint checks\ncargo test\n";
        let _ = std::fs::write(cmd_dir.join("test-all.md"), cmd_content);

        let commands = discover(Provider::Claude, &temp);
        let found = commands.iter().find(|c| c.name == "test-all");
        assert!(found.is_some(), "claude command should be discovered");
        let cmd = found.unwrap();
        assert_eq!(cmd.description, "Run all unit tests and lint checks");
        assert_eq!(cmd.source, CommandSource::Project);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn parses_tolerant_frontmatter_even_with_junk() {
        let (name, desc) = parse_skill_frontmatter("not yaml at all", "fallback");
        assert_eq!(name, "fallback");
        assert_eq!(desc, "not yaml at all");

        let (name, desc) = parse_skill_frontmatter("", "fallback");
        assert_eq!(name, "fallback");
        assert_eq!(desc, "Agent skill");

        let yaml = "---\nname: \"custom-name\"\ndescription: 'Quoted description'\n---\n";
        let (name, desc) = parse_skill_frontmatter(yaml, "fallback");
        assert_eq!(name, "custom-name");
        assert_eq!(desc, "Quoted description");
    }

    #[test]
    fn filters_commands_by_query_prioritizing_prefix() {
        let commands = vec![
            SlashCommand { name: "goal".into(), description: "Run task".into(), source: CommandSource::Builtin },
            SlashCommand { name: "grill-me".into(), description: "Interactive plan".into(), source: CommandSource::Builtin },
            SlashCommand { name: "subgoal".into(), description: "Helper".into(), source: CommandSource::Skill },
            SlashCommand { name: "help".into(), description: "View goal tips".into(), source: CommandSource::Builtin },
        ];

        let results = filter(&commands, "go");
        let names: Vec<&str> = results.iter().map(|c| c.name.as_str()).collect();
        // "goal" has exact prefix "go", "subgoal" contains "go", "help" has description matching "goal"
        assert_eq!(names[0], "goal", "exact prefix should be ranked first");
        assert_eq!(names[1], "subgoal", "substring in name should be next");
        assert_eq!(names[2], "help", "description match should be last");
    }

    #[test]
    fn empty_query_returns_all_commands() {
        let commands = vec![
            SlashCommand { name: "a".into(), description: "".into(), source: CommandSource::Builtin },
            SlashCommand { name: "b".into(), description: "".into(), source: CommandSource::Builtin },
        ];
        assert_eq!(filter(&commands, "").len(), 2);
        assert_eq!(filter(&commands, "   ").len(), 2);
    }

    /// Three models, as the catalogue would hand them over.
    fn catalogue() -> Vec<Model> {
        ["Opus", "Sonnet", "Haiku"]
            .into_iter()
            .map(|label| Model {
                id: label.to_lowercase(),
                label: label.to_owned(),
                efforts: vec!["low".to_owned(), "high".to_owned()],
            })
            .collect()
    }

    #[test]
    fn every_provider_gets_vipers_own_commands() {
        // Codex's own list has no /effort or /permission, but Viper sets both on
        // the spawn, so they have to be offered whichever CLI is answering.
        for &provider in &Provider::ALL {
            let commands = discover(provider, Path::new("no such folder"));
            for wanted in ["model", "effort", "permission", "clear", "settings"] {
                let found = commands.iter().find(|c| c.name == wanted);
                let found = found.unwrap_or_else(|| panic!("{provider:?} should offer /{wanted}"));
                assert_eq!(found.source, CommandSource::Viper, "/{wanted} is ours, not the CLI's");
            }
            // The CLI's own spelling of the same thing doesn't get a second row.
            for alias in ALSO_OURS {
                assert!(!commands.iter().any(|c| c.name == *alias), "{provider:?} lists /{alias} twice");
            }
        }
    }

    #[test]
    fn a_command_says_where_it_will_run_before_it_is_sent() {
        // Ours, so the menu can promise it takes effect.
        assert_eq!(handling("model", CommandSource::Viper), Handling::Here);
        // Signing in needs the CLI's own terminal; nothing headless can do it.
        assert_eq!(handling("login", CommandSource::Builtin), Handling::Terminal);
        assert_eq!(handling("theme", CommandSource::Builtin), Handling::Terminal);
        // A skill or a project command is a prompt, and prompts work fine headless.
        assert_eq!(handling("verifying-a-ui-change", CommandSource::Skill), Handling::Cli);
        assert_eq!(handling("test-all", CommandSource::Project), Handling::Cli);
        // Anything else goes to the CLI, because we can't know that it won't work.
        assert_eq!(handling("review", CommandSource::Builtin), Handling::Cli);
    }

    #[test]
    fn the_settings_commands_read_their_arguments() {
        let models = catalogue();
        let efforts = vec!["low".to_owned(), "high".to_owned()];
        let read = |input: &str| intercept(input, &models, &efforts).map(|result| result.expect(input));

        assert_eq!(read("/model opus"), Some(SlashAction::Model(Some("opus".to_owned()))));
        // The picker shows labels, so the label has to work as well as the id.
        assert_eq!(read("/model Sonnet"), Some(SlashAction::Model(Some("sonnet".to_owned()))));
        assert_eq!(read("/model default"), Some(SlashAction::Model(None)));
        assert_eq!(read("/effort high"), Some(SlashAction::Effort(Some("high".to_owned()))));
        assert_eq!(read("/clear"), Some(SlashAction::Clear));
        assert_eq!(read("/settings"), Some(SlashAction::OpenSettings));

        // Each CLI names these modes differently; all of the spellings land right.
        for spelling in ["/permission full", "/mode Full access", "/permissions bypassPermissions"] {
            assert_eq!(read(spelling), Some(SlashAction::Permission(PermissionMode::Full)), "{spelling}");
        }
        assert_eq!(read("/mode accept-edits"), Some(SlashAction::Permission(PermissionMode::AcceptEdits)));

        // Signing in can't be done by sending words to a headless run, so it opens a
        // terminal instead of spending a paid turn on a prompt that can't work.
        assert_eq!(read("/login"), Some(SlashAction::OpenTerminal("login".to_owned())));

        // Not ours: it goes to the CLI as the prompt it is.
        assert_eq!(read("/review this branch"), None);
        assert_eq!(read("not a command at all"), None);
        assert_eq!(read(""), None);
    }

    #[test]
    fn a_setting_that_cant_be_applied_says_what_would_work() {
        let models = catalogue();
        let efforts = vec!["low".to_owned(), "high".to_owned()];
        let fails = |input: &str| {
            intercept(input, &models, &efforts).unwrap_or_else(|| panic!("{input} is ours")).expect_err(input)
        };

        // Naming nothing lists what there is, rather than silently doing nothing.
        let no_argument = fails("/model");
        assert!(no_argument.contains("opus") && no_argument.contains("haiku"), "{no_argument}");
        let wrong = fails("/model gpt-5");
        assert!(wrong.contains("gpt-5") && wrong.contains("sonnet"), "{wrong}");
        assert!(fails("/effort ludicrous").contains("high"));
        assert!(fails("/mode whatever").contains("plan"));

        // While the CLI is still being asked for its models, say so rather than
        // listing an empty set.
        let loading = intercept("/model opus", &[], &efforts).expect("ours").expect_err("no models yet");
        assert!(loading.contains("still being read"), "{loading}");
    }

    #[test]
    fn live_viper_skills_discovered() {
        let project_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let commands = discover(Provider::Antigravity, project_dir);
        let names: HashSet<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains("adding-a-provider"), "this repo's skills must be discovered: {:?}", names);
        assert!(names.contains("verifying-a-ui-change"), "this repo's skills must be discovered: {:?}", names);
        assert!(names.contains("changing-a-stream-parser"), "this repo's skills must be discovered: {:?}", names);
    }
}
