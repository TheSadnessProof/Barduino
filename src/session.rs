use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::claude::{self, AgentEvent, PermissionMode, RunningTurn, Turn};
use crate::terminal::Terminal;

/// Tool output longer than this is cut off in the chat so huge outputs don't slow the UI.
const MAX_TOOL_OUTPUT_CHARS: usize = 4000;
const UNTITLED: &str = "New session";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    User(String),
    Claude(String),
    Tool { name: String, detail: String },
    ToolOutput { text: String, is_error: bool },
    Notice(String),
    Error(String),
}

/// One conversation with an agent, tied to a project folder.
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: u64,
    pub title: String,
    pub project_dir: PathBuf,
    pub permission_mode: PermissionMode,
    /// The CLI's own session ID, used to continue the conversation.
    pub claude_session_id: Option<String>,
    pub model: Option<String>,
    pub entries: Vec<Entry>,
    /// Unsent text in the message box.
    pub input: String,

    /// Text Claude is still streaming, shown below the finished entries.
    #[serde(skip)]
    pub streaming: String,
    #[serde(skip)]
    pub focus_composer: bool,
    /// Started the first time the terminal tab is shown for this session.
    #[serde(skip)]
    pub terminal: Option<Result<Terminal, String>>,
    #[serde(skip)]
    turn: Option<RunningTurn>,
    #[serde(skip)]
    stop_requested: bool,
}

impl Session {
    pub fn new(id: u64, project_dir: PathBuf, permission_mode: PermissionMode) -> Self {
        Self {
            id,
            title: UNTITLED.to_owned(),
            project_dir,
            permission_mode,
            claude_session_id: None,
            model: None,
            entries: Vec::new(),
            input: String::new(),
            streaming: String::new(),
            focus_composer: true,
            terminal: None,
            turn: None,
            stop_requested: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.turn.is_some()
    }

    /// The folder's name, for showing in the session list.
    pub fn folder_name(&self) -> String {
        self.project_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.project_dir.display().to_string())
    }

    /// Sends the message box contents to Claude. `on_event` is called from a
    /// background thread for everything that happens during the turn.
    pub fn send(&mut self, exe: &Path, on_event: impl Fn(AgentEvent) + Send + 'static) {
        let prompt = self.input.trim().to_owned();
        if prompt.is_empty() || self.is_running() {
            return;
        }

        let turn = Turn {
            prompt: prompt.clone(),
            cwd: self.project_dir.clone(),
            resume_session: self.claude_session_id.clone(),
            permission_mode: self.permission_mode,
        };
        match claude::start_turn(exe, turn, on_event) {
            Ok(running) => {
                if self.title == UNTITLED {
                    self.title = title_from(&prompt);
                }
                self.entries.push(Entry::User(prompt));
                self.input.clear();
                self.turn = Some(running);
            }
            Err(err) => self.entries.push(Entry::Error(format!("Couldn't start the Claude CLI: {err}"))),
        }
    }

    pub fn stop(&mut self) {
        if let Some(turn) = &self.turn {
            self.stop_requested = true;
            turn.stop();
        }
    }

    pub fn handle_event(&mut self, event: AgentEvent) {
        match event {
            AgentEvent::Started { session_id, model } => {
                self.claude_session_id = Some(session_id);
                self.model = Some(model);
            }
            AgentEvent::TextDelta(text) => self.streaming.push_str(&text),
            AgentEvent::Text(text) => {
                self.streaming.clear();
                if !text.trim().is_empty() {
                    self.entries.push(Entry::Claude(text));
                }
            }
            AgentEvent::ToolUse { name, detail } => {
                self.streaming.clear();
                self.entries.push(Entry::Tool { name, detail });
            }
            AgentEvent::ToolResult { text, is_error } => {
                let cut_off = text.chars().count() > MAX_TOOL_OUTPUT_CHARS;
                let mut text: String = text.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
                if cut_off {
                    text.push_str("\n… (cut off)");
                }
                self.entries.push(Entry::ToolOutput { text, is_error });
            }
            AgentEvent::Finished { session_id, error, denied_tools } => {
                // Resuming can hand back a new session ID; always continue from the latest one.
                if session_id.is_some() {
                    self.claude_session_id = session_id;
                }
                if let Some(error) = error {
                    self.entries.push(Entry::Error(error));
                }
                if !denied_tools.is_empty() {
                    self.entries.push(Entry::Notice(format!(
                        "Claude wasn't allowed to use: {}. Change \"Permissions\" at the top to allow more.",
                        denied_tools.join(", ")
                    )));
                }
            }
            AgentEvent::Exited { error } => {
                // Keep any text that was cut off by Stop.
                let partial = std::mem::take(&mut self.streaming);
                if !partial.trim().is_empty() {
                    self.entries.push(Entry::Claude(partial));
                }
                if self.stop_requested {
                    self.entries.push(Entry::Notice("Stopped.".into()));
                } else if let Some(error) = error {
                    self.entries.push(Entry::Error(error));
                }
                self.turn = None;
                self.stop_requested = false;
                self.focus_composer = true;
            }
        }
    }
}

/// A short session title taken from the first message.
fn title_from(prompt: &str) -> String {
    const MAX_CHARS: usize = 48;
    let first_line = prompt.lines().next().unwrap_or_default().trim();
    let mut title: String = first_line.chars().take(MAX_CHARS).collect();
    if first_line.chars().count() > MAX_CHARS {
        title.push('…');
    }
    title
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session::new(1, PathBuf::from("C:\\work\\demo"), PermissionMode::ReadOnly)
    }

    #[test]
    fn titles_use_the_first_line() {
        assert_eq!(title_from("Fix the login bug\nmore details"), "Fix the login bug");
        assert_eq!(title_from(&"a".repeat(60)), format!("{}…", "a".repeat(48)));
    }

    #[test]
    fn final_text_replaces_streamed_text() {
        let mut s = session();
        s.handle_event(AgentEvent::TextDelta("Hel".into()));
        s.handle_event(AgentEvent::TextDelta("lo".into()));
        assert_eq!(s.streaming, "Hello");
        s.handle_event(AgentEvent::Text("Hello".into()));
        assert!(s.streaming.is_empty());
        assert_eq!(s.entries, vec![Entry::Claude("Hello".into())]);
    }

    #[test]
    fn exit_keeps_partial_text_and_reports_errors() {
        let mut s = session();
        s.handle_event(AgentEvent::TextDelta("half a sent".into()));
        s.handle_event(AgentEvent::Exited { error: Some("boom".into()) });
        assert_eq!(s.entries, vec![Entry::Claude("half a sent".into()), Entry::Error("boom".into())]);
        assert!(s.focus_composer);
    }

    #[test]
    fn finished_updates_session_and_lists_denied_tools() {
        let mut s = session();
        s.handle_event(AgentEvent::Started { session_id: "first".into(), model: "m".into() });
        s.handle_event(AgentEvent::Finished {
            session_id: Some("second".into()),
            error: None,
            denied_tools: vec!["Bash".into(), "Write".into()],
        });
        assert_eq!(s.claude_session_id.as_deref(), Some("second"));
        assert!(matches!(&s.entries[..], [Entry::Notice(text)] if text.contains("Bash, Write")));
    }

    #[test]
    fn long_tool_output_is_cut_off() {
        let mut s = session();
        s.handle_event(AgentEvent::ToolResult { text: "é".repeat(MAX_TOOL_OUTPUT_CHARS + 1), is_error: false });
        let [Entry::ToolOutput { text, .. }] = &s.entries[..] else { panic!("expected tool output") };
        assert!(text.ends_with("(cut off)"));
    }

    #[test]
    fn short_non_ascii_tool_output_is_kept_whole() {
        let mut s = session();
        s.handle_event(AgentEvent::ToolResult { text: "é".repeat(MAX_TOOL_OUTPUT_CHARS), is_error: false });
        let [Entry::ToolOutput { text, .. }] = &s.entries[..] else { panic!("expected tool output") };
        assert!(!text.contains("cut off"));
    }
}
