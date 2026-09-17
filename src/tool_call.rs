//! Turning a tool call into a line a person reads.
//!
//! Each CLI names its tools in its own vocabulary. Running a command is `Bash` to
//! Claude, `run_command` to Antigravity and `command_execution` to Codex; reading a
//! file is `Read`, `view_file` or a shell call. Left alone, the transcript shows
//! whichever word that CLI happened to use, so the same session reads differently
//! depending on who answered it. This maps all three onto one small set of
//! families, each with a verb.
//!
//! Everything here works from the tool's name and its detail string — both of
//! which are saved with the session. That is deliberate: conversations already on
//! disk read as well as new ones, without a migration.
//!
//! No `egui` in this module. What a family *looks like* is the view's business;
//! what a tool *is* can then be tested without a window.

use std::path::Path;

/// The family a tool belongs to, whichever CLI named it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    /// Reading a file or listing a folder. Nothing changes.
    Read,
    /// Changing a file.
    Edit,
    /// Running a command.
    Run,
    /// Looking for something, by name or by content.
    Search,
    /// Reaching the network.
    Web,
    /// Writing down a plan or a to-do list.
    Plan,
    /// Handing work to another agent.
    Delegate,
}

/// What a tool call is, in one word each: which family it belongs to, and the verb
/// the transcript announces it with.
///
/// `None` for a tool none of the three CLIs is known to have — an MCP server's, or
/// one added since this was written. The caller shows the tool's own name then,
/// which is worth more than a wrong guess.
pub fn describe(name: &str) -> Option<(ToolKind, &'static str)> {
    // Matched on a flattened name so that one arm covers every CLI's spelling:
    // `view_file`, `ViewFile` and `viewfile` are the same tool.
    let flat: String = name.chars().filter(|ch| ch.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    let described = match flat.as_str() {
        "read" | "readfile" | "viewfile" | "viewcodeitem" | "viewfileoutline" | "notebookread" => {
            (ToolKind::Read, "Read")
        }
        "listdir" | "ls" | "listdirectory" => (ToolKind::Read, "Listed"),
        "write" | "writefile" | "createfile" => (ToolKind::Edit, "Wrote"),
        "edit" | "multiedit" | "editfile" | "notebookedit" | "replacefilecontent" | "filechange"
        | "applypatch" | "strreplace" => (ToolKind::Edit, "Edited"),
        "bash" | "shell" | "runcommand" | "commandexecution" | "runterminalcommand" => (ToolKind::Run, "Ran"),
        "bashoutput" => (ToolKind::Run, "Read output of"),
        "killshell" | "killbash" => (ToolKind::Run, "Stopped"),
        "glob" | "findbyname" | "fileglob" => (ToolKind::Search, "Found"),
        "grep" | "grepsearch" | "search" | "codebasesearch" | "semanticsearch" => (ToolKind::Search, "Searched"),
        "webfetch" | "fetch" | "readurlcontent" => (ToolKind::Web, "Fetched"),
        "websearch" | "searchweb" => (ToolKind::Web, "Searched the web for"),
        "todowrite" | "todolist" | "updateplan" | "plan" => (ToolKind::Plan, "Planned"),
        "exitplanmode" => (ToolKind::Plan, "Proposed a plan"),
        "task" | "agent" | "subagent" | "spawnagent" => (ToolKind::Delegate, "Handed off to"),
        _ => return None,
    };
    Some(described)
}

/// A path as the transcript should show it: relative to the project when it is
/// inside it, with forward slashes whichever platform wrote it, and shortened from
/// the front when it is still too long — the end of a path is the part that says
/// which file it is.
/// Anything that is not a path under the project is returned untouched, so this is
/// safe to run over a whole detail line: a regex like `\d+` and a phrase like
/// "lines 40–90" come back exactly as they went in.
pub fn short_path(path: &str, project_dir: &Path, max: usize) -> String {
    let shown = match inside_project(path, project_dir) {
        // Below the project, written the way a person would write it.
        Some(rest) => rest.replace('\\', "/"),
        None => path.to_owned(),
    };
    if shown.chars().count() <= max {
        return shown;
    }
    // Keep the tail: the end of a path is the part that says which file it is. A
    // leading "…/" says plainly that something was cut.
    let tail: String = shown.chars().skip(shown.chars().count() - max.saturating_sub(2)).collect();
    format!("…/{}", tail.trim_start_matches(['/', '\\']))
}

/// The part of a path below the project folder, or `None` if it isn't under it.
fn inside_project(path: &str, project_dir: &Path) -> Option<String> {
    if project_dir.as_os_str().is_empty() {
        return None;
    }
    if let Ok(rest) = Path::new(path).strip_prefix(project_dir) {
        return Some(rest.display().to_string());
    }
    // Windows hands back whatever casing the CLI used, which is not always the
    // casing the folder was opened with, and strip_prefix compares exactly.
    let prefix = project_dir.display().to_string().to_lowercase();
    let rest = path.to_lowercase().strip_prefix(&prefix)?.len();
    Some(path[path.len() - rest..].trim_start_matches(['/', '\\']).to_owned())
}

/// The shells a CLI wraps a command in. Anything before the flag has to be one of
/// these for the wrapper to be stripped, so an `echo -c hello` is left alone.
const SHELLS: [&str; 7] = ["powershell", "pwsh", "bash", "sh", "zsh", "cmd", "command"];

/// The command a shell was actually asked to run, without the wrapper the CLI put
/// around it.
///
/// Codex sends the whole line it spawned — most of which is the absolute path to
/// `powershell.exe` — and showing that instead of the command is the difference
/// between a readable transcript and a wall of `C:\WINDOWS\System32\…`.
pub fn bare_command(command: &str) -> &str {
    let trimmed = command.trim();
    for flag in ["-Command", "-EncodedCommand", "-lc", "-c", "/c", "/C"] {
        let Some(at) = trimmed.find(flag) else { continue };
        let (head, tail) = trimmed.split_at(at);
        // The flag has to be a word of its own, not the tail of a longer one.
        if !tail[flag.len()..].starts_with([' ', '\t']) {
            continue;
        }
        let launcher: String = head
            .trim()
            .trim_end_matches(['"', '\''])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .trim_end_matches(".exe")
            .to_lowercase();
        if !SHELLS.contains(&launcher.as_str()) {
            continue;
        }
        // An encoded command is base64 and says nothing, so the wrapper stays.
        if flag == "-EncodedCommand" {
            return trimmed;
        }
        return unquote(tail[flag.len()..].trim());
    }
    trimmed
}

/// Strips one layer of matching quotes, which is how a wrapped command arrives.
fn unquote(text: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = text.strip_prefix(quote).and_then(|rest| rest.strip_suffix(quote)) {
            return inner;
        }
    }
    text
}

/// Splits a tool's detail into the phrase that goes on its own row and the lines
/// that belong underneath it.
///
/// A to-do list is why this exists: its first line says how far along the work is,
/// and the rest is the list itself.
pub fn split_detail(detail: &str) -> (&str, Vec<&str>) {
    let mut lines = detail.lines();
    let first = lines.next().unwrap_or_default();
    (first, lines.filter(|line| !line.trim().is_empty()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_job_reads_the_same_whichever_cli_did_it() {
        // Running a command, as each of the three names it.
        for name in ["Bash", "run_command", "command_execution", "Shell"] {
            assert_eq!(describe(name).map(|(kind, _)| kind), Some(ToolKind::Run), "{name}");
        }
        // Reading a file.
        for name in ["Read", "view_file", "ViewFile"] {
            assert_eq!(describe(name), Some((ToolKind::Read, "Read")), "{name}");
        }
        // Editing one. Codex's file_change and Antigravity's replace_file_content
        // are the same act as Claude's Edit.
        for name in ["Edit", "MultiEdit", "file_change", "replace_file_content"] {
            assert_eq!(describe(name), Some((ToolKind::Edit, "Edited")), "{name}");
        }
        // A write is an edit with nothing on the old side, but it reads better as
        // its own verb.
        assert_eq!(describe("Write"), Some((ToolKind::Edit, "Wrote")));

        // A tool nobody here has heard of — an MCP server's, say — keeps its own
        // name rather than being guessed at.
        assert_eq!(describe("mcp__linear__create_issue"), None);
        assert_eq!(describe(""), None);
    }

    #[test]
    fn a_path_is_shown_relative_to_the_project() {
        let project = Path::new(r"C:\Users\ditob\Documents\Barduino");
        assert_eq!(short_path(r"C:\Users\ditob\Documents\Barduino\src\app.rs", project, 40), "src/app.rs");
        // The casing a CLI reports isn't always the casing the folder was opened
        // with, and on Windows both name the same file.
        assert_eq!(short_path(r"c:\users\ditob\documents\barduino\src\app.rs", project, 40), "src/app.rs");
        // Outside the project it is left exactly as it came: that it is somewhere
        // else is the point, and it is not ours to tidy.
        let elsewhere = short_path(r"C:\Windows\System32\drivers\etc\hosts", project, 60);
        assert_eq!(elsewhere, r"C:\Windows\System32\drivers\etc\hosts");
        // Which means this is safe to run over a detail that holds no path at all.
        assert_eq!(short_path(r"\bfn\s+parse\b", project, 40), r"\bfn\s+parse\b", "a regex is not a path");
        assert_eq!(short_path("lines 40–90", project, 40), "lines 40–90");
        // A session with no folder yet can't have anything under it.
        assert_eq!(short_path(r"C:\x\y.rs", Path::new(""), 40), r"C:\x\y.rs");
        // Too long to fit is cut at the front, because the end names the file.
        let cut = short_path(r"C:\Users\ditob\Documents\Barduino\src\very\deep\nested\module.rs", project, 20);
        assert!(cut.ends_with("module.rs"), "{cut}");
        assert!(cut.starts_with('…'), "{cut}");
        assert!(cut.chars().count() <= 21, "{cut} is {} chars", cut.chars().count());
    }

    /// The exact line Codex reported in `testdata/codex_edit_file.jsonl`: nearly all
    /// of it is the path to PowerShell, and none of that is what the agent did.
    #[test]
    fn a_wrapped_command_is_shown_as_the_command() {
        let wrapped = r#""C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe" -Command 'Get-Content -LiteralPath notes.txt'"#;
        assert_eq!(bare_command(wrapped), "Get-Content -LiteralPath notes.txt");
        assert_eq!(bare_command(r#"bash -lc "cargo test""#), "cargo test");
        assert_eq!(bare_command("cmd.exe /c dir"), "dir");
        assert_eq!(bare_command("/usr/bin/sh -c 'ls -la'"), "ls -la");

        // A plain command is left exactly alone.
        assert_eq!(bare_command("cargo clippy --all-targets"), "cargo clippy --all-targets");
        // And so is one whose own arguments happen to look like a shell's.
        assert_eq!(bare_command("echo -c hello"), "echo -c hello");
        assert_eq!(bare_command("gcc -c main.c"), "gcc -c main.c");
        // Base64 says nothing to a reader, so the wrapper is more use than its body.
        let encoded = "powershell.exe -EncodedCommand ZwBjAG0A";
        assert_eq!(bare_command(encoded), encoded);
    }

    #[test]
    fn a_detail_carries_its_list_below_the_row() {
        let (phrase, body) = split_detail("2 of 3 done\n✓ First\n▸ Second\n· Third");
        assert_eq!(phrase, "2 of 3 done");
        assert_eq!(body, ["✓ First", "▸ Second", "· Third"]);

        // The ordinary case: one line, nothing underneath.
        let (phrase, body) = split_detail("src/app.rs");
        assert_eq!(phrase, "src/app.rs");
        assert!(body.is_empty());
        assert_eq!(split_detail("").0, "");
    }
}
