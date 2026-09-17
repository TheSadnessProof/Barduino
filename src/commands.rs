//! Provider slash commands (`/`): built-in actions, custom project commands,
//! and agent skills discovered in real time as the user types.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::agent::{self, Provider};

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
}

impl CommandSource {
    pub fn badge(self) -> &'static str {
        match self {
            Self::Builtin => "Built-in",
            Self::Skill => "Skill",
            Self::Project => "Project",
        }
    }
}

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
        let temp = std::env::temp_dir().join("barduino_test_skills");
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
        let temp = std::env::temp_dir().join("barduino_test_claude_cmds");
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

    #[test]
    fn live_barduino_skills_discovered() {
        let project_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let commands = discover(Provider::Antigravity, project_dir);
        let names: HashSet<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains("adding-a-provider"), "this repo's skills must be discovered: {:?}", names);
        assert!(names.contains("verifying-a-ui-change"), "this repo's skills must be discovered: {:?}", names);
        assert!(names.contains("changing-a-stream-parser"), "this repo's skills must be discovered: {:?}", names);
    }
}
