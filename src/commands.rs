//! Provider slash commands (`/`): built-in actions, custom project commands,
//! and agent skills discovered in real time as the user types.

use std::collections::HashSet;
use std::path::Path;

use crate::agent::{self, Provider};

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
            ("help", "Show available Claude commands and CLI usage"),
            ("compact", "Clear conversation history but keep a summary in context"),
            ("cost", "Show token usage and estimated cost for this session"),
            ("review", "Review recent changes and diffs in the project"),
            ("init", "Initialize CLAUDE.md guidelines and project config"),
            ("doctor", "Check system health and tool installations"),
            ("terminal-setup", "Configure terminal integration and Shift+Enter bindings"),
            ("bug", "Report a bug or issue to Anthropic"),
            ("clear", "Reset conversation context and clear the screen"),
            ("config", "Open configuration to manage settings"),
            ("pr-comments", "Fetch and review GitHub pull request comments"),
            ("login", "Sign in with your Anthropic account"),
            ("logout", "Sign out of your Anthropic account"),
        ],
        Provider::Antigravity => &[
            ("goal", "Run an autonomous long-running task thoroughly until completion"),
            ("schedule", "Schedule an instruction on a recurring cron schedule or timer"),
            ("grill-me", "Align on a plan through an interactive interview to resolve decisions"),
            ("learn", "Extract and persist learnings, patterns, and conventions from this session"),
            ("help", "View Antigravity help, commands, and CLI options"),
            ("mode", "Set agent execution mode (plan or accept-edits)"),
            ("model", "View or switch the active reasoning model"),
            ("mcp", "Manage MCP servers and external tools"),
            ("plugin", "Manage and inspect installed plugins"),
            ("changelog", "View recent Antigravity release notes and changes"),
        ],
        Provider::Codex => &[
            ("help", "View Codex help and available commands"),
            ("review", "Review changes in the repository for bugs and style"),
            ("fix", "Diagnose and fix errors in code or failing tests"),
            ("explain", "Explain code structure, architecture, or logic"),
            ("test", "Generate comprehensive unit tests for the current code"),
            ("plan", "Create an implementation plan before making modifications"),
        ],
    }
}

/// Discovers all available slash commands for a provider in real time,
/// including built-in commands, project skills, and custom commands.
pub fn discover(provider: Provider, project_dir: &Path) -> Vec<SlashCommand> {
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
                scan_skills(&project_dir.join(".agents").join("skills"), &mut commands, &mut seen);
            }
        }
    }

    // 3. User global commands and skills.
    if let Some(home) = agent::home_dir() {
        match provider {
            Provider::Claude => {
                scan_claude_commands(&home.join(".claude").join("commands"), &mut commands, &mut seen);
            }
            Provider::Antigravity => {
                scan_skills(&home.join(".gemini").join("config").join("skills"), &mut commands, &mut seen);
                scan_skills(
                    &home.join(".gemini").join("antigravity-ide").join("builtin").join("skills"),
                    &mut commands,
                    &mut seen,
                );
            }
            Provider::Codex => {}
        }
    }

    commands
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
