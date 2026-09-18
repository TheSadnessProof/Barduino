//! The middle column, when it is the CLI's own terminal rather than Barduino's chat.
//!
//! The chat in `chat.rs` reads a headless CLI's JSON and draws the conversation
//! itself. This does the opposite: it runs the CLI the way a person would, in a
//! PTY, and lets the CLI draw its own interface. Its own slash menu, its own
//! approval prompts, its own model picker — all of them correct by construction,
//! and none of them ours to maintain.
//!
//! One per session, kept alive while its session is hidden, exactly as the
//! right-hand tool panels are. Switching session swaps the terminal rather than
//! carrying one project's conversation into the next.
//!
//! What Barduino still has to do is carry things *into* that terminal — a page
//! element picked in the browser, most of all — which is what [`AgentTerminal::attach`]
//! is for.

use std::path::Path;

use eframe::egui;

use crate::agent::Provider;
use crate::browser::PickedElement;
use crate::chat::{self, ConversationAction};
use crate::session::Session;
use crate::settings::Settings;
use crate::terminal::{self, Input, Terminal};

/// What the terminal chat needs from `app.rs`.
pub enum TerminalAction {
    None,
    ChangeFolder,
    SelectProvider(Provider),
}

/// One session's agent, running in its own terminal.
#[derive(Default)]
pub struct AgentTerminal {
    /// Started the first time it is drawn, so opening a session costs nothing until
    /// it is looked at.
    terminal: Option<Result<Terminal, String>>,
    /// Waiting to go in once the terminal is running.
    pending: Vec<Input>,
    /// Set while this terminal should take the keyboard the next time it is drawn.
    focus: bool,
    /// The provider whose command was typed in, so a session that switches agent
    /// starts the new one rather than staying on the old.
    started: Option<Provider>,
}

impl AgentTerminal {
    /// Puts page elements into the CLI's prompt, as a paste rather than as typing:
    /// an element carries a fenced block of its HTML, and a CLI reading keystrokes
    /// would take each newline in it as "send this now".
    ///
    /// `submit` presses Enter afterwards, which is the difference between the
    /// browser's "Add comment" and its "Send".
    pub fn attach(&mut self, elements: &[PickedElement], submit: bool) {
        if elements.is_empty() {
            return;
        }
        let prompt = elements.iter().map(PickedElement::as_prompt).collect::<Vec<_>>().join("\n\n");
        self.pending.push(Input::Paste(prompt));
        if submit {
            // Its own write, so the paste is closed before the Enter arrives.
            self.pending.push(Input::Run("\r".to_owned()));
        }
        self.focus = true;
    }

    pub fn take_keyboard(&mut self) {
        self.focus = true;
    }

    /// Runs the agent. Everything before this is choosing what to run and where.
    ///
    /// The command goes to the front of the queue, not the back: a comment left in
    /// the browser before Start was pressed is already waiting, and it is meant for
    /// the agent rather than for the shell that launches it.
    pub fn start(&mut self, provider: Provider) {
        self.pending.insert(0, Input::Run(format!("{}\r", provider.command())));
        self.started = Some(provider);
        self.focus = true;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, session: &Session, settings: &Settings, shell: &Path) -> TerminalAction {
        // Until it is started this is the agent and folder picker, and nothing else.
        // There is no message box, because there is nothing yet to type into.
        let Some(started) = self.started else {
            return match chat::empty_session_ui(ui, session, settings, true) {
                ConversationAction::Start => {
                    self.start(session.provider);
                    TerminalAction::None
                }
                ConversationAction::ChangeFolder => TerminalAction::ChangeFolder,
                ConversationAction::SelectProvider(provider) => TerminalAction::SelectProvider(provider),
                ConversationAction::None => TerminalAction::None,
            };
        };

        // A session that changed agent after starting runs the new one. The old is
        // left above it, which is also what happens in a terminal of your own.
        if started != session.provider {
            self.start(session.provider);
        }

        let take_keyboard = std::mem::take(&mut self.focus);
        let restarted =
            terminal::show(ui, &mut self.terminal, &session.project_dir, shell, &mut self.pending, take_keyboard);
        if restarted {
            // The replacement is a bare shell, so it needs the command again.
            self.pending.push(Input::Run(format!("{}\r", session.provider.command())));
            self.focus = true;
        }
        TerminalAction::None
    }
}

/// Where each session's agent terminal lives, keyed by session ID so that switching
/// session swaps the conversation instead of carrying one project's into the next.
///
/// Kept when the middle column goes back to being Barduino's own chat, rather than
/// closed: switching is for comparing the two, and an agent part-way through a task
/// shouldn't be killed by a glance at the other one.
#[derive(Default)]
pub struct AgentTerminals(std::collections::BTreeMap<u64, AgentTerminal>);

impl AgentTerminals {
    pub fn get(&mut self, session: u64) -> &mut AgentTerminal {
        self.0.entry(session).or_default()
    }

    /// Ends a session's terminal, which also stops the agent running in it. Used
    /// when the session is deleted, and when its folder changes — a conversation
    /// belongs to the folder it was held in.
    pub fn close(&mut self, session: u64) {
        self.0.remove(&session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(note: Option<&str>) -> PickedElement {
        PickedElement {
            url: "https://example.com/pricing".to_owned(),
            selector: "div.plans > button.buy".to_owned(),
            tag: "button".to_owned(),
            text: "Buy now".to_owned(),
            html: "<button class=\"buy\">Buy now</button>".to_owned(),
            width: 120,
            height: 40,
            note: note.map(str::to_owned),
        }
    }

    #[test]
    fn an_attached_element_goes_in_as_one_paste() {
        let mut agent = AgentTerminal::default();
        agent.attach(&[element(Some("make this bigger"))], false);
        // One write, not one per line: the element's prompt carries a fenced block of
        // HTML, and a newline typed into a CLI means "send what I have so far".
        assert_eq!(agent.pending.len(), 1);
        let Some(Input::Paste(prompt)) = agent.pending.first() else { panic!("a paste, not typing") };
        assert!(prompt.contains("make this bigger"), "the note is what the user wrote: {prompt}");
        assert!(prompt.contains("div.plans > button.buy"), "and the selector says which element");
        assert!(prompt.lines().count() > 1, "it is genuinely multi-line, which is the whole point");
    }

    #[test]
    fn sending_an_element_presses_enter_and_adding_one_does_not() {
        let mut adding = AgentTerminal::default();
        adding.attach(&[element(None)], false);
        assert!(!adding.pending.iter().any(|input| matches!(input, Input::Run(_))), "Add leaves it to be edited");

        let mut sending = AgentTerminal::default();
        sending.attach(&[element(None)], true);
        let [Input::Paste(_), Input::Run(enter)] = &sending.pending[..] else {
            panic!("a paste then Enter: {}", sending.pending.len())
        };
        assert_eq!(enter, "\r");

        // Nothing attached is nothing sent, rather than an empty prompt.
        let mut empty = AgentTerminal::default();
        empty.attach(&[], true);
        assert!(empty.pending.is_empty());
    }

    #[test]
    fn nothing_runs_until_start_is_pressed() {
        let mut agent = AgentTerminal::default();
        assert_eq!(agent.started, None);
        assert!(agent.pending.is_empty(), "no agent is launched just by looking at the session");

        agent.start(Provider::Claude);
        let [Input::Run(command)] = &agent.pending[..] else { panic!("one command: {}", agent.pending.len()) };
        assert_eq!(command, "claude\r", "the CLI's own command, exactly as you would type it");
        assert_eq!(agent.started, Some(Provider::Claude));
    }

    #[test]
    fn a_comment_left_before_start_is_meant_for_the_agent_not_the_shell() {
        // Picking something in the browser and only then pressing Start. The comment
        // is already queued, and whatever goes in first is read by the shell — so the
        // command that launches the agent has to overtake it.
        let mut agent = AgentTerminal::default();
        agent.attach(&[element(Some("this bit"))], false);
        agent.start(Provider::Codex);

        let [Input::Run(command), Input::Paste(prompt)] = &agent.pending[..] else {
            panic!("the command, then the comment: {}", agent.pending.len())
        };
        assert_eq!(command, "codex\r");
        assert!(prompt.contains("this bit"), "{prompt}");
    }

    #[test]
    fn several_comments_arrive_together() {
        let mut agent = AgentTerminal::default();
        agent.attach(&[element(Some("first")), element(Some("second"))], false);
        let Some(Input::Paste(prompt)) = agent.pending.first() else { panic!("one paste for all of them") };
        assert!(prompt.contains("first") && prompt.contains("second"), "{prompt}");
    }

    #[test]
    fn a_terminal_belongs_to_its_session() {
        let mut terminals = AgentTerminals::default();
        terminals.get(1).attach(&[element(Some("for session one"))], false);
        terminals.get(2).attach(&[element(Some("for session two"))], false);

        let Some(Input::Paste(one)) = terminals.get(1).pending.first() else { panic!("session 1 kept its own") };
        assert!(one.contains("for session one"), "{one}");

        // Deleting a session ends its terminal and leaves the others alone.
        terminals.close(1);
        assert_eq!(terminals.0.len(), 1, "session 2 is still there");
        assert!(terminals.get(1).pending.is_empty(), "session 1 starts over");
        let Some(Input::Paste(two)) = terminals.get(2).pending.first() else { panic!("session 2 untouched") };
        assert!(two.contains("for session two"), "{two}");
    }
}
