use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{
    self, AgentEvent, ApprovalDecision, ApprovalRequest, PermissionMode, Provider, RunningTurn,
    Turn,
};
use crate::browser::{BrowserState, PickedElement};
use crate::line_diff::FileEdit;
use crate::usage::Usage;

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
    /// An interactive approval request waiting on or resolved by the user.
    Approval(ApprovalRequest),
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
///
/// `default` at the struct level as well as on the newer fields: `SavedState` holds
/// every session in one RON document, so a single unreadable field here would take
/// the whole file — all sessions, settings and history — down with it.
#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub id: u64,
    pub title: String,
    pub project_dir: PathBuf,
    /// The isolated worktree directory for this session, if configured.
    #[serde(default)]
    pub worktree_dir: Option<PathBuf>,
    /// The git branch dedicated to this session's isolated changes.
    #[serde(default)]
    pub worktree_branch: Option<String>,
    /// The base branch or ref the worktree branch was created from.
    #[serde(default)]
    pub worktree_base: Option<String>,
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
    /// What the last finished turn cost. The input side of it is how much context
    /// this conversation is carrying, which is the only place that number exists —
    /// no CLI reports the context separately.
    #[serde(default)]
    pub last_usage: Option<Usage>,
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
    /// What this session wants shown in the shared browser. Each session has its
    /// own, so switching session changes the page rather than keeping the last one.
    #[serde(default)]
    pub browser: BrowserState,

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
    /// Ephemeral slash command autocomplete state in the composer.
    #[serde(skip)]
    pub slash_selected: usize,
    #[serde(skip)]
    pub slash_query: String,
    #[serde(skip)]
    pub slash_dismissed: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self::new(0, PathBuf::new(), Provider::default(), PermissionMode::default())
    }
}

impl Session {
    pub fn new(id: u64, project_dir: PathBuf, provider: Provider, permission_mode: PermissionMode) -> Self {
        Self {
            id,
            title: UNTITLED.to_owned(),
            project_dir,
            worktree_dir: None,
            worktree_branch: None,
            worktree_base: None,
            provider,
            permission_mode,
            chosen_model: None,
            effort: None,
            last_usage: None,
            agent_session_id: None,
            model: None,
            entries: Vec::new(),
            input: String::new(),
            elements: Vec::new(),
            browser: BrowserState::default(),
            streaming: String::new(),
            focus_composer: true,
            turn: None,
            stop_requested: false,
            error_shown: false,
            slash_selected: 0,
            slash_query: String::new(),
            slash_dismissed: false,
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

    /// The active directory an agent or terminal runs inside: the isolated worktree
    /// directory if one is active, or the root project directory.
    pub fn working_dir(&self) -> &Path {
        self.worktree_dir.as_deref().unwrap_or(&self.project_dir)
    }

    /// Previewable web artifacts (HTML, SVG, XHTML) generated or modified in this session.
    pub fn previewable_artifacts(&self) -> Vec<PathBuf> {
        crate::preview::extract_previewable_artifacts(&self.entries, self.working_dir())
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
            cwd: self.working_dir().to_path_buf(),
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

    /// Returns true if any approval request in this session is still waiting for a response.
    pub fn has_pending_approval(&self) -> bool {
        self.pending_approval().is_some()
    }

    /// Returns the most recent pending approval request, if any.
    pub fn pending_approval(&self) -> Option<&ApprovalRequest> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Approval(r) if r.is_pending() => Some(r),
            _ => None,
        })
    }

    /// Resolves an approval request and notifies the running turn if active.
    pub fn resolve_approval(&mut self, id: &str, decision: ApprovalDecision) -> bool {
        let mut resolved = false;
        for entry in &mut self.entries {
            if let Entry::Approval(req) = entry
                && req.id == id
                && req.is_pending()
            {
                resolved = req.resolve(decision);
                break;
            }
        }
        if resolved
            && let Some(turn) = &self.turn
        {
            turn.respond_approval(id, decision);
        }
        resolved
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
            AgentEvent::ApprovalRequest(request) => {
                self.keep_streamed_text();
                self.entries.push(Entry::Approval(request));
            }
            AgentEvent::Finished { session_id, error, mut denied_tools, .. } => {
                self.keep_streamed_text();
                // Resuming can hand back a new session ID; always continue from the latest one.
                if session_id.is_some() {
                    self.agent_session_id = session_id;
                }
                if denied_tools.is_empty()
                    && let Some(err) = &error
                    && is_permission_error(err)
                {
                    denied_tools.push(infer_denied_tool(err).to_owned());
                }
                if !denied_tools.is_empty() {
                    self.error_shown = true;
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
                } else if let Some(error) = error {
                    self.error_shown = true;
                    self.entries.push(Entry::Error(error));
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
                    if is_permission_error(&error) {
                        let tool = infer_denied_tool(&error);
                        let what_to_do = if self.permission_mode == PermissionMode::Full {
                            "It refused even on full access, so it may be the CLI's own setting.".to_owned()
                        } else {
                            format!(
                                "Pick \"{}\" under the message box to let it do that without asking.",
                                PermissionMode::Full.label()
                            )
                        };
                        self.entries.push(Entry::Notice(format!(
                            "{} wasn't allowed to use: {tool}. {what_to_do}",
                            self.provider.short_name()
                        )));
                    } else {
                        self.entries.push(Entry::Error(error));
                    }
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

fn is_permission_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("sandbox")
        || lower.contains("permission")
        || lower.contains("disallowed")
        || lower.contains("read-only")
        || lower.contains("read only")
        || lower.contains("read_only")
        || lower.contains("operation not permitted")
        || lower.contains("access is denied")
        || lower.contains("access denied")
        || lower.contains("forbidden")
}

fn infer_denied_tool(text: &str) -> &'static str {
    let lower = text.to_lowercase();
    if lower.contains("command")
        || lower.contains("terminal")
        || lower.contains("bash")
        || lower.contains("shell")
    {
        "Shell"
    } else if lower.contains("edit")
        || lower.contains("write")
        || lower.contains("file")
    {
        "Edit"
    } else {
        "Action"
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
    use crate::agent::ApprovalStatus;

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
    fn denied_tools_suppresses_process_exit_error() {
        let mut s = session(Provider::Codex);
        s.handle_event(AgentEvent::Finished {
            session_id: None,
            error: None,
            denied_tools: vec!["Edit".into()],
            usage: None,
        });
        s.handle_event(AgentEvent::Exited {
            error: Some("Codex exited with code 1. Sandbox violation.".into()),
        });
        assert_eq!(s.entries.len(), 1);
        let [Entry::Notice(text)] = &s.entries[..] else { panic!("expected a notice") };
        assert!(text.contains("wasn't allowed to use: Edit"), "{text}");
    }

    #[test]
    fn denied_tools_take_precedence_over_finished_error() {
        let mut s = session(Provider::Claude);
        s.handle_event(AgentEvent::Finished {
            session_id: None,
            error: Some("Generic failure message".into()),
            denied_tools: vec!["Bash".into()],
            usage: None,
        });
        assert_eq!(s.entries.len(), 1);
        let [Entry::Notice(text)] = &s.entries[..] else { panic!("expected a notice, not an error") };
        assert!(text.contains("wasn't allowed to use: Bash"), "{text}");
    }

    #[test]
    fn process_exit_permission_error_becomes_denied_tool_notice() {
        let mut s = session(Provider::Codex);
        s.handle_event(AgentEvent::Exited {
            error: Some("Codex exited with code 1. Sandbox violation: writing to file is forbidden in read-only mode.".into()),
        });
        assert_eq!(s.entries.len(), 1);
        let [Entry::Notice(text)] = &s.entries[..] else { panic!("expected a notice, not an error") };
        assert!(text.contains("wasn't allowed to use: Edit"), "{text}");
    }

    #[test]
    fn finished_permission_error_without_denied_tools_becomes_notice() {
        let mut s = session(Provider::Claude);
        s.handle_event(AgentEvent::Finished {
            session_id: None,
            error: Some("Permission denied: command execution not allowed in read-only mode".into()),
            denied_tools: Vec::new(),
            usage: None,
        });
        assert_eq!(s.entries.len(), 1);
        let [Entry::Notice(text)] = &s.entries[..] else { panic!("expected a notice, not an error") };
        assert!(text.contains("wasn't allowed to use: Shell"), "{text}");
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

    #[test]
    fn session_handles_approval_request_event_and_updates_entries() {
        let mut s = session(Provider::Claude);
        s.streaming.push_str("Thinking about changes...");
        let req = ApprovalRequest::new("req-1", "bash", "cargo test", None);
        s.handle_event(AgentEvent::ApprovalRequest(req.clone()));

        assert!(s.streaming.is_empty(), "streamed text must be flushed before approval entry");
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.entries[0], Entry::Agent("Thinking about changes...".into()));
        assert_eq!(s.entries[1], Entry::Approval(req));
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().map(|r| r.id.as_str()), Some("req-1"));
    }

    #[test]
    fn resolving_session_approval_updates_entry_and_relays_to_turn() {
        let mut s = session(Provider::Claude);
        let req = ApprovalRequest::new("req-42", "edit_file", "src/main.rs", None);
        s.entries.push(Entry::Approval(req));
        assert!(s.has_pending_approval());

        let (tx, rx) = std::sync::mpsc::channel();
        let cmd = if cfg!(windows) { "powershell.exe" } else { "sh" };
        let arg = if cfg!(windows) { "-Command" } else { "-c" };
        let script = if cfg!(windows) { "Start-Sleep -Seconds 2" } else { "sleep 2" };
        let child = std::process::Command::new(cmd).args([arg, script]).spawn().unwrap();
        s.turn = Some(RunningTurn::with_approval_channel(child, Some(tx)));

        let resolved = s.resolve_approval("req-42", ApprovalDecision::Approved);
        assert!(resolved, "approval should be resolved");
        assert!(!s.has_pending_approval(), "pending state should be cleared");
        assert_eq!(s.pending_approval(), None);

        let Entry::Approval(updated) = &s.entries[0] else {
            panic!("expected Entry::Approval");
        };
        assert_eq!(updated.status, ApprovalStatus::Approved);

        let relayed = rx.recv().expect("relay channel should receive response");
        assert_eq!(relayed.id, "req-42");
        assert_eq!(relayed.decision, ApprovalDecision::Approved);
        s.stop();
    }

    #[test]
    fn resolving_nonexistent_or_already_resolved_approval_returns_false() {
        let mut s = session(Provider::Claude);
        let req = ApprovalRequest::new("req-1", "bash", "cargo build", None);
        s.entries.push(Entry::Approval(req));

        assert!(!s.resolve_approval("nonexistent", ApprovalDecision::Approved));
        assert!(s.resolve_approval("req-1", ApprovalDecision::Denied));
        assert!(!s.resolve_approval("req-1", ApprovalDecision::Approved), "cannot resolve twice");

        let Entry::Approval(updated) = &s.entries[0] else {
            panic!("expected Entry::Approval");
        };
        assert_eq!(updated.status, ApprovalStatus::Denied);
    }

    #[test]
    fn approval_entries_round_trip_cleanly_through_ron() {
        let req = ApprovalRequest::new("req-ron", "bash", "git status", None);
        let entry = Entry::Approval(req);
        let serialized = ron::to_string(&entry).unwrap();
        let deserialized: Entry = ron::from_str(&serialized).unwrap();
        assert_eq!(entry, deserialized);

        let mut resolved_req = ApprovalRequest::new("req-2", "edit", "README.md", None);
        resolved_req.resolve(ApprovalDecision::Approved);
        let entry2 = Entry::Approval(resolved_req);
        let serialized2 = ron::to_string(&entry2).unwrap();
        let deserialized2: Entry = ron::from_str(&serialized2).unwrap();
        assert_eq!(entry2, deserialized2);
    }

    #[test]
    fn existing_saved_sessions_without_approvals_still_deserialize() {
        let old_ron = r#"[
            User("Hello"),
            Agent("I am ready to help."),
            Notice("Working..."),
            Error("Something failed.")
        ]"#;
        let entries: Vec<Entry> = ron::from_str(old_ron).unwrap();
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0], Entry::User(UserMessage { text: "Hello".into(), elements: Vec::new() }));
        assert_eq!(entries[1], Entry::Agent("I am ready to help.".into()));
        assert_eq!(entries[2], Entry::Notice("Working...".into()));
        assert_eq!(entries[3], Entry::Error("Something failed.".into()));
    }

    #[test]
    fn session_approval_lifecycle_and_edge_cases() {
        let mut s = session(Provider::Claude);

        // 1. Initial state: no pending approvals
        assert!(!s.has_pending_approval());
        assert_eq!(s.pending_approval(), None);

        // 2. Empty ID and Unicode IDs
        let req_empty = ApprovalRequest::new("", "tool", "detail", None);
        s.entries.push(Entry::Approval(req_empty));
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "");
        assert!(s.resolve_approval("", ApprovalDecision::Approved));
        assert!(!s.has_pending_approval());

        let req_unicode = ApprovalRequest::new("⚡-rocket-🚀", "bash", "deploy", None);
        s.entries.push(Entry::Approval(req_unicode));
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "⚡-rocket-🚀");
        assert!(s.resolve_approval("⚡-rocket-🚀", ApprovalDecision::Denied));
        assert!(!s.has_pending_approval());

        // 3. Multiple approvals in same session, resolved out-of-order
        let r1 = ApprovalRequest::new("req-1", "tool1", "d1", None);
        let r2 = ApprovalRequest::new("req-2", "tool2", "d2", None);
        let r3 = ApprovalRequest::new("req-3", "tool3", "d3", None);
        s.entries.push(Entry::Approval(r1));
        s.entries.push(Entry::Approval(r2));
        s.entries.push(Entry::Approval(r3));

        // pending_approval returns the latest pending approval (LIFO / newest)
        assert_eq!(s.pending_approval().unwrap().id, "req-3");

        // Resolve the middle one first
        assert!(s.resolve_approval("req-2", ApprovalDecision::Approved));
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "req-3");

        // Resolve req-3
        assert!(s.resolve_approval("req-3", ApprovalDecision::Denied));
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "req-1");

        // Resolve req-1
        assert!(s.resolve_approval("req-1", ApprovalDecision::Approved));
        assert!(!s.has_pending_approval());
        assert_eq!(s.pending_approval(), None);

        // Double resolution of any of them must fail
        assert!(!s.resolve_approval("req-1", ApprovalDecision::Approved));
        assert!(!s.resolve_approval("req-2", ApprovalDecision::Denied));
        assert!(!s.resolve_approval("req-3", ApprovalDecision::Approved));
    }

    #[test]
    fn duplicate_approval_ids_across_turns_behavior() {
        let mut s = session(Provider::Claude);
        let mut req1 = ApprovalRequest::new("call-1", "bash", "ls", None);
        req1.resolve(ApprovalDecision::Approved);
        s.entries.push(Entry::Approval(req1));

        let req2 = ApprovalRequest::new("call-1", "bash", "cat file", None);
        s.entries.push(Entry::Approval(req2));

        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().detail, "cat file");

        // With the Challenger M1 fix (matching req.is_pending()), resolve_approval skips the
        // already-resolved entry and resolves the pending entry with the same ID:
        let resolved = s.resolve_approval("call-1", ApprovalDecision::Approved);
        assert!(resolved, "pending entry is resolved even if an earlier resolved entry shared the same id");
        assert!(!s.has_pending_approval(), "no approvals remain pending");
        let Entry::Approval(ref r2) = s.entries[1] else { panic!("expected approval entry") };
        assert_eq!(r2.status, ApprovalStatus::Approved);
    }

    #[test]
    fn older_session_and_saved_state_ron_formats_deserialize_cleanly() {
        let legacy_session_ron = r#"(
            id: 101,
            title: "Legacy Session Title",
            project_dir: "C:\\projects\\legacy",
            provider: Claude,
            permission_mode: Plan,
            entries: [
                User("Please review our architecture"),
                Agent("Here is the architectural review: everything looks sound."),
                Tool(
                    name: "read_file",
                    detail: "src/main.rs",
                    edit: None,
                ),
                ToolOutput(
                    text: "fn main() { println!(\"hello\"); }",
                    is_error: false,
                ),
                Notice("Session completed cleanly"),
                Error("Non-fatal legacy notice"),
            ],
            input: "draft follow-up",
        )"#;

        let session: Session = ron::from_str(legacy_session_ron).expect("legacy session must deserialize cleanly");
        assert_eq!(session.id, 101);
        assert_eq!(session.title, "Legacy Session Title");
        assert_eq!(session.entries.len(), 6);
        assert!(!session.has_pending_approval(), "legacy session has no pending approvals");
        assert_eq!(session.pending_approval(), None);

        let thin_approval_ron = r#"Approval((
            id: "thin-req-1",
            tool_name: "bash",
            detail: "cargo check",
        ))"#;
        let entry: Entry = ron::from_str(thin_approval_ron).expect("thin approval must deserialize");
        let Entry::Approval(req) = entry else { panic!("expected Entry::Approval") };
        assert_eq!(req.id, "thin-req-1");
        assert_eq!(req.tool_name, "bash");
        assert_eq!(req.detail, "cargo check");
        assert_eq!(req.edit, None);
        assert_eq!(req.status, ApprovalStatus::Pending);
    }

    #[test]
    fn multi_approval_fifo_and_arbitrary_ordering_lifecycle() {
        let mut s = session(Provider::Claude);
        let (tx, rx) = std::sync::mpsc::channel();
        let cmd = if cfg!(windows) { "powershell.exe" } else { "sh" };
        let arg = if cfg!(windows) { "-Command" } else { "-c" };
        let script = if cfg!(windows) { "Start-Sleep -Seconds 2" } else { "sleep 2" };
        let child = std::process::Command::new(cmd).args([arg, script]).spawn().unwrap();
        s.turn = Some(RunningTurn::with_approval_channel(child, Some(tx)));

        for i in 1..=5 {
            s.entries.push(Entry::Approval(ApprovalRequest::new(
                format!("req-{i}"),
                format!("tool-{i}"),
                format!("detail {i}"),
                None,
            )));
        }

        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "req-5", "pending_approval returns most recent");

        // Arbitrary resolution: resolve req-3 (middle)
        assert!(s.resolve_approval("req-3", ApprovalDecision::Approved));
        let relay = rx.recv().expect("relay channel receives req-3 response");
        assert_eq!(relay.id, "req-3");
        assert_eq!(relay.decision, ApprovalDecision::Approved);
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "req-5");

        // FIFO resolution: resolve req-1 (first)
        assert!(s.resolve_approval("req-1", ApprovalDecision::Denied));
        let relay = rx.recv().expect("relay channel receives req-1 response");
        assert_eq!(relay.id, "req-1");
        assert_eq!(relay.decision, ApprovalDecision::Denied);
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "req-5");

        // Resolve req-5 (latest)
        assert!(s.resolve_approval("req-5", ApprovalDecision::Approved));
        let relay = rx.recv().expect("relay channel receives req-5 response");
        assert_eq!(relay.id, "req-5");
        assert_eq!(relay.decision, ApprovalDecision::Approved);
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "req-4", "req-4 is now latest pending");

        // Double-resolving already resolved approvals returns false and does NOT relay
        assert!(!s.resolve_approval("req-3", ApprovalDecision::Denied));
        assert!(!s.resolve_approval("req-1", ApprovalDecision::Approved));
        assert!(!s.resolve_approval("req-5", ApprovalDecision::Denied));
        assert!(rx.try_recv().is_err(), "no spurious responses relayed for redundant calls");

        // Resolve remaining in arbitrary order: req-4, then req-2
        assert!(s.resolve_approval("req-4", ApprovalDecision::Denied));
        let relay = rx.recv().expect("relay channel receives req-4 response");
        assert_eq!(relay.id, "req-4");
        assert_eq!(relay.decision, ApprovalDecision::Denied);
        assert!(s.has_pending_approval());
        assert_eq!(s.pending_approval().unwrap().id, "req-2");

        assert!(s.resolve_approval("req-2", ApprovalDecision::Approved));
        let relay = rx.recv().expect("relay channel receives req-2 response");
        assert_eq!(relay.id, "req-2");
        assert_eq!(relay.decision, ApprovalDecision::Approved);

        // All 5 approvals now resolved
        assert!(!s.has_pending_approval(), "no approvals should remain pending");
        assert_eq!(s.pending_approval(), None);

        let statuses: Vec<(String, ApprovalStatus)> = s.entries.iter().filter_map(|e| match e {
            Entry::Approval(r) => Some((r.id.clone(), r.status)),
            _ => None,
        }).collect();
        assert_eq!(statuses, vec![
            ("req-1".into(), ApprovalStatus::Denied),
            ("req-2".into(), ApprovalStatus::Approved),
            ("req-3".into(), ApprovalStatus::Approved),
            ("req-4".into(), ApprovalStatus::Denied),
            ("req-5".into(), ApprovalStatus::Approved),
        ]);

        s.stop();
    }

    #[test]
    fn session_state_transitions_between_waiting_running_and_idle_under_multi_entry_flow() {
        use crate::sidebar::{session_state, SessionState};

        let mut s = session(Provider::Claude);

        // 1. Fresh session starts Idle
        assert_eq!(session_state(&s), SessionState::Idle);

        // 2. Turn starts: Running
        let cmd = if cfg!(windows) { "powershell.exe" } else { "sh" };
        let arg = if cfg!(windows) { "-Command" } else { "-c" };
        let script = if cfg!(windows) { "Start-Sleep -Seconds 2" } else { "sleep 2" };
        let child = std::process::Command::new(cmd).args([arg, script]).spawn().unwrap();
        s.turn = Some(RunningTurn::with_approval_channel(child, None));
        assert_eq!(session_state(&s), SessionState::Running);

        // 3. Approval 1 arrives: WaitingForApproval
        s.entries.push(Entry::Approval(ApprovalRequest::new("app-1", "bash", "ls", None)));
        assert_eq!(session_state(&s), SessionState::WaitingForApproval);

        // 4. Intermediate tool entries arrive: still WaitingForApproval
        s.entries.push(Entry::Tool { name: "bash".into(), detail: "pwd".into(), edit: None });
        s.entries.push(Entry::ToolOutput { text: "/root".into(), is_error: false });
        assert_eq!(session_state(&s), SessionState::WaitingForApproval);

        // 5. Approval 2 arrives: still WaitingForApproval
        s.entries.push(Entry::Approval(ApprovalRequest::new("app-2", "edit", "main.rs", None)));
        assert_eq!(session_state(&s), SessionState::WaitingForApproval);

        // 6. User resolves app-1: still WaitingForApproval because app-2 is pending
        assert!(s.resolve_approval("app-1", ApprovalDecision::Approved));
        assert_eq!(session_state(&s), SessionState::WaitingForApproval);

        // 7. User resolves app-2: transitions back to Running (turn is active)
        assert!(s.resolve_approval("app-2", ApprovalDecision::Approved));
        assert_eq!(session_state(&s), SessionState::Running);

        // 8. Approval 3 arrives and is Denied: transitions WaitingForApproval -> Running
        s.entries.push(Entry::Approval(ApprovalRequest::new("app-3", "bash", "rm -rf", None)));
        assert_eq!(session_state(&s), SessionState::WaitingForApproval);
        assert!(s.resolve_approval("app-3", ApprovalDecision::Denied));
        assert_eq!(session_state(&s), SessionState::Running);

        // 9. Turn finishes with normal Agent reply: transitions to Idle
        s.stop();
        s.turn = None;
        s.entries.push(Entry::Agent("Done with all tasks".into()));
        assert_eq!(session_state(&s), SessionState::Idle);

        // 10. Turn finishes with Error: transitions to Failed
        s.entries.push(Entry::Error("Process exited with code 1".into()));
        assert_eq!(session_state(&s), SessionState::Failed);
    }

    #[test]
    fn working_dir_falls_back_to_project_dir_when_worktree_none() {
        let s = session(Provider::Claude);
        assert_eq!(s.worktree_dir, None);
        assert_eq!(s.working_dir(), s.project_dir.as_path());
    }

    #[test]
    fn working_dir_uses_worktree_dir_when_present() {
        let mut s = session(Provider::Claude);
        let isolated = PathBuf::from(r"C:\projects\repo\.viper\worktrees\99");
        s.worktree_dir = Some(isolated.clone());
        assert_eq!(s.working_dir(), isolated.as_path());
    }

    #[test]
    fn sessions_without_worktree_fields_deserialize_cleanly_with_defaults() {
        let ron_text = r#"(
            id: 200,
            title: "Pre-Worktree Session",
            project_dir: "C:\\projects\\old_repo",
            provider: Codex,
            permission_mode: Full,
            entries: [],
        )"#;
        let s: Session = ron::from_str(ron_text).expect("older sessions without worktree fields must deserialize");
        assert_eq!(s.id, 200);
        assert_eq!(s.worktree_dir, None);
        assert_eq!(s.worktree_branch, None);
        assert_eq!(s.worktree_base, None);
        assert_eq!(s.working_dir(), Path::new(r"C:\projects\old_repo"));
    }

    #[test]
    fn sessions_with_worktree_fields_roundtrip_and_partial_defaults() {
        // Only worktree_dir present, branch and base missing:
        let partial_ron = r#"(
            id: 201,
            title: "Partial Worktree",
            project_dir: "C:\\projects\\repo",
            worktree_dir: Some("C:\\projects\\repo\\.viper\\worktrees\\201"),
            provider: Claude,
            permission_mode: Full,
            entries: [],
        )"#;
        let s: Session = ron::from_str(partial_ron).expect("partial worktree fields deserialize");
        assert_eq!(s.worktree_dir, Some(PathBuf::from(r"C:\projects\repo\.viper\worktrees\201")));
        assert_eq!(s.worktree_branch, None);
        assert_eq!(s.worktree_base, None);
        assert_eq!(s.working_dir(), Path::new(r"C:\projects\repo\.viper\worktrees\201"));

        // Full worktree fields roundtrip
        let mut full = Session::new(202, PathBuf::from(r"C:\work\alpha"), Provider::Antigravity, PermissionMode::Plan);
        full.worktree_dir = Some(PathBuf::from(r"C:\work\alpha\.viper\worktrees\202"));
        full.worktree_branch = Some("viper/session-202".into());
        full.worktree_base = Some("main".into());

        let ron_str = ron::to_string(&full).expect("session serializes");
        let restored: Session = ron::from_str(&ron_str).expect("session deserializes");
        assert_eq!(restored.worktree_dir, full.worktree_dir);
        assert_eq!(restored.worktree_branch, full.worktree_branch);
        assert_eq!(restored.worktree_base, full.worktree_base);
        assert_eq!(restored.working_dir(), Path::new(r"C:\work\alpha\.viper\worktrees\202"));
    }

    #[test]
    fn session_extracts_previewable_artifacts_from_working_dir() {
        let mut s = Session::new(301, PathBuf::from(r"C:\work\app"), Provider::Claude, PermissionMode::Full);
        s.entries.push(Entry::Tool {
            name: "write_to_file".into(),
            detail: "preview.html".into(),
            edit: None,
        });
        s.entries.push(Entry::Tool {
            name: "Edit".into(),
            detail: "update svg".into(),
            edit: Some(FileEdit::new("logo.svg", "", "<svg></svg>")),
        });

        let artifacts = s.previewable_artifacts();
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0], PathBuf::from(r"C:\work\app\logo.svg"));
        assert_eq!(artifacts[1], PathBuf::from(r"C:\work\app\preview.html"));
    }

    #[test]
    fn sessions_without_auto_refresh_deserialize_cleanly_with_defaults() {
        let legacy_ron = r#"(
            id: 302,
            title: "Legacy Browser Session",
            project_dir: "C:\\work\\app",
            provider: Codex,
            permission_mode: Plan,
            entries: [],
            browser: (
                address: "http://localhost:5173",
                viewport: Desktop,
                custom_size: (1280, 800),
            ),
        )"#;
        let s: Session = ron::from_str(legacy_ron).expect("legacy session without auto_refresh must deserialize");
        assert_eq!(s.id, 302);
        assert_eq!(s.browser.address, "http://localhost:5173");
        assert!(s.browser.auto_refresh, "older session without auto_refresh defaults to true");

        // Even older session with no browser field at all
        let bare_ron = r#"(
            id: 303,
            title: "Bare Session",
            project_dir: "C:\\work\\bare",
            provider: Claude,
            permission_mode: ReadOnly,
            entries: [],
        )"#;
        let bare: Session = ron::from_str(bare_ron).expect("bare session must deserialize");
        assert!(bare.browser.auto_refresh);
    }

    #[test]
    fn session_with_explicit_auto_refresh_false_roundtrips_through_ron() {
        let mut s = Session::new(304, PathBuf::from(r"C:\work\roundtrip"), Provider::Antigravity, PermissionMode::Full);
        s.browser.auto_refresh = false;
        s.browser.address = "file:///C:/work/roundtrip/index.html".into();
        s.worktree_dir = Some(PathBuf::from(r"C:\work\roundtrip\.viper\worktrees\304"));
        s.worktree_branch = Some("viper/session-304".into());
        s.worktree_base = Some("main".into());

        let ron_str = ron::to_string(&s).expect("session with auto_refresh=false serializes");
        let restored: Session = ron::from_str(&ron_str).expect("session with auto_refresh=false deserializes");
        assert!(!restored.browser.auto_refresh);
        assert_eq!(restored.browser.address, "file:///C:/work/roundtrip/index.html");
        assert_eq!(restored.worktree_dir, s.worktree_dir);
        assert_eq!(restored.worktree_branch, s.worktree_branch);
        assert_eq!(restored.worktree_base, s.worktree_base);
    }
}

