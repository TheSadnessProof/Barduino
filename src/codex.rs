//! Runs OpenAI's Codex CLI headless (`codex exec --json`) and turns its JSONL
//! event stream into events the UI can show. Codex signs in through the ChatGPT
//! account the `codex` command already uses, so there is nothing extra to set up.

use std::path::PathBuf;

use serde_json::Value;

use crate::agent::{self, AgentEvent, PermissionMode, Turn, string, tool_detail};
use crate::usage::Usage;

/// Finds `codex` on PATH or where the Codex installer puts it.
pub fn find_executable() -> Option<PathBuf> {
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    if let Some(exe) = agent::find_on_path(&[name]) {
        return Some(exe);
    }

    // Apps launched from the Start menu don't always see the PATH a terminal has. On Windows
    // the installer's copy is a symlink into the release it unpacked under the home folder.
    let mut fallbacks = Vec::new();
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let programs = PathBuf::from(local_app_data).join("Programs");
        fallbacks.push(programs.join("OpenAI").join("Codex").join("bin").join(name));
    }
    if let Some(home) = agent::home_dir() {
        fallbacks.push(home.join(".codex").join("bin").join(name));
    }
    fallbacks.into_iter().find(|candidate| candidate.is_file())
}

/// Arguments for one headless turn. The prompt itself is sent on stdin.
pub fn args(turn: &Turn) -> Vec<String> {
    let sandbox = match turn.permission_mode {
        PermissionMode::ReadOnly => Some("read-only"),
        PermissionMode::AcceptEdits => Some("workspace-write"),
        // Codex has no plan mode. Keeping it read-only means a plan can't quietly change files.
        PermissionMode::Plan => Some("read-only"),
        // Full access has its own flag, which also stops Codex asking for approval.
        PermissionMode::Full => None,
    };

    let mut args: Vec<String> = vec!["exec".into()];
    if let Some(thread_id) = &turn.resume_session {
        args.extend(["resume".to_owned(), thread_id.clone()]);
    }
    // Codex refuses to run outside a git repository unless told it's fine.
    args.extend(["--json".to_owned(), "--skip-git-repo-check".to_owned()]);
    // `codex exec resume` takes neither -s/--sandbox nor -C/--cd, so the sandbox is set through
    // the config override that both forms accept: an unquoted value that isn't valid TOML, such
    // as "read-only", is used as a literal string. Resume reads its working root from the
    // process's own folder, which start_turn already points at turn.cwd.
    match sandbox {
        Some(sandbox) => args.extend(["-c".to_owned(), format!("sandbox_mode={sandbox}")]),
        None => args.push("--dangerously-bypass-approvals-and-sandbox".to_owned()),
    }
    if turn.resume_session.is_none() {
        args.extend(["-C".to_owned(), turn.cwd.display().to_string()]);
    }
    // "-" makes Codex read the prompt from stdin, which start_turn writes and then closes.
    // Prompts can carry a whole page of HTML, far more than a command line holds on Windows.
    args.push("-".to_owned());
    args
}

/// Parses one line of `codex exec --json` output.
pub fn parse_line(line: &str) -> Vec<AgentEvent> {
    let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
        return Vec::new();
    };

    match msg["type"].as_str().unwrap_or_default() {
        // Nothing in the stream names the model, and the UI copes with not knowing it.
        "thread.started" => {
            vec![AgentEvent::Started { session_id: string(&msg["thread_id"]), model: String::new() }]
        }
        "item.started" => match tool_name(&msg["item"]) {
            Some(name) => vec![AgentEvent::ToolUse { name, detail: item_detail(&msg["item"]) }],
            None => Vec::new(),
        },
        "item.completed" => completed_item(&msg["item"]),
        "turn.completed" => vec![AgentEvent::Finished {
            session_id: None,
            error: None,
            denied_tools: Vec::new(),
            usage: Some(turn_usage(&msg["usage"])),
        }],
        "turn.failed" => {
            let message = string(&msg["error"]["message"]);
            vec![AgentEvent::Finished {
                session_id: None,
                error: Some(if message.is_empty() { "Codex stopped with an error.".to_owned() } else { message }),
                denied_tools: Vec::new(),
                usage: None,
            }]
        }
        // A thread-level "error" repeats the message of the "turn.failed" that follows it, and
        // only one event may end the turn, so the failure is reported from there instead.
        _ => Vec::new(),
    }
}

/// Token counts from `turn.completed`. Codex follows the OpenAI API, where the input
/// count already includes the cached tokens, so those are taken out of it here.
/// Reasoning tokens are part of the output count, and no cost is reported.
fn turn_usage(usage: &Value) -> Usage {
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let cache_read = count("cached_input_tokens");
    Usage {
        turns: 1,
        input: count("input_tokens").saturating_sub(cache_read),
        output: count("output_tokens"),
        cache_read,
        cache_write: count("cache_write_input_tokens"),
        cost_usd: None,
    }
}

/// The name to show for an item that is work rather than words. Codex has no tool
/// names of its own outside MCP, so each item type gets one. Returning None marks the
/// items that aren't tools at all, such as the reply itself.
fn tool_name(item: &Value) -> Option<String> {
    Some(match item["type"].as_str()? {
        "command_execution" => "Shell".to_owned(),
        "file_change" => "Edit".to_owned(),
        "web_search" => "Search".to_owned(),
        "todo_list" => "Plan".to_owned(),
        "collab_tool_call" => "Subagent".to_owned(),
        "mcp_tool_call" => format!("{}.{}", string(&item["server"]), string(&item["tool"])),
        _ => return None,
    })
}

fn completed_item(item: &Value) -> Vec<AgentEvent> {
    match item["type"].as_str().unwrap_or_default() {
        // A finished block of text, which the UI shows in place of anything streamed before it.
        "agent_message" => match string(&item["text"]) {
            text if text.trim().is_empty() => Vec::new(),
            text => vec![AgentEvent::Text(text)],
        },
        // Trouble Codex worked around, such as an unknown model name, arrives as its own item.
        "error" => vec![AgentEvent::ToolResult { text: string(&item["message"]), is_error: true }],
        // Reasoning has no matching event, and `codex exec` doesn't summarise it anyway.
        _ if tool_name(item).is_some() => {
            vec![AgentEvent::ToolResult { text: item_output(item), is_error: item_failed(item) }]
        }
        _ => Vec::new(),
    }
}

/// A one-line summary of what an item is about to do.
fn item_detail(item: &Value) -> String {
    match item["type"].as_str().unwrap_or_default() {
        "file_change" => one_line(&changes(item).join(", ")),
        // Codex's built-in node_repl server titles each call; other servers just get their input.
        "mcp_tool_call" => match item["arguments"]["title"].as_str() {
            Some(title) => one_line(title),
            None => tool_detail(&item["arguments"]),
        },
        // Commands and searches name what they touch in a key tool_detail already prefers.
        _ => tool_detail(item),
    }
}

/// What a finished item produced, as text. Item types whose output has no useful text,
/// such as a plan update, show as an empty (and so collapsed) tool output.
fn item_output(item: &Value) -> String {
    match item["type"].as_str().unwrap_or_default() {
        "command_execution" => string(&item["aggregated_output"]),
        "file_change" => changes(item).join("\n"),
        "mcp_tool_call" => match item["error"]["message"].as_str() {
            Some(message) => message.to_owned(),
            None => mcp_text(&item["result"]["content"]),
        },
        _ => String::new(),
    }
}

fn item_failed(item: &Value) -> bool {
    match item["type"].as_str().unwrap_or_default() {
        "command_execution" => item["exit_code"].as_i64().unwrap_or(0) != 0,
        "mcp_tool_call" => !item["error"].is_null(),
        _ => item["status"] == "failed",
    }
}

/// The files a `file_change` item touches, as "update C:\\work\\demo\\main.rs".
fn changes(item: &Value) -> Vec<String> {
    item["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|change| {
            let path = change["path"].as_str()?;
            Some(match change["kind"].as_str() {
                Some(kind) => format!("{kind} {path}"),
                None => path.to_owned(),
            })
        })
        .collect()
}

/// The text parts of an MCP tool's result.
fn mcp_text(content: &Value) -> String {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Borrows tool_detail's shortening for text that isn't a tool input.
fn one_line(text: &str) -> String {
    tool_detail(&serde_json::json!({ "command": text }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Provider;

    fn events(recording: &str) -> Vec<AgentEvent> {
        recording.lines().flat_map(parse_line).collect()
    }

    /// Real output from codex-cli 0.154.0 reading a file with a shell command.
    #[test]
    fn parses_recorded_file_read() {
        let events = events(include_str!("../testdata/codex_read_file.jsonl"));
        assert_eq!(
            events.first(),
            Some(&AgentEvent::Started {
                session_id: "01a0b08f-4a34-79b3-9961-881d4eb97784".into(),
                model: String::new(),
            })
        );
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, detail }
            if name == "Shell" && detail.contains("Get-Content"))));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolResult { text, is_error: false }
            if text.contains("hello from barduino"))));
        assert!(events.contains(&AgentEvent::Text("hello from barduino".into())));
        assert_eq!(
            events.last(),
            Some(&AgentEvent::Finished {
                session_id: None,
                error: None,
                denied_tools: Vec::new(),
                // 34018 input tokens, 16768 of which were read from the cache.
                usage: Some(Usage { turns: 1, input: 17250, output: 123, cache_read: 16768, ..Default::default() }),
            })
        );
    }

    /// Real output from codex-cli 0.154.0 editing a file with workspace-write.
    #[test]
    fn parses_recorded_file_change() {
        let events = events(include_str!("../testdata/codex_edit_file.jsonl"));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, detail }
            if name == "Edit" && detail == "update C:\\Users\\ditob\\Documents\\barduino-codex-probe\\notes.txt")));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolResult { text, is_error: false }
            if text.ends_with("notes.txt"))));
        assert!(matches!(events.last(), Some(AgentEvent::Finished { usage: Some(usage), error: None, .. })
            if usage.input > 0 && usage.output > 0));
    }

    /// Real output from codex-cli 0.154.0 asked for a model that doesn't exist.
    #[test]
    fn parses_recorded_failed_turn() {
        let events = events(include_str!("../testdata/codex_failed_turn.jsonl"));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolResult { text, is_error: true }
            if text.contains("Model metadata for `no-such-model` not found"))));
        // Only the turn.failed reports the failure, so the message isn't shown twice.
        let finished: Vec<&AgentEvent> = events.iter().filter(|e| matches!(e, AgentEvent::Finished { .. })).collect();
        assert!(matches!(&finished[..], [AgentEvent::Finished { error: Some(message), usage: None, .. }]
            if message.contains("is not supported when using Codex with a ChatGPT account")));
    }

    #[test]
    fn builds_headless_arguments() {
        let turn = Turn {
            prompt: "-v what does this do?".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::AcceptEdits,
        };
        assert_eq!(
            args(&turn),
            [
                "exec",
                "--json",
                "--skip-git-repo-check",
                "-c",
                "sandbox_mode=workspace-write",
                "-C",
                "C:\\work\\demo",
                "-",
            ]
        );
    }

    #[test]
    fn full_access_turns_off_the_sandbox_and_approvals() {
        let turn = Turn {
            prompt: "run the tests".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::Full,
        };
        let args = args(&turn);
        assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_owned()), "{args:?}");
        assert!(!args.iter().any(|arg| arg.starts_with("sandbox_mode=")), "one or the other, not both: {args:?}");
    }

    #[test]
    fn resuming_drops_the_flags_resume_rejects() {
        let turn = Turn {
            prompt: "and now?".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: Some("01a0b087-2797-7a71-952c-496a702e3409".into()),
            permission_mode: PermissionMode::Plan,
        };
        assert_eq!(
            args(&turn),
            [
                "exec",
                "resume",
                "01a0b087-2797-7a71-952c-496a702e3409",
                "--json",
                "--skip-git-repo-check",
                "-c",
                "sandbox_mode=read-only",
                "-",
            ]
        );
    }

    #[test]
    fn parses_an_mcp_tool_call() {
        let started = r#"{"type":"item.started","item":{"id":"i1","type":"mcp_tool_call","server":"node_repl","tool":"js","arguments":{"code":"1+1","title":"Add two numbers"},"result":null,"error":null,"status":"in_progress"}}"#;
        let completed = r#"{"type":"item.completed","item":{"id":"i1","type":"mcp_tool_call","server":"node_repl","tool":"js","arguments":{},"result":{"content":[{"type":"text","text":"2"}]},"error":null,"status":"completed"}}"#;
        assert_eq!(
            parse_line(started),
            vec![AgentEvent::ToolUse { name: "node_repl.js".into(), detail: "Add two numbers".into() }]
        );
        assert_eq!(parse_line(completed), vec![AgentEvent::ToolResult { text: "2".into(), is_error: false }]);
    }

    #[test]
    fn a_failed_command_is_an_error_result() {
        let line = r#"{"type":"item.completed","item":{"id":"i1","type":"command_execution","command":"cargo test","aggregated_output":"boom","exit_code":101,"status":"completed"}}"#;
        assert_eq!(parse_line(line), vec![AgentEvent::ToolResult { text: "boom".into(), is_error: true }]);
    }

    #[test]
    fn skips_progress_and_noise() {
        // item.updated repeats an item that is still running, which would double the tool entry.
        let updated = r#"{"type":"item.updated","item":{"id":"i1","type":"command_execution","command":"ls","aggregated_output":"a","exit_code":null,"status":"in_progress"}}"#;
        assert!(parse_line(updated).is_empty());
        assert!(parse_line(r#"{"type":"turn.started"}"#).is_empty());
        assert!(parse_line(r#"{"type":"item.completed","item":{"id":"i1","type":"reasoning"}}"#).is_empty());
        assert!(parse_line("not json").is_empty());
        assert!(parse_line("").is_empty());
    }

    fn run_turn(turn: Turn) -> Vec<AgentEvent> {
        let exe = Provider::Codex.find().expect("Codex should be installed");
        let (tx, rx) = std::sync::mpsc::channel();
        let _running = agent::start_turn(Provider::Codex, &exe, turn, move |event| {
            let _ = tx.send(event);
        })
        .expect("the CLI should start");
        rx.iter().take_while(|e| !matches!(e, AgentEvent::Exited { .. })).collect()
    }

    /// Runs the installed codex twice, the second time resuming the first thread. It
    /// spends tokens on the user's ChatGPT account, so it only runs when asked for:
    /// `cargo test -- --ignored codex --nocapture`
    #[test]
    #[ignore]
    fn runs_the_real_codex_cli() {
        let dir = std::env::temp_dir().join("barduino-codex-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("note.txt"), "the secret word is pineapple\n").unwrap();

        let first = run_turn(Turn {
            prompt: "Read note.txt and reply with only the secret word.".into(),
            cwd: dir.clone(),
            resume_session: None,
            permission_mode: PermissionMode::ReadOnly,
        });
        println!("{first:#?}");
        let Some(AgentEvent::Started { session_id: thread_id, .. }) = first.first() else {
            panic!("the first turn should report the thread it started");
        };
        let Some(AgentEvent::Finished { error: None, usage: Some(usage), .. }) = first.last() else {
            panic!("the first turn should finish cleanly and report usage");
        };
        assert!(usage.input > 0 && usage.output > 0, "{usage:?}");

        let second = run_turn(Turn {
            prompt: "What word did you just reply with? Answer in upper case, nothing else.".into(),
            cwd: dir,
            resume_session: Some(thread_id.clone()),
            permission_mode: PermissionMode::ReadOnly,
        });
        println!("{second:#?}");
        let reply: String = second
            .iter()
            .filter_map(|e| if let AgentEvent::Text(t) = e { Some(t.as_str()) } else { None })
            .collect();
        assert!(reply.contains("PINEAPPLE"), "the second turn should remember the first: {reply:?}");
    }
}
