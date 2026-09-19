//! Runs Google Antigravity's `agy` CLI in print mode and turns its streaming
//! JSON output into events the UI can show. `agy` uses the Antigravity app's
//! own sign-in, so there is nothing extra to set up.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::agent::{self, AgentEvent, PermissionMode, Turn, string};
use crate::tool_call;
use crate::usage::Usage;

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

/// Arguments for one print-mode turn. The prompt itself is sent on stdin.
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
        // Without this agy asks for approval before running commands, which print mode
        // can't answer, so it refuses them instead.
        PermissionMode::Full => args.push("--dangerously-skip-permissions".into()),
        PermissionMode::Plan => args.extend(["--mode".into(), "plan".into()]),
    }
    if let Some(model) = &turn.model {
        args.extend(["--model".into(), model.clone()]);
        // Most agy models have their effort in the name, like "gemini-3.8-flash-high",
        // so the flag is only added when the name doesn't already say.
        let named_level = ["-low", "-medium", "-high"].iter().any(|level| model.ends_with(level));
        if let Some(effort) = &turn.effort
            && !named_level
        {
            args.extend(["--effort".into(), effort.clone()]);
        }
    } else if let Some(effort) = &turn.effort {
        args.extend(["--effort".into(), effort.clone()]);
    }
    if let Some(conversation_id) = &turn.resume_session {
        args.extend(["--conversation".into(), conversation_id.clone()]);
    }
    // Prompts can carry a whole page of HTML from attached elements, far more than
    // a command line holds on Windows. start_turn sends the prompt on stdin, and agy
    // runs in print mode when given --output-format stream-json. Passing bare "--print"
    // is rejected by Go's flag parser because -print requires a prompt argument.
    args
}

/// Arguments for starting Antigravity interactively inside an embedded terminal.
///
/// Unlike print-mode turns, interactive mode omits `--output-format` and runs
/// directly inside the terminal with the project workspace added via `--add-dir`.
#[allow(dead_code)] // Wired into central terminal in Milestone 2.
pub fn interactive_args(
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mut args = vec!["--add-dir".to_owned(), cwd.display().to_string()];
    match permission_mode {
        PermissionMode::ReadOnly => {}
        PermissionMode::AcceptEdits => args.extend(["--mode".to_owned(), "accept-edits".to_owned()]),
        PermissionMode::Full => args.push("--dangerously-skip-permissions".to_owned()),
        PermissionMode::Plan => args.extend(["--mode".to_owned(), "plan".to_owned()]),
    }
    if let Some(model) = model {
        args.extend(["--model".to_owned(), model.to_owned()]);
        let named_level = ["-low", "-medium", "-high"].iter().any(|level| model.ends_with(level));
        if let Some(effort) = effort
            && !named_level
        {
            args.extend(["--effort".to_owned(), effort.to_owned()]);
        }
    } else if let Some(effort) = effort {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    if let Some(conversation_id) = resume_id {
        args.extend(["--conversation".to_owned(), conversation_id.to_owned()]);
    }
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

            let status = string(&result["status"]);
            let err_msg = match result["error"]["message"].as_str() {
                Some(message) if !message.is_empty() => message.to_owned(),
                _ => match result["error"].as_str() {
                    Some(message) if !message.is_empty() => message.to_owned(),
                    _ => match result["message"].as_str() {
                        Some(message) if !message.is_empty() => message.to_owned(),
                        _ => string(&result["response"]),
                    },
                },
            };
            let lower_err = err_msg.to_lowercase();
            if denied_tools.is_empty()
                && (status == "PERMISSION_DENIED"
                    || status.contains("DENIED")
                    || lower_err.contains("permission")
                    || lower_err.contains("sandbox")
                    || lower_err.contains("forbidden"))
            {
                if lower_err.contains("command")
                    || lower_err.contains("terminal")
                    || lower_err.contains("bash")
                    || lower_err.contains("shell")
                {
                    denied_tools.push("RunCommand".to_owned());
                } else if lower_err.contains("edit")
                    || lower_err.contains("write")
                    || lower_err.contains("file")
                {
                    denied_tools.push("EditFile".to_owned());
                } else {
                    denied_tools.push("Action".to_owned());
                }
            }
            denied_tools.sort();
            denied_tools.dedup();

            let error = (status != "SUCCESS" && denied_tools.is_empty()).then(|| {
                if !err_msg.is_empty() {
                    err_msg
                } else {
                    format!("Antigravity stopped with status {status}.")
                }
            });
            vec![AgentEvent::Finished {
                session_id: result["conversation_id"].as_str().map(str::to_owned),
                error,
                denied_tools,
                usage: Some(result_usage(&result["usage"])),
            }]
        }
        _ => Vec::new(),
    }
}

/// Token counts from a `result` message. agy follows the Gemini API, where the
/// input count already includes cached tokens, so those are taken out of it here.
/// Thinking tokens are part of the output count.
fn result_usage(usage: &Value) -> Usage {
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let cache_read = count("cache_read_tokens");
    Usage {
        turns: 1,
        input: count("input_tokens").saturating_sub(cache_read),
        output: count("output_tokens"),
        cache_read,
        cache_write: 0,
        cost_usd: None,
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
                // agy names the file it changes but not the text either side, so
                // there is no diff to pass on.
                "ACTIVE" => vec![AgentEvent::ToolUse {
                    edit: None,
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
    // What it was looking for comes first. A search carries both a pattern and the
    // folder to search, and listing the folder alone — which is what happened while
    // SearchDirectory sat above Pattern in the list below — says nothing about what
    // the agent was after.
    if let Some(pattern) = parameters["Pattern"].as_str().or_else(|| parameters["Query"].as_str()) {
        let detail = match parameters["SearchDirectory"].as_str() {
            Some(directory) => format!("{pattern} · {directory}"),
            None => pattern.to_owned(),
        };
        return agent::tool_detail(&serde_json::json!({ "command": detail }));
    }
    const KEYS: [&str; 6] =
        ["CommandLine", "AbsolutePath", "TargetFile", "DirectoryPath", "SearchDirectory", "Url"];
    let value = KEYS
        .iter()
        .find_map(|key| parameters[*key].as_str())
        .or_else(|| parameters.as_object()?.values().find_map(Value::as_str));
    match value {
        // A command arrives as the whole line the shell was given, wrapper and all.
        Some(text) => agent::tool_detail(&serde_json::json!({ "command": tool_call::bare_command(text) })),
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
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, detail, .. }
            if name == "view_file" && detail.ends_with("note.txt"))));

        // The same recording's search used to show only the folder it looked in:
        // SearchDirectory sat above Pattern in the key list, so the row said
        // "C:\…\agytest" and never said the agent was looking for note.txt.
        let Some(AgentEvent::ToolUse { detail, .. }) = events
            .iter()
            .find(|e| matches!(e, AgentEvent::ToolUse { name, .. } if name == "find_by_name"))
        else {
            panic!("the recording searches for a file: {events:?}")
        };
        let (pattern, directory) = detail.split_once(" · ").unwrap_or_else(|| panic!("{detail}"));
        assert_eq!(pattern, "note.txt", "what it was looking for comes first");
        assert!(directory.ends_with("agytest"), "and where it looked is still there: {directory}");
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
                usage: Some(Usage { turns: 1, input: 40709, output: 415, ..Default::default() }),
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

    /// Full access is the only mode where agy may run commands: print mode can't
    /// answer its approval prompt, so otherwise it refuses them.
    #[test]
    fn full_access_skips_approval_prompts() {
        let args = args(&full_access_turn());
        assert!(args.contains(&"--dangerously-skip-permissions".to_owned()), "{args:?}");
        assert!(!args.iter().any(|arg| arg == "--mode"), "the flag covers everything: {args:?}");
    }

    fn full_access_turn() -> Turn {
        Turn {
            prompt: "run the tests".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::Full,
            model: None,
            effort: None,
        }
    }

    /// Most agy models carry their effort in the name, so the flag would clash.
    #[test]
    fn effort_is_left_out_when_the_model_name_already_says_it() {
        let turn = |model: &str| Turn {
            prompt: "hello".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::ReadOnly,
            model: Some(model.to_owned()),
            effort: Some("low".into()),
        };
        let named = args(&turn("gemini-3.8-flash-high"));
        assert!(named.windows(2).any(|pair| pair == ["--model", "gemini-3.8-flash-high"]), "{named:?}");
        assert!(!named.iter().any(|arg| arg == "--effort"), "{named:?}");

        let plain = args(&turn("claude-sonnet-4-6"));
        assert!(plain.windows(2).any(|pair| pair == ["--effort", "low"]), "{plain:?}");
    }

    #[test]
    fn builds_print_mode_arguments() {
        let turn = Turn {
            prompt: "-v what does this do?".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: Some("abc".into()),
            permission_mode: PermissionMode::AcceptEdits,
            model: None,
            effort: None,
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
            ]
        );
    }

    #[test]
    fn prompts_do_not_appear_on_the_command_line() {
        let large_prompt = "x".repeat(100_000);
        let turn = Turn {
            prompt: large_prompt,
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::ReadOnly,
            model: None,
            effort: None,
        };
        let cli_args = args(&turn);
        assert!(!cli_args.iter().any(|arg| arg == "--print"), "bare --print is rejected by Go's flag parser");
        assert!(!cli_args.iter().any(|arg| arg.contains("xxxx")), "prompts are sent on stdin, never on argv");
    }

    #[test]
    fn failed_results_become_errors() {
        let line = r#"{"event":"result","result":{"conversation_id":"x","status":"ERROR","response":""}}"#;
        assert!(matches!(&parse_line(line)[..], [AgentEvent::Finished { error: Some(e), .. }] if e.contains("ERROR")));
    }

    #[test]
    fn permission_failure_with_denied_tools_omits_error() {
        let line = r#"{"event":"result","result":{"conversation_id":"x","status":"PERMISSION_DENIED","response":"","denied_actions":[{"action":"run_command","display_name":"RunCommand"}]}}"#;
        let events = parse_line(line);
        assert_eq!(
            events,
            vec![AgentEvent::Finished {
                session_id: Some("x".into()),
                error: None,
                denied_tools: vec!["RunCommand".into()],
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );

        // Even with a generic failure status, having denied tools reports as a tool denial rather than a crash.
        let failed = r#"{"event":"result","result":{"conversation_id":"x","status":"FAILED","response":"","denied_actions":[{"action":"run_command","display_name":"RunCommand"}]}}"#;
        assert_eq!(
            parse_line(failed),
            vec![AgentEvent::Finished {
                session_id: Some("x".into()),
                error: None,
                denied_tools: vec!["RunCommand".into()],
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );

        // If denied_actions is empty but status is PERMISSION_DENIED or error mentions permission, infer denied tool.
        let denied_no_actions = r#"{"event":"result","result":{"conversation_id":"x","status":"PERMISSION_DENIED","response":"","error":{"message":"Permission check failed: command execution not allowed"}}}"#;
        assert_eq!(
            parse_line(denied_no_actions),
            vec![AgentEvent::Finished {
                session_id: Some("x".into()),
                error: None,
                denied_tools: vec!["RunCommand".into()],
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );
    }
}
