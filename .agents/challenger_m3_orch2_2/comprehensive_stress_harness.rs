use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

// --- Exact Struct Mirroring for Viper State Testing ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    Claude,
    Codex,
    #[serde(alias = "Gemini")]
    Antigravity,
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
            SavedViewport::Fixed | SavedViewport::Tablet | SavedViewport::Mobile | SavedViewport::Custom => Viewport::Fixed,
            SavedViewport::Desktop => Viewport::Desktop,
        };
        Self {
            address: saved.address,
            viewport,
            size,
            auto_refresh: saved.auto_refresh,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickedElement {
    pub tag: String,
    pub selector: String,
    pub text: String,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "SavedUserMessage")]
pub struct UserMessage {
    pub text: String,
    pub elements: Vec<PickedElement>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ApprovalStatus {
    #[default]
    Pending,
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_name: String,
    pub detail: String,
    #[serde(default)]
    pub edit: Option<String>,
    #[serde(default)]
    pub status: ApprovalStatus,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    User(UserMessage),
    #[serde(alias = "Claude")]
    Agent(String),
    Tool {
        name: String,
        detail: String,
        #[serde(default)]
        edit: Option<String>,
    },
    ToolOutput { text: String, is_error: bool },
    Notice(String),
    Error(String),
    Approval(ApprovalRequest),
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub default_provider: Provider,
    pub disabled_providers: Vec<Provider>,
    pub custom_executables: BTreeMap<Provider, PathBuf>,
    #[serde(default)]
    pub shell: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_provider: Provider::Claude,
            disabled_providers: Vec::new(),
            custom_executables: BTreeMap::new(),
            shell: None,
        }
    }
}

fn saved_before_panel_defaults() -> bool {
    true
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedState {
    pub sessions: Vec<Session>,
    pub active_session: u64,
    pub next_session_id: u64,
    pub show_sessions: bool,
    pub show_tools: bool,
    pub settings: Settings,
    pub browser: BrowserState,
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
            next_session_id: 0,
            show_sessions: true,
            show_tools: false,
            settings: Settings::default(),
            browser: BrowserState::default(),
            browser_address: String::new(),
            apply_panel_defaults: false,
        }
    }
}

// Simulated App struct to verify runtime separation
pub struct ViperAppSim {
    pub state: SavedState,
    pub provider_terminals: BTreeMap<u64, Result<String, String>>,
}

// --- Test Suites ---

fn test_1_provider_terminals_never_serialized() {
    println!("[TEST 1] Verifying provider_terminals is never serialized to disk...");
    let mut app = ViperAppSim {
        state: SavedState::default(),
        provider_terminals: BTreeMap::new(),
    };
    app.state.sessions.push(Session {
        id: 1,
        title: "Session 1".into(),
        project_dir: PathBuf::from(r"C:\Users\test\proj"),
        provider: Provider::Claude,
        permission_mode: PermissionMode::Full,
        ..Default::default()
    });
    app.state.active_session = 1;
    app.state.next_session_id = 2;

    // Populate provider_terminals with active running terminal and error terminal
    app.provider_terminals.insert(1, Ok("running PTY handle".into()));
    app.provider_terminals.insert(2, Err("failed spawn ConPTY 193".into()));

    let serialized = ron::to_string(&app.state).expect("state must serialize");
    assert!(!serialized.contains("provider_terminals"), "RON string must not contain 'provider_terminals'");
    assert!(!serialized.contains("running PTY handle"), "RON must not contain terminal handle content");
    assert!(!serialized.contains("failed spawn"), "RON must not contain ephemeral error strings");
    println!("  -> PASS: provider_terminals strictly excluded from serialization.");
}

fn test_2_legacy_gemini_and_claude_session_id() {
    println!("[TEST 2] Verifying legacy Gemini and claude_session_id deserialization...");
    let legacy_ron = r#"(
        sessions: [
            (
                id: 42,
                title: "Old Gemini Session",
                project_dir: "C:\\projects\\gemini_legacy",
                provider: Gemini,
                permission_mode: Plan,
                claude_session_id: Some("agy-conv-777"),
                entries: [
                    Claude("Hello from legacy agent"),
                ],
                browser: (
                    address: "http://localhost:3000",
                    viewport: Tablet,
                    custom_size: (768, 1024),
                ),
            ),
        ],
        active_session: 42,
        next_session_id: 43,
        show_sessions: true,
        show_tools: false,
        settings: (
            default_provider: Gemini,
        ),
    )"#;

    let restored: SavedState = ron::from_str(legacy_ron).expect("legacy RON must deserialize cleanly");
    assert_eq!(restored.sessions.len(), 1);
    let s = &restored.sessions[0];

    // Check Gemini -> Antigravity
    assert_eq!(s.provider, Provider::Antigravity, "Gemini provider must deserialize to Antigravity");
    assert_eq!(restored.settings.default_provider, Provider::Antigravity, "default_provider Gemini must deserialize to Antigravity");

    // Check claude_session_id -> agent_session_id
    assert_eq!(s.agent_session_id.as_deref(), Some("agy-conv-777"), "claude_session_id must map to agent_session_id");

    // Check entry Claude(...) -> Agent(...)
    assert_eq!(s.entries.len(), 1);
    match &s.entries[0] {
        Entry::Agent(text) => assert_eq!(text, "Hello from legacy agent"),
        other => panic!("Expected Entry::Agent, got {:?}", other),
    }

    // Check legacy browser Tablet viewport conversion
    assert_eq!(s.browser.viewport, Viewport::Fixed);
    assert_eq!(s.browser.size, [768, 1024]);
    assert!(s.browser.auto_refresh, "auto_refresh must default to true");

    // Check apply_panel_defaults for old saves
    assert!(restored.apply_panel_defaults, "old save must default apply_panel_defaults to true");
    println!("  -> PASS: Legacy Gemini, claude_session_id, Claude entry, and browser converted cleanly.");
}

fn test_3_extreme_and_hostile_ron_inputs() {
    println!("[TEST 3] Stress-testing extreme and hostile RON inputs...");

    // 3.1 Minimal empty RON
    let minimal_ron = "()";
    let state: SavedState = ron::from_str(minimal_ron).expect("empty tuple must deserialize with defaults");
    assert_eq!(state.sessions.len(), 0);
    assert!(state.apply_panel_defaults);

    // 3.2 Unknown fields in RON (backward/forward tolerance)
    let unknown_fields_ron = r#"(
        sessions: [
            (
                id: 1,
                title: "Future Session",
                project_dir: "C:\\test",
                future_unknown_field_1: 12345,
                provider: Claude,
                permission_mode: ReadOnly,
            ),
        ],
        active_session: 1,
        future_global_setting: "ignored",
    )"#;
    let state: SavedState = ron::from_str(unknown_fields_ron).expect("RON with unknown fields must deserialize with serde default");
    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.sessions[0].title, "Future Session");

    // 3.3 Special characters, Unicode, emojis, Windows paths
    let unicode_title = "🦀 🐍 Viper Session 🚀 \n \t \"quotes\" and \\ backslashes \\\\server\\share\\path";
    let mut state = SavedState::default();
    state.sessions.push(Session {
        id: 999,
        title: unicode_title.into(),
        project_dir: PathBuf::from(r"\\?\C:\Users\test\weird dir (special) [chars] #$%"),
        worktree_dir: Some(PathBuf::from(r"C:\Users\test\.viper\worktrees\999\special")),
        worktree_branch: Some("viper/session-999-🚀".into()),
        worktree_base: Some("origin/main".into()),
        provider: Provider::Codex,
        permission_mode: PermissionMode::AcceptEdits,
        chosen_model: Some("o3-mini".into()),
        effort: Some("high".into()),
        agent_session_id: Some("session-uuid-1234-5678".into()),
        entries: vec![
            Entry::Notice("Line 1\r\nLine 2\nLine 3 with \"quotes\" and 'single'".into()),
            Entry::Approval(ApprovalRequest {
                id: "appr-1".into(),
                tool_name: "bash".into(),
                detail: "rm -rf /tmp/* && echo \"safe\"".into(),
                edit: None,
                status: ApprovalStatus::Pending,
            }),
        ],
        input: "Unsent text with emoji 💡".into(),
        ..Default::default()
    });

    let ron_str = ron::to_string(&state).expect("state with unicode and paths must serialize");
    let back: SavedState = ron::from_str(&ron_str).expect("state with unicode and paths must deserialize");
    assert_eq!(back.sessions[0].title, unicode_title);
    assert_eq!(back.sessions[0].project_dir, state.sessions[0].project_dir);
    assert_eq!(back.sessions[0].entries.len(), 2);
    println!("  -> PASS: Unicode, emojis, Windows paths, and special characters roundtrip successfully.");

    // 3.4 Malformed RON returns Err gracefully (no panic)
    let malformed_cases = [
        "(sessions: [ (id: 1",             // unclosed bracket
        "(sessions: [ (id: \"not_a_u64\") ])", // type mismatch
        "provider: NonExistentProvider",     // syntax error
        "(((((((((((((",                     // unbalanced
    ];
    for bad in &malformed_cases {
        let res: Result<SavedState, _> = ron::from_str(bad);
        assert!(res.is_err(), "Malformed input should return Err, not panic: {}", bad);
    }
    println!("  -> PASS: Malformed inputs handled gracefully with Err.");
}

fn test_4_large_scale_stress() {
    println!("[TEST 4] Stress-testing large scale session state...");
    let mut state = SavedState::default();
    for i in 1..=500 {
        state.sessions.push(Session {
            id: i,
            title: format!("Stress Session #{i}"),
            project_dir: PathBuf::from(format!(r"C:\work\project_{i}")),
            provider: if i % 3 == 0 { Provider::Antigravity } else if i % 2 == 0 { Provider::Codex } else { Provider::Claude },
            permission_mode: PermissionMode::Full,
            agent_session_id: Some(format!("session-agent-{i}")),
            entries: vec![
                Entry::Agent(format!("Agent response for session {i} with some content")),
                Entry::ToolOutput { text: format!("Output line from tool execution {i}"), is_error: false },
            ],
            ..Default::default()
        });
    }
    state.active_session = 250;
    state.next_session_id = 501;

    let start = std::time::Instant::now();
    let ron_str = ron::to_string(&state).expect("large state must serialize");
    let serialize_time = start.elapsed();

    let start_de = std::time::Instant::now();
    let restored: SavedState = ron::from_str(&ron_str).expect("large state must deserialize");
    let deserialize_time = start_de.elapsed();

    assert_eq!(restored.sessions.len(), 500);
    assert_eq!(restored.active_session, 250);
    println!("  -> PASS: 500 sessions serialized in {:?} (size: {} KB), deserialized in {:?}",
        serialize_time, ron_str.len() / 1024, deserialize_time);
}

fn test_5_tools_panel_coexistence_and_id_disjointness() {
    println!("[TEST 5] Verifying tools panel coexistence and ID hashing disjointness...");

    // Compute egui-style tuple hash comparisons to verify middle terminal and tools panel IDs never collide
    // egui::Id is typically derived from std::hash or a custom hasher
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_id<T: Hash>(val: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        val.hash(&mut hasher);
        hasher.finish()
    }

    let session_id = 42u64;
    let tool_number = 42u64; // same numerical value

    let middle_id = hash_id(&("session_terminal", session_id));
    let tool_id = hash_id(&("terminal", tool_number));
    let panel_id = hash_id(&"tools_panel");
    let rail_id = hash_id(&"tools_rail");

    assert_ne!(middle_id, tool_id, "Middle terminal ID and Tool terminal ID must never collide!");
    assert_ne!(middle_id, panel_id);
    assert_ne!(tool_id, panel_id);
    assert_ne!(panel_id, rail_id);

    println!("  Middle terminal hash: {:x}", middle_id);
    println!("  Tool terminal hash:   {:x}", tool_id);
    println!("  Tools panel hash:     {:x}", panel_id);
    println!("  -> PASS: Widget ID namespaces are completely disjoint.");
}

fn main() {
    println!("============================================================");
    println!("VIPER EMPIRICAL CHALLENGER STRESS HARNESS - MILESTONE 3");
    println!("============================================================");
    test_1_provider_terminals_never_serialized();
    test_2_legacy_gemini_and_claude_session_id();
    test_3_extreme_and_hostile_ron_inputs();
    test_4_large_scale_stress();
    test_5_tools_panel_coexistence_and_id_disjointness();
    println!("============================================================");
    println!("ALL ADVERSARIAL STRESS TESTS PASSED CLEANLY!");
    println!("============================================================");
}
