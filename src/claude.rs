//! Runs the Claude Code CLI in headless mode and turns its streaming JSON
//! output into events the UI can show.

use std::path::PathBuf;

use serde_json::Value;

use crate::agent::{self, AgentEvent, PermissionMode, Turn, string, tool_detail};
use crate::line_diff::FileEdit;
use crate::usage::Usage;

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
        PermissionMode::Full => "bypassPermissions",
        PermissionMode::Plan => "plan",
    };
    let mut args: Vec<String> = ["-p", "--verbose", "--output-format", "stream-json", "--include-partial-messages"]
        .map(String::from)
        .to_vec();
    args.extend(["--permission-mode".to_owned(), permission_mode.to_owned()]);
    if turn.permission_mode == PermissionMode::ReadOnly {
        args.extend(["--disallowed-tools".to_owned(), "Bash".to_owned()]);
    }
    // Headless mode can't answer permission prompts: anything requiring approval is denied automatically.
    args.extend(["--permission-prompts".to_owned(), "none".to_owned()]);
    if let Some(model) = &turn.model {
        args.extend(["--model".to_owned(), model.clone()]);
    }
    if let Some(effort) = &turn.effort {
        args.extend(["--effort".to_owned(), effort.clone()]);
    }
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
        // Claude reports how much of the plan's limits are used as the reply streams.
        "rate_limit_event" => crate::plan::from_claude(&msg["rate_limit_info"])
            .map(|plan| vec![AgentEvent::Plan(plan)])
            .unwrap_or_default(),
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
                    detail: detail_for(block["name"].as_str().unwrap_or_default(), &block["input"]),
                    edit: file_edit(block["name"].as_str().unwrap_or_default(), &block["input"]),
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

            let subtype = string(&msg["subtype"]);
            let result = string(&msg["result"]);
            let terminal_reason = string(&msg["terminal_reason"]);
            if denied_tools.is_empty()
                && (subtype == "error_disallowed_tool"
                    || subtype.contains("permission")
                    || terminal_reason.contains("permission")
                    || terminal_reason.contains("disallowed")
                    || result.to_lowercase().contains("permission denied")
                    || result.to_lowercase().contains("disallowed tool"))
            {
                let lower = format!("{result} {subtype} {terminal_reason}").to_lowercase();
                if lower.contains("bash") || lower.contains("command") || lower.contains("terminal") {
                    denied_tools.push("Bash".to_owned());
                } else if lower.contains("edit") {
                    denied_tools.push("Edit".to_owned());
                } else if lower.contains("write") {
                    denied_tools.push("Write".to_owned());
                } else {
                    denied_tools.push("Tool".to_owned());
                }
            }
            denied_tools.sort();
            denied_tools.dedup();

            let error = (msg["is_error"].as_bool().unwrap_or(false) && denied_tools.is_empty()).then(|| {
                if result.is_empty() {
                    format!("Claude stopped with an error ({}).", subtype)
                } else {
                    result
                }
            });

            vec![AgentEvent::Finished {
                session_id: msg["session_id"].as_str().map(str::to_owned),
                error,
                denied_tools,
                usage: Some(result_usage(&msg)),
            }]
        }
        _ => Vec::new(),
    }
}

/// The change an editing tool is about to make. Claude sends the text on both
/// sides, so the chat can show a diff without reading the file.
/// What a tool is working on, for the tools whose own shape says it better than a
/// single field can. Everything else falls through to the shared `tool_detail`.
fn detail_for(tool: &str, input: &Value) -> String {
    match tool {
        "TodoWrite" => agent::checklist(&input["todos"]).unwrap_or_else(|| tool_detail(input)),
        // A pattern on its own doesn't say where it was looked for, and "in the
        // whole project" and "in src/" are different enough to be worth the words.
        "Grep" | "Glob" => {
            let pattern = string(&input["pattern"]);
            let filter = input["glob"].as_str().or_else(|| input["type"].as_str()).unwrap_or_default();
            let mut detail = match filter {
                "" => pattern,
                filter => format!("{pattern} in {filter} files"),
            };
            if let Some(path) = input["path"].as_str() {
                detail.push_str(&format!(" · {path}"));
            }
            detail
        }
        // A read of part of a file is not a read of the file.
        "Read" => match (input["offset"].as_u64(), input["limit"].as_u64()) {
            (Some(offset), Some(limit)) => format!("{} · lines {}–{}", string(&input["file_path"]), offset, offset + limit),
            _ => tool_detail(input),
        },
        _ => tool_detail(input),
    }
}

fn file_edit(tool: &str, input: &Value) -> Option<FileEdit> {
    let path = input["file_path"].as_str()?.to_owned();
    match tool {
        "Edit" => Some(FileEdit::new(
            path,
            string(&input["old_string"]),
            string(&input["new_string"]),
        )),
        // A write replaces the file, so everything in it counts as added.
        "Write" => Some(FileEdit::new(path, String::new(), string(&input["content"]))),
        // Several edits to one file, shown as the run of changes they make.
        "MultiEdit" => {
            let edits = input["edits"].as_array()?;
            let mut old = String::new();
            let mut new = String::new();
            for edit in edits {
                old.push_str(&string(&edit["old_string"]));
                old.push('\n');
                new.push_str(&string(&edit["new_string"]));
                new.push('\n');
            }
            Some(FileEdit::new(path, old, new))
        }
        _ => None,
    }
}

/// Token counts from a `result` message. Claude reports cache reads and writes
/// separately from the rest of the input.
fn result_usage(msg: &Value) -> Usage {
    let usage = &msg["usage"];
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    Usage {
        turns: 1,
        input: count("input_tokens"),
        output: count("output_tokens"),
        cache_read: count("cache_read_input_tokens"),
        cache_write: count("cache_creation_input_tokens"),
        cost_usd: msg["total_cost_usd"].as_f64(),
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
    fn full_access_bypasses_permission_prompts() {
        let turn = Turn {
            prompt: "run the tests".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::Full,
            model: None,
            effort: None,
        };
        let args = args(&turn);
        let mode = args.windows(2).find(|pair| pair[0] == "--permission-mode").map(|pair| pair[1].clone());
        assert_eq!(mode.as_deref(), Some("bypassPermissions"), "{args:?}");
    }

    #[test]
    fn asks_for_a_model_and_effort_when_the_session_chose_them() {
        let turn = Turn {
            prompt: "hello".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::ReadOnly,
            model: Some("opus".into()),
            effort: Some("max".into()),
        };
        let args = args(&turn);
        assert!(args.windows(2).any(|pair| pair == ["--model", "opus"]), "{args:?}");
        assert!(args.windows(2).any(|pair| pair == ["--effort", "max"]), "{args:?}");
    }

    #[test]
    fn read_only_restricts_command_tools_and_bypasses_prompts() {
        let turn = Turn {
            prompt: "hello".into(),
            cwd: PathBuf::from("C:\\work\\demo"),
            resume_session: None,
            permission_mode: PermissionMode::ReadOnly,
            model: None,
            effort: None,
        };
        let args = args(&turn);
        assert!(args.windows(2).any(|pair| pair == ["--disallowed-tools", "Bash"]), "{args:?}");
        assert!(args.windows(2).any(|pair| pair == ["--permission-prompts", "none"]), "{args:?}");
    }

    #[test]
    fn an_edit_carries_both_sides_of_the_change() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"C:\\work\\a.rs","old_string":"let x = 1;","new_string":"let x = 2;"}}]},"parent_tool_use_id":null}"#;
        let [AgentEvent::ToolUse { edit: Some(edit), .. }] = &parse_line(line)[..] else {
            panic!("an edit should come through")
        };
        assert_eq!(edit.path, "C:\\work\\a.rs");
        assert_eq!(edit.counts(), (1, 1));

        // A write has nothing on the old side, so it reads as a new file.
        let write = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t2","name":"Write","input":{"file_path":"new.txt","content":"one\ntwo"}}]},"parent_tool_use_id":null}"#;
        let [AgentEvent::ToolUse { edit: Some(edit), .. }] = &parse_line(write)[..] else {
            panic!("a write should come through")
        };
        assert_eq!(edit.counts(), (2, 0));

        // Reading a file changes nothing, so there is no diff to show.
        let read = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t3","name":"Read","input":{"file_path":"a.rs"}}]},"parent_tool_use_id":null}"#;
        assert!(matches!(&parse_line(read)[..], [AgentEvent::ToolUse { edit: None, .. }]));
    }

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
            vec![AgentEvent::ToolUse { name: "Bash".into(), detail: "ls -la…".into(), edit: None }]
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
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );
    }

    /// Real output from Claude Code 2.1.271 answering "hi".
    #[test]
    fn reads_usage_from_a_recorded_reply() {
        let events: Vec<AgentEvent> =
            include_str!("../testdata/claude_reply_hi.jsonl").lines().flat_map(parse_line).collect();
        let Some(AgentEvent::Finished { usage: Some(usage), error: None, .. }) = events.last() else {
            panic!("expected the turn to finish with usage: {events:?}");
        };
        assert_eq!(
            *usage,
            Usage { turns: 1, input: 2, output: 4, cache_read: 0, cache_write: 31984, cost_usd: Some(0.31995) }
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
    fn permission_failure_with_denials_omits_error() {
        let line = r#"{"type":"result","subtype":"error_permission","is_error":true,"result":"Permission denied for Bash","session_id":"abc","permission_denials":[{"tool_name":"Bash"}]}"#;
        let events = parse_line(line);
        assert_eq!(
            events,
            vec![AgentEvent::Finished {
                session_id: Some("abc".into()),
                error: None,
                denied_tools: vec!["Bash".into()],
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );

        // When disallowed tools trigger turn termination, report as a tool denial rather than a crash.
        let disallowed = r#"{"type":"result","subtype":"error_disallowed_tool","is_error":true,"result":"Disallowed tool Bash called","session_id":"abc","permission_denials":[{"tool_name":"Bash"}]}"#;
        assert_eq!(
            parse_line(disallowed),
            vec![AgentEvent::Finished {
                session_id: Some("abc".into()),
                error: None,
                denied_tools: vec!["Bash".into()],
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );

        // Even if permission_denials is empty, an error_disallowed_tool subtype identifies the denied tool.
        let disallowed_empty = r#"{"type":"result","subtype":"error_disallowed_tool","is_error":true,"result":"Disallowed tool Bash called","session_id":"abc","permission_denials":[]}"#;
        assert_eq!(
            parse_line(disallowed_empty),
            vec![AgentEvent::Finished {
                session_id: Some("abc".into()),
                error: None,
                denied_tools: vec!["Bash".into()],
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );

        // A result specifying terminal_reason as permission_denied identifies the denied tool.
        let terminal_reason_denied = r#"{"type":"result","subtype":"error","terminal_reason":"permission_denied","is_error":true,"result":"Command execution restricted","session_id":"abc","permission_denials":[]}"#;
        assert_eq!(
            parse_line(terminal_reason_denied),
            vec![AgentEvent::Finished {
                session_id: Some("abc".into()),
                error: None,
                denied_tools: vec!["Bash".into()],
                usage: Some(Usage { turns: 1, ..Default::default() }),
            }]
        );
    }

    #[test]
    fn a_tools_own_shape_says_more_than_one_field_can() {
        let detail = |tool: &str, input: serde_json::Value| detail_for(tool, &input);

        // A to-do list used to arrive as `{"todos":[{"content":"Fix the…` in the
        // transcript. It is a list, so it reads as one.
        let todos = detail(
            "TodoWrite",
            serde_json::json!({"todos": [
                {"content": "Restore the pickers", "status": "completed"},
                {"content": "Add the context readout", "status": "in_progress"},
            ]}),
        );
        assert_eq!(todos, "1 of 2 done\n✓ Restore the pickers\n▸ Add the context readout");

        // A pattern on its own doesn't say where it was looked for, and "everywhere"
        // and "in src" are different enough to be worth the words.
        let grep = detail("Grep", serde_json::json!({"pattern": "fn parse", "glob": "*.rs", "path": "src"}));
        assert_eq!(grep, "fn parse in *.rs files · src");
        assert_eq!(detail("Grep", serde_json::json!({"pattern": "fn parse"})), "fn parse");

        // Reading part of a file is not the same as reading the file.
        let part = detail("Read", serde_json::json!({"file_path": "src/app.rs", "offset": 40, "limit": 50}));
        assert_eq!(part, "src/app.rs · lines 40–90");
        assert_eq!(detail("Read", serde_json::json!({"file_path": "src/app.rs"})), "src/app.rs");

        // A shape that isn't what we expect falls back rather than showing nonsense:
        // these formats change between releases and a wrong list is worse than none.
        assert_eq!(detail("TodoWrite", serde_json::json!({"todos": "not a list"})), "not a list");
        assert_eq!(detail("TodoWrite", serde_json::json!({})), "");
    }

    #[test]
    fn skips_subagent_output_and_noise() {
        let subagent = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"inner"}]},"parent_tool_use_id":"t1"}"#;
        assert!(parse_line(subagent).is_empty());
        assert!(parse_line(r#"{"type":"rate_limit_event"}"#).is_empty());
        let limits = r#"{"type":"rate_limit_event","rate_limit_info":{"rateLimitType":"five_hour","utilization":0.25}}"#;
        assert!(matches!(&parse_line(limits)[..], [AgentEvent::Plan(plan)] if plan.windows[0].used == 0.25));
        assert!(parse_line("not json").is_empty());
        assert!(parse_line("").is_empty());
    }
}
