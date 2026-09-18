//! The parts every agent CLI shares: which providers exist, the events the UI
//! understands, and running one turn as a background process.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::line_diff::FileEdit;
use crate::plan::PlanUsage;
use crate::usage::Usage;
use crate::{antigravity, claude, codex};

/// An agent CLI that can act as the brain of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    Claude,
    Codex,
    /// Sessions saved while Gemini CLI was an option now use Antigravity.
    #[serde(alias = "Gemini")]
    Antigravity,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Self::Claude, Self::Codex, Self::Antigravity];

    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity (agy)",
        }
    }

    /// The short name used in sentences like "Claude is working…".
    pub fn short_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::Antigravity => "Antigravity",
        }
    }

    /// The command that starts the CLI interactively, e.g. to sign in.
    pub fn command(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "agy",
        }
    }

    pub fn install_hint(self) -> &'static str {
        match self {
            Self::Claude => "Install it from https://claude.com/claude-code",
            Self::Codex => "Install it from https://developers.openai.com/codex/cli",
            Self::Antigravity => "Install Google Antigravity, which includes the agy command",
        }
    }

    /// Finds this CLI's executable on this computer.
    pub fn find(self) -> Option<PathBuf> {
        match self {
            Self::Claude => claude::find_executable(),
            Self::Codex => codex::find_executable(),
            Self::Antigravity => antigravity::find_executable(),
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
    ToolUse {
        name: String,
        detail: String,
        /// What the tool is about to change, when it says enough for a diff.
        edit: Option<FileEdit>,
    },
    ToolResult { text: String, is_error: bool },
    /// What the CLI says about the account's plan limits, which belongs to the
    /// provider rather than to this session.
    Plan(PlanUsage),
    /// The agent finished the turn.
    Finished {
        session_id: Option<String>,
        error: Option<String>,
        denied_tools: Vec<String>,
        /// Tokens the turn used, if the CLI reported them.
        usage: Option<Usage>,
    },
    /// The CLI process ended. `error` is set if it didn't exit cleanly.
    Exited { error: Option<String> },
}

/// What the agent is allowed to do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PermissionMode {
    /// The default, because a save that has lost this field must not come back
    /// granting more than the user had chosen.
    #[default]
    ReadOnly,
    AcceptEdits,
    /// Everything the agent asks for is allowed, including running commands.
    Full,
    Plan,
}

// Permission mode descriptors retained for slash command expansions (/permissions, /mode).
#[allow(dead_code)]
impl PermissionMode {
    /// In the order they appear in the picker, from least to most it may do.
    pub const ALL: [PermissionMode; 4] = [Self::Plan, Self::ReadOnly, Self::AcceptEdits, Self::Full];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read only",
            Self::AcceptEdits => "Can edit files",
            Self::Full => "Full access",
            Self::Plan => "Plan only",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ReadOnly => "The agent can read the project but not change it or run commands.",
            Self::AcceptEdits => "The agent can edit files. Running commands still needs approval, \
                                  which a headless agent can't be asked for, so it gets refused.",
            Self::Full => "The agent may edit files and run any command without asking. Use it only \
                           in folders you trust.",
            Self::Plan => "The agent works out a plan and doesn't change anything.",
        }
    }

    /// The word this mode is asked for by in `/permission full` and the like.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::ReadOnly => "read",
            Self::AcceptEdits => "edit",
            Self::Full => "full",
            Self::Plan => "plan",
        }
    }

    /// True for the mode that gives up the safety net, so the UI can say so.
    pub fn is_risky(self) -> bool {
        self == Self::Full
    }
}

pub struct Turn {
    pub prompt: String,
    pub cwd: PathBuf,
    pub resume_session: Option<String>,
    pub permission_mode: PermissionMode,
    /// The model the session asked for, or None for the CLI's own default.
    pub model: Option<String>,
    /// How hard the model should work, in the provider's own words, e.g. "high".
    pub effort: Option<String>,
}

/// A turn whose CLI process is still running.
pub struct RunningTurn {
    child: Arc<Mutex<Child>>,
    /// Everything the CLI starts, such as commands its tools run.
    tree: ProcessTree,
}

impl RunningTurn {
    fn new(child: Child) -> Self {
        let tree = ProcessTree::new(&child);
        Self { child: Arc::new(Mutex::new(child)), tree }
    }

    /// Stops the CLI and every program it started.
    pub fn stop(&self) {
        self.tree.kill();
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = child.kill();
    }
}

impl Drop for RunningTurn {
    // Deleting a session or closing the app mid-turn shouldn't leave the agent, or anything
    // it started, running in the background. A turn that already finished is left alone,
    // since the agent may have started something on purpose, like a dev server.
    fn drop(&mut self) {
        let finished = matches!(self.child.lock().unwrap_or_else(PoisonError::into_inner).try_wait(), Ok(Some(_)));
        if !finished {
            self.stop();
        }
    }
}

/// On Windows, killing a process leaves the programs it started running, so the
/// CLI goes into a job object: programs it starts join the same job, and the whole
/// job can be ended at once.
#[cfg(windows)]
struct ProcessTree(Option<windows::Win32::Foundation::HANDLE>);

// The job handle is only used to end the job or close it, which is safe from any thread.
#[cfg(windows)]
unsafe impl Send for ProcessTree {}
#[cfg(windows)]
unsafe impl Sync for ProcessTree {}

#[cfg(windows)]
impl ProcessTree {
    fn new(child: &Child) -> Self {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let job = unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.ok();
        let job = job.filter(|job| {
            // Configure the job so that if Barduino terminates or crashes,
            // the Windows kernel automatically terminates all child processes in the job.
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    *job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            }
            .is_ok();
            if !configured {
                let _ = unsafe { CloseHandle(*job) };
                return false;
            }

            let assigned = unsafe { AssignProcessToJobObject(*job, HANDLE(child.as_raw_handle())) }.is_ok();
            if !assigned {
                let _ = unsafe { CloseHandle(*job) };
            }
            assigned
        });
        Self(job)
    }

    fn kill(&self) {
        if let Some(job) = self.0 {
            let _ = unsafe { windows::Win32::System::JobObjects::TerminateJobObject(job, 1) };
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        if let Some(job) = self.0.take() {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(job) };
        }
    }
}

/// On Unix, child processes join a process group so stopping the turn terminates
/// the CLI and any subprocesses (test runners, build tools) it spawned.
#[cfg(unix)]
struct ProcessTree(u32);

#[cfg(unix)]
impl ProcessTree {
    fn new(child: &Child) -> Self {
        Self(child.id())
    }

    fn kill(&self) {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        let pgid = self.0 as i32;
        unsafe {
            let _ = kill(-pgid, 15); // SIGTERM
            let _ = kill(-pgid, 9);  // SIGKILL
        }
    }
}

#[cfg(not(any(windows, unix)))]
struct ProcessTree;

#[cfg(not(any(windows, unix)))]
impl ProcessTree {
    fn new(_child: &Child) -> Self {
        Self
    }

    fn kill(&self) {}
}

/// Starts one turn of the conversation. `on_event` is called from a background
/// thread for each event.
pub fn start_turn(
    provider: Provider,
    exe: &Path,
    turn: Turn,
    on_event: impl Fn(AgentEvent) + Send + 'static,
) -> std::io::Result<RunningTurn> {
    let mut cmd = hidden_command(exe);
    let parse_line: fn(&str) -> Vec<AgentEvent> = match provider {
        Provider::Claude => {
            cmd.args(claude::args(&turn));
            claude::parse_line
        }
        Provider::Codex => {
            cmd.args(codex::args(&turn));
            codex::parse_line
        }
        Provider::Antigravity => {
            cmd.args(antigravity::args(&turn));
            antigravity::parse_line
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.current_dir(&turn.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let running = RunningTurn::new(child);

    // Claude reads the prompt from stdin, which avoids command-line quoting and length
    // limits. Dropping stdin afterwards tells the CLI the prompt is complete.
    let prompt = turn.prompt;
    thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });

    let stderr_buf = Arc::new(Mutex::new(Vec::new()));
    let stderr_clone = Arc::clone(&stderr_buf);
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match stderr.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let mut data = stderr_clone.lock().unwrap_or_else(PoisonError::into_inner);
                    data.extend_from_slice(&buf[..n]);
                }
                Err(_) => break,
            }
        }
    });

    let (event_tx, event_rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).split(b'\n') {
            let Ok(line) = line else { break };
            for event in parse_line(&String::from_utf8_lossy(&line)) {
                if event_tx.send(event).is_err() {
                    return;
                }
            }
        }
    });

    let waiter = Arc::clone(&running.child);
    let process_waiter = thread::spawn(move || wait_process(&waiter));

    thread::spawn(move || {
        loop {
            match event_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(event) => on_event(event),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // When the agent CLI exits, don't wait forever on stdout EOF
                    // if an inherited pipe handle is kept open by a grandchild process.
                    if process_waiter.is_finished() {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        let status = process_waiter.join().unwrap_or_else(|_| {
            Err(std::io::Error::other("Process waiter panicked"))
        });

        // Drain any lingering events buffered right before the process exited.
        while let Ok(event) = event_rx.try_recv() {
            on_event(event);
        }

        let stderr = {
            let data = stderr_buf.lock().unwrap_or_else(PoisonError::into_inner);
            String::from_utf8_lossy(&data).into_owned()
        };
        let error = match status {
            Ok(status) if status.success() => None,
            Ok(status) => Some(exit_error(provider, status.code(), &stderr)),
            Err(err) => Some(format!("Lost track of {}: {err}", provider.label())),
        };
        on_event(AgentEvent::Exited { error });
    });

    Ok(running)
}

/// A command for a background program that doesn't flash a console window on Windows.
pub fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Waits synchronously for the child process to exit.
fn wait_process(child: &Mutex<Child>) -> std::io::Result<ExitStatus> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        unsafe extern "system" {
            fn WaitForSingleObject(hHandle: HANDLE, dwMilliseconds: u32) -> u32;
        }
        let raw_handle = {
            let guard = child.lock().unwrap_or_else(PoisonError::into_inner);
            HANDLE(guard.as_raw_handle())
        };
        unsafe {
            WaitForSingleObject(raw_handle, 0xFFFF_FFFF);
        }
        child.lock().unwrap_or_else(PoisonError::into_inner).try_wait()?.ok_or_else(|| {
            std::io::Error::other("Process wait finished but status unavailable")
        })
    }
    #[cfg(not(windows))]
    {
        loop {
            let status = child.lock().unwrap_or_else(PoisonError::into_inner).try_wait()?;
            if let Some(status) = status {
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Turns a failed CLI run into a message the user can act on.
fn exit_error(provider: Provider, code: Option<i32>, stderr: &str) -> String {
    let details = error_summary(stderr);
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

pub fn string(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// A one-line summary of a tool call, such as the command or file it touches.
pub fn tool_detail(input: &Value) -> String {
    const KEYS: [&str; 10] = [
        "command", "file_path", "absolute_path", "path", "dir_path", "pattern", "url", "query", "description",
        "plan",
    ];
    let raw = KEYS
        .iter()
        .find_map(|key| input[*key].as_str())
        // Any string it does have, rather than the object printed as JSON. A tool
        // with none of these keys used to fill the row with `{"todos":[{"content…`.
        .or_else(|| input.as_object()?.values().find_map(Value::as_str))
        .unwrap_or_default();
    first_line(raw, 160)
}

/// A to-do list or plan, as the checklist it is rather than the JSON it arrives in.
/// The first line says how far along the work is; the rest is the list.
///
/// Deliberately tolerant about which keys carry the text and the state, because
/// all three CLIs report a plan and none of them agrees on the shape. Anything it
/// can't make sense of returns `None`, so the caller falls back rather than showing
/// something wrong.
pub fn checklist(items: &Value) -> Option<String> {
    const LABELS: [&str; 5] = ["content", "text", "title", "step", "description"];
    const STATES: [&str; 3] = ["status", "state", "stage"];

    let items = items.as_array().filter(|items| !items.is_empty())?;
    let state = |item: &Value| {
        STATES.iter().find_map(|key| item[*key].as_str()).unwrap_or_default().to_lowercase()
    };
    let label = |item: &Value| {
        LABELS.iter().find_map(|key| item[*key].as_str()).map(str::to_owned)
            // A plain list of strings is a plan too.
            .or_else(|| item.as_str().map(str::to_owned))
    };
    // If none of them carries text, this isn't a checklist and guessing would only
    // produce a column of empty bullets.
    let labelled: Vec<String> = items.iter().filter_map(label).collect();
    if labelled.len() != items.len() {
        return None;
    }

    let done = items.iter().filter(|item| state(item).starts_with("complet")).count();
    let mut lines = vec![format!("{done} of {} done", items.len())];
    lines.extend(items.iter().zip(labelled).map(|(item, text)| {
        let state = state(item);
        let mark = if state.starts_with("complet") {
            "✓"
        } else if state.contains("progress") || state.starts_with("active") || state.starts_with("running") {
            "▸"
        } else {
            "·"
        };
        format!("{mark} {}", first_line(&text, 120))
    }));
    Some(lines.join("\n"))
}

/// The first line of some text, cut to `max` characters, with an ellipsis when
/// anything was left out.
fn first_line(raw: &str, max: usize) -> String {
    let mut detail: String = raw.lines().next().unwrap_or_default().trim_end().chars().take(max).collect();
    if detail.chars().count() < raw.trim_end().chars().count() {
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
        // A tool with none of the expected keys used to print its whole input as
        // JSON into the transcript. Any string it does carry is worth more, and
        // nothing at all is worth more than a brace.
        assert_eq!(tool_detail(&serde_json::json!({"shell_id": "bash_3"})), "bash_3");
        assert_eq!(tool_detail(&serde_json::json!({"x": 1})), "");
        // Long input is cut on a character, not in the middle of one.
        let wide = serde_json::json!({"command": "é".repeat(200)});
        assert_eq!(tool_detail(&wide).chars().count(), 161, "160 characters and the ellipsis");
    }

    #[test]
    fn a_plan_reads_as_a_checklist_whichever_cli_sent_it() {
        // Claude's shape.
        let claude = serde_json::json!([
            {"content": "Restore the pickers", "status": "completed", "activeForm": "Restoring the pickers"},
            {"content": "Add the context readout", "status": "in_progress"},
            {"content": "Write it up", "status": "pending"},
        ]);
        let lines: Vec<String> = checklist(&claude).expect("a checklist").lines().map(str::to_owned).collect();
        assert_eq!(lines[0], "1 of 3 done");
        assert_eq!(lines[1], "✓ Restore the pickers");
        assert_eq!(lines[2], "▸ Add the context readout");
        assert_eq!(lines[3], "· Write it up");

        // A CLI that calls the same things something else still reads right.
        let other = serde_json::json!([{"text": "One", "state": "COMPLETED"}, {"text": "Two", "state": "active"}]);
        assert_eq!(checklist(&other).expect("a checklist"), "1 of 2 done\n✓ One\n▸ Two");
        // And a plain list of strings is a plan too.
        assert_eq!(checklist(&serde_json::json!(["One", "Two"])).expect("a list"), "0 of 2 done\n· One\n· Two");

        // Shapes it can't read fall back rather than showing a column of bullets.
        assert_eq!(checklist(&serde_json::json!([])), None, "an empty list is not a plan");
        assert_eq!(checklist(&serde_json::json!([{"id": 1}])), None, "nothing to label the row with");
        assert_eq!(checklist(&serde_json::json!({"todos": []})), None, "not a list at all");
    }

    fn run_turn(provider: Provider, turn: Turn) -> Vec<AgentEvent> {
        let exe = provider.find().expect("the CLI should be installed");
        let (tx, rx) = std::sync::mpsc::channel();
        let _running = start_turn(provider, &exe, turn, move |event| {
            let _ = tx.send(event);
        })
        .expect("the CLI should start");
        rx.iter().take_while(|e| !matches!(e, AgentEvent::Exited { .. })).collect()
    }

    /// Runs the installed agy twice, the second time continuing the first
    /// conversation. It uses the Antigravity account, so it only runs when asked for:
    /// `cargo test -- --ignored antigravity --nocapture`
    #[test]
    #[ignore]
    fn runs_the_real_antigravity_cli() {
        let dir = std::env::temp_dir().join("barduino-agy-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("note.txt"), "the secret word is pineapple\n").unwrap();

        let first = run_turn(
            Provider::Antigravity,
            Turn {
                prompt: "Read note.txt and reply with only the secret word.".into(),
                cwd: dir.clone(),
                resume_session: None,
                permission_mode: PermissionMode::ReadOnly,
                model: None,
                effort: None,
            },
        );
        println!("{first:#?}");
        let Some(AgentEvent::Finished { session_id: Some(conversation), error: None, usage: Some(usage), .. }) =
            first.last()
        else {
            panic!("the first turn should finish cleanly and report usage");
        };
        assert!(usage.input > 0 && usage.output > 0, "{usage:?}");

        let second = run_turn(
            Provider::Antigravity,
            Turn {
                prompt: "What word did you just reply with? Answer in upper case, nothing else.".into(),
                cwd: dir,
                resume_session: Some(conversation.clone()),
                permission_mode: PermissionMode::ReadOnly,
                model: None,
                effort: None,
            },
        );
        println!("{second:#?}");
        let reply: String =
            second.iter().filter_map(|e| if let AgentEvent::TextDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert!(reply.contains("PINEAPPLE"), "the second turn should remember the first: {reply:?}");
    }

    /// Stands in for an agent whose tool started a long-running program, then
    /// checks that stopping the turn stops that program too.
    #[cfg(windows)]
    #[test]
    fn stopping_a_turn_stops_programs_the_agent_started() {
        // An unusual ping count makes the process easy to find.
        const MARKER: &str = "-n 4747";
        let find = || {
            let script = format!(
                "Get-CimInstance Win32_Process | Where-Object {{ $_.Name -eq 'PING.EXE' -and $_.CommandLine -like '*{MARKER}*' }} | ForEach-Object ProcessId"
            );
            let output = hidden_command("powershell.exe").args(["-NoProfile", "-Command", &script]).output().unwrap();
            String::from_utf8_lossy(&output.stdout).split_whitespace().map(str::to_owned).collect::<Vec<_>>()
        };

        let child = hidden_command("powershell.exe")
            .args(["-NoProfile", "-Command", &format!("ping {MARKER} 127.0.0.1")])
            .spawn()
            .unwrap();
        let turn = RunningTurn::new(child);
        let mut started = Vec::new();
        for _ in 0..40 {
            started = find();
            if !started.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
        assert!(!started.is_empty(), "the program should have started");

        turn.stop();
        thread::sleep(Duration::from_secs(1));
        let left = find();
        for pid in &left {
            let _ = Command::new("taskkill").args(["/F", "/PID", pid]).output();
        }
        assert!(left.is_empty(), "still running after Stop: {left:?}");
    }

    /// Checks that full access really lets a CLI run a command, which is what the
    /// other modes refuse in headless mode. It spends tokens, so it only runs when
    /// asked for: `cargo test -- --ignored full_access --nocapture`
    #[test]
    #[ignore]
    fn full_access_really_runs_commands() {
        for provider in [Provider::Antigravity, Provider::Codex] {
            let Some(exe) = provider.find() else {
                panic!("{} should be installed", provider.label());
            };
            let (tx, rx) = std::sync::mpsc::channel();
            let turn = Turn {
                prompt: "Run the shell command `echo barduino-full-access` and reply with its output only.".into(),
                cwd: std::env::temp_dir(),
                resume_session: None,
                permission_mode: PermissionMode::Full,
                model: None,
                effort: None,
            };
            let _running = start_turn(provider, &exe, turn, move |event| {
                let _ = tx.send(event);
            })
            .expect("the CLI should start");
            let events: Vec<AgentEvent> = rx.iter().collect();
            println!("=== {} ===
{events:#?}", provider.label());

            let denied: Vec<&AgentEvent> = events
                .iter()
                .filter(|event| matches!(event, AgentEvent::Finished { denied_tools, .. } if !denied_tools.is_empty()))
                .collect();
            assert!(denied.is_empty(), "{} refused something: {denied:?}", provider.label());
            let ran = events.iter().any(|event| matches!(event, AgentEvent::ToolUse { .. }));
            assert!(ran, "{} should have run the command", provider.label());
        }
    }

    #[test]
    fn saved_gemini_sessions_load_as_antigravity() {
        let provider: Provider = serde_json::from_str(r#""Gemini""#).unwrap();
        assert_eq!(provider, Provider::Antigravity);
    }

    #[test]
    fn error_summary_drops_stack_traces_and_colors() {
        let stderr = "Error authenticating: boom\n    at initOauthClient (file:///x.js:1:2)\n  exitCode: 41\n}\n\x1b[31mManual authorization is required.\x1b[0m\n";
        assert_eq!(error_summary(stderr), "Error authenticating: boom Manual authorization is required.");
        assert_eq!(exit_error(Provider::Claude, Some(1), ""), "Claude Code exited with code 1. ");
    }

    #[test]
    fn process_exit_cleanly_emits_exited_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let cmd_path = if cfg!(windows) {
            PathBuf::from("cmd.exe")
        } else {
            PathBuf::from("true")
        };
        let turn = Turn {
            prompt: "test".into(),
            cwd: std::env::temp_dir(),
            resume_session: None,
            permission_mode: PermissionMode::ReadOnly,
            model: None,
            effort: None,
        };
        let running = start_turn(Provider::Antigravity, &cmd_path, turn, move |event| {
            let _ = tx.send(event);
        });
        assert!(running.is_ok(), "process should spawn");
        let events: Vec<AgentEvent> = rx.iter().collect();
        let exited = events.iter().any(|e| matches!(e, AgentEvent::Exited { .. }));
        assert!(exited, "turn should exit and emit Exited event");
    }
}
