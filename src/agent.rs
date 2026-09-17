//! The parts every agent CLI shares: which providers exist, the events the UI
//! understands, and running one turn as a background process.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{claude, gemini};

/// An agent CLI that can act as the brain of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    Claude,
    Gemini,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Self::Claude, Self::Gemini];

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Gemini => "Gemini CLI",
        }
    }

    /// The short name used in sentences like "Gemini is working…".
    pub fn short_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Gemini => "Gemini",
        }
    }

    /// The command that starts the CLI interactively, e.g. to sign in.
    pub fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Gemini => "gemini",
        }
    }

    pub fn install_hint(self) -> &'static str {
        match self {
            Self::Claude => "Install it from https://claude.com/claude-code",
            Self::Gemini => "Install it with: npm install -g @google/gemini-cli",
        }
    }

    /// Finds how to start this CLI on this computer.
    pub fn find(self) -> Option<Launcher> {
        match self {
            Self::Claude => claude::find_launcher(),
            Self::Gemini => gemini::find_launcher(),
        }
    }
}

/// How to start a CLI: a program plus any arguments that must come first.
#[derive(Debug, Clone, PartialEq)]
pub struct Launcher {
    pub program: PathBuf,
    pub leading_args: Vec<OsString>,
}

impl Launcher {
    pub fn program(program: PathBuf) -> Self {
        Self { program, leading_args: Vec::new() }
    }

    /// What to show the user as the CLI's location.
    pub fn display(&self) -> String {
        match self.leading_args.last() {
            Some(script) => PathBuf::from(script).display().to_string(),
            None => self.program.display().to_string(),
        }
    }
}

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
    /// A warning or error the CLI reported while it kept going.
    Notice { text: String, is_error: bool },
    /// The agent finished the turn.
    Finished {
        session_id: Option<String>,
        error: Option<String>,
        denied_tools: Vec<String>,
    },
    /// The CLI process ended. `error` is set if it didn't exit cleanly.
    Exited { error: Option<String> },
}

/// What the agent is allowed to do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl Drop for RunningTurn {
    // Deleting a session or closing the app shouldn't leave the agent running in the background.
    fn drop(&mut self) {
        self.stop();
    }
}

/// Starts one turn of the conversation. `on_event` is called from a background
/// thread for each event.
pub fn start_turn(
    provider: Provider,
    launcher: &Launcher,
    turn: Turn,
    on_event: impl Fn(AgentEvent) + Send + 'static,
) -> std::io::Result<RunningTurn> {
    let mut cmd = Command::new(&launcher.program);
    cmd.args(&launcher.leading_args);
    let parse_line: fn(&str) -> Vec<AgentEvent> = match provider {
        Provider::Claude => {
            cmd.args(claude::args(&turn));
            claude::parse_line
        }
        Provider::Gemini => {
            cmd.args(gemini::args(&turn));
            gemini::parse_line
        }
    };
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
    // Dropping stdin afterwards tells the CLI the prompt is complete.
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
        for line in BufReader::new(stdout).split(b'\n') {
            let Ok(line) = line else { break };
            for event in parse_line(&String::from_utf8_lossy(&line)) {
                on_event(event);
            }
        }

        let status = wait(&waiter);
        let stderr = stderr_reader.join().unwrap_or_default();
        let error = match status {
            Ok(status) if status.success() => None,
            Ok(status) => Some(exit_error(provider, status.code(), &stderr)),
            Err(err) => Some(format!("Lost track of {}: {err}", provider.label())),
        };
        on_event(AgentEvent::Exited { error });
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

/// Turns a failed CLI run into a message the user can act on.
fn exit_error(provider: Provider, code: Option<i32>, stderr: &str) -> String {
    // Gemini CLI exits with 41 when it can't authenticate.
    const GEMINI_AUTH_FAILED: i32 = 41;

    let details = error_summary(stderr);
    if provider == Provider::Gemini && code == Some(GEMINI_AUTH_FAILED) {
        return format!(
            "Gemini CLI isn't signed in. Open Settings, click \"Run `gemini` in the terminal\" and choose \
             \"Sign in with Google\". Gemini said: {details}"
        );
    }
    match code {
        Some(code) => format!("{} exited with code {code}. {details}", provider.label()),
        None => format!("{} stopped unexpectedly. {details}", provider.label()),
    }
}

/// The useful part of a CLI's error output: the last message lines, without
/// stack traces or terminal color codes.
fn error_summary(stderr: &str) -> String {
    let mut plain = String::new();
    let mut chars = stderr.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip an escape sequence such as "\x1b[31m".
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }

    let lines: Vec<&str> = plain
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("at ") && !line.starts_with("exitCode"))
        .filter(|line| !matches!(*line, "{" | "}"))
        .collect();
    lines[lines.len().saturating_sub(2)..].join(" ")
}

/// Looks for the first of `names` in the folders on PATH.
pub fn find_on_path(names: &[&str]) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Where npm puts globally installed commands on Windows.
pub fn npm_global_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|app_data| Path::new(&app_data).join("npm"))
}

pub fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// A one-line summary of a tool call, such as the command or file it touches.
pub fn tool_detail(input: &Value) -> String {
    const KEYS: [&str; 9] =
        ["command", "file_path", "absolute_path", "path", "dir_path", "pattern", "url", "query", "description"];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_detail_prefers_the_most_useful_field() {
        let input = serde_json::json!({"command": "ls -la\ncd src", "description": "List files"});
        assert_eq!(tool_detail(&input), "ls -la…");
        assert_eq!(tool_detail(&serde_json::json!({"dir_path": "src"})), "src");
        assert_eq!(tool_detail(&serde_json::json!({"x": 1})), r#"{"x":1}"#);
    }

    /// Runs the installed Gemini CLI end to end. It needs Gemini CLI and a network
    /// connection, so it only runs when asked for:
    /// `GEMINI_API_KEY=invalid cargo test -- --ignored gemini`
    /// Run it with a temporary USERPROFILE/HOME so your real Gemini settings aren't used.
    #[test]
    #[ignore]
    fn runs_the_real_gemini_cli() {
        let launcher = Provider::Gemini.find().expect("Gemini CLI should be installed");
        let (tx, rx) = std::sync::mpsc::channel();
        let turn = Turn {
            prompt: "Reply with exactly: a & b | c".into(),
            cwd: std::env::temp_dir(),
            resume_session: None,
            permission_mode: PermissionMode::ReadOnly,
        };
        let _running = start_turn(Provider::Gemini, &launcher, turn, move |event| {
            let _ = tx.send(event);
        })
        .expect("Gemini CLI should start");

        let events: Vec<AgentEvent> = rx.iter().take_while(|e| !matches!(e, AgentEvent::Exited { .. })).collect();
        assert!(matches!(events.first(), Some(AgentEvent::Started { .. })), "{events:?}");
        assert!(matches!(events.last(), Some(AgentEvent::Finished { .. })), "{events:?}");
        println!("{events:#?}");
    }

    #[test]
    fn gemini_sign_in_failures_explain_the_fix() {
        let error = exit_error(Provider::Gemini, Some(41), "Invalid auth method selected.\n");
        assert!(error.starts_with("Gemini CLI isn't signed in."), "{error}");
        assert!(error.ends_with("Gemini said: Invalid auth method selected."), "{error}");
    }

    #[test]
    fn error_summary_drops_stack_traces_and_colors() {
        let stderr = "Error authenticating: boom\n    at initOauthClient (file:///x.js:1:2)\n  exitCode: 41\n}\n\x1b[31mManual authorization is required.\x1b[0m\n";
        assert_eq!(error_summary(stderr), "Error authenticating: boom Manual authorization is required.");
        assert_eq!(exit_error(Provider::Claude, Some(1), ""), "Claude Code exited with code 1. ");
    }
}
