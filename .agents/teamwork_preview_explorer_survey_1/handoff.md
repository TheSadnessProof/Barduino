# Technical Investigation & Architectural Specification: Interactive In-App Approvals Foundation (R1)

## 1. Observation

### 1.1 Process Execution & Turn Lifecycle (`src/agent.rs`)
- **`PermissionMode`** (`src/agent.rs:109-161`):
  - Defines four variants: `ReadOnly`, `AcceptEdits`, `Full`, `Plan`.
  - The doc comment on `PermissionMode::AcceptEdits` (`src/agent.rs:139-140`) notes:
    > "The agent can edit files. Running commands still needs approval, which a headless agent can't be asked for, so it gets refused."
- **Turn Spawning & Stdin Management** (`src/agent.rs:312-358`):
  - `start_turn` takes `provider: Provider`, `exe: &Path`, `turn: Turn`, and `on_event: impl Fn(AgentEvent) + Send + 'static`.
  - Child stdin is piped: `let mut stdin = child.stdin.take().expect("stdin is piped");` (`src/agent.rs:344`).
  - Stdin is consumed in an ephemeral background thread:
    ```rust
    thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    ```
    `stdin` is dropped immediately after writing `prompt`, closing the pipe (EOF).
- **`RunningTurn` Lifecycle** (`src/agent.rs:175-205`):
  - `RunningTurn` owns `child: Arc<Mutex<Child>>` and `tree: ProcessTree`.
  - Its only public method is `stop(&self)`, which terminates the process tree and kills the child.
  - There is currently no channel, sender, or method to communicate back into a running turn.
- **`AgentEvent` Definition** (`src/agent.rs:78-106`):
  - Contains: `Started`, `TextDelta`, `Text`, `ToolUse`, `ToolResult`, `Plan`, `Finished`, `Exited`.
  - Currently contains no event variant for approval requests or interactive confirmation.

### 1.2 Session State & Event Handling (`src/session.rs`)
- **`Entry` Enum** (`src/session.rs:17-32`):
  ```rust
  #[derive(Debug, PartialEq, Serialize, Deserialize)]
  pub enum Entry {
      User(UserMessage),
      #[serde(alias = "Claude")]
      Agent(String),
      Tool {
          name: String,
          detail: String,
          #[serde(default)]
          edit: Option<FileEdit>,
      },
      ToolOutput { text: String, is_error: bool },
      Notice(String),
      Error(String),
  }
  ```
- **Event Handling** (`src/session.rs:263-358`):
  - Denied tools are only processed at the end of a turn in `AgentEvent::Finished { denied_tools, .. }` (`src/session.rs:293-326`) or `AgentEvent::Exited { error }` (`src/session.rs:327-357`).
  - Denials produce a static `Entry::Notice(format!("{} wasn't allowed to use: {}. ...", self.provider.short_name(), denied_tools.join(", ")))`.
  - Mid-turn intervention is not currently supported.

### 1.3 UI Architecture & Action Dispatch (`src/chat.rs` & `src/app.rs`)
- **Action Pattern Invariant** (`AGENTS.md:154-171`):
  > "Panels return actions; they never mutate app state. Every panel's `ui()` returns an action enum with a `None` variant, and `app.rs` is the only place that acts on it."
- **`chat::conversation`** (`src/chat.rs:538-622`):
  - Takes `ui: &mut egui::Ui, session: &Session, settings: &Settings, _markdown: &mut CommonMarkCache -> ConversationAction`.
  - Takes `&Session` immutably; cannot mutate entries directly.
  - Iterates through `session.entries` and calls `show_entry(ui, (session.id, index), entry, ...)`.
  - Currently returns `ConversationAction`: `None`, `ChangeFolder`, `SelectProvider(Provider)`.
- **`app.rs` Panel Routing** (`src/app.rs:484-508`):
  - Evaluates `conv_action = chat::conversation(...)`.
  - Matches on `conv_action` and applies mutations to `self.active_session_mut()`.
- **Sidebar State Rendering** (`src/sidebar.rs:30-45`, `398-422`):
  - `SessionState`: `Running`, `Failed`, `Idle`.
  - `SessionState::Running` displays a green pulsing indicator.
  - There is no representation for a session paused waiting for approval.

---

## 2. Logic Chain

1. **Protocol Need**: Requirement R1 requires surfacing mid-turn approval requests when an agent asks to execute sensitive actions (shell commands or modifying files), presenting interactive Approve/Deny options, and relaying decisions back to the agent process.
2. **Data Structure Placement**:
   - Defining `ApprovalStatus` (`Pending`, `Approved`, `Denied`) and `ApprovalRequest` (`id`, `tool_name`, `detail`, `edit: Option<FileEdit>`, `status: ApprovalStatus`) establishes the core state model.
   - `ApprovalRequest` must be embeddable in both `AgentEvent::ApprovalRequest` (transport from provider/process to app) and `Entry::Approval` (stored in session history).
3. **Serialization & Backward Compatibility**:
   - In RON, existing sessions do not contain `Entry::Approval`. Adding `Entry::Approval(ApprovalRequest)` to `Entry` is non-breaking for existing deserialization because existing entries are unaffected.
   - `ApprovalRequest` fields can provide defaults (`#[serde(default)]`) to guard future schema evolutions.
4. **Relay Back to Process**:
   - `RunningTurn` must provide a bidirectional bridge. By adding an optional `approval_tx: Option<mpsc::Sender<ApprovalResponse>>` to `RunningTurn`, `RunningTurn::respond_approval(&self, id: &str, decision: ApprovalDecision) -> bool` can forward decisions to a background worker thread.
   - For CLIs accepting stdin responses or stdin control protocols, this thread writes the formatted decision to `stdin` without closing `stdin` prematurely.
   - For mock/test harnesses and internal providers, this channel allows immediate deterministic testing of approval state transitions.
5. **Architectural Separation in UI**:
   - Per AGENTS.md rule 4, `chat::conversation` must remain immutable over `&Session`.
   - `ConversationAction` should be extended with `Approve(String)` and `Deny(String)` (carrying approval IDs).
   - When the user clicks "Approve" or "Deny", `chat::conversation` emits `ConversationAction::Approve(id)` or `Deny(id)`.
   - `app.rs` receives the action and calls `session.resolve_approval(&id, decision)`, which updates `Entry::Approval` and forwards the decision through `session.turn`.
6. **Session & Sidebar Liveness**:
   - In `sidebar.rs`, extending `SessionState` with `WaitingForApproval` ensures that when any session has a pending approval (`session.has_pending_approval()`), an amber/alert status indicator is rendered.

---

## 3. Proposed Technical Design

### 3.1 Data Models (`src/agent.rs` & `src/session.rs`)

#### Approval Core Types (`src/agent.rs` or new module `src/approval.rs` re-exported in `agent`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ApprovalStatus {
    #[default]
    Pending,
    Approved,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalResponse {
    pub id: String,
    pub decision: ApprovalDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool_name: String,
    pub detail: String,
    #[serde(default)]
    pub edit: Option<FileEdit>,
    #[serde(default)]
    pub status: ApprovalStatus,
}

impl ApprovalRequest {
    pub fn new(
        id: impl Into<String>,
        tool_name: impl Into<String>,
        detail: impl Into<String>,
        edit: Option<FileEdit>,
    ) -> Self {
        Self {
            id: id.into(),
            tool_name: tool_name.into(),
            detail: detail.into(),
            edit,
            status: ApprovalStatus::Pending,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.status == ApprovalStatus::Pending
    }

    pub fn resolve(&mut self, decision: ApprovalDecision) -> bool {
        if self.status != ApprovalStatus::Pending {
            return false;
        }
        self.status = match decision {
            ApprovalDecision::Approved => ApprovalStatus::Approved,
            ApprovalDecision::Denied => ApprovalStatus::Denied,
        };
        true
    }
}
```

#### Event & Entry Additions
In `AgentEvent` (`src/agent.rs`):
```rust
pub enum AgentEvent {
    // Existing variants ...
    ApprovalRequest(ApprovalRequest),
}
```

In `Entry` (`src/session.rs`):
```rust
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    User(UserMessage),
    #[serde(alias = "Claude")]
    Agent(String),
    Tool {
        name: String,
        detail: String,
        #[serde(default)]
        edit: Option<FileEdit>,
    },
    ToolOutput { text: String, is_error: bool },
    Notice(String),
    Error(String),
    /// Mid-turn interactive approval request.
    Approval(ApprovalRequest),
}
```

### 3.2 RunningTurn Relay Mechanism (`src/agent.rs`)
Extend `RunningTurn`:
```rust
pub struct RunningTurn {
    child: Arc<Mutex<Child>>,
    tree: ProcessTree,
    approval_tx: Option<mpsc::Sender<ApprovalResponse>>,
}

impl RunningTurn {
    pub fn with_approval_channel(
        child: Child,
        approval_tx: Option<mpsc::Sender<ApprovalResponse>>,
    ) -> Self {
        let tree = ProcessTree::new(&child);
        Self {
            child: Arc::new(Mutex::new(child)),
            tree,
            approval_tx,
        }
    }

    pub fn respond_approval(&self, id: &str, decision: ApprovalDecision) -> bool {
        if let Some(tx) = &self.approval_tx {
            tx.send(ApprovalResponse {
                id: id.to_owned(),
                decision,
            }).is_ok()
        } else {
            false
        }
    }
}
```

### 3.3 Session Approval Methods (`src/session.rs`)
Add approval query and resolution methods to `Session`:
```rust
impl Session {
    pub fn has_pending_approval(&self) -> bool {
        self.entries.iter().any(|e| matches!(e, Entry::Approval(r) if r.is_pending()))
    }

    pub fn pending_approval(&self) -> Option<&ApprovalRequest> {
        self.entries.iter().rev().find_map(|e| match e {
            Entry::Approval(r) if r.is_pending() => Some(r),
            _ => None,
        })
    }

    pub fn resolve_approval(&mut self, id: &str, decision: ApprovalDecision) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if let Entry::Approval(req) = entry
                && req.id == id
            {
                changed = req.resolve(decision);
                break;
            }
        }
        if changed {
            if let Some(turn) = &self.turn {
                turn.respond_approval(id, decision);
            }
        }
        changed
    }
}
```
In `Session::handle_event`:
```rust
AgentEvent::ApprovalRequest(request) => {
    self.keep_streamed_text();
    self.entries.push(Entry::Approval(request));
}
```

### 3.4 Interactive Chat Widget (`src/chat.rs`)
Extend `ConversationAction`:
```rust
pub enum ConversationAction {
    None,
    ChangeFolder,
    SelectProvider(Provider),
    Approve(String),
    Deny(String),
}
```

In `show_entry` (`src/chat.rs`):
Render `Entry::Approval(request)`:
- Styled container with border stroke and rounded corners.
- Tool family indicator using `tool_call::describe(&request.tool_name)` and `kind_colour`.
- Badge showing "Approval Required" or "Permission Request".
- Action description: command string or shortened file path (`tool_call::short_path`).
- Diff preview if `request.edit` is present (`edit_view(ui, id, edit)`).
- Action buttons when `request.status == ApprovalStatus::Pending`:
  - `[✓ Approve]` button: emits `ConversationAction::Approve(request.id.clone())`.
  - `[✕ Deny]` button: emits `ConversationAction::Deny(request.id.clone())`.
- Static badges when already resolved:
  - `ApprovalStatus::Approved`: green pill `✓ Approved`.
  - `ApprovalStatus::Denied`: red pill `✕ Denied`.

### 3.5 Sidebar Status (`src/sidebar.rs`)
Update `SessionState` and `session_state`:
```rust
pub enum SessionState {
    Running,
    WaitingForApproval,
    Failed,
    Idle,
}

pub fn session_state(session: &Session) -> SessionState {
    if session.has_pending_approval() {
        SessionState::WaitingForApproval
    } else if session.is_running() {
        SessionState::Running
    } else if matches!(session.entries.last(), Some(Entry::Error(_))) {
        SessionState::Failed
    } else {
        SessionState::Idle
    }
}
```
`SessionState::WaitingForApproval` draws an amber indicator dot (`Color32::from_rgb(214, 158, 46)`), alerting the user that the session is waiting for review.

---

## 4. Caveats

- **External CLI Protocol Differences**: Different external CLIs (Claude, Codex, Antigravity) have varying headless stream-json support for interactive stdin prompts. For example, some flags (like `--permission-prompts none` in Claude) tell the CLI to fail immediately rather than waiting for stdin. The foundation introduces the high-level data models, event interfaces, session state transitions, UI widgets, and relay plumbing, allowing providers to wire their respective stream protocols without altering the core app architecture.
- **Multiple Simultaneous Approvals**: The data model supports multiple requests indexed by unique ID, though typical agent turns emit approvals sequentially.

---

## 5. Conclusion

1. The interactive in-app approvals architecture cleanly layers into the existing Viper foundation without breaking existing serialized sessions or violating repository design rules.
2. The UI interaction adheres strictly to `ConversationAction` without mutating state during rendering.
3. The relay model allows responses to flow from the UI widget -> `app.rs` -> `session.resolve_approval` -> `running_turn.respond_approval`.
4. The implementation requires zero new external dependencies and maintains zero clippy warnings.

---

## 6. Verification Method

### Test Suite Execution
Run the following commands to verify that code compiles cleanly and existing tests pass without regressions:
```bash
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

### Proposed Unit Tests for Implementation
Add unit tests in `src/session.rs` and `src/agent.rs`:
1. `an_approval_request_starts_pending_and_transitions_to_approved()`:
   Validates `ApprovalRequest::new` creates a request with `ApprovalStatus::Pending` and transitions to `Approved` via `resolve(ApprovalDecision::Approved)`.
2. `an_approval_request_cannot_be_resolved_twice()`:
   Validates calling `resolve` on an already resolved request returns `false` and preserves the initial decision.
3. `session_handles_approval_request_event_and_updates_entries()`:
   Validates `session.handle_event(AgentEvent::ApprovalRequest(...))` flushes streamed text, appends `Entry::Approval`, and causes `session.has_pending_approval()` to return `true`.
4. `resolving_session_approval_updates_entry_and_clears_pending_state()`:
   Validates `session.resolve_approval(&id, ApprovalDecision::Approved)` updates the matching entry and `session.has_pending_approval()` returns `false`.
5. `sidebar_session_state_reflects_waiting_for_approval()`:
   Validates `session_state(&session)` returns `SessionState::WaitingForApproval` when a pending approval is present, and returns `SessionState::Idle` once resolved.
6. `approval_entries_serialize_and_deserialize_cleanly()`:
   Validates RON serialization and deserialization of a `Session` containing `Entry::Approval`.
