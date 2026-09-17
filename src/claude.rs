//! Runs the Claude Code CLI in headless mode and turns its streaming JSON
//! output into events the UI can show.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

/// Something that happened during a turn, in a form the UI can display.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// The CLI started (or resumed) a session.
    Started { session_id: String, model: String },
    /// A piece of text that is still being streamed.
    TextDelta(String),
    /// A finished block of text. Replaces any streamed text before it.
    Text(String),
    ToolUse { name: String, detail: String },
    ToolResult { text: String, is_error: bool },
    /// Claude finished the turn.
    Finished {
        session_id: Option<String>,
        error: Option<String>,
        denied_tools: Vec<String>,
    },
    /// The CLI process ended. `error` is set if it didn't exit cleanly.
    Exited { error: Option<String> },
}

/// What Claude is allowed to do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    ReadOnly,
    AcceptEdits,
    Plan,
}

impl PermissionMode {
    pub const ALL: [PermissionMode; 3] = [Self::ReadOnly, Self::AcceptEdits, Self::Plan];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read only",
            Self::AcceptEdits => "Can edit files",
            Self::Plan => "Plan only",
        }
    }

    fn cli_value(self) -> &'static str {
        match self {
            // Headless mode can't ask for approval, so anything that needs it is denied.
            Self::ReadOnly => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::Plan => "plan",
        }
    }
}

pub struct Turn {
    pub prompt: String,
    pub cwd: PathBuf,
    pub resume_session: Option<String>,
    pub permission_mode: PermissionMode,
}

/// A turn whose CLI process is still running.
pub struct RunningTurn {
    child: Arc<Mutex<Child>>,
}

impl RunningTurn {
    pub fn stop(&self) {
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = child.kill();
    }
}

/// Finds the `claude` executable on PATH or in the usual install locations.
pub fn find_executable() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };

    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
            .find(|candidate| candidate.is_file())
    });
    if on_path.is_some() {
        return on_path;
    }

    // Apps launched from the Start menu don't always see the PATH a terminal has.
    let home = PathBuf::from(std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?);
    let mut fallbacks = vec![
        home.join(".local").join("bin").join(names[0]),
        home.join(".claude").join("local").join(names[0]),
    ];
    if let Some(app_data) = std::env::var_os("APPDATA") {
        fallbacks.push(PathBuf::from(app_data).join("npm").join("claude.cmd"));
    }
    fallbacks.into_iter().find(|candidate| candidate.is_file())
}

/// Starts one turn of the conversation. Events are sent to `events`, and
/// `notify` is called after each one so the UI can redraw.
pub fn start_turn(
    exe: &Path,
    turn: Turn,
    events: Sender<AgentEvent>,
    notify: impl Fn() + Send + 'static,
) -> std::io::Result<RunningTurn> {
    let mut cmd = Command::new(exe);
    cmd.args(["-p", "--verbose", "--output-format", "stream-json", "--include-partial-messages"])
        .args(["--permission-mode", turn.permission_mode.cli_value()]);
    if let Some(session_id) = &turn.resume_session {
        cmd.args(["--resume", session_id]);
    }
    cmd.current_dir(&turn.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Stops a console window from flashing up for the CLI.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let child = Arc::new(Mutex::new(child));

    // The prompt goes through stdin, which avoids command-line quoting and length limits.
    let prompt = turn.prompt;
    thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });

    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });

    let waiter = Arc::clone(&child);
    thread::spawn(move || {
        let send = |event: AgentEvent| {
            let _ = events.send(event);
            notify();
        };

        for line in BufReader::new(stdout).split(b'\n') {
            let Ok(line) = line else { break };
            for event in parse_line(&String::from_utf8_lossy(&line)) {
                send(event);
            }
        }

        let status = wait(&waiter);
        let stderr = stderr_reader.join().unwrap_or_default();
        let error = match status {
            Ok(status) if status.success() => None,
            Ok(status) => Some(format!("The Claude CLI exited with {status}. {}", stderr.trim())),
            Err(err) => Some(format!("Lost track of the Claude CLI: {err}")),
        };
        send(AgentEvent::Exited { error });
    });

    Ok(RunningTurn { child })
}

fn wait(child: &Mutex<Child>) -> std::io::Result<ExitStatus> {
    loop {
        // Lock only briefly so a Stop click is never stuck behind this loop.
        let status = child.lock().unwrap_or_else(PoisonError::into_inner).try_wait()?;
        if let Some(status) = status {
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Parses one line of `--output-format stream-json` output.
pub fn parse_line(line: &str) -> Vec<AgentEvent> {
    let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
        return Vec::new();
    };
    // Subagents report through the same stream. For now only the main conversation is shown.
    if !msg["parent_tool_use_id"].is_null() {
        return Vec::new();
    }

    match msg["type"].as_str().unwrap_or_default() {
        "system" if msg["subtype"] == "init" => vec![AgentEvent::Started {
            session_id: string(&msg["session_id"]),
            model: string(&msg["model"]),
        }],
        "stream_event" => {
            let event = &msg["event"];
            let delta = &event["delta"];
            if event["type"] == "content_block_delta" && delta["type"] == "text_delta" {
                vec![AgentEvent::TextDelta(string(&delta["text"]))]
            } else {
                Vec::new()
            }
        }
        "assistant" => content_blocks(&msg)
            .filter_map(|block| match block["type"].as_str()? {
                "text" => Some(AgentEvent::Text(string(&block["text"]))),
                "tool_use" => Some(AgentEvent::ToolUse {
                    name: string(&block["name"]),
                    detail: tool_detail(&block["input"]),
                }),
                _ => None,
            })
            .collect(),
        "user" => content_blocks(&msg)
            .filter(|block| block["type"] == "tool_result")
            .map(|block| AgentEvent::ToolResult {
                text: tool_result_text(&block["content"]),
                is_error: block["is_error"].as_bool().unwrap_or(false),
            })
            .collect(),
        "result" => {
            let mut denied_tools: Vec<String> = msg["permission_denials"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|denial| string(&denial["tool_name"]))
                .collect();
            denied_tools.sort();
            denied_tools.dedup();

            let error = msg["is_error"].as_bool().unwrap_or(false).then(|| {
                let result = string(&msg["result"]);
                if result.is_empty() {
                    format!("Claude stopped with an error ({}).", string(&msg["subtype"]))
                } else {
                    result
                }
            });

            vec![AgentEvent::Finished {
                session_id: msg["session_id"].as_str().map(str::to_owned),
                error,
                denied_tools,
            }]
        }
        _ => Vec::new(),
    }
}

fn content_blocks(msg: &Value) -> impl Iterator<Item = &Value> {
    msg["message"]["content"].as_array().into_iter().flatten()
}

fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// A one-line summary of a tool call, such as the command or file it touches.
fn tool_detail(input: &Value) -> String {
    const KEYS: [&str; 6] = ["command", "file_path", "path", "pattern", "url", "description"];
    let raw = KEYS
        .iter()
        .find_map(|key| input[*key].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| input.to_string());

    let first_line = raw.lines().next().unwrap_or_default();
    let mut detail: String = first_line.chars().take(160).collect();
    if detail.len() < raw.trim_end().len() {
        detail.push('…');
    }
    detail
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_init() {
        let line = r#"{"type":"system","subtype":"init","cwd":"C:\\x","session_id":"abc","model":"claude-opus-5[1m]","permissionMode":"default"}"#;
        assert_eq!(
            parse_line(line),
            vec![AgentEvent::Started { session_id: "abc".into(), model: "claude-opus-5[1m]".into() }]
        );
    }

    #[test]
    fn parses_streamed_and_final_text() {
        let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}},"session_id":"abc","parent_tool_use_id":null}"#;
        let full = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]},"parent_tool_use_id":null,"session_id":"abc"}"#;
        assert_eq!(parse_line(delta), vec![AgentEvent::TextDelta("hi".into())]);
        assert_eq!(parse_line(full), vec![AgentEvent::Text("hi".into())]);
    }

    #[test]
    fn parses_tool_use_and_result() {
        let tool_use = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls -la\ncd src","description":"List files"}}]},"parent_tool_use_id":null}"#;
        let result = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"README.md"}],"is_error":false}]},"parent_tool_use_id":null}"#;
        assert_eq!(
            parse_line(tool_use),
            vec![AgentEvent::ToolUse { name: "Bash".into(), detail: "ls -la…".into() }]
        );
        assert_eq!(
            parse_line(result),
            vec![AgentEvent::ToolResult { text: "README.md".into(), is_error: false }]
        );
    }

    #[test]
    fn parses_result_with_denials() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"done","session_id":"abc","permission_denials":[{"tool_name":"Write"},{"tool_name":"Bash"},{"tool_name":"Write"}]}"#;
        assert_eq!(
            parse_line(line),
            vec![AgentEvent::Finished {
                session_id: Some("abc".into()),
                error: None,
                denied_tools: vec!["Bash".into(), "Write".into()],
            }]
        );
    }

    #[test]
    fn reports_errors_from_result() {
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"abc"}"#;
        let events = parse_line(line);
        let [AgentEvent::Finished { error, .. }] = events.as_slice() else {
            panic!("expected one Finished event");
        };
        assert_eq!(error.as_deref(), Some("Claude stopped with an error (error_max_turns)."));
    }

    #[test]
    fn skips_subagent_output_and_noise() {
        let subagent = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"inner"}]},"parent_tool_use_id":"t1"}"#;
        assert!(parse_line(subagent).is_empty());
        assert!(parse_line(r#"{"type":"rate_limit_event"}"#).is_empty());
        assert!(parse_line("not json").is_empty());
        assert!(parse_line("").is_empty());
    }
}
