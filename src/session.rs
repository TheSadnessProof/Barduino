use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{self, AgentEvent, PermissionMode, Provider, RunningTurn, Turn};
use crate::browser::PickedElement;
use crate::line_diff::FileEdit;

/// Tool output longer than this is cut off in the chat so huge outputs don't slow the UI.
const MAX_TOOL_OUTPUT_CHARS: usize = 4000;
const UNTITLED: &str = "New session";
/// Stands in for the folder name until one is chosen.
pub const NO_FOLDER: &str = "No folder yet";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    User(UserMessage),
    /// A reply from the agent. Sessions saved when only Claude was supported call it `Claude`.
    #[serde(alias = "Claude")]
    Agent(String),
    Tool {
        name: String,
        detail: String,
        /// The change the tool is about to make, when the agent described it.
        #[serde(default)]
        edit: Option<FileEdit>,
    },
    ToolOutput { text: String, is_error: bool },
    Notice(String),
    Error(String),
}

/// What the user sent: their text and any page elements attached to it.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "SavedUserMessage")]
pub struct UserMessage {
    pub text: String,
    pub elements: Vec<PickedElement>,
}

/// Messages saved before attachments existed are plain text.
#[derive(Deserialize)]
#[serde(untagged)]
enum SavedUserMessage {
    Text(String),
    Full {
        text: String,
        #[serde(default)]
        elements: Vec<PickedElement>,
    },
}

impl From<SavedUserMessage> for UserMessage {
    fn from(saved: SavedUserMessage) -> Self {
        match saved {
            SavedUserMessage::Text(text) => Self { text, elements: Vec::new() },
            SavedUserMessage::Full { text, elements } => Self { text, elements },
        }
    }
}

impl UserMessage {
    /// The full prompt for the agent, with each attached element described after the text.
    pub fn prompt(&self) -> String {
        let mut parts = Vec::new();
        if !self.text.is_empty() {
            parts.push(self.text.clone());
        }
        parts.extend(self.elements.iter().map(PickedElement::as_prompt));
        parts.join("

")
    }
}

/// One conversation with an agent, tied to a project folder.
#[derive(Serialize, Deserialize)]
pub struct Session {
    pub id: u64,
    pub title: String,
    pub project_dir: PathBuf,
    /// Which CLI answers in this session. Sessions saved before this existed used Claude.
    #[serde(default)]
    pub provider: Provider,
    pub permission_mode: PermissionMode,
    /// The model this session asked for, or None for the CLI's own default.
    #[serde(default)]
    pub chosen_model: Option<String>,
    /// How hard the model should work, in the provider's own words, e.g. "high".
    #[serde(default)]
    pub effort: Option<String>,
    /// The CLI's own session ID, used to continue the conversation.
    #[serde(alias = "claude_session_id")]
    pub agent_session_id: Option<String>,
    pub model: Option<String>,
    pub entries: Vec<Entry>,
    /// Unsent text in the message box.
    pub input: String,
    /// Page elements attached to the unsent message.
    #[serde(default)]
    pub elements: Vec<PickedElement>,

    /// Text the agent is still streaming, shown below the finished entries.
    #[serde(skip)]
    pub streaming: String,
    #[serde(skip)]
    pub focus_composer: bool,
    #[serde(skip)]
    turn: Option<RunningTurn>,
    #[serde(skip)]
    stop_requested: bool,
    /// Set once this turn has shown an error, so the CLI's exit doesn't repeat it.
    #[serde(skip)]
    error_shown: bool,
}

impl Session {
    pub fn new(id: u64, project_dir: PathBuf, provider: Provider, permission_mode: PermissionMode) -> Self {
        Self {
            id,
            title: UNTITLED.to_owned(),
            project_dir,
            provider,
            permission_mode,
            chosen_model: None,
            effort: None,
            agent_session_id: None,
            model: None,
            entries: Vec::new(),
            input: String::new(),
            elements: Vec::new(),
            streaming: String::new(),
            focus_composer: true,
            turn: None,
            stop_requested: false,
            error_shown: false,
        }
    }

    pub fn is_running(&self) -> bool {
        self.turn.is_some()
    }

    /// A conversation belongs to one CLI, so the provider can only change before it starts.
    pub fn can_change_provider(&self) -> bool {
        self.entries.is_empty() && !self.is_running()
    }

    /// Whether there's anything to send.
    pub fn has_message(&self) -> bool {
        !self.input.trim().is_empty() || !self.elements.is_empty()
    }

    /// Attaches a page element to the unsent message. Picking the same element again does nothing.
    pub fn attach(&mut self, element: PickedElement) {
        // Two comments on one element are both worth keeping, so the note counts too.
        let already = self
            .elements
            .iter()
            .any(|e| e.url == element.url && e.selector == element.selector && e.note() == element.note());
        if !already {
            self.elements.push(element);
        }
    }

    /// Whether a project folder has been chosen. A new session starts without one,
    /// so nothing is sent to an agent before the user says where it should work.
    pub fn has_folder(&self) -> bool {
        !self.project_dir.as_os_str().is_empty()
    }

    /// The folder's name, for showing in the session list.
    pub fn folder_name(&self) -> String {
        if !self.has_folder() {
            return NO_FOLDER.to_owned();
        }
        self.project_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.project_dir.display().to_string())
    }

    /// Sends the message box contents to the agent. `on_event` is called from a
    /// background thread for everything that happens during the turn.
    pub fn send(&mut self, exe: &Path, on_event: impl Fn(AgentEvent) + Send + 'static) {
        // An agent is always run inside a folder, so there is nothing to send without one.
        if !self.has_message() || self.is_running() || !self.has_folder() {
            return;
        }
        let message = UserMessage { text: self.input.trim().to_owned(), elements: self.elements.clone() };

        let turn = Turn {
            prompt: message.prompt(),
            cwd: self.project_dir.clone(),
            resume_session: self.agent_session_id.clone(),
            permission_mode: self.permission_mode,
            model: self.chosen_model.clone(),
            effort: self.effort.clone(),
        };
        match agent::start_turn(self.provider, exe, turn, on_event) {
            Ok(running) => {
                if self.title == UNTITLED {
                    self.title = match message.elements.first() {
                        Some(element) if message.text.is_empty() => {
                            title_from(element.note().unwrap_or(&element.short_label()))
                        }
                        _ => title_from(&message.text),
                    };
                }
                self.entries.push(Entry::User(message));
                self.input.clear();
                self.elements.clear();
                self.turn = Some(running);
                self.error_shown = false;
            }
            Err(err) => {
                let name = self.provider.label();
                self.entries.push(Entry::Error(format!("Couldn't start {name}: {err}")));
            }
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
                self.agent_session_id = Some(session_id);
                // Not every CLI reports its model.
                if !model.is_empty() {
                    self.model = Some(model);
                }
            }
            // The app records plan figures for the provider before this is called.
            AgentEvent::Plan(_) => {}
            AgentEvent::TextDelta(text) => self.streaming.push_str(&text),
            AgentEvent::Text(text) => {
                self.streaming.clear();
                if !text.trim().is_empty() {
                    self.entries.push(Entry::Agent(text));
                }
            }
            AgentEvent::ToolUse { name, detail, edit } => {
                self.keep_streamed_text();
                self.entries.push(Entry::Tool { name, detail, edit });
            }
            AgentEvent::ToolResult { text, is_error } => {
                let cut_off = text.chars().count() > MAX_TOOL_OUTPUT_CHARS;
                let mut text: String = text.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
                if cut_off {
                    text.push_str("\n… (cut off)");
                }
                self.entries.push(Entry::ToolOutput { text, is_error });
            }
            AgentEvent::Finished { session_id, error, denied_tools, .. } => {
                self.keep_streamed_text();
                // Resuming can hand back a new session ID; always continue from the latest one.
                if session_id.is_some() {
                    self.agent_session_id = session_id;
                }
                if let Some(error) = error {
                    self.error_shown = true;
                    self.entries.push(Entry::Error(error));
                }
                if !denied_tools.is_empty() {
                    // Headless CLIs can't be asked for approval, so anything needing it is
                    // refused until the session is on full access.
                    let what_to_do = if self.permission_mode == PermissionMode::Full {
                        "It refused even on full access, so it may be the CLI's own setting.".to_owned()
                    } else {
                        format!(
                            "Pick \"{}\" under the message box to let it do that without asking.",
                            PermissionMode::Full.label()
                        )
                    };
                    self.entries.push(Entry::Notice(format!(
                        "{} wasn't allowed to use: {}. {what_to_do}",
                        self.provider.short_name(),
                        denied_tools.join(", ")
                    )));
                }
            }
            AgentEvent::Exited { error } => {
                // Keeps any text that was cut off by Stop.
                self.keep_streamed_text();
                if self.stop_requested {
                    self.entries.push(Entry::Notice("Stopped.".into()));
                } else if let Some(error) = error
                    && !self.error_shown
                {
                    self.entries.push(Entry::Error(error));
                }
                self.turn = None;
                self.stop_requested = false;
                self.focus_composer = true;
            }
        }
    }

    /// Turns text that was streamed but never sent as a finished block into a reply.
    fn keep_streamed_text(&mut self) {
        let text = std::mem::take(&mut self.streaming);
        if !text.trim().is_empty() {
            self.entries.push(Entry::Agent(text));
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

    fn session(provider: Provider) -> Session {
        Session::new(1, PathBuf::from("C:\\work\\demo"), provider, PermissionMode::ReadOnly)
    }

    #[test]
    fn titles_use_the_first_line() {
        assert_eq!(title_from("Fix the login bug\nmore details"), "Fix the login bug");
        assert_eq!(title_from(&"a".repeat(60)), format!("{}…", "a".repeat(48)));
    }

    #[test]
    fn final_text_replaces_streamed_text() {
        let mut s = session(Provider::Claude);
        s.handle_event(AgentEvent::TextDelta("Hel".into()));
        s.handle_event(AgentEvent::TextDelta("lo".into()));
        assert_eq!(s.streaming, "Hello");
        s.handle_event(AgentEvent::Text("Hello".into()));
        assert!(s.streaming.is_empty());
        assert_eq!(s.entries, vec![Entry::Agent("Hello".into())]);
    }

    #[test]
    fn streamed_text_is_kept_before_tools_and_at_the_end() {
        // Antigravity never sends a finished block, only pieces.
        let mut s = session(Provider::Antigravity);
        s.handle_event(AgentEvent::TextDelta("Let me look.".into()));
        s.handle_event(AgentEvent::ToolUse { name: "read_file".into(), detail: "a.txt".into(), edit: None });
        s.handle_event(AgentEvent::TextDelta("Done.".into()));
        s.handle_event(AgentEvent::Finished { session_id: None, error: None, denied_tools: Vec::new(), usage: None });
        assert_eq!(
            s.entries,
            vec![
                Entry::Agent("Let me look.".into()),
                Entry::Tool { name: "read_file".into(), detail: "a.txt".into(), edit: None },
                Entry::Agent("Done.".into()),
            ]
        );
    }

    #[test]
    fn exit_keeps_partial_text_and_reports_errors() {
        let mut s = session(Provider::Claude);
        s.handle_event(AgentEvent::TextDelta("half a sent".into()));
        s.handle_event(AgentEvent::Exited { error: Some("boom".into()) });
        assert_eq!(s.entries, vec![Entry::Agent("half a sent".into()), Entry::Error("boom".into())]);
        assert!(s.focus_composer);
    }

    #[test]
    fn exit_error_is_not_repeated_after_a_reported_error() {
        let mut s = session(Provider::Antigravity);
        s.handle_event(AgentEvent::Finished {
            session_id: None,
            error: Some("API key not valid".into()),
            denied_tools: Vec::new(),
            usage: None,
        });
        s.handle_event(AgentEvent::Exited { error: Some("Antigravity (agy) exited with code 1.".into()) });
        assert_eq!(s.entries, vec![Entry::Error("API key not valid".into())]);
    }

    #[test]
    fn finished_updates_session_and_lists_denied_tools() {
        let mut s = session(Provider::Claude);
        s.handle_event(AgentEvent::Started { session_id: "first".into(), model: "m".into() });
        s.handle_event(AgentEvent::Finished {
            session_id: Some("second".into()),
            error: None,
            denied_tools: vec!["Bash".into(), "Write".into()],
            usage: None,
        });
        assert_eq!(s.agent_session_id.as_deref(), Some("second"));
        let [Entry::Notice(text)] = &s.entries[..] else { panic!("expected a notice") };
        assert!(text.starts_with("Claude wasn't allowed to use: Bash, Write"), "{text}");
        assert!(text.contains("Full access"), "the notice names the mode that allows it: {text}");
    }

    #[test]
    fn denials_on_full_access_point_at_the_cli() {
        let mut s = session(Provider::Antigravity);
        s.permission_mode = PermissionMode::Full;
        s.handle_event(AgentEvent::Finished {
            session_id: None,
            error: None,
            denied_tools: vec!["RunCommand".into()],
            usage: None,
        });
        let [Entry::Notice(text)] = &s.entries[..] else { panic!("expected a notice") };
        assert!(text.contains("refused even on full access"), "{text}");
    }

    #[test]
    fn long_tool_output_is_cut_off() {
        let mut s = session(Provider::Claude);
        s.handle_event(AgentEvent::ToolResult { text: "é".repeat(MAX_TOOL_OUTPUT_CHARS + 1), is_error: false });
        let [Entry::ToolOutput { text, .. }] = &s.entries[..] else { panic!("expected tool output") };
        assert!(text.ends_with("(cut off)"));
    }

    #[test]
    fn short_non_ascii_tool_output_is_kept_whole() {
        let mut s = session(Provider::Claude);
        s.handle_event(AgentEvent::ToolResult { text: "é".repeat(MAX_TOOL_OUTPUT_CHARS), is_error: false });
        let [Entry::ToolOutput { text, .. }] = &s.entries[..] else { panic!("expected tool output") };
        assert!(!text.contains("cut off"));
    }

    fn element(selector: &str) -> PickedElement {
        PickedElement {
            url: "http://localhost:3000/".into(),
            selector: selector.into(),
            tag: "button.primary".into(),
            text: "Sign in".into(),
            html: "<button class=\"primary\">Sign in</button>".into(),
            width: 120,
            height: 36,
            note: None,
        }
    }

    #[test]
    fn a_session_without_a_folder_says_so() {
        let mut fresh = Session::new(1, PathBuf::new(), Provider::Claude, PermissionMode::ReadOnly);
        assert!(!fresh.has_folder());
        assert_eq!(fresh.folder_name(), NO_FOLDER);

        // Nothing is sent before a folder is picked, even with a message ready.
        fresh.input = "do something".into();
        assert!(fresh.has_message());
        fresh.send(Path::new("claude"), |_| {});
        assert!(fresh.entries.is_empty(), "no message is sent without a folder");
        assert!(!fresh.is_running());
    }

    #[test]
    fn attached_elements_are_described_after_the_text() {
        let mut s = session(Provider::Claude);
        s.attach(element("main > button"));
        s.attach(element("main > button"));
        assert_eq!(s.elements.len(), 1, "the same element is only attached once");
        let commented = PickedElement { note: Some("make this blue".into()), ..element("main > button") };
        s.attach(commented);
        assert_eq!(s.elements.len(), 2, "a comment on the same element is its own attachment");
        assert!(s.has_message(), "an element alone can be sent");

        let message = UserMessage { text: "Make this blue".into(), elements: s.elements.clone() };
        let prompt = message.prompt();
        assert!(prompt.starts_with("Make this blue

Element on http://localhost:3000/"));
        assert!(prompt.contains("Selector: `main > button`"));
    }

    #[test]
    fn saved_text_messages_still_load() {
        // App state is saved as RON.
        let old: Vec<Entry> = ron::from_str(r#"[User("hi"), Agent("hello")]"#).unwrap();
        assert_eq!(old[0], Entry::User(UserMessage { text: "hi".into(), elements: Vec::new() }));

        let new = Entry::User(UserMessage { text: "hi".into(), elements: vec![element("main > button")] });
        let saved = ron::to_string(&new).unwrap();
        assert_eq!(ron::from_str::<Entry>(&saved).unwrap(), new);
    }

    #[test]
    fn provider_can_only_change_before_the_conversation_starts() {
        let mut s = session(Provider::Claude);
        assert!(s.can_change_provider());
        s.entries.push(Entry::User(UserMessage { text: "hi".into(), elements: Vec::new() }));
        assert!(!s.can_change_provider());
    }
}
