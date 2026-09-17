//! Runs Google's Gemini CLI in headless mode and turns its streaming JSON
//! output into events the UI can show.

use std::ffi::OsString;
use std::path::PathBuf;

use serde_json::Value;

use crate::agent::{self, AgentEvent, Launcher, PermissionMode, Turn, string, tool_detail};

/// Finds Gemini CLI. On Windows npm installs it as a `.cmd` wrapper, which would
/// run through cmd.exe; starting Node with the script directly avoids that.
pub fn find_launcher() -> Option<Launcher> {
    if !cfg!(windows) {
        return agent::find_on_path(&["gemini"]).map(Launcher::program);
    }

    let wrapper_dirs = agent::find_on_path(&["gemini.cmd"])
        .and_then(|cmd| cmd.parent().map(PathBuf::from))
        .into_iter()
        .chain(agent::npm_global_dir());
    for dir in wrapper_dirs {
        let script = dir.join("node_modules").join("@google").join("gemini-cli").join("bundle").join("gemini.js");
        if !script.is_file() {
            continue;
        }
        let node = Some(dir.join("node.exe"))
            .filter(|node| node.is_file())
            .or_else(|| agent::find_on_path(&["node.exe"]))
            .or_else(|| {
                let program_files = std::env::var_os("ProgramFiles")?;
                Some(PathBuf::from(program_files).join("nodejs").join("node.exe")).filter(|node| node.is_file())
            })?;
        return Some(Launcher { program: node, leading_args: vec![OsString::from(script)] });
    }
    None
}

/// Arguments for one headless turn. The prompt itself is sent on stdin.
pub fn args(turn: &Turn) -> Vec<String> {
    let approval_mode = match turn.permission_mode {
        // In headless mode, tools that would need approval aren't offered at all.
        PermissionMode::ReadOnly => "default",
        PermissionMode::AcceptEdits => "auto_edit",
        PermissionMode::Plan => "plan",
    };
    let mut args: Vec<String> = ["--output-format", "stream-json", "--approval-mode", approval_mode]
        .map(String::from)
        .to_vec();
    // The user picked this folder in Barduino, so trust it instead of letting Gemini refuse to work in it.
    args.push("--skip-trust".to_owned());
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

    let event = match msg["type"].as_str().unwrap_or_default() {
        "init" => AgentEvent::Started { session_id: string(&msg["session_id"]), model: string(&msg["model"]) },
        // Gemini only streams pieces of text; they're kept when a tool runs or the turn ends.
        "message" if msg["role"] == "assistant" => AgentEvent::TextDelta(string(&msg["content"])),
        "tool_use" => AgentEvent::ToolUse {
            name: string(&msg["tool_name"]),
            detail: tool_detail(&msg["parameters"]),
        },
        "tool_result" => {
            let is_error = msg["status"] == "error";
            let text = if is_error { string(&msg["error"]["message"]) } else { string(&msg["output"]) };
            AgentEvent::ToolResult { text, is_error }
        }
        "error" => AgentEvent::Notice {
            text: readable_error(&string(&msg["message"])),
            is_error: msg["severity"] == "error",
        },
        "result" => AgentEvent::Finished {
            session_id: None,
            error: (msg["status"] == "error").then(|| match msg["error"]["message"].as_str() {
                Some(message) if !message.is_empty() => readable_error(message),
                _ => "Gemini stopped with an error.".to_owned(),
            }),
            denied_tools: Vec::new(),
        },
        _ => return Vec::new(),
    };
    vec![event]
}

/// Gemini passes API errors along as JSON nested inside strings several times
/// over. This digs out the innermost message, such as "API key not valid".
fn readable_error(message: &str) -> String {
    let unescaped = message.replace('\\', "");
    let inner = unescaped.rfind("\"message\"").and_then(|start| {
        let rest = unescaped[start + "\"message\"".len()..].trim_start();
        let rest = rest.strip_prefix(':')?.trim_start().strip_prefix('"')?;
        rest.split('"').next().map(str::trim).filter(|text| !text.is_empty())
    });
    inner.unwrap_or(message).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(permission_mode: PermissionMode, resume_session: Option<&str>) -> Turn {
        Turn {
            prompt: "hi".into(),
            cwd: PathBuf::from("."),
            resume_session: resume_session.map(str::to_owned),
            permission_mode,
        }
    }

    #[test]
    fn builds_headless_arguments() {
        assert_eq!(
            args(&turn(PermissionMode::AcceptEdits, Some("abc"))),
            ["--output-format", "stream-json", "--approval-mode", "auto_edit", "--skip-trust", "--resume", "abc"]
        );
        assert!(args(&turn(PermissionMode::Plan, None)).contains(&"plan".to_owned()));
    }

    /// Real output from Gemini CLI 0.60.0 when its API key is rejected.
    #[test]
    fn parses_recorded_error_run() {
        let events: Vec<AgentEvent> =
            include_str!("../testdata/gemini_invalid_key.jsonl").lines().flat_map(parse_line).collect();
        assert_eq!(
            events,
            vec![
                AgentEvent::Started { session_id: "39f69837-8a83-4824-bfab-dbeddceee6a9".into(), model: "auto".into() },
                AgentEvent::Finished {
                    session_id: None,
                    error: Some("API key not valid. Please pass a valid API key.".into()),
                    denied_tools: Vec::new(),
                },
            ]
        );
    }

    #[test]
    fn parses_text_and_tools() {
        let lines = [
            r#"{"type":"message","role":"assistant","content":"Hel","delta":true}"#,
            r#"{"type":"tool_use","tool_name":"read_file","tool_id":"t1","parameters":{"absolute_path":"C:\\x\\a.txt"}}"#,
            r#"{"type":"tool_result","tool_id":"t1","status":"success","output":"hello"}"#,
            r#"{"type":"tool_result","tool_id":"t2","status":"error","output":"","error":{"type":"X","message":"no such file"}}"#,
            r#"{"type":"error","severity":"warning","message":"Loop detected"}"#,
            r#"{"type":"result","status":"success","stats":{}}"#,
        ];
        let events: Vec<AgentEvent> = lines.into_iter().flat_map(parse_line).collect();
        assert_eq!(
            events,
            vec![
                AgentEvent::TextDelta("Hel".into()),
                AgentEvent::ToolUse { name: "read_file".into(), detail: "C:\\x\\a.txt".into() },
                AgentEvent::ToolResult { text: "hello".into(), is_error: false },
                AgentEvent::ToolResult { text: "no such file".into(), is_error: true },
                AgentEvent::Notice { text: "Loop detected".into(), is_error: false },
                AgentEvent::Finished { session_id: None, error: None, denied_tools: Vec::new() },
            ]
        );
    }

    #[test]
    fn ignores_the_echoed_user_message_and_noise() {
        assert!(parse_line(r#"{"type":"message","role":"user","content":"hi"}"#).is_empty());
        assert!(parse_line("Loaded cached credentials.").is_empty());
    }

    #[test]
    fn plain_errors_are_kept_as_they_are() {
        assert_eq!(readable_error("Quota exceeded"), "Quota exceeded");
    }
}
