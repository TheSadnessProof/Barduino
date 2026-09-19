use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Provider {
    Claude,
    Codex,
    #[serde(alias = "Gemini")]
    Antigravity,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MiniSession {
    id: u64,
    provider: Provider,
    #[serde(alias = "claude_session_id")]
    agent_session_id: Option<String>,
}

fn main() {
    let legacy = r#"(
        id: 42,
        provider: Gemini,
        claude_session_id: Some("session-1234"),
    )"#;
    let s: MiniSession = ron::from_str(legacy).expect("failed to parse");
    assert_eq!(s.provider, Provider::Antigravity);
    assert_eq!(s.agent_session_id, Some("session-1234".to_string()));
    println!("SUCCESS: MiniSession parsed legacy Gemini and claude_session_id");
}
