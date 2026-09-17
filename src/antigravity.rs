//! Runs Google Antigravity's `agy` CLI in print mode and turns its streaming
//! JSON output into events the UI can show. `agy` uses the Antigravity app's
//! own sign-in, so there is nothing extra to set up.

use std::path::PathBuf;

use serde_json::Value;

use crate::agent::{self, AgentEvent, PermissionMode, Turn, string};

/// Finds `agy` on PATH or where the Antigravity installer puts it.
pub fn find_executable() -> Option<PathBuf> {
    let name = if cfg!(windows) { "agy.exe" } else { "agy" };
    if let Some(exe) = agent::find_on_path(&[name]) {
        return Some(exe);
    }
    // Apps launched from the Start menu don't always see the PATH a terminal has.
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local_app_data).join("agy").join("bin").join(name))
        .filter(|exe| exe.is_file())
}

/// Arguments for one print-mode turn.
pub fn args(turn: &Turn) -> Vec<String> {
    // agy doesn't treat the current folder as its workspace, so it has to be added explicitly.
    let mut args: Vec<String> = vec![
        "--output-format".into(),
        "stream-json".into(),
        "--add-dir".into(),
        turn.cwd.display().to_string(),
    ];
    // Without a mode, agy asks for review, which print mode can't do, so those actions are denied.
    match turn.permission_mode {
        PermissionMode::ReadOnly => {}
        PermissionMode::AcceptEdits => args.extend(["--mode".into(), "accept-edits".into()]),
        PermissionMode::Plan => args.extend(["--mode".into(), "plan".into()]),
    }
    if let Some(conversation_id) = &turn.resume_session {
        args.extend(["--conversation".into(), conversation_id.clone()]);
    }
    // Attached with "=" so a prompt starting with "-" isn't read as a flag. agy.exe is a
    // real executable, so the argument reaches it without cmd.exe reinterpreting it.
    args.push(format!("--print={}", turn.prompt));
    args
}

/// Parses one line of `--output-format stream-json` output.
pub fn parse_line(line: &str) -> Vec<AgentEvent> {
    let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
        return Vec::new();
    };

    match msg["event"].as_str().unwrap_or_default() {
        "init" => vec![AgentEvent::Started {
            session_id: string(&msg["conversation_id"]),
            model: string(&msg["init"]["model"]),
        }],
        "step_update" => parse_step(&msg["step_update"]),
        "result" => {
            let result = &msg["result"];
            let mut denied_tools: Vec<String> = result["denied_actions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|denied| string(&denied["display_name"]))
                .filter(|name| !name.is_empty())
                .collect();
            denied_tools.sort();
            denied_tools.dedup();

            let status = string(&result["status"]);
            let error = (status != "SUCCESS").then(|| match result["error"]["message"].as_str() {
                Some(message) if !message.is_empty() => message.to_owned(),
                _ => format!("Antigravity stopped with status {status}."),
            });
            vec![AgentEvent::Finished {
                session_id: result["conversation_id"].as_str().map(str::to_owned),
                error,
                denied_tools,
            }]
        }
        _ => Vec::new(),
    }
}

fn parse_step(step: &Value) -> Vec<AgentEvent> {
    let state = step["state"].as_str().unwrap_or_default();
    match step["step_type"].as_str().unwrap_or_default() {
        "agent_response" => match step["text_delta"].as_str() {
            Some(text) if !text.is_empty() => vec![AgentEvent::TextDelta(text.to_owned())],
            _ => Vec::new(),
        },
        // Each tool step is reported when it starts and again when it ends.
        "tool" => {
            let info = &step["tool_info"];
            match state {
                "ACTIVE" => vec![AgentEvent::ToolUse {
                    name: string(&step["tool_name"]),
                    detail: tool_detail(&info["parameters"]),
                }],
                "DONE" => vec![AgentEvent::ToolResult { text: string(&info["output"]), is_error: false }],
                "ERROR" => vec![AgentEvent::ToolResult { text: string(&info["error"]["message"]), is_error: true }],
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

/// A one-line summary of a tool call. agy names its parameters in PascalCase.
fn tool_detail(parameters: &Value) -> String {
    const KEYS: [&str; 8] =
        ["CommandLine", "AbsolutePath", "TargetFile", "DirectoryPath", "SearchDirectory", "Url", "Query", "Pattern"];
    let value = KEYS
        .iter()
        .find_map(|key| parameters[*key].as_str())
        .or_else(|| parameters.as_object()?.values().find_map(Value::as_str));
    match value {
        Some(text) => agent::tool_detail(&serde_json::json!({ "command": text })),
        None => agent::tool_detail(parameters),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(recording: &str) -> Vec<AgentEvent> {
        recording.lines().flat_map(parse_line).collect()
    }

    /// Real output from agy 1.2.5 reading a file in the project folder.
    #[test]
    fn parses_recorded_file_read() {
        let events = events(include_str!("../testdata/agy_read_file.jsonl"));
        assert_eq!(
            events.first(),
            Some(&AgentEvent::Started { session_id: "b1752c20-bb81-4047-b2e1-480c12c67d6e".into(), model: String::new() })
        );
        assert!(events.contains(&AgentEvent::ToolResult { text: "note.txt".into(), is_error: false }));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, detail }
            if name == "view_file" && detail.ends_with("note.txt"))));
        let text: String = events
            .iter()
            .filter_map(|e| if let AgentEvent::TextDelta(t) = e { Some(t.as_str()) } else { None })
            .collect();
        assert_eq!(text, "hello from note\n");
        assert_eq!(
            events.last(),
            Some(&AgentEvent::Finished {
                session_id: Some("b1752c20-bb81-4047-b2e1-480c12c67d6e".into()),
                error: None,
                denied_tools: Vec::new(),
            })
        );
    }

    /// Real output from agy 1.2.5 when a tool needed permission print mode can't ask for.
    #[test]
    fn parses_recorded_denied_permission() {
        let events = events(include_str!("../testdata/agy_denied.jsonl"));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, text } if text.contains("denied"))));
        assert!(matches!(events.last(), Some(AgentEvent::Finished { denied_tools, error: None, .. }) if denied_tools == &["ListDir"]));
    }

    #[test]
    fn builds_print_mode_arguments() {
        let turn = Turn {
            prompt: "-v what does this do?".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: Some("abc".into()),
            permission_mode: PermissionMode::AcceptEdits,
        };
        assert_eq!(
            args(&turn),
            [
                "--output-format",
                "stream-json",
                "--add-dir",
                "C:\\work\\demo",
                "--mode",
                "accept-edits",
                "--conversation",
                "abc",
                "--print=-v what does this do?",
            ]
        );
    }

    #[test]
    fn failed_results_become_errors() {
        let line = r#"{"event":"result","result":{"conversation_id":"x","status":"ERROR","response":""}}"#;
        assert!(matches!(&parse_line(line)[..], [AgentEvent::Finished { error: Some(e), .. }] if e.contains("ERROR")));
    }
}
