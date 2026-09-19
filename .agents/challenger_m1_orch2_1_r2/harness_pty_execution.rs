// Empirical test harness for PTY execution and exit status handling.
// Links directly with portable_pty and vt100 from target/debug/deps.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use vt100::Parser;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

#[cfg(windows)]
unsafe extern "system" {
    fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> HANDLE;
}

#[cfg(windows)]
struct TerminalJob(Option<HANDLE>);

#[cfg(windows)]
impl TerminalJob {
    fn new(pid: Option<u32>) -> (Self, bool, u32) {
        let Some(pid) = pid else { return (Self(None), false, 0) };
        let job = unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.ok();
        let mut assign_ok = false;
        let mut last_error = 0;
        let job = job.filter(|job| {
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
                unsafe { CloseHandle(*job) };
                return false;
            }

            // PROCESS_SET_QUOTA (0x0100) | PROCESS_TERMINATE (0x0001)
            let process_handle = unsafe { OpenProcess(0x0100 | 0x0001, 0, pid) };
            if process_handle.is_invalid() {
                last_error = unsafe { windows::Win32::Foundation::GetLastError().0 };
                unsafe { CloseHandle(*job) };
                return false;
            }

            assign_ok = unsafe { AssignProcessToJobObject(*job, process_handle) }.is_ok();
            if !assign_ok {
                last_error = unsafe { windows::Win32::Foundation::GetLastError().0 };
                unsafe { CloseHandle(*job) };
            }
            let _ = unsafe { CloseHandle(process_handle) };
            assign_ok
        });
        (Self(job), assign_ok, last_error)
    }
}

#[cfg(windows)]
impl Drop for TerminalJob {
    fn drop(&mut self) {
        if let Some(job) = self.0.take() {
            let _ = unsafe { TerminateJobObject(job, 1) };
            let _ = unsafe { CloseHandle(job) };
        }
    }
}

pub fn is_batch_script(program: &Path) -> bool {
    program
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
}

pub fn comspec() -> PathBuf {
    std::env::var_os("ComSpec")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("cmd.exe"))
}

pub fn build_command(cwd: &Path, program: &Path, args: &[String]) -> CommandBuilder {
    #[cfg(windows)]
    let mut cmd = if is_batch_script(program) {
        let mut cmd = CommandBuilder::new(comspec());
        cmd.arg("/c");
        cmd.arg(program);
        cmd
    } else {
        CommandBuilder::new(program)
    };

    #[cfg(not(windows))]
    let mut cmd = CommandBuilder::new(program);

    cmd.args(args);
    cmd.cwd(cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd
}

pub struct TestTerminal {
    pub parser: Arc<Mutex<Parser>>,
    pub writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    pub master: Box<dyn MasterPty + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    job: TerminalJob,
    pub exited: Arc<AtomicBool>,
    pub job_assigned: bool,
    pub job_error: u32,
}

impl TestTerminal {
    pub fn start_command(cwd: &Path, program: &Path, args: &[String]) -> Result<Self, String> {
        let size = (24, 80);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: size.0,
                cols: size.1,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("openpty error: {e}"))?;

        let cmd = build_command(cwd, program, args);
        eprintln!("[start_command] cwd: {:?}, argv: {:?}", cmd.get_cwd(), cmd.get_argv());
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("spawn error: {e}"))?;
        drop(pair.slave);

        let pid = child.process_id();
        let (job, job_assigned, job_error) = TerminalJob::new(pid);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(|e| e.to_string())?));
        let parser = Arc::new(Mutex::new(Parser::new(size.0, size.1, 1000)));
        let exited = Arc::new(AtomicBool::new(false));

        let output = Arc::clone(&parser);
        let reply_writer = Arc::clone(&writer);
        let done = Arc::clone(&exited);
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        eprintln!("[reader thread] EOF (0 bytes)");
                        break;
                    }
                    Err(e) => {
                        eprintln!("[reader thread] read error: {e}");
                        break;
                    }
                    Ok(n) => {
                        let text = String::from_utf8_lossy(&buf[..n]);
                        eprintln!("[reader thread] read {n} bytes: {text:?}");
                        if text.contains("\x1b[6n") {
                            eprintln!("[reader thread] answering cursor position report with \\x1b[1;1R");
                            let mut w = reply_writer.lock().unwrap();
                            let _ = w.write_all(b"\x1b[1;1R");
                            let _ = w.flush();
                        }
                        let mut p = output.lock().unwrap_or_else(PoisonError::into_inner);
                        p.process(&buf[..n]);
                    }
                }
            }
            done.store(true, Ordering::Relaxed);
        });

        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            job,
            exited,
            job_assigned,
            job_error,
        })
    }

    pub fn has_exited(&mut self) -> bool {
        self.exited.load(Ordering::Relaxed) || matches!(self.child.try_wait(), Ok(Some(_)))
    }

    pub fn screen_contents(&self) -> String {
        self.parser.lock().unwrap().screen().contents()
    }

    pub fn write_all(&self, bytes: &[u8]) {
        let mut w = self.writer.lock().unwrap();
        let _ = w.write_all(bytes);
        let _ = w.flush();
    }
}

fn processes_with(marker: &str) -> Vec<String> {
    let script = format!(
        "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{marker}*' -and $_.Name -ne 'powershell.exe' }} | ForEach-Object {{ \"$($_.ProcessId) $($_.Name)\" }}"
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .filter(|l| !l.is_empty())
        .collect()
}

fn main() {
    println!("=== TEST 1: PTY BATCH EXECUTION IN PATH WITH SPACES ===");
    let temp_dir = std::env::temp_dir();
    let spaces_dir = temp_dir.join(format!("viper_test spaces_{}", std::process::id()));
    std::fs::create_dir_all(&spaces_dir).unwrap();

    let bat_file = spaces_dir.join("test script.bat");
    std::fs::write(&bat_file, "@echo off\r\necho BATCH_SPACES_OK\r\n").unwrap();

    let mut term = TestTerminal::start_command(&spaces_dir, &bat_file, &[]).expect("Must start batch with spaces");
    assert!(term.job_assigned, "Job object must be assigned! Err: {}", term.job_error);

    let start = Instant::now();
    let mut captured = false;
    while start.elapsed() < Duration::from_secs(4) {
        if term.screen_contents().contains("BATCH_SPACES_OK") {
            captured = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(captured, "Did not capture output from batch script in path with spaces! Got: {:?}", term.screen_contents());
    println!("-> PASS: Batch script in path with spaces executed and output captured!");

    println!("=== TEST 2: PTY WITH UNICODE WORKING DIRECTORY ===");
    let unicode_dir = temp_dir.join(format!("viper_тест_🚀_{}", std::process::id()));
    std::fs::create_dir_all(&unicode_dir).unwrap();

    let prog = PathBuf::from("cmd.exe");
    let args = vec!["/c".to_owned(), "echo UNICODE_CWD_OK && cd".to_owned()];
    let mut term = TestTerminal::start_command(&unicode_dir, &prog, &args).expect("Must start in unicode cwd");
    let start = Instant::now();
    let mut captured = false;
    while start.elapsed() < Duration::from_secs(4) {
        if term.screen_contents().contains("UNICODE_CWD_OK") {
            captured = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(captured, "Did not capture output from unicode cwd! Got: {:?}", term.screen_contents());
    println!("-> PASS: Process in unicode working directory executed successfully!");

    println!("=== TEST 3: PROCESS EXIT STATUS HANDLING (CODE 0, 42) ===");
    // Test code 0
    let mut term_0 = TestTerminal::start_command(&temp_dir, &prog, &["/c".to_owned(), "echo zero && exit 0".to_owned()]).unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if term_0.has_exited() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(term_0.has_exited(), "term_0 should report has_exited=true for exit code 0");
    let status_0 = term_0.child.try_wait().unwrap().expect("child exit status must be present");
    assert!(status_0.success(), "exit 0 should be success");
    println!("-> PASS: Exit code 0 correctly detected as exited and successful!");

    // Test non-zero code (42)
    let mut term_42 = TestTerminal::start_command(&temp_dir, &prog, &["/c".to_owned(), "echo nonzero && exit 42".to_owned()]).unwrap();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        if term_42.has_exited() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(term_42.has_exited(), "term_42 should report has_exited=true for exit code 42");
    let status_42 = term_42.child.try_wait().unwrap().expect("child exit status must be present");
    assert!(!status_42.success(), "exit 42 should NOT be success");
    #[cfg(windows)]
    {
        // On Windows exit_code() is available on ExitStatus
        println!("status_42: {:?}", status_42);
    }
    println!("-> PASS: Exit code 42 correctly detected as exited and failed!");

    // Test while running: has_exited() must be false
    let mut term_running = TestTerminal::start_command(&temp_dir, &prog, &["/c".to_owned(), "ping -n 4 127.0.0.1".to_owned()]).unwrap();
    thread::sleep(Duration::from_millis(300));
    assert!(!term_running.has_exited(), "running ping should report has_exited=false while active");
    drop(term_running);
    println!("-> PASS: Running process correctly reports has_exited=false!");

    println!("=== TEST 4: INTERACTIVE WRITING TO PTY STDIN ===");
    let mut term_echo = TestTerminal::start_command(&temp_dir, &prog, &["/k".to_owned()]).unwrap();
    thread::sleep(Duration::from_millis(400));
    term_echo.write_all(b"echo HELLO_PTY_INTERACTIVE\r\n");
    let start = Instant::now();
    let mut captured = false;
    while start.elapsed() < Duration::from_secs(4) {
        if term_echo.screen_contents().contains("HELLO_PTY_INTERACTIVE") {
            captured = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(captured, "Input sent via write_all was not echoed back! Screen: {:?}", term_echo.screen_contents());
    drop(term_echo);
    println!("-> PASS: Interactive write_all successfully delivered input to PTY process!");

    println!("=== TEST 5: JOB OBJECT PROCESS TREE KILL ON DROP ===");
    const MARKER: &str = "-n 4991";
    let bat_kill = temp_dir.join(format!("viper_kill_{}.bat", std::process::id()));
    std::fs::write(&bat_kill, format!("@echo off\r\nping {MARKER} 127.0.0.1\r\n")).unwrap();

    let term_tree = TestTerminal::start_command(&temp_dir, &bat_kill, &[]).unwrap();
    assert!(term_tree.job_assigned, "Job assignment must succeed");

    let mut running = Vec::new();
    for _ in 0..40 {
        running = processes_with(MARKER);
        if !running.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(!running.is_empty(), "Grandchild ping must start");
    println!("Grandchild running: {:?}", running);

    // Drop terminal
    drop(term_tree);

    // Wait for cleanup with retries up to 4s
    let mut left = running.clone();
    for _ in 0..40 {
        left = processes_with(MARKER);
        if left.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&bat_kill);
    assert!(left.is_empty(), "Grandchild ping must be killed on terminal drop! Still left: {:?}", left);
    println!("-> PASS: Grandchild process was cleanly terminated on terminal drop!");

    println!("=== TEST 6: RAPID SPAWN AND DROP STRESS (50 CYCLES) ===");
    for i in 0..50 {
        let t = TestTerminal::start_command(&temp_dir, &prog, &["/c".to_owned(), "exit 0".to_owned()])
            .unwrap_or_else(|e| panic!("Failed at iteration {i}: {e}"));
        drop(t);
    }
    println!("-> PASS: 50 rapid spawn/drop cycles completed without crash or exhaustion!");

    // Clean up test directories
    let _ = std::fs::remove_dir_all(&spaces_dir);
    let _ = std::fs::remove_dir_all(&unicode_dir);

    println!("=== ALL PTY EXECUTION EMPIRICAL TESTS PASSED! ===");
}
