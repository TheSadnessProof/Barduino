// Empirical test harness for provider argument permutations and batch file detection.
// Tests every combination of Provider x PermissionMode x Model x Effort x ResumeID,
// plus boundary conditions (spaces, unicode, long paths, special characters).

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Codex,
    Antigravity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionMode {
    ReadOnly,
    AcceptEdits,
    Full,
    Plan,
}

// Logic copied verbatim from src/claude.rs:67-100
pub fn claude_interactive_args(
    _cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mut args = Vec::new();
    match permission_mode {
        PermissionMode::ReadOnly => {
            args.extend(["--permission-mode".to_owned(), "default".to_owned()]);
            args.extend(["--disallowed-tools".to_owned(), "Bash".to_owned()]);
        }
        PermissionMode::AcceptEdits => {
            args.extend(["--permission-mode".to_owned(), "acceptEdits".to_owned()]);
        }
        PermissionMode::Full => {
            args.push("--dangerously-skip-permissions".to_owned());
        }
        PermissionMode::Plan => {
            args.extend(["--permission-mode".to_owned(), "plan".to_owned()]);
        }
    }
    if let Some(model) = model {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = effort {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    if let Some(session_id) = resume_id {
        args.extend(["--resume".to_owned(), session_id.to_owned()]);
    }
    args
}

// Logic copied verbatim from src/codex.rs:80-110
pub fn codex_interactive_args(
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(session_id) = resume_id {
        args.extend(["resume".to_owned(), session_id.to_owned()]);
    }
    args.extend(["-C".to_owned(), cwd.display().to_string()]);
    match permission_mode {
        PermissionMode::ReadOnly | PermissionMode::Plan => {
            args.extend(["-s".to_owned(), "read-only".to_owned()]);
        }
        PermissionMode::AcceptEdits => {
            args.extend(["-s".to_owned(), "workspace-write".to_owned()]);
        }
        PermissionMode::Full => {
            args.push("--dangerously-bypass-approvals-and-sandbox".to_owned());
        }
    }
    if let Some(model) = model {
        args.extend(["-m".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = effort {
        args.extend(["-c".to_owned(), format!("model_reasoning_effort={effort}")]);
    }
    args
}

// Logic copied verbatim from src/antigravity.rs:71-100
pub fn antigravity_interactive_args(
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

pub fn build_interactive_command(
    provider: Provider,
    exe: &Path,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    resume_id: Option<&str>,
    permission_mode: PermissionMode,
) -> (PathBuf, Vec<String>) {
    let args = match provider {
        Provider::Claude => claude_interactive_args(cwd, model, effort, resume_id, permission_mode),
        Provider::Codex => codex_interactive_args(cwd, model, effort, resume_id, permission_mode),
        Provider::Antigravity => antigravity_interactive_args(cwd, model, effort, resume_id, permission_mode),
    };
    (exe.to_path_buf(), args)
}

// Logic copied verbatim from src/terminal.rs:176-195
pub fn is_batch_script(program: &Path) -> bool {
    let has_batch_ext = |p: &Path| {
        p.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
            .unwrap_or(false)
    };
    if has_batch_ext(program) {
        return true;
    }
    if program.extension().is_none() {
        let name = program.to_string_lossy();
        if let Some(paths) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&paths) {
                for ext in &[".cmd", ".bat"] {
                    let candidate = dir.join(format!("{name}{ext}"));
                    if candidate.is_file() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn main() {
    println!("=== RUNNING PERMUTATION AND BOUNDARY STRESS TESTS ===");
    let mut total_permutations = 0;
    let mut passed_permutations = 0;

    let providers = [Provider::Claude, Provider::Codex, Provider::Antigravity];
    let permission_modes = [
        PermissionMode::ReadOnly,
        PermissionMode::AcceptEdits,
        PermissionMode::Full,
        PermissionMode::Plan,
    ];
    let models: [Option<&str>; 5] = [
        None,
        Some("claude-3-7-sonnet"),
        Some("o3-mini"),
        Some("gemini-2.5-flash-thinking-high"),
        Some("model with spaces and 'quotes'"),
    ];
    let efforts: [Option<&str>; 5] = [
        None,
        Some("low"),
        Some("medium"),
        Some("high"),
        Some("custom-effort-level"),
    ];
    let resume_ids: [Option<&str>; 4] = [
        None,
        Some("session-123"),
        Some("a0125d67-fa94-4235-b6c6-6c5de7c55c26"),
        Some("id with spaces and кириллица"),
    ];
    let cwds = [
        Path::new(r"C:\simple\path"),
        Path::new(r"C:\Program Files (x86)\Viper App\Workspace Dir"),
        Path::new(r"C:\Проекты\日本語\🚀_unicode"),
        Path::new(r"\\server\share\deep\network\folder"),
        Path::new(r#"C:\path with 'single' and "double" quotes & special %VAR% ^ chars"#),
    ];

    // Matrix test
    for &provider in &providers {
        for &pm in &permission_modes {
            for &m in &models {
                for &eff in &efforts {
                    for &res in &resume_ids {
                        for &cwd in &cwds {
                            total_permutations += 1;
                            let exe = Path::new("test_exe");
                            let (out_exe, args) = build_interactive_command(provider, exe, cwd, m, eff, res, pm);

                            assert_eq!(out_exe, exe);

                            // Invariant checks
                            match provider {
                                Provider::Claude => {
                                    // Must contain permission mode flags
                                    match pm {
                                        PermissionMode::ReadOnly => {
                                            assert!(args.windows(2).any(|w| w == ["--permission-mode", "default"]));
                                            assert!(args.windows(2).any(|w| w == ["--disallowed-tools", "Bash"]));
                                        }
                                        PermissionMode::AcceptEdits => {
                                            assert!(args.windows(2).any(|w| w == ["--permission-mode", "acceptEdits"]));
                                        }
                                        PermissionMode::Full => {
                                            assert!(args.contains(&"--dangerously-skip-permissions".to_owned()));
                                        }
                                        PermissionMode::Plan => {
                                            assert!(args.windows(2).any(|w| w == ["--permission-mode", "plan"]));
                                        }
                                    }
                                    if let Some(model) = m {
                                        assert!(args.windows(2).any(|w| w == ["--model", model]));
                                    }
                                    if let Some(effort) = eff {
                                        assert!(args.windows(2).any(|w| w == ["--effort", effort]));
                                    }
                                    if let Some(resume) = res {
                                        assert!(args.windows(2).any(|w| w == ["--resume", resume]));
                                    }
                                }
                                Provider::Codex => {
                                    // Must always have -C with cwd.display()
                                    assert!(args.windows(2).any(|w| w == ["-C", &cwd.display().to_string()]));

                                    // Resume subcommand must be the very first elements if present
                                    if let Some(resume) = res {
                                        assert_eq!(&args[0..2], &["resume", resume]);
                                    }

                                    // Permission mode checks
                                    match pm {
                                        PermissionMode::ReadOnly | PermissionMode::Plan => {
                                            assert!(args.windows(2).any(|w| w == ["-s", "read-only"]));
                                        }
                                        PermissionMode::AcceptEdits => {
                                            assert!(args.windows(2).any(|w| w == ["-s", "workspace-write"]));
                                        }
                                        PermissionMode::Full => {
                                            assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_owned()));
                                        }
                                    }
                                    if let Some(model) = m {
                                        assert!(args.windows(2).any(|w| w == ["-m", model]));
                                    }
                                    if let Some(effort) = eff {
                                        let expected_cfg = format!("model_reasoning_effort={effort}");
                                        assert!(args.windows(2).any(|w| w == ["-c", &expected_cfg]));
                                    }
                                }
                                Provider::Antigravity => {
                                    // Must start with --add-dir cwd
                                    assert_eq!(&args[0..2], &["--add-dir", &cwd.display().to_string()]);

                                    match pm {
                                        PermissionMode::ReadOnly => {}
                                        PermissionMode::AcceptEdits => {
                                            assert!(args.windows(2).any(|w| w == ["--mode", "accept-edits"]));
                                        }
                                        PermissionMode::Full => {
                                            assert!(args.contains(&"--dangerously-skip-permissions".to_owned()));
                                        }
                                        PermissionMode::Plan => {
                                            assert!(args.windows(2).any(|w| w == ["--mode", "plan"]));
                                        }
                                    }
                                    if let Some(model) = m {
                                        assert!(args.windows(2).any(|w| w == ["--model", model]));
                                        // If model ends in -high, -medium, -low, effort should be suppressed!
                                        if model.ends_with("-high") || model.ends_with("-medium") || model.ends_with("-low") {
                                            assert!(!args.contains(&"--effort".to_owned()), "effort should be suppressed for model: {model}");
                                        } else if let Some(effort) = eff {
                                            assert!(args.windows(2).any(|w| w == ["--effort", effort]));
                                        }
                                    } else if let Some(effort) = eff {
                                        assert!(args.windows(2).any(|w| w == ["--effort", effort]));
                                    }
                                    if let Some(resume) = res {
                                        assert!(args.windows(2).any(|w| w == ["--conversation", resume]));
                                    }
                                }
                            }
                            passed_permutations += 1;
                        }
                    }
                }
            }
        }
    }

    println!("All {} permutations PASSED successfully!", passed_permutations);

    // Batch script extension variations
    println!("=== TESTING BATCH SCRIPT EXTENSION VARIATIONS ===");
    let valid_batch_paths = [
        "claude.cmd",
        "claude.bat",
        "CLAUDE.CMD",
        "CLAUDE.BAT",
        "Claude.Cmd",
        "Claude.Bat",
        "script.cMd",
        "script.bAt",
        r"C:\Program Files\Node\claude.cmd",
        r"C:\Users\tester\AppData\Roaming\npm\claude.cmd",
        r"\\network\share\tools\claude.CMD",
        r"C:\Проекты\скрипт.bat",
        r"C:\folder.with.dots\another.folder\run.cmd",
    ];

    for path in &valid_batch_paths {
        assert!(is_batch_script(Path::new(path)), "Failed to recognize batch script: {path}");
    }

    let non_batch_paths = [
        "claude.exe",
        "codex.exe",
        "agy.exe",
        "CLAUDE.EXE",
        "script.ps1",
        "script.sh",
        "script.vbs",
        "script.com",
        "script_without_extension_that_does_not_exist_on_path_9999",
        r"C:\Tools\codex.exe",
        r"C:\Windows\System32\cmd.exe",
    ];

    for path in &non_batch_paths {
        assert!(!is_batch_script(Path::new(path)), "Incorrectly identified non-batch script as batch: {path}");
    }

    println!("All {} batch and non-batch extension cases PASSED!", valid_batch_paths.len() + non_batch_paths.len());
    println!("ALL ARGUMENT AND BATCH EXTENSION EMPIRICAL TESTS PASSED!");
}
