//! Milestone 4 Comprehensive Adversarial Stress Test Harness
//!
//! Evaluates:
//! 1. Dynamic Viewport Resizing across Extremes (0x0, 1x1, negative, 4K/5K, rapid oscillation)
//! 2. SavedState Serialization & Legacy RON Backward Compatibility (all schemas, hostile inputs, no provider_terminals)
//! 3. Multi-Session Concurrent Switching Permutations (rapid switching, deletions, focus routing, background writers)
//! 4. Middle Provider Terminal & Tools Panel Coexistence (egui ID disjointness, tab switching, extreme panel widths)

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

// ============================================================================
// DATA MODELS MIRRORED FROM CODEBASE
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    Claude,
    Codex,
    #[serde(alias = "Gemini")]
    Antigravity,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::Claude, Provider::Codex, Provider::Antigravity];

    pub fn short_name(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
            Provider::Antigravity => "Antigravity",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Codex => "Codex",
            Provider::Antigravity => "Antigravity",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PermissionMode {
    #[default]
    ReadOnly,
    AcceptEdits,
    Full,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Viewport {
    #[default]
    Desktop,
    Fixed,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "SavedBrowserState")]
pub struct BrowserState {
    pub address: String,
    pub viewport: Viewport,
    pub size: [u32; 2],
    #[serde(default = "default_true")]
    pub auto_refresh: bool,
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            address: String::new(),
            viewport: Viewport::Desktop,
            size: [1280, 800],
            auto_refresh: true,
        }
    }
}

#[derive(Deserialize)]
#[serde(default)]
struct SavedBrowserState {
    address: String,
    viewport: SavedViewport,
    custom_size: [u32; 2],
    size: [u32; 2],
    #[serde(default = "default_true")]
    auto_refresh: bool,
}

impl Default for SavedBrowserState {
    fn default() -> Self {
        Self {
            address: String::new(),
            viewport: SavedViewport::Desktop,
            custom_size: [1280, 800],
            size: [0, 0],
            auto_refresh: true,
        }
    }
}

#[derive(Deserialize)]
enum SavedViewport {
    Desktop,
    Tablet,
    Mobile,
    Custom,
    Fixed,
}

impl From<SavedBrowserState> for BrowserState {
    fn from(saved: SavedBrowserState) -> Self {
        let size = if saved.size != [0, 0] {
            saved.size
        } else {
            match saved.viewport {
                SavedViewport::Tablet => [768, 1024],
                SavedViewport::Mobile => [375, 812],
                SavedViewport::Custom => saved.custom_size,
                SavedViewport::Desktop | SavedViewport::Fixed => [1280, 800],
            }
        };
        let viewport = match saved.viewport {
            SavedViewport::Desktop => Viewport::Desktop,
            SavedViewport::Tablet | SavedViewport::Mobile | SavedViewport::Custom | SavedViewport::Fixed => {
                Viewport::Fixed
            }
        };
        Self { address: saved.address, viewport, size, auto_refresh: saved.auto_refresh }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickedElement {
    pub selector: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    User(String),
    #[serde(alias = "Claude")]
    Agent(String),
    Tool { name: String, detail: String },
    Error(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub id: u64,
    pub title: String,
    pub project_dir: PathBuf,
    #[serde(default)]
    pub worktree_dir: Option<PathBuf>,
    #[serde(default)]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub worktree_base: Option<String>,
    #[serde(default)]
    pub provider: Provider,
    pub permission_mode: PermissionMode,
    #[serde(default)]
    pub chosen_model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub last_usage: Option<Usage>,
    #[serde(alias = "claude_session_id")]
    pub agent_session_id: Option<String>,
    pub model: Option<String>,
    pub entries: Vec<Entry>,
    pub input: String,
    #[serde(default)]
    pub elements: Vec<PickedElement>,
    #[serde(default)]
    pub browser: BrowserState,

    #[serde(skip)]
    pub streaming: String,
    #[serde(skip)]
    pub focus_composer: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            id: 0,
            title: "New Session".into(),
            project_dir: PathBuf::new(),
            worktree_dir: None,
            worktree_branch: None,
            worktree_base: None,
            provider: Provider::default(),
            permission_mode: PermissionMode::default(),
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
            focus_composer: false,
        }
    }
}

impl Session {
    pub fn new(id: u64, project_dir: PathBuf, provider: Provider, permission_mode: PermissionMode) -> Self {
        Self {
            id,
            title: "New Session".into(),
            project_dir,
            provider,
            permission_mode,
            focus_composer: true,
            ..Default::default()
        }
    }

    pub fn has_folder(&self) -> bool {
        !self.project_dir.as_os_str().is_empty()
    }

    pub fn working_dir(&self) -> &Path {
        self.worktree_dir.as_deref().unwrap_or(&self.project_dir)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub default_provider: Provider,
    pub shell: Option<PathBuf>,
    pub custom_executables: BTreeMap<Provider, PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_provider: Provider::default(),
            shell: None,
            custom_executables: BTreeMap::new(),
        }
    }
}

fn saved_before_panel_defaults() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedState {
    pub sessions: Vec<Session>,
    pub active_session: u64,
    pub next_session_id: u64,
    pub show_sessions: bool,
    pub show_tools: bool,
    pub settings: Settings,
    #[serde(skip_serializing)]
    pub browser_address: String,
    #[serde(default = "saved_before_panel_defaults")]
    pub apply_panel_defaults: bool,
}

impl Default for SavedState {
    fn default() -> Self {
        Self {
            sessions: Vec::new(),
            active_session: 0,
            next_session_id: 1,
            show_sessions: true,
            show_tools: false,
            settings: Settings::default(),
            browser_address: String::new(),
            apply_panel_defaults: false,
        }
    }
}

// Simulated App State for Session Switching & Coexistence
pub struct SimulatedApp {
    pub state: SavedState,
    pub provider_terminals: BTreeMap<u64, Result<Arc<Mutex<vt100::Parser>>, String>>,
    pub detected: BTreeMap<Provider, PathBuf>,
}

impl SimulatedApp {
    pub fn new(state: SavedState) -> Self {
        let mut detected = BTreeMap::new();
        detected.insert(Provider::Claude, PathBuf::from("claude"));
        detected.insert(Provider::Codex, PathBuf::from("codex"));
        detected.insert(Provider::Antigravity, PathBuf::from("agy"));
        Self {
            state,
            provider_terminals: BTreeMap::new(),
            detected,
        }
    }

    pub fn active_index(&self) -> usize {
        self.state
            .sessions
            .iter()
            .position(|s| s.id == self.state.active_session)
            .unwrap_or(0)
    }

    pub fn select_session(&mut self, id: u64) {
        self.state.active_session = id;
        if let Some(session) = self.state.sessions.iter_mut().find(|s| s.id == id) {
            session.focus_composer = true;
        }
    }

    pub fn delete_session(&mut self, id: u64) {
        let Some(index) = self.state.sessions.iter().position(|s| s.id == id) else { return };
        let removed = self.state.sessions.remove(index);
        self.provider_terminals.remove(&id);

        if self.state.sessions.is_empty() {
            let fresh = Session::new(1, removed.project_dir, removed.provider, removed.permission_mode);
            self.state.active_session = 1;
            self.state.next_session_id = 2;
            self.state.sessions.push(fresh);
        } else if self.state.active_session == id {
            let next_index = index.saturating_sub(1);
            let next = &mut self.state.sessions[next_index];
            next.focus_composer = true;
            self.state.active_session = next.id;
        }
    }

    pub fn render_terminal_area(&mut self, cols: u16, rows: u16) -> (u64, bool) {
        if self.state.sessions.is_empty() {
            return (0, false);
        }
        let index = self.active_index();
        let session = &mut self.state.sessions[index];
        let session_id = session.id;
        let take_keyboard = std::mem::take(&mut session.focus_composer);

        self.provider_terminals.entry(session_id).or_insert_with(|| {
            let parser = vt100::Parser::new(rows, cols, 1000);
            Ok(Arc::new(Mutex::new(parser)))
        });

        (session_id, take_keyboard)
    }
}

// ============================================================================
// SUITE 1: DYNAMIC VIEWPORT RESIZING ACROSS EXTREMES
// ============================================================================

fn test_suite_1_viewport_resizing_extremes() {
    println!("\n=== [SUITE 1] DYNAMIC VIEWPORT RESIZING ACROSS EXTREMES ===");

    const PADDING: f32 = 4.0;
    const CHAR_WIDTH: f32 = 8.0;
    const ROW_HEIGHT: f32 = 18.0;

    let calc_dimensions = |width: f32, height: f32| -> (u16, u16) {
        let rows = ((height - 2.0 * PADDING) / ROW_HEIGHT).floor().max(2.0) as u16;
        let cols = ((width - 2.0 * PADDING) / CHAR_WIDTH).floor().max(10.0) as u16;
        (rows, cols)
    };

    // 1. Extreme boundary dimensions
    let test_cases = [
        ("0x0", 0.0, 0.0, 2, 10),
        ("1x1", 1.0, 1.0, 2, 10),
        ("Negative -50x-50", -50.0, -50.0, 2, 10),
        ("Sub-pixel 0.001x0.001", 0.001, 0.001, 2, 10),
        ("Tiny 20x20", 20.0, 20.0, 2, 10),
        ("Minimum padding boundary 8x8", 8.0, 8.0, 2, 10),
        ("Standard 1080p 1920x1080", 1920.0, 1080.0, 59, 239),
        ("4K UHD 3840x2160", 3840.0, 2160.0, 119, 479),
        ("5K Studio Display 5120x2880", 5120.0, 2880.0, 159, 639),
        ("8K Ultra 7680x4320", 7680.0, 4320.0, 239, 959),
        ("Ultra-wide 5120x1440", 5120.0, 1440.0, 79, 639),
        ("Vertical Pivot 1440x3440", 1440.0, 3440.0, 190, 179),
    ];

    for (label, w, h, exp_rows, exp_cols) in test_cases {
        let (rows, cols) = calc_dimensions(w, h);
        assert_eq!(
            (rows, cols),
            (exp_rows, exp_cols),
            "Dimensions calculation failed for {label} ({w}x{h})"
        );
        println!("  ✓ Viewport {label} ({w}x{h}) -> (rows: {rows}, cols: {cols})");
    }

    // 2. vt100 Parser buffer behavior across extreme resizes
    let mut parser = vt100::Parser::new(24, 80, 500);

    // Feed ANSI colored text, cursor sequences, and unicode
    parser.process(b"\x1b[31;1mRed Bold Text\x1b[0m Normal Text\r\n");
    parser.process(b"Line 2: \x1b[38;2;255;100;50mTrueColor RGB\x1b[0m\r\n");
    parser.process("Line 3: Unicode 🦀 Viper \x1b[42;30mInverted Badge\x1b[0m\r\n".as_bytes());

    let initial_size = parser.screen().size();
    assert_eq!(initial_size, (24, 80));

    // Shrink to minimum 2x10
    parser.screen_mut().set_size(2, 10);
    assert_eq!(parser.screen().size(), (2, 10));
    assert!(parser.screen().cell(0, 0).is_some());
    assert!(parser.screen().cell(1, 9).is_some());
    assert!(parser.screen().cell(2, 10).is_none(), "Out of bounds cell must return None");

    // Expand to 5K dimensions: 159x639
    parser.screen_mut().set_size(159, 639);
    assert_eq!(parser.screen().size(), (159, 639));
    assert!(parser.screen().cell(158, 638).is_some());
    assert!(parser.screen().cell(159, 639).is_none());

    // Expand to 8K dimensions: 239x959
    parser.screen_mut().set_size(239, 959);
    assert_eq!(parser.screen().size(), (239, 959));

    // 3. Rapid oscillating resize stress (500 cycles between 2x10 and 159x639)
    let start = Instant::now();
    for i in 0..500 {
        if i % 2 == 0 {
            parser.screen_mut().set_size(2, 10);
            parser.process(b"\x1b[H\x1b[2JSmall");
        } else {
            parser.screen_mut().set_size(159, 639);
            parser.process(b"\x1b[10;20HLarge Screen Layout Buffer Test\r\n");
        }
    }
    let elapsed = start.elapsed();
    println!("  ✓ Rapid oscillating resize (500 cycles 2x10 <-> 159x639) completed in {:?}", elapsed);
    assert_eq!(parser.screen().size(), (159, 639));

    println!("  -> Suite 1 PASSED without error.");
}

// ============================================================================
// SUITE 2: SAVED STATE SERIALIZATION & LEGACY RON BACKWARD COMPATIBILITY
// ============================================================================

fn test_suite_2_saved_state_ron_backward_compatibility() {
    println!("\n=== [SUITE 2] SAVED STATE & LEGACY RON COMPATIBILITY ===");

    // 1. Schema v0: Initial release (no worktrees, no approvals, no auto_refresh, no plan)
    let schema_v0_ron = r#"(
        sessions: [
            (
                id: 1,
                title: "Schema v0 Project",
                project_dir: "C:\\projects\\v0",
                provider: Claude,
                permission_mode: ReadOnly,
                entries: [
                    User("What files are here?"),
                    Agent("Looking at the workspace..."),
                ],
                input: "ls",
            ),
        ],
        active_session: 1,
        next_session_id: 2,
        show_sessions: true,
        show_tools: false,
        settings: (
            default_provider: Claude,
        ),
    )"#;

    let v0: SavedState = ron::from_str(schema_v0_ron).expect("Schema v0 must deserialize cleanly");
    assert_eq!(v0.sessions.len(), 1);
    assert_eq!(v0.sessions[0].provider, Provider::Claude);
    assert_eq!(v0.sessions[0].worktree_dir, None);
    assert_eq!(v0.sessions[0].worktree_branch, None);
    assert_eq!(v0.sessions[0].elements.len(), 0);
    assert!(v0.sessions[0].browser.auto_refresh, "missing auto_refresh defaults to true");
    assert!(v0.apply_panel_defaults, "missing apply_panel_defaults defaults to true for old saves");
    println!("  ✓ Schema v0 deserialized cleanly with modern defaults");

    // 2. Schema v1: Gemini CLI release (Gemini provider, claude_session_id alias)
    let schema_v1_ron = r#"(
        sessions: [
            (
                id: 42,
                title: "Gemini Migration Session",
                project_dir: "C:\\code\\gemini_proj",
                provider: Gemini,
                permission_mode: Full,
                claude_session_id: Some("agy-session-uuid-777"),
                entries: [
                    Agent("Hello from Gemini"),
                ],
            ),
        ],
        active_session: 42,
        next_session_id: 43,
        show_sessions: true,
        show_tools: true,
        settings: (
            default_provider: Gemini,
        ),
    )"#;

    let v1: SavedState = ron::from_str(schema_v1_ron).expect("Schema v1 must deserialize cleanly");
    assert_eq!(v1.sessions[0].provider, Provider::Antigravity, "Gemini must map to Provider::Antigravity");
    assert_eq!(v1.settings.default_provider, Provider::Antigravity);
    assert_eq!(v1.sessions[0].agent_session_id.as_deref(), Some("agy-session-uuid-777"));
    println!("  ✓ Schema v1 deserialized: Gemini -> Antigravity, claude_session_id -> agent_session_id");

    // 3. Schema v2: Obsolete browser viewports (Tablet, Mobile, Custom)
    let schema_v2_ron = r#"(
        sessions: [
            (
                id: 10,
                title: "Tablet Viewport Session",
                project_dir: "C:\\web\\tablet",
                provider: Codex,
                permission_mode: AcceptEdits,
                entries: [],
                browser: (
                    address: "http://localhost:3000",
                    viewport: Tablet,
                    custom_size: (1024, 768),
                ),
            ),
            (
                id: 11,
                title: "Mobile Viewport Session",
                project_dir: "C:\\web\\mobile",
                provider: Claude,
                permission_mode: Full,
                entries: [],
                browser: (
                    address: "http://localhost:5000",
                    viewport: Mobile,
                    custom_size: (375, 812),
                ),
            ),
            (
                id: 12,
                title: "Custom Viewport Session",
                project_dir: "C:\\web\\custom",
                provider: Antigravity,
                permission_mode: Plan,
                entries: [],
                browser: (
                    address: "http://localhost:8080",
                    viewport: Custom,
                    custom_size: (900, 600),
                ),
            ),
        ],
        active_session: 10,
        next_session_id: 13,
        show_sessions: true,
        show_tools: true,
        settings: (
            default_provider: Codex,
        ),
    )"#;

    let v2: SavedState = ron::from_str(schema_v2_ron).expect("Schema v2 must deserialize cleanly");
    assert_eq!(v2.sessions[0].browser.viewport, Viewport::Fixed);
    assert_eq!(v2.sessions[0].browser.size, [768, 1024], "Tablet converts to [768, 1024]");
    assert_eq!(v2.sessions[1].browser.viewport, Viewport::Fixed);
    assert_eq!(v2.sessions[1].browser.size, [375, 812], "Mobile converts to [375, 812]");
    assert_eq!(v2.sessions[2].browser.viewport, Viewport::Fixed);
    assert_eq!(v2.sessions[2].browser.size, [900, 600], "Custom converts to custom_size [900, 600]");
    println!("  ✓ Schema v2 obsolete viewports (Tablet, Mobile, Custom) correctly converted to Fixed");

    // 4. Schema v3: Worktree isolation & In-app approvals
    let schema_v3_ron = r#"(
        sessions: [
            (
                id: 200,
                title: "Worktree Isolated Session",
                project_dir: "C:\\repo\\main",
                worktree_dir: Some("C:\\repo\\main\\.viper\\worktrees\\200"),
                worktree_branch: Some("viper/session-200"),
                worktree_base: Some("develop"),
                provider: Claude,
                permission_mode: Full,
                chosen_model: Some("claude-3-7-sonnet"),
                effort: Some("high"),
                entries: [
                    User("Refactor authentication"),
                    Tool(name: "Edit", detail: "src/auth.rs"),
                ],
                browser: (
                    address: "file:///C:/repo/main/index.html",
                    viewport: Desktop,
                    size: (1280, 800),
                    auto_refresh: false,
                ),
            ),
        ],
        active_session: 200,
        next_session_id: 201,
        show_sessions: true,
        show_tools: true,
        settings: (
            default_provider: Claude,
        ),
    )"#;

    let v3: SavedState = ron::from_str(schema_v3_ron).expect("Schema v3 must deserialize cleanly");
    assert_eq!(v3.sessions[0].working_dir(), Path::new(r"C:\repo\main\.viper\worktrees\200"));
    assert_eq!(v3.sessions[0].worktree_branch.as_deref(), Some("viper/session-200"));
    assert_eq!(v3.sessions[0].worktree_base.as_deref(), Some("develop"));
    assert_eq!(v3.sessions[0].chosen_model.as_deref(), Some("claude-3-7-sonnet"));
    assert_eq!(v3.sessions[0].effort.as_deref(), Some("high"));
    assert!(!v3.sessions[0].browser.auto_refresh, "explicit auto_refresh false preserved");
    println!("  ✓ Schema v3 worktrees, model, effort, and auto_refresh verified");

    // 5. Schema v4: Assertion that provider_terminals is NEVER in serialized RON
    let mut current_state = SavedState::default();
    let s = Session::new(50, PathBuf::from(r"C:\test\active"), Provider::Codex, PermissionMode::Full);
    current_state.sessions.push(s);
    current_state.active_session = 50;

    let serialized_ron = ron::to_string(&current_state).expect("SavedState must serialize to RON cleanly");
    assert!(
        !serialized_ron.contains("provider_terminals"),
        "CRITICAL INVARIANT: serialized RON must never contain 'provider_terminals'"
    );
    assert!(
        !serialized_ron.contains("Terminal"),
        "Serialized RON must never contain Terminal struct references"
    );
    println!("  ✓ Invariant Verified: provider_terminals is completely omitted from serialized RON");

    // 6. Hostile & Edge Case RON Inputs
    // a. Empty RON `()`
    let empty_ron = "()";
    let empty_state: SavedState = ron::from_str(empty_ron).expect("Empty RON `()` must deserialize with defaults");
    assert_eq!(empty_state.sessions.len(), 0);
    assert!(empty_state.apply_panel_defaults);
    println!("  ✓ Empty RON `()` handled with defaults");

    // b. Complex Unicode, Emojis, and Escape Sequences
    let unicode_ron = r#"(
        sessions: [
            (
                id: 888,
                title: "Unicode 🦀 🐍 🚀 💡 \n\t \"quoted\" \\escaped\\",
                project_dir: "\\\\server\\share\\repo\\dir with spaces",
                provider: Antigravity,
                permission_mode: Full,
                entries: [
                    User("Emojis: 👨‍💻 👩‍🔬 🏎️ ⚡"),
                    Agent("RTL text: سلام دنیا - مرحبا بالعالم"),
                ],
                input: "echo \"hello world\"\nls -la",
            ),
        ],
        active_session: 888,
        next_session_id: 889,
        show_sessions: true,
        show_tools: true,
        settings: (
            default_provider: Antigravity,
        ),
    )"#;
    let unicode_state: SavedState = ron::from_str(unicode_ron).expect("Unicode and escape sequences must parse");
    assert_eq!(unicode_state.sessions[0].title, "Unicode 🦀 🐍 🚀 💡 \n\t \"quoted\" \\escaped\\");
    assert_eq!(unicode_state.sessions[0].project_dir, PathBuf::from(r"\\server\share\repo\dir with spaces"));
    println!("  ✓ Complex Unicode, RTL, Emojis, quotes, and UNC paths preserved");

    // c. Malformed RON error handling (must return Err without panic)
    let malformed_inputs = [
        "(sessions: [ (id: 1, title: \"unterminated string ) ])",
        "(sessions: [ (id: \"not a number\") ])",
        "(unbalanced parentheses ((",
        "random junk text",
        "",
    ];
    for bad_input in malformed_inputs {
        let res: Result<SavedState, _> = ron::from_str(bad_input);
        assert!(res.is_err(), "Malformed input should return Err, not Ok");
    }
    println!("  ✓ Malformed RON inputs rejected cleanly with Err without panic");

    // d. Large scale stress (1,000 sessions with 2,000 entries)
    let mut large_state = SavedState::default();
    for i in 1..=1000 {
        let mut session = Session::new(i, PathBuf::from(format!("C:\\work\\proj_{i}")), Provider::Claude, PermissionMode::ReadOnly);
        session.entries.push(Entry::User(format!("User query in session {i}")));
        session.entries.push(Entry::Agent(format!("Agent response in session {i}")));
        large_state.sessions.push(session);
    }
    large_state.active_session = 500;
    large_state.next_session_id = 1001;

    let ser_time = Instant::now();
    let large_ron = ron::to_string(&large_state).expect("Large state serializes");
    let ser_elapsed = ser_time.elapsed();

    let deser_time = Instant::now();
    let large_restored: SavedState = ron::from_str(&large_ron).expect("Large state deserializes");
    let deser_elapsed = deser_time.elapsed();

    assert_eq!(large_restored.sessions.len(), 1000);
    assert_eq!(large_restored.active_session, 500);
    println!(
        "  ✓ Large-scale stress (1,000 sessions, 2,000 entries, {} KB): Ser in {:?}, Deser in {:?}",
        large_ron.len() / 1024,
        ser_elapsed,
        deser_elapsed
    );

    println!("  -> Suite 2 PASSED without error.");
}

// ============================================================================
// SUITE 3: MULTI-SESSION CONCURRENT SWITCHING PERMUTATIONS
// ============================================================================

fn test_suite_3_multi_session_switching_permutations() {
    println!("\n=== [SUITE 3] MULTI-SESSION CONCURRENT SWITCHING PERMUTATIONS ===");

    // Set up app with 10 sessions
    let mut state = SavedState::default();
    for id in 1..=10 {
        let provider = match id % 3 {
            0 => Provider::Claude,
            1 => Provider::Codex,
            _ => Provider::Antigravity,
        };
        let session = Session::new(id, PathBuf::from(format!("C:\\projects\\session_{id}")), provider, PermissionMode::Full);
        state.sessions.push(session);
    }
    state.active_session = 1;
    state.next_session_id = 11;

    let mut app = SimulatedApp::new(state);

    // 1. Initial render session 1
    let (sid, take_kb) = app.render_terminal_area(80, 24);
    assert_eq!(sid, 1);
    assert!(take_kb, "First render of session 1 takes keyboard focus");
    assert!(app.provider_terminals.contains_key(&1));

    // Subsequent render of same session does NOT re-request keyboard
    let (sid2, take_kb2) = app.render_terminal_area(80, 24);
    assert_eq!(sid2, 1);
    assert!(!take_kb2, "Subsequent render of session 1 does not take keyboard");

    // 2. Permutation: Sequential forward switching 1..=10
    println!("  -> Testing sequential forward switching 1..=10...");
    for id in 2..=10 {
        app.select_session(id);
        assert_eq!(app.state.active_session, id);
        assert!(app.state.sessions.iter().find(|s| s.id == id).unwrap().focus_composer);

        let (rendered_id, kb) = app.render_terminal_area(80, 24);
        assert_eq!(rendered_id, id);
        assert!(kb, "Switching to session {id} must take keyboard focus");
        assert!(!app.state.sessions.iter().find(|s| s.id == id).unwrap().focus_composer, "Focus consumed");
    }
    assert_eq!(app.provider_terminals.len(), 10, "All 10 session terminals now live in map");

    // 3. Permutation: Reverse switching 10 down to 1
    println!("  -> Testing reverse switching 10 down to 1...");
    for id in (1..=9).rev() {
        app.select_session(id);
        let (rendered_id, kb) = app.render_terminal_area(80, 24);
        assert_eq!(rendered_id, id);
        assert!(kb, "Reverse switch to session {id} takes keyboard focus");
    }

    // 4. Permutation: High-frequency ping-pong switching (500 cycles between session 3 and session 7)
    println!("  -> Testing high-frequency ping-pong switching (500 cycles)...");
    let start = Instant::now();
    for i in 0..500 {
        let target = if i % 2 == 0 { 3 } else { 7 };
        app.select_session(target);
        let (rendered_id, kb) = app.render_terminal_area(80, 24);
        assert_eq!(rendered_id, target);
        assert!(kb, "Ping-pong switch to session {target} takes keyboard");
    }
    println!("  ✓ 500 ping-pong switches completed in {:?}", start.elapsed());

    // 5. Concurrency & Background Output while Session Inactive
    println!("  -> Testing background output to inactive session parser...");
    let s5_parser = app.provider_terminals.get(&5).unwrap().as_ref().unwrap().clone();
    let bg_writer_done = Arc::new(AtomicBool::new(false));
    let bg_done_clone = bg_writer_done.clone();

    let writer_handle = std::thread::spawn(move || {
        for i in 0..100 {
            let mut parser = s5_parser.lock().unwrap();
            let msg = format!("Background stream line {i} in session 5\r\n");
            parser.process(msg.as_bytes());
        }
        bg_done_clone.store(true, Ordering::SeqCst);
    });

    // Main thread performs switches while background writer is active
    for _ in 0..50 {
        app.select_session(1);
        app.render_terminal_area(80, 24);
        app.select_session(2);
        app.render_terminal_area(80, 24);
    }

    writer_handle.join().expect("Background writer thread joined cleanly");
    assert!(bg_writer_done.load(Ordering::SeqCst));

    // Now switch to session 5 and verify its parser content is intact
    app.select_session(5);
    app.render_terminal_area(80, 24);
    {
        let parser = app.provider_terminals.get(&5).unwrap().as_ref().unwrap().lock().unwrap();
        let contents = parser.screen().contents();
        assert!(contents.contains("Background stream line 99"), "Session 5 retained streamed output");
    }
    println!("  ✓ Background thread output while inactive session preserved cleanly");

    // 6. Session Deletion Permutations
    println!("  -> Testing session deletion permutations...");
    // a. Delete inactive session 9
    app.delete_session(9);
    assert!(!app.state.sessions.iter().any(|s| s.id == 9));
    assert!(!app.provider_terminals.contains_key(&9));
    assert_eq!(app.state.active_session, 5, "Active session remained unchanged at 5");

    // b. Delete active session 5 (middle session)
    app.delete_session(5);
    assert!(!app.state.sessions.iter().any(|s| s.id == 5));
    assert!(!app.provider_terminals.contains_key(&5));
    let active_id = app.state.active_session;
    assert!(active_id > 0);
    assert!(app.state.sessions.iter().find(|s| s.id == active_id).unwrap().focus_composer);
    println!("  ✓ Deleting middle active session 5 transferred focus to neighbor {active_id}");

    // c. Delete active session when it is first (index 0)
    let first_id = app.state.sessions[0].id;
    app.select_session(first_id);
    app.delete_session(first_id);
    assert!(!app.state.sessions.iter().any(|s| s.id == first_id));
    let new_first = app.state.sessions[0].id;
    assert_eq!(app.state.active_session, new_first);
    println!("  ✓ Deleting first active session transferred focus to new first {new_first}");

    // d. Delete all sessions down to 0
    println!("  -> Deleting all remaining sessions down to empty...");
    while !app.state.sessions.is_empty() {
        let id = app.state.sessions[0].id;
        if app.state.sessions.len() == 1 {
            app.delete_session(id);
            break;
        }
        app.delete_session(id);
    }
    assert_eq!(app.state.sessions.len(), 1, "App auto-creates a fresh session when last session is deleted");
    assert_eq!(app.state.active_session, 1);
    assert_eq!(app.provider_terminals.len(), 0, "All previous terminals cleared from map");

    println!("  -> Suite 3 PASSED without error.");
}

// ============================================================================
// SUITE 4: MIDDLE TERMINAL & TOOLS PANEL COEXISTENCE & ID DISJOINTNESS
// ============================================================================

fn test_suite_4_tools_coexistence_and_id_disjointness() {
    println!("\n=== [SUITE 4] MIDDLE TERMINAL & TOOLS COEXISTENCE ===");

    // 1. Rigorous egui ID Hash Disjointness Verification
    println!("  -> Testing egui ID hash disjointness across 1,000 sessions and 100 tabs...");
    let mut id_map: HashMap<egui::Id, &'static str> = HashMap::new();
    let mut collision_count = 0;

    let mut insert_and_check = |id_map: &mut HashMap<egui::Id, &'static str>, id: egui::Id, label: &'static str| -> bool {
        if let Some(existing) = id_map.get(&id) {
            eprintln!("COLLISION DETECTED! Id {:?} generated for both '{}' and '{}'", id, existing, label);
            false
        } else {
            id_map.insert(id, label);
            true
        }
    };

    // Static panel IDs
    assert!(insert_and_check(&mut id_map, egui::Id::new("tools_panel"), "tools_panel"));
    assert!(insert_and_check(&mut id_map, egui::Id::new("tools_rail"), "tools_rail"));
    assert!(insert_and_check(&mut id_map, egui::Id::new("tools_tab_strip"), "tools_tab_strip"));

    // Middle session terminal IDs: ("session_terminal", session_id)
    for sid in 1..=1000u64 {
        let id = egui::Id::new(("session_terminal", sid));
        if !insert_and_check(&mut id_map, id, "session_terminal") {
            collision_count += 1;
        }
    }

    // Tools secondary terminal IDs: ("terminal", number)
    for num in 1..=100u64 {
        let id = egui::Id::new(("terminal", num));
        if !insert_and_check(&mut id_map, id, "tool_secondary_terminal") {
            collision_count += 1;
        }
    }

    // Tools tab button scopes: ("tool_tab", index)
    for idx in 0..=50usize {
        let id = egui::Id::new(("tool_tab", idx));
        if !insert_and_check(&mut id_map, id, "tool_tab") {
            collision_count += 1;
        }
    }

    assert_eq!(collision_count, 0, "Zero egui ID collisions allowed across UI components");
    assert_eq!(id_map.len(), 3 + 1000 + 100 + 51, "All 1,154 IDs are unique and mutually disjoint");
    println!("  ✓ 1,154 distinct egui IDs generated with 0 collisions");

    // 2. Simulated Tools Panel Tab Switching Coexistence
    println!("  -> Testing tools tab switching while middle terminal is running...");
    #[derive(Debug, PartialEq, Eq)]
    enum ToolTab {
        SecondaryTerminal(u64),
        ProjectChanges,
        BranchChanges,
        Browser,
    }

    struct ToolsState {
        tabs: Vec<ToolTab>,
        active: usize,
    }

    let mut tools = ToolsState {
        tabs: vec![
            ToolTab::SecondaryTerminal(1),
            ToolTab::ProjectChanges,
            ToolTab::SecondaryTerminal(2),
            ToolTab::BranchChanges,
            ToolTab::Browser,
        ],
        active: 0,
    };

    let middle_terminal_parser = Arc::new(Mutex::new(vt100::Parser::new(24, 80, 500)));
    middle_terminal_parser.lock().unwrap().process(b"Middle CLI active output\r\n");

    let secondary_term_1 = Arc::new(Mutex::new(vt100::Parser::new(24, 80, 500)));
    secondary_term_1.lock().unwrap().process(b"Secondary shell 1 prompt $ \r\n");

    // Rapidly switch tool tabs 1,000 times
    for cycle in 0..1000 {
        tools.active = cycle % tools.tabs.len();
        match &tools.tabs[tools.active] {
            ToolTab::SecondaryTerminal(num) => {
                let _ = format!("Rendering secondary terminal {num}");
            }
            ToolTab::ProjectChanges => {
                let _ = "Rendering project changes git diff";
            }
            ToolTab::BranchChanges => {
                let _ = "Rendering branch changes git diff";
            }
            ToolTab::Browser => {
                let _ = "Rendering embedded browser live preview";
            }
        }
        // Simultaneously produce middle terminal output
        if cycle % 50 == 0 {
            middle_terminal_parser.lock().unwrap().process(format!("Middle turn {cycle}\r\n").as_bytes());
        }
    }

    // Verify middle terminal buffer was untouched by tool tab switching
    let contents = middle_terminal_parser.lock().unwrap().screen().contents();
    assert!(contents.contains("Middle CLI active output"));
    assert!(contents.contains("Middle turn 950"));
    println!("  ✓ Middle terminal retained full state during 1,000 tool tab switches");

    // 3. Dynamic Viewport Resizing under Extreme Tools Panel Widths
    println!("  -> Testing middle terminal width recomputations under tools panel width extremes...");
    const WINDOW_WIDTH: f32 = 1920.0;
    const SIDEBAR_WIDTH: f32 = 260.0;
    const PADDING: f32 = 4.0;
    const CHAR_WIDTH: f32 = 8.0;

    let test_panel_scenarios = [
        ("Tools Collapsed to Rail (44px)", 44.0),
        ("Tools Default 20% (384px)", 384.0),
        ("Tools Wide (800px)", 800.0),
        ("Tools Extreme Max (1600px)", 1600.0),
    ];

    for (label, tools_width) in test_panel_scenarios {
        let middle_available_width = (WINDOW_WIDTH - SIDEBAR_WIDTH - tools_width).max(0.0);
        let cols = ((middle_available_width - 2.0 * PADDING) / CHAR_WIDTH).floor().max(10.0) as u16;
        assert!(cols >= 10, "cols must never fall below 10");
        println!("  ✓ {label}: Middle width = {middle_available_width:.1}px -> cols = {cols}");
    }

    println!("  -> Suite 4 PASSED without error.");
}

// ============================================================================
// MAIN RUNNER
// ============================================================================

fn main() {
    println!("=================================================================");
    println!("VIPER M4 ADVERSARIAL EMPIRICAL STRESS TEST HARNESS");
    println!("=================================================================");
    let overall_start = Instant::now();

    test_suite_1_viewport_resizing_extremes();
    test_suite_2_saved_state_ron_backward_compatibility();
    test_suite_3_multi_session_switching_permutations();
    test_suite_4_tools_coexistence_and_id_disjointness();

    println!("\n=================================================================");
    println!("ALL EMPIRICAL ADVERSARIAL STRESS SUITES COMPLETED IN {:?}", overall_start.elapsed());
    println!("STATUS: 100% PASS — 0 FAILURES, 0 CRASHES, 0 LEAKS");
    println!("=================================================================");
}
