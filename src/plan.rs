//! What each agent CLI says about its own plan limits. These are the providers'
//! official figures, not anything Viper works out: Claude Code sends them with
//! every reply, and Codex writes them into the session file it keeps for each run.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};

use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::{self, Provider};

/// Session files bigger than this are skipped, so reading can't stall on a huge one.
const MAX_SESSION_FILE: u64 = 32 * 1024 * 1024;
/// How many recent session files to look through for the latest figures.
const RECENT_FILES: usize = 6;

/// One of a provider's limit windows, such as Claude's five-hour window.
///
/// `default` because this is saved: without it, adding a field here would stop the
/// whole file loading, and the user would lose every session rather than a figure
/// that gets read again on the next turn anyway.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    pub name: String,
    /// The share of the limit used, where 1.0 is all of it.
    pub used: f32,
    /// When the window starts over, in seconds since the Unix epoch.
    pub resets_at: Option<i64>,
}

/// A provider's plan usage as it reported it. `default` for the same reason as
/// [`Window`]: a missing field here must not cost the user their sessions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanUsage {
    pub windows: Vec<Window>,
    /// Anything else the provider mentions, such as the plan name or credits left.
    pub notes: Vec<String>,
    /// When these figures were read, in seconds since the Unix epoch.
    pub read_at: i64,
}

/// Claude Code's `rate_limit_event`. Its utilization is already a share of the limit.
pub fn from_claude(info: &Value) -> Option<PlanUsage> {
    let mut windows = Vec::new();
    if let Some(unified) = info["unifiedWindows"].as_object() {
        for (key, window) in unified {
            if let Some(used) = window["utilization"].as_f64() {
                let resets_at = window["resetsAt"].as_i64();
                windows.push(Window { name: claude_window_name(key), used: used as f32, resets_at });
            }
        }
    }
    // Older versions report only the window that is closest to its limit.
    if windows.is_empty() {
        let used = info["utilization"].as_f64()?;
        let name = claude_window_name(info["rateLimitType"].as_str().unwrap_or_default());
        windows.push(Window { name, used: used as f32, resets_at: info["resetsAt"].as_i64() });
    }
    windows.sort_by_key(|window| window.resets_at.unwrap_or(i64::MAX));

    let mut notes = Vec::new();
    if info["isUsingOverage"].as_bool() == Some(true) {
        notes.push("Using extra usage beyond the plan".to_owned());
    }
    Some(PlanUsage { windows, notes, read_at: now() })
}

fn claude_window_name(key: &str) -> String {
    match key {
        "five_hour" => "5-hour limit".to_owned(),
        "seven_day" => "7-day limit".to_owned(),
        "seven_day_opus" => "7-day Opus limit".to_owned(),
        "" => "Plan limit".to_owned(),
        other => format!("{} limit", other.replace('_', " ")),
    }
}

/// The windows in one Codex `rate_limits` entry, each with the length of its window,
/// where the figures are percentages. Codex puts them in two named slots and doesn't
/// always fill both: lately it reports only the weekly one.
fn codex_windows(limits: &Value) -> Vec<(i64, Window)> {
    let mut windows = Vec::new();
    for key in ["primary", "secondary"] {
        let window = &limits[key];
        if let Some(percent) = window["used_percent"].as_f64() {
            let minutes = window["window_minutes"].as_i64();
            windows.push((
                minutes.unwrap_or_default(),
                Window {
                    name: codex_window_name(minutes),
                    used: (percent / 100.0) as f32,
                    resets_at: window["resets_at"].as_i64(),
                },
            ));
        }
    }
    windows
}

/// What Codex says about the account besides the windows themselves.
fn codex_notes(limits: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(plan) = limits["plan_type"].as_str().filter(|plan| !plan.is_empty()) {
        notes.push(format!("{plan} plan"));
    }
    let credits = &limits["credits"];
    if credits["has_credits"].as_bool() == Some(true) {
        if credits["unlimited"].as_bool() == Some(true) {
            notes.push("Unlimited credits".to_owned());
        } else if let Some(balance) = credits["balance"].as_str().and_then(|b| b.parse::<f64>().ok()) {
            notes.push(format!("${balance:.2} in credits"));
        }
    }
    notes
}

fn codex_window_name(minutes: Option<i64>) -> String {
    match minutes {
        Some(60) => "Hourly limit".to_owned(),
        Some(300) => "5-hour limit".to_owned(),
        Some(1440) => "Daily limit".to_owned(),
        Some(10080) => "Weekly limit".to_owned(),
        Some(43200) => "Monthly limit".to_owned(),
        Some(minutes) if minutes % 1440 == 0 => format!("{}-day limit", minutes / 1440),
        Some(minutes) if minutes % 60 == 0 => format!("{}-hour limit", minutes / 60),
        Some(minutes) => format!("{minutes}-minute limit"),
        None => "Plan limit".to_owned(),
    }
}

/// Why a provider has no figures to show yet.
pub fn why_missing(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude Code reports its limits while it answers. Send a message, or press Check now.",
        Provider::Codex => "Codex saves its limits whenever it runs. Press Check now once you have used it.",
        Provider::Antigravity => "Press Check now to ask agy for your limits.",
    }
}

/// What checking costs, since one provider has to be asked through a reply.
pub fn check_note(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => {
            "Sends Claude Code a one-word message, which is the only way it reports limits. \
             It costs a fraction of a cent."
        }
        Provider::Codex => "Reads the limits Codex saved the last time it ran. Free.",
        Provider::Antigravity => "Asks agy for its own usage table. Free.",
    }
}

/// Whether asking this provider costs anything. Only Claude Code has to be asked
/// through a reply; Viper checks the free ones by itself.
pub fn is_free(provider: Provider) -> bool {
    provider != Provider::Claude
}

/// Asks a provider for its figures now. Claude Code only reports them while it
/// answers, so it gets a tiny message; the others cost nothing.
pub fn check(provider: Provider, exe: &Path) -> Result<PlanUsage, String> {
    match provider {
        Provider::Claude => check_claude(exe),
        Provider::Codex => {
            read_codex().ok_or_else(|| "Codex has not saved any limits on this computer yet.".to_owned())
        }
        Provider::Antigravity => check_agy(exe),
    }
}

/// Claude Code reports limits as an event while replying, so this asks it the
/// shortest question it can, using its cheapest model.
fn check_claude(exe: &Path) -> Result<PlanUsage, String> {
    let mut child = agent::hidden_command(exe)
        .args(["-p", "--output-format", "stream-json", "--verbose", "--model", "haiku"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("Couldn't start Claude Code: {err}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"hi");
    }
    let stdout = child.stdout.take().ok_or("Claude Code didn't reply.")?;
    let child = Arc::new(Mutex::new(child));
    stop_if_stuck(Arc::clone(&child));

    let mut found = None;
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message["type"] == "rate_limit_event" {
            found = from_claude(&message["rate_limit_info"]);
        }
    }
    let _ = child.lock().unwrap_or_else(PoisonError::into_inner).wait();
    found.ok_or_else(|| "Claude Code didn't mention any limits this time.".to_owned())
}

/// agy answers `/usage` by itself, without asking a model.
fn check_agy(exe: &Path) -> Result<PlanUsage, String> {
    let output = agent::hidden_command(exe)
        .args(["--output-format", "text", "--print=/usage"])
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Couldn't start agy: {err}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    from_agy(&text).ok_or_else(|| match text.lines().find(|line| !line.trim().is_empty()) {
        Some(line) => format!("agy didn't report usage: {}", line.trim()),
        None => "agy didn't report any usage. Open `agy` in a terminal to check you're signed in.".to_owned(),
    })
}

/// agy's `/usage` table, whose lines look like
/// "Gemini Models<TAB>Weekly Limit Remaining<TAB>98%<TAB>2026-09-24T13:35:53Z".
/// The percentage is what's left rather than what's been used.
fn from_agy(text: &str) -> Option<PlanUsage> {
    let mut windows = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').map(str::trim).collect();
        let [group, limit, remaining, rest @ ..] = &fields[..] else { continue };
        let Ok(remaining) = remaining.trim_end_matches('%').parse::<f32>() else { continue };
        let resets_at = rest
            .first()
            .and_then(|when| chrono::DateTime::parse_from_rfc3339(when).ok())
            .map(|when| when.timestamp());
        windows.push(Window {
            name: format!("{group} · {}", limit.trim_end_matches(" Remaining")),
            used: (1.0 - remaining / 100.0).clamp(0.0, 1.0),
            resets_at,
        });
    }
    (!windows.is_empty()).then(|| PlanUsage { windows, notes: Vec::new(), read_at: now() })
}

/// Ends a check that never finished, so no CLI is left running in the background.
fn stop_if_stuck(child: Arc<Mutex<std::process::Child>>) {
    const GIVE_UP_AFTER: std::time::Duration = std::time::Duration::from_secs(120);
    std::thread::spawn(move || {
        std::thread::sleep(GIVE_UP_AFTER);
        let mut child = child.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
        }
    });
}

/// Checks running in the background, and what they came back with.
#[derive(Default)]
pub struct Checks {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    running: BTreeSet<Provider>,
    finished: Vec<(Provider, Result<PlanUsage, String>)>,
}

impl Checks {
    pub fn is_running(&self, provider: Provider) -> bool {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).running.contains(&provider)
    }

    /// Starts a check unless one is already under way for that provider.
    pub fn start(&self, provider: Provider, exe: PathBuf, ctx: &egui::Context) {
        if !self.inner.lock().unwrap_or_else(PoisonError::into_inner).running.insert(provider) {
            return;
        }
        let (inner, ctx) = (Arc::clone(&self.inner), ctx.clone());
        std::thread::spawn(move || {
            let result = check(provider, &exe);
            let mut inner = inner.lock().unwrap_or_else(PoisonError::into_inner);
            inner.running.remove(&provider);
            inner.finished.push((provider, result));
            drop(inner);
            ctx.request_repaint();
        });
    }

    /// The results that have arrived since this was last called.
    pub fn take_finished(&self) -> Vec<(Provider, Result<PlanUsage, String>)> {
        std::mem::take(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner).finished)
    }
}

/// Reads the figures that a CLI only keeps on disk. Runs on a background thread
/// and asks `ctx` to redraw once they're ready.
pub fn read_in_background(ctx: &egui::Context) -> Arc<Mutex<Option<BTreeMap<Provider, PlanUsage>>>> {
    let slot = Arc::new(Mutex::new(None));
    let (into, ctx) = (Arc::clone(&slot), ctx.clone());
    std::thread::spawn(move || {
        let mut found = BTreeMap::new();
        if let Some(codex) = read_codex() {
            found.insert(Provider::Codex, codex);
        }
        *into.lock().unwrap_or_else(PoisonError::into_inner) = Some(found);
        ctx.request_repaint();
    });
    slot
}

/// Codex records its limits in the session file for each run, so its recent files hold
/// the latest figures, including from runs outside Viper. Each entry only carries the
/// windows Codex felt like sending, so the newest figure for each window is gathered
/// across entries, and a window whose reset time has passed is dropped as out of date.
fn read_codex() -> Option<PlanUsage> {
    let sessions = agent::home_dir()?.join(".codex").join("sessions");
    let mut windows: BTreeMap<i64, Window> = BTreeMap::new();
    let mut notes = Vec::new();
    for path in newest_files(&sessions) {
        for limits in codex_session_file(&path) {
            if notes.is_empty() {
                notes = codex_notes(&limits);
            }
            for (minutes, window) in codex_windows(&limits) {
                // Entries come newest first, so the first of each window wins.
                windows.entry(minutes).or_insert(window);
            }
        }
    }
    let now = now();
    let mut windows: Vec<Window> =
        windows.into_values().filter(|window| window.resets_at.is_none_or(|at| at > now)).collect();
    windows.sort_by_key(|window| window.resets_at.unwrap_or(i64::MAX));
    if windows.is_empty() {
        return None;
    }
    Some(PlanUsage { windows, notes, read_at: now })
}

/// The `rate_limits` entries in one session file, newest first.
fn codex_session_file(path: &Path) -> Vec<Value> {
    if std::fs::metadata(path).map(|file| file.len()).unwrap_or(u64::MAX) > MAX_SESSION_FILE {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    text.lines()
        .rev()
        .filter(|line| line.contains("rate_limits"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .map(|entry| entry["payload"]["rate_limits"].clone())
        .filter(|limits| !codex_windows(limits).is_empty())
        .collect()
}

/// The most recently changed files under `dir`, newest first. Codex sorts its
/// sessions into year, month and day folders, so this looks a few levels down.
fn newest_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut folders = vec![(dir.to_owned(), 0_u32)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < 4 {
                folders.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                let changed = entry.metadata().and_then(|m| m.modified()).ok();
                files.push((changed.unwrap_or(std::time::UNIX_EPOCH), entry.path()));
            }
        }
    }
    files.sort_by_key(|(changed, _)| std::cmp::Reverse(*changed));
    files.into_iter().take(RECENT_FILES).map(|(_, path)| path).collect()
}

/// "resets in 2h 10m" for something soon, or "resets Friday 09:00" for something further off.
pub fn resets_text(resets_at: Option<i64>) -> Option<String> {
    let resets_at = resets_at?;
    let when = chrono::DateTime::from_timestamp(resets_at, 0)?.with_timezone(&chrono::Local);
    Some(resets_in((resets_at - now()) / 60, &when))
}

fn resets_in(minutes: i64, when: &chrono::DateTime<chrono::Local>) -> String {
    match minutes {
        ..=0 => "resets any moment".to_owned(),
        1..60 => format!("resets in {minutes}m"),
        60..1440 => format!("resets in {}h {}m", minutes / 60, minutes % 60),
        _ => format!("resets {}", when.format("%A %H:%M")),
    }
}

/// "as of 22:19", or with the date once the figures are from another day.
pub fn read_at_text(read_at: i64) -> String {
    let Some(when) = chrono::DateTime::from_timestamp(read_at, 0) else {
        return String::new();
    };
    let when = when.with_timezone(&chrono::Local);
    if when.date_naive() == chrono::Local::now().date_naive() {
        format!("as of {}", when.format("%H:%M"))
    } else {
        format!("as of {}", when.format("%d %b %H:%M"))
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claudes_two_windows() {
        let info = serde_json::json!({
            "status": "allowed_warning",
            "resetsAt": 1_789_707_600_i64,
            "rateLimitType": "seven_day",
            "utilization": 0.87,
            "isUsingOverage": false,
            "unifiedWindows": {
                "five_hour": { "utilization": 0.0, "resetsAt": 1_789_677_000_i64 },
                "seven_day": { "utilization": 0.87, "resetsAt": 1_789_707_600_i64 }
            }
        });
        let plan = from_claude(&info).expect("the event should describe the plan");
        // The window that starts over soonest comes first.
        assert_eq!(plan.windows[0].name, "5-hour limit");
        assert_eq!(plan.windows[1], Window { name: "7-day limit".into(), used: 0.87, resets_at: Some(1_789_707_600) });
        assert!(plan.notes.is_empty());
    }

    #[test]
    fn reads_claudes_older_single_window() {
        let info = serde_json::json!({ "rateLimitType": "five_hour", "utilization": 0.4, "isUsingOverage": true });
        let plan = from_claude(&info).expect("one window is enough");
        assert_eq!(plan.windows, vec![Window { name: "5-hour limit".into(), used: 0.4, resets_at: None }]);
        assert_eq!(plan.notes, vec!["Using extra usage beyond the plan"]);
        assert!(from_claude(&serde_json::json!({})).is_none());
    }

    #[test]
    fn reads_codex_percentages_and_credits() {
        let limits = serde_json::json!({
            "limit_id": "codex",
            "primary": { "used_percent": 100.0, "window_minutes": 10080, "resets_at": 1_789_805_599_i64 },
            "secondary": null,
            "credits": { "has_credits": true, "unlimited": false, "balance": "171.6073735000" },
            "plan_type": "pro"
        });
        let windows = codex_windows(&limits);
        assert_eq!(windows, vec![(
            10080,
            Window { name: "Weekly limit".into(), used: 1.0, resets_at: Some(1_789_805_599) }
        )]);
        assert_eq!(codex_notes(&limits), vec!["pro plan", "$171.61 in credits"]);
        assert!(codex_windows(&serde_json::json!({ "primary": null })).is_empty());

        // Codex used to report the five-hour window in the first slot and the week in the second.
        let both = serde_json::json!({
            "primary": { "used_percent": 12.5, "window_minutes": 300, "resets_at": 1_789_670_000_i64 },
            "secondary": { "used_percent": 80.0, "window_minutes": 10080, "resets_at": 1_789_805_599_i64 }
        });
        let names: Vec<String> = codex_windows(&both).into_iter().map(|(_, window)| window.name).collect();
        assert_eq!(names, ["5-hour limit", "Weekly limit"]);
    }

    #[test]
    fn takes_the_last_limits_in_a_session_file() {
        let dir = std::env::temp_dir().join("viper-plan-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("2026").join("09").join("17")).unwrap();
        let path = dir.join("2026").join("09").join("17").join("rollout-test.jsonl");
        let entry = |percent: f64| {
            format!(
                r#"{{"type":"event_msg","payload":{{"type":"token_count","rate_limits":{{"primary":{{"used_percent":{percent},"window_minutes":300,"resets_at":1}},"plan_type":"pro"}}}}}}"#
            )
        };
        std::fs::write(&path, format!("{{\"type\":\"other\"}}\n{}\n{}\n", entry(12.0), entry(34.0))).unwrap();

        let entries = codex_session_file(&path);
        assert_eq!(entries.len(), 2, "both entries carry limits");
        // Newest first, so the later figure is the one that counts.
        let (minutes, window) = codex_windows(&entries[0]).remove(0);
        assert_eq!((minutes, window.name.as_str()), (300, "5-hour limit"));
        assert!((window.used - 0.34).abs() < 0.001, "{window:?}");
        assert_eq!(newest_files(&dir), vec![path]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn describes_when_windows_reset() {
        let when = chrono::DateTime::from_timestamp(1_789_707_600, 0).unwrap().with_timezone(&chrono::Local);
        assert_eq!(resets_in(130, &when), "resets in 2h 10m");
        assert_eq!(resets_in(1, &when), "resets in 1m");
        assert_eq!(resets_in(59, &when), "resets in 59m");
        assert_eq!(resets_in(0, &when), "resets any moment");
        assert_eq!(resets_in(-5, &when), "resets any moment");
        assert!(resets_in(4320, &when).starts_with("resets "), "a few days off names the day");
        assert_eq!(resets_text(None), None);
        assert!(resets_text(Some(now() + 600)).is_some());
        assert!(read_at_text(now()).starts_with("as of "));
    }

    /// Reads the real Codex limits on this computer. Depends on Codex having run
    /// here, so it only runs when asked for:
    /// `cargo test -- --ignored plan --nocapture`
    #[test]
    #[ignore]
    fn reads_codex_limits_from_this_computer() {
        let started = std::time::Instant::now();
        let plan = read_codex().expect("Codex should have saved limits on this computer");
        println!("read in {:?}: {plan:#?}", started.elapsed());
        assert!(!plan.windows.is_empty());
    }

    #[test]
    fn reads_the_agy_usage_table() {
        // Real output from `agy --print=/usage`, where the percentage is what is left.
        let table = "Gemini Models\tWeekly Limit Remaining\t98%\t2026-09-24T13:35:53Z\n\
                     Gemini Models\tFive Hour Limit Remaining\t80%\t2026-09-17T18:35:53Z\n\
                     Claude and GPT models\tWeekly Limit Remaining\t96%\t2026-09-24T14:50:02Z\n";
        let plan = from_agy(table).expect("the table should describe the plan");
        assert_eq!(plan.windows.len(), 3);
        assert_eq!(plan.windows[0].name, "Gemini Models · Weekly Limit");
        assert!((plan.windows[0].used - 0.02).abs() < 0.001, "{:?}", plan.windows[0]);
        assert!((plan.windows[1].used - 0.20).abs() < 0.001, "{:?}", plan.windows[1]);
        assert_eq!(plan.windows[2].resets_at, Some(1_790_261_402), "2026-09-24T14:50:02Z");
        assert!(from_agy("Signed out. Run `agy login`.").is_none());
        assert!(from_agy("").is_none());
    }

    /// Asks the CLIs that answer for free what this account's limits are. Depends
    /// on them being installed and signed in, so it only runs when asked for:
    /// `cargo test -- --ignored checks_real --nocapture`
    #[test]
    #[ignore]
    fn checks_real_plan_limits() {
        for provider in [Provider::Antigravity, Provider::Codex] {
            let exe = provider.find().expect("the CLI should be installed");
            let started = std::time::Instant::now();
            let plan = check(provider, &exe).expect("the CLI should report its limits");
            println!("{} in {:?}: {plan:#?}", provider.label(), started.elapsed());
            assert!(!plan.windows.is_empty());
            assert!(plan.windows.iter().all(|window| (0.0..=1.0).contains(&window.used)), "{plan:?}");
        }
    }

    #[test]
    fn names_odd_codex_windows() {
        assert_eq!(codex_window_name(Some(4320)), "3-day limit");
        assert_eq!(codex_window_name(Some(180)), "3-hour limit");
        assert_eq!(codex_window_name(Some(7)), "7-minute limit");
        assert_eq!(codex_window_name(None), "Plan limit");
    }
}
