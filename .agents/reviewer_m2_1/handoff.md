# Handoff Report: Reviewer M2-1 — Milestone 2 Evaluation

## Review Summary

**Verdict**: **APPROVE**

Milestone 2 (Git Worktree Session Isolation Foundation) implementation is verified to be sound, safe, and fully conforming to the requirements in `ORIGINAL_REQUEST.md`, `PROJECT.md`, and `AGENTS.md`. Zero integrity violations, zero build errors, zero test failures, and zero clippy warnings were observed.

---

## 1. Observation

1. **Compilation and Static Analysis**:
   - `cargo check`: Finished dev profile in 0.31s with exit code 0.
   - `cargo test`: `196 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 2.20s` with exit code 0. All 8 ignored tests were properly preserved without being run wholesale.
   - `cargo clippy --all-targets -- -D warnings`: Finished in 0.34s with exit code 0 and 0 warnings.
   - `git diff Cargo.toml`: Empty (zero new dependencies added).

2. **Module Layout and Exports**:
   - `src/main.rs`: Line 27 registers `mod worktree;`.
   - `src/worktree.rs`: Pure Rust module (495 lines) opening with doc comment `//! Git worktree management for session isolation.`. Implements `WorktreeInfo`, `worktree_path`, `worktree_branch`, `ensure_viper_ignored`, `create_worktree`, `remove_worktree`, `delete_branch`, `prune_worktrees`, `parse_worktree_list`, `list_worktrees`, and `repo_root`.
   - `src/git_diff.rs`: Line 118-135 implements `pub fn branch_changes(dir: &Path, base: &str) -> Result<Vec<FileDiff>, String>`. Line 189 marks `pub fn git_executable() -> Option<PathBuf>`.

3. **Session Worktree Tracking & Resolution**:
   - `src/session.rs`: Lines 94-101 add `#[serde(default)] pub worktree_dir: Option<PathBuf>`, `#[serde(default)] pub worktree_branch: Option<String>`, and `#[serde(default)] pub worktree_base: Option<String>` to `Session`.
   - `src/session.rs`: Lines 235-237 define `pub fn working_dir(&self) -> &Path { self.worktree_dir.as_deref().unwrap_or(&self.project_dir) }`.
   - `src/session.rs`: Line 250 in `Session::send` passes `cwd: self.working_dir().to_path_buf()` to `Turn`.
   - `src/session.rs`: Lines 300-310 update `resolve_approval` condition to `req.id == id && req.is_pending()`, fixing pending resolution when identical approval IDs appear across multiple turns.

4. **Changes & Tools UI Integration**:
   - `src/changes.rs`: Lines 16-20 introduce `Source::Branch { dir: PathBuf, branch: String, base: String }`.
   - `src/changes.rs`: Lines 66-71 update `watches(&self, dir: &Path)`:
     ```rust
     match &self.source {
         Source::Project(project) => project == dir || dir.starts_with(project),
         Source::Branch { dir: worktree, .. } => worktree == dir || worktree.starts_with(dir),
         Source::Files { .. } => false,
     }
     ```
   - `src/tools.rs`: Lines 126-143 implement `pub fn open_branch_changes(&mut self, dir: &Path, branch: &str, base: &str, ctx: &egui::Context)` with tab deduplication.
   - `src/app.rs`: Lines 272-279 implement `tool_cwd(&self) -> PathBuf` returning `session.working_dir().to_path_buf()`.
   - `src/app.rs`: Lines 304-322 implement `setup_session_worktree(&mut self, session_id: u64, base_ref: Option<&str>) -> Result<PathBuf, String>`.
   - `src/app.rs`: Lines 331-336 in `delete_session` call `remove_worktree` and `delete_branch` on session deletion.
   - `src/app.rs`: Lines 444-461 route `SidebarAction::OpenChanges` to `open_branch_changes` when the active session has an active worktree.
   - `src/app.rs`: Lines 777-780 trigger `panel.refresh_changes(session.working_dir(), ctx)` and `panel.refresh_changes(&session.project_dir, ctx)` on `AgentEvent::Exited`.

5. **Porcelain Parser Tolerance**:
   - `worktree::parse_worktree_list` is a total function tested against empty strings, junk input, and full porcelain records with detached, bare, locked, and prunable attributes.

---

## 2. Logic Chain

1. **Isolation from Project Root Without Breaking Sidebar Grouping**:
   - *Observation 3 & 4*: The sidebar groups sessions by `session.project_dir`. Changing `project_dir` to the worktree path would shatter workspace grouping in the sidebar.
   - *Logic*: Retaining `Session.project_dir` as the git repository root while routing process working directories, terminals, and file diffs through `Session::working_dir()` (`worktree_dir.unwrap_or(project_dir)`) guarantees complete agent process isolation while keeping session grouping intact.
   - *Conclusion*: Satisfies R2 and architecture requirements.

2. **Git Status Cleanliness & Exclude Safety**:
   - *Observation 2 & 5*: Creating worktrees under `.viper/worktrees/<session-id>` could make `.viper/` show up as untracked in `git status`.
   - *Logic*: `ensure_viper_ignored` dynamically inspects `.git/info/exclude` (resolving common git directories for linked worktrees) and appends `\n.viper/\n` only if not already present. It does not touch user-committed `.gitignore` files.
   - *Conclusion*: Keeps root repository git status pristine and clean.

3. **Backward-Compatible Persistence**:
   - *Observation 3 & 1*: `Session` uses `#[serde(default)]` on all new fields (`worktree_dir`, `worktree_branch`, `worktree_base`).
   - *Logic*: Verified by `session::tests::sessions_without_worktree_fields_deserialize_cleanly_with_defaults` and `app::tests::session_with_worktree_fields_serializes_and_deserializes_in_saved_state`. Older RON files deserialize without error or data loss.
   - *Conclusion*: Invariant from `AGENTS.md` Rule 3.5 is preserved.

4. **Approval Resolution Multi-Turn Fix**:
   - *Observation 3*: `resolve_approval` checking `req.is_pending()` prevents breaking early on an already resolved approval from a previous turn.
   - *Logic*: Validated by `session::tests::duplicate_approval_ids_across_turns_behavior`. Multi-turn approval workflows operate reliably.
   - *Conclusion*: Defect fix verified.

---

## 3. Caveats

1. **Worktree Base Ref Default ("HEAD")**:
   - When `create_worktree` is invoked with `base_ref: None`, it defaults to `"HEAD"`, creating the worktree branch at the current checkout's commit and setting `worktree_base = Some("HEAD")`.
   - If an agent in the worktree makes git commits, running `git diff HEAD` inside the worktree will diff against the worktree branch's new HEAD rather than the parent repository's original commit. For full branch diffing after commits, the caller or UI should supply an explicit base ref (e.g. `main` or commit SHA).
2. **Windows File Locks During Deletion**:
   - On Windows, if a terminal or external tool has an open handle in the worktree directory, `git worktree remove` may encounter file locks. The implementation handles this gracefully via fallback to `fs::remove_dir_all` and `git worktree prune`.
3. No other caveats.

---

## 4. Adversarial Review & Attack Surface Analysis

1. **Attack: Arbitrary Path Traversal in Worktree Directory**:
   - *Hypothesis*: Can an adversarial or malformed session ID escape `.viper/worktrees/` via `../`?
   - *Result*: Session ID is strictly typed as `u64`. String conversion yields exclusively ASCII digits `0..9`. Directory traversal is structurally impossible.
2. **Attack: Shell Injection via Worktree Branch or Ref Arguments**:
   - *Hypothesis*: Can shell command injection occur through branch or ref arguments?
   - *Result*: Commands are spawned directly via `hidden_command(git).args([...])` as separate argument vectors, bypassing the shell. No shell injection is possible.
3. **Attack: Corrupted or Malformed `git worktree list --porcelain` Output**:
   - *Hypothesis*: Can unexpected output crash `parse_worktree_list`?
   - *Result*: Tolerant string stripping (`strip_prefix`) and non-panicking option flushes handle empty lines, unknown keys, missing headers, and junk output safely.
4. **Attack: Git Status Pollution in Parent Repository**:
   - *Hypothesis*: Does creating an isolated session worktree mark the parent repo dirty?
   - *Result*: Tested in `creates_and_lists_isolated_worktree_in_temp_repo`. `git status --porcelain` in the parent repo confirms zero untracked `.viper/` entries.

---

## 5. Integrity Check

- **Hardcoded test results embedded in source code**: None found.
- **Dummy or facade implementations**: None found. All operations invoke real git binaries or mutate verified session structures.
- **Shortcuts bypassing the intended task**: None found. Complete lifecycle (create, list, remove, prune, delete branch, diff) implemented in pure Rust.
- **Fabricated verification outputs or logs**: None. Verification commands were executed independently.
- **Self-certifying work without genuine independent verification**: None. Real git repositories were created and inspected in temp directories during testing.

---

## 6. Conclusion

**Verdict: APPROVE**

Milestone 2 fulfills all requirements in `ORIGINAL_REQUEST.md` and `PROJECT.md`. The code is idiomatic, conforms to `AGENTS.md`, passes all 196 tests with 0 failures, maintains 0 clippy warnings, and introduces 0 new dependencies.

---

## 7. Verification Method

To independently reproduce this verification:
```powershell
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```
Key tests to inspect:
- `worktree::tests::creates_and_lists_isolated_worktree_in_temp_repo`
- `worktree::tests::session_worktree_modifications_isolate_from_main_repo`
- `worktree::tests::cleans_up_worktree_and_deletes_branch_cleanly`
- `session::tests::sessions_without_worktree_fields_deserialize_cleanly_with_defaults`
- `session::tests::duplicate_approval_ids_across_turns_behavior`
- `app::tests::app_setup_session_worktree_and_delete_session_cleans_up_on_disk`
- `changes::tests::branch_source_title_and_watches_match_worktree_and_project`
- `tools::tests::open_branch_changes_creates_branch_tab_and_reuses_it`
