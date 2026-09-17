//! Runs the Claude Code CLI in headless mode and turns its streaming JSON
//! output into events the UI can show.

use std::path::PathBuf;

use serde_json::Value;

use crate::agent::{self, AgentEvent, PermissionMode, Turn, string, tool_detail};

/// Finds the `claude` executable on PATH or in the usual install locations.
pub fn find_executable() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
    if let Some(exe) = agent::find_on_path(names) {
        return Some(exe);
    }

    // Apps launched from the Start menu don't always see the PATH a terminal has.
    let home = agent::home_dir()?;
    let mut fallbacks = vec![
        home.join(".local").join("bin").join(names[0]),
        home.join(".claude").join("local").join(names[0]),
    ];
    if let Some(app_data) = std::env::var_os("APPDATA") {
        fallbacks.push(PathBuf::from(app_data).join("npm").join("claude.cmd"));
    }
    fallbacks.into_iter().find(|candidate| candidate.is_file())
}

/// Arguments for one headless turn. The prompt itself is sent on stdin.
pub fn args(turn: &Turn) -> Vec<String> {
    let permission_mode = match turn.permission_mode {
        // Headless mode can't ask for approval, so anything that needs it is denied.
        PermissionMode::ReadOnly => "default",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::Plan => "plan",
    };
    let mut args: Vec<String> = ["-p", "--verbose", "--output-format", "stream-json", "--include-partial-messages"]
        .map(String::from)
        .to_vec();
    args.extend(["--permission-mode".to_owned(), permission_mode.to_owned()]);
    if let Some(session_id) = &turn.resume_session {
        args.extend(["--resume".to_owned(), session_id.clone()]);
    }
    args
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
