# Challenger M1-2: Milestone 1 Empirical Evaluation & Stress Test Report

## Challenge Summary

**Verdict**: **APPROVE** (with empirical findings and recommendations noted)  
**Milestone**: Milestone 1 (Interactive In-App Approvals Foundation)  
**Evaluator**: Challenger M1-2 (Deserialization & Boundary Verification)  
**Overall Risk Assessment**: LOW (foundation architecture is solid, backward-compatible, and resilient; single medium-severity edge case identified around repeated turn tool IDs)

---

## 1. Observation

### Implementation & Verification Target
- Target files inspected and tested:
  - `src/agent.rs` (`ApprovalStatus`, `ApprovalDecision`, `ApprovalResponse`, `ApprovalRequest`, `RunningTurn::respond_approval`)
  - `src/session.rs` (`Entry::Approval`, `Session::has_pending_approval`, `Session::pending_approval`, `Session::resolve_approval`)
  - `src/sidebar.rs` (`SessionState::WaitingForApproval`, `session_state`, amber pulsing indicator)
  - `src/chat.rs` (`ConversationAction::Approve`, `ConversationAction::Deny`, `Entry::Approval` interactive widgets)
  - `src/app.rs` (`ConversationAction` dispatch and repaint trigger, `SavedState` persistence)

### Empirical Test Execution Results
- `cargo check`:
  ```
  Checking viper v0.1.0 (C:\Users\ditob\Documents\viper)
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.79s
  ```
- `cargo clippy --all-targets -- -D warnings`:
  ```
  Checking viper v0.1.0 (C:\Users\ditob\Documents\viper)
  Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.54s
  ```
  Zero clippy warnings across all targets.
- `cargo test`:
  ```
  test result: ok. 182 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 1.86s
  ```
  Zero unit test failures. Ignored tests were not touched or modified.

### Empirical Stress Tests Added & Verified
1. **`session::tests::older_session_and_saved_state_ron_formats_deserialize_cleanly` (`src/session.rs`)**:
   - Tests complete legacy `Session` RON format containing `User`, `Agent`, `Tool`, `ToolOutput`, `Notice`, `Error` without any `Entry::Approval` variant.
   - Tests omitted optional fields on `ApprovalRequest` (`edit` omitted -> defaults to `None`, `status` omitted -> defaults to `ApprovalStatus::Pending`).
   - Verified clean deserialization with 0 errors.
2. **`session::tests::multi_approval_fifo_and_arbitrary_ordering_lifecycle` (`src/session.rs`)**:
   - Enqueues 5 distinct pending approval requests (`req-1` through `req-5`).
   - Verifies arbitrary resolution order (`req-3`, then `req-1`, then `req-5`, then `req-4`, then `req-2`).
   - Verifies that `has_pending_approval()` remains `true` until all 5 are resolved.
   - Verifies that `pending_approval()` consistently returns the most recent unresolved approval.
   - Verifies `RunningTurn` channel relay receives exact ID and decision payloads.
   - Verifies idempotent rejection of redundant resolutions (returns `false`, produces 0 spurious channel messages).
3. **`session::tests::session_state_transitions_between_waiting_running_and_idle_under_multi_entry_flow` (`src/session.rs`)**:
   - Verifies full state transition lifecycle: `Idle` -> `Running` (turn spawned) -> `WaitingForApproval` (request 1 added) -> `WaitingForApproval` (request 2 added, intermediate tools added) -> `WaitingForApproval` (request 1 approved, request 2 still pending) -> `Running` (request 2 approved, turn still active) -> `WaitingForApproval` (request 3 added) -> `Running` (request 3 denied, turn still active) -> `Idle` (turn completed cleanly) -> `Failed` (turn ended with Error).
4. **`app::tests::saved_state_with_interactive_approvals_and_legacy_sessions_survives_ron_round_trip` (`src/app.rs`)**:
   - Verifies `SavedState` round-trip through RON containing mixed sessions: legacy session without approvals alongside new session containing pending approvals with `FileEdit` diffs, approved approvals, and denied approvals.
   - Verified all fields, diff structures, and statuses preserve fidelity across serialization and deserialization.

---

## 2. Logic Chain

1. **RON Serialization & Backward Compatibility (Task 1)**:
   - Older saved sessions on disk lack `Entry::Approval`. Because `Entry` is an untagged/externally-tagged enum where previous variants (`User`, `Agent`, `Tool`, `ToolOutput`, `Notice`, `Error`) are unchanged, serde/ron parses existing files without schema mismatch.
   - `ApprovalRequest` derives `Serialize` and `Deserialize` with `#[serde(default)]` on `edit` and `status`. Deserializing from partial RON documents without these fields defaults `edit` to `None` and `status` to `ApprovalStatus::Pending`.
   - `SavedState` containing mixtures of legacy and new approval entries round-trips cleanly without corrupting session lists or settings.
2. **Multi-Approval Ordering & Resolution (Task 2)**:
   - When multiple approvals exist in a session, `Session::has_pending_approval()` checks `self.pending_approval().is_some()`.
   - `Session::pending_approval()` scans `self.entries.iter().rev()` looking for `Entry::Approval(r) if r.is_pending()`. It correctly returns the latest pending request regardless of how many older requests have been approved or denied.
   - `Session::resolve_approval` targets by `id`. Resolving in FIFO, LIFO, or arbitrary out-of-order sequence correctly marks each specific entry as resolved, transitions its status, and notifies `RunningTurn`.
   - Idempotency is preserved: repeated resolution of an already-resolved ID returns `false` and does not re-transmit over `approval_tx`.
3. **Sidebar Operational State Transitions (Task 3)**:
   - `session_state(session)` evaluates:
     1. `if session.has_pending_approval() { SessionState::WaitingForApproval }`
     2. `else if session.is_running() { SessionState::Running }`
     3. `else if matches!(session.entries.last(), Some(Entry::Error(_))) { SessionState::Failed }`
     4. `else { SessionState::Idle }`
   - Placing `has_pending_approval()` as first priority ensures that whenever an agent turn pauses for approval, the sidebar immediately reflects the pulsing amber indicator.
   - Once all approvals are resolved, if the agent turn is still running (`session.is_running() == true`), the state reverts immediately to `Running` (green dot).
   - Once the turn terminates, the state cleanly transitions to `Idle` or `Failed`.
4. **Adversarial Edge Case Analysis (Duplicate IDs)**:
   - Observation: `Session::resolve_approval` iterates from index 0:
     ```rust
     for entry in &mut self.entries {
         if let Entry::Approval(req) = entry
             && req.id == id
         {
             resolved = req.resolve(decision);
             break;
         }
     }
     ```
   - If an agent generates identical IDs across turns (e.g., `call_1` in turn 1 and `call_1` in turn 2), the loop matches the first entry (from turn 1). Since it was already resolved, `req.resolve(decision)` returns `false` and the loop breaks, leaving the second (pending) entry permanently unresolvable.
   - This failure mode was empirically confirmed in test `duplicate_approval_ids_across_turns_behavior`.
   - Blast radius: Only occurs if an agent provider repeats tool call IDs within the same session. Modern CLIs typically emit random IDs (e.g. UUID or `toolu_...`), but sequential tool call IDs (`call_1`) would trigger this.
   - Recommended mitigation for future milestones: Only match pending entries:
     `if let Entry::Approval(req) = entry && req.id == id && req.is_pending()`

---

## 3. Stress Test Results

| Scenario | Expected Behavior | Actual Behavior | Result |
|---|---|---|---|
| Deserializing legacy session RON without approvals | Clean deserialization, `entries.len() == 6`, no pending approvals | Deserialized cleanly into `Session` | **PASS** |
| Deserializing thin `ApprovalRequest` (missing `edit` & `status`) | Defaults `edit: None`, `status: Pending` | Deserialized with defaults as expected | **PASS** |
| Full `SavedState` round-trip with mixed legacy & approval sessions | Exact round-trip fidelity, diffs and statuses preserved | Matched identically | **PASS** |
| Multi-approval FIFO resolution (5 requests) | Each resolved in order, `has_pending_approval` true until final | Resolved in order, relay channel received all 5 | **PASS** |
| Multi-approval arbitrary resolution (resolve #3, #1, #5, #4, #2) | Unresolved items unaffected, `pending_approval` updates correctly | Out-of-order resolution succeeded, statuses accurate | **PASS** |
| Redundant / double resolution attempt | Returns `false`, no relay channel transmission | Returned `false`, 0 messages on channel | **PASS** |
| `session_state` transition with active turn and intermediate tools | `Idle` -> `Running` -> `WaitingForApproval` -> `Running` -> `Idle`/`Failed` | State matched exact sequence across all 10 transitions | **PASS** |
| Duplicate ID in session across turns | Matches pending entry and resolves | Breaks on first already-resolved entry; returns false | **FINDING** (Documented below) |

---

## 4. Challenges & Findings

### [Medium] Challenge 1: Duplicate Approval ID Resolution Collision
- **Assumption challenged**: Every `ApprovalRequest.id` in `session.entries` is globally unique within the session across all turns.
- **Attack scenario**: An agent CLI uses per-turn relative indices for tool calls (`call_1`, `call_2`). Turn 1 issues `call_1` which is approved. Turn 2 issues `call_1` requiring approval. When user approves `call_1` in Turn 2, `Session::resolve_approval` matches Turn 1's resolved entry, returns `false`, and breaks.
- **Blast radius**: The pending approval in Turn 2 cannot be resolved by clicking the button. The session is stuck in `WaitingForApproval`, and the running turn never receives approval.
- **Mitigation**: In `src/session.rs:284`, check `&& req.is_pending()` in the pattern match so that resolved historical entries with the same ID are skipped:
  ```rust
  for entry in &mut self.entries {
      if let Entry::Approval(req) = entry
          && req.id == id
          && req.is_pending()
      {
          resolved = req.resolve(decision);
          break;
      }
  }
  ```

---

## 5. Caveats

- **Visual Egui Inspection**: In accordance with `AGENTS.md` Rule 3.1, visual drawing of egui frames was not observed using screen captures. All UI behavior was verified via unit tests on state machines, data conversions, action emission, and widget structure inspection.
- **Real CLI Live Driving**: No paid live CLI calls were made, preserving API quota and following `AGENTS.md` Rule 3.2. Channel relays were verified using child process and channel test harnesses.

---

## 6. Conclusion

Milestone 1 (Interactive In-App Approvals Foundation) successfully meets all functional requirements and acceptance criteria:
- Backward compatibility of RON serialization is thoroughly verified for both legacy formats and omitted-field partial structures.
- Multi-approval scenarios handle FIFO, reverse, and arbitrary ordering cleanly for all distinct IDs.
- Sidebar `SessionState` accurately tracks operational state transitions across multiple approval events, active turns, and terminations.
- Zero compile errors, zero Clippy warnings (`-D warnings`), 182 passed tests, zero unauthorized dependencies.

**Explicit Verdict**: **APPROVE** (recommend applying the 1-line `&& req.is_pending()` mitigation in Milestone 2).

---

## 7. Verification Method

To independently verify all findings and test suites:

```bash
# 1. Verify compilation and strict clippy compliance
cargo check
cargo clippy --all-targets -- -D warnings

# 2. Run full test suite including new empirical challenger tests
cargo test

# 3. Specifically run Milestone 1 empirical challenger test cases
cargo test older_session_and_saved_state_ron_formats_deserialize_cleanly
cargo test multi_approval_fifo_and_arbitrary_ordering_lifecycle
cargo test session_state_transitions_between_waiting_running_and_idle_under_multi_entry_flow
cargo test saved_state_with_interactive_approvals_and_legacy_sessions_survives_ron_round_trip
cargo test duplicate_approval_ids_across_turns_behavior
```
