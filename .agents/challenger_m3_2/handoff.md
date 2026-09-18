# Challenger Evaluation Report: Milestone 3 — Preview Mounting, Auto-Reload & RON Backward Compatibility

## Verdict: APPROVE

---

## 1. Observation

### 1.1 Preview Mounting & Tab Deduplication (`src/tools.rs:172-189`)
```rust
    pub fn mount_preview(&mut self, url: &str, _reload_if_loaded: bool) {
        let normalized = crate::browser::normalize_url(url);
        if let Some(index) = self.tabs.iter().position(|tab| matches!(tab, Tab::Browser { .. })) {
            if let Tab::Browser { state, .. } = &mut self.tabs[index] {
                state.address = normalized;
            }
            self.active = index;
        } else {
            let number = self.next_browser_number;
            self.next_browser_number += 1;
            let mut state = self.browser_states().last().cloned().unwrap_or_default();
            state.address = normalized;
            self.tabs.push(Tab::Browser { number, state });
            self.active = self.tabs.len() - 1;
        }
    }
```
- **Tab Creation**: When `mount_preview` is invoked without existing browser tabs, it initializes a new `Tab::Browser`, copies default or predecessor browser state, assigns `state.address = normalized`, appends it to `self.tabs`, and activates it.
- **Tab Reuse & Address Update**: When a browser tab already exists, `mount_preview` updates `state.address` directly on the existing tab and switches `self.active` to that tab index without duplicating the tab.
- **State Preservation**: Verified via `tools::tests::mounting_preview_preserves_viewport_and_size_while_updating_address` that custom `viewport` (e.g. `Viewport::Fixed`), custom pixel sizes (`[800, 600]`), and `auto_refresh` settings are strictly preserved when updating the address.
- **Path Normalization**: Verified via `tools::tests::mounting_preview_normalizes_raw_windows_path_and_reuses_tab_among_mixed_tabs` that raw Windows paths with spaces and special characters (`C:\work\my app\page #1.html`) are canonicalized into percent-encoded `file:///` URLs (`file:///C:/work/my%20app/page%20%231.html`) and activate the browser tab among mixed terminal and changes tabs.

### 1.2 Disambiguation: Project Changes vs Branch Changes (`src/tools.rs:116-120`, `src/tools.rs:130-134`)
```rust
    pub fn open_changes(&mut self, dir: &Path, ctx: &egui::Context) {
        let existing = self.tabs.iter().position(|tab| match tab {
            Tab::Changes(changes) => matches!(&changes.source, Source::Project(p) if p == dir),
            _ => false,
        });
...
    pub fn open_branch_changes(&mut self, dir: &Path, branch: &str, base: &str, ctx: &egui::Context) {
        let existing = self.tabs.iter().position(|tab| match tab {
            Tab::Changes(changes) => matches!(&changes.source, Source::Branch { dir: d, branch: b, .. } if d == dir && b == branch),
            _ => false,
        });
```
- Previously, `changes.watches(dir)` matched on `dir` alone without distinguishing `Source::Project` from `Source::Branch`, which created tab deduplication collisions between the project root and branch worktrees.
- Empirically verified via `tools::tests::project_changes_and_branch_changes_do_not_collide` and `tools::tests::multiple_branch_changes_and_project_changes_tabs_coexist_without_collision`:
  - Opening project changes for project A creates Tab 0 (`Source::Project`).
  - Opening branch changes for worktree 1 (`Source::Branch` for branch 1) creates Tab 1.
  - Opening branch changes for worktree 2 (`Source::Branch` for branch 2) creates Tab 2.
  - Opening project changes for project B creates Tab 3.
  - Re-invoking `open_branch_changes` on worktree 1 switches to Tab 1 without duplicating.
  - Re-invoking `open_changes` on project A switches to Tab 0 without duplicating.
  - Re-invoking `open_branch_changes` on worktree 2 switches to Tab 2 without duplicating.

### 1.3 Auto-Reload on Turn Exit in `app.rs` (`src/app.rs:773-785`)
```rust
    // Check if active browser tab is displaying a file:// URL inside the session folder.
    if let Some(panel) = self.tools.get(&id)
        && panel.active_browser_auto_refresh()
        && let Some(url) = panel.active_browser_url()
        && let Some(path) = preview::file_url_to_path(url)
    {
        let is_artifact = session.previewable_artifacts().contains(&path);
        if is_artifact || path.starts_with(session.working_dir()) || path.starts_with(&session.project_dir) {
            self.browser.reload();
            ctx.request_repaint();
        }
    }
```
- **Worktree & Root Path Awareness**: Verified via `app::tests::app_turn_exit_auto_reload_triggers_for_isolated_worktree_artifacts` that when a session runs inside an isolated git worktree (`session.worktree_dir` = `Some(wt)`), a preview showing a file within the worktree triggers `self.browser.reload()`.
- **Auto-Refresh Toggle**: Verified via `app::tests::app_turn_exit_auto_reload_suppressed_when_auto_refresh_is_false` that setting `auto_refresh = false` on the active browser tab suppresses reload on turn exit.
- **Unrelated URL Filtering**: Verified via `app::tests::app_turn_exit_auto_reload_suppressed_for_external_web_and_unrelated_file_urls` that external web URLs (`https://example.com`) and local files outside `working_dir`, `project_dir`, and `previewable_artifacts` do not trigger reloads.
- **Frontmost Tab Specificity**: Verified via `app::tests::app_turn_exit_auto_reload_suppressed_when_active_tab_is_not_browser` that when the frontmost tab is a Terminal or Changes tab, browser reloads are suppressed.
- **Event Lifecycle Discrimination**: Verified via `app::tests::app_turn_exit_auto_reload_only_triggers_on_exited_event_not_stream_or_finish` that intermediate turn streaming events (`AgentEvent::TextDelta`) and turn finish notifications (`AgentEvent::Finished`) do not trigger reloads; reload triggers strictly upon `AgentEvent::Exited`.

### 1.4 RON Backward Compatibility (`src/browser.rs`, `src/session.rs`, `src/app.rs`)
- `BrowserState` & `SavedBrowserState`:
  ```rust
  #[serde(default = "default_true")]
  pub auto_refresh: bool,
  ```
- Empirically verified via `app::tests::legacy_saved_state_ron_without_auto_refresh_deserializes_and_defaults_to_true` and `session::tests::sessions_without_auto_refresh_deserialize_cleanly_with_defaults`:
  - Legacy `SavedState` strings completely omitting `auto_refresh` deserialize cleanly, with `restored.browser.auto_refresh == true` and `session.browser.auto_refresh == true`.
  - Bare session RON records lacking `browser` deserialize cleanly with default browser settings.
  - Explicit `auto_refresh: false` round-trips with high fidelity through RON serialization and deserialization at both top-level `SavedState` and per-session levels (`session::tests::session_with_explicit_auto_refresh_false_roundtrips_through_ron`).

### 1.5 Full Verification Execution Results
- `cargo check`: Finished in 0.90s with exit code 0 (0 errors, 0 warnings).
- `cargo test`: 228 passed, 0 failed, 8 pre-existing machine/paid-dependent tests ignored in 2.17s.
- `cargo clippy --all-targets -- -D warnings`: Finished in 0.37s with exit code 0 (0 warnings).

---

## 2. Logic Chain

1. **Deterministic Preview Mounting**:
   - *From Observation 1.1*: `Tools::mount_preview` first checks `self.tabs.iter().position(...)` for `Tab::Browser`. If found, it updates the tab's address in place and focuses it; if absent, it allocates a single browser tab.
   - *From Observation 1.1*: Custom fixed viewport dimensions and user toggles are not overwritten during address updates.
   - *Therefore*: Preview mounting is idempotent, prevents duplicate browser tab explosion, and preserves user view configuration.

2. **Worktree Tab Isolation**:
   - *From Observation 1.2*: `open_changes` filters strictly for `Source::Project(p) if p == dir`, while `open_branch_changes` filters strictly for `Source::Branch { dir, branch, .. } if d == dir && b == branch`.
   - *Therefore*: Root project diffs and worktree branch diffs operate independently without colliding or stealing each other's tabs.

3. **Accurate Turn-Exit Triggering**:
   - *From Observation 1.3*: The auto-reload handler checks `panel.active_browser_auto_refresh()` and inspects whether the active URL is a `file://` scheme pointing inside `session.working_dir()` (which defaults to worktree path when worktrees are active), `session.project_dir`, or `session.previewable_artifacts()`.
   - *From Observation 1.3*: Non-exit events (deltas, turn finish) and background tabs do not fire reloads.
   - *Therefore*: Browser reloading occurs only when relevant web artifacts are modified and only when the preview tab is actively being viewed with auto-refresh enabled.

4. **Persistence Safety**:
   - *From Observation 1.4*: Both `BrowserState` and `SavedBrowserState` specify `#[serde(default = "default_true")]` for `auto_refresh`. Existing serialized session and state RON files on disk lack this field, so RON assigns `true` by default upon deserialization without parse failure.
   - *Therefore*: Upgrading preserves existing user sessions without breaking saved state (satisfying AGENTS.md Rule 3.5).

---

## 3. Caveats

- **Visual WebView Pixel Inspection**: In compliance with `AGENTS.md` Rule 3.1 and the `verifying-a-ui-change` skill, the embedded `wry::WebView` pixel rasterization was not observed visually. All behavior was verified through rigorous unit, behavioral, and state-transition tests.
- **Cross-Origin Restrictions in Local WebViews**: System webviews may restrict local `fetch` requests between different `file://` paths under modern browser security policies. Local HTML and SVG files referencing relative assets (`<img src="...">`, `<link rel="stylesheet" href="...">`) function as expected.

---

## 4. Conclusion

Milestone 3 (Webview Live Preview & Artifact Integration Foundation) satisfies all technical, architectural, and quality requirements:
- Preview mounting deduplicates cleanly and preserves viewport/size configurations.
- Tab deduplication between project root changes and branch worktree changes is strictly isolated.
- Agent turn-exit auto-reload accurately triggers for project and worktree artifacts while properly suppressing reloads for inactive tabs, disabled auto-refresh, and external web URLs.
- RON serialization is backward-compatible with older session and state files.
- The test suite comprises 228 passing tests with 0 failures, and `cargo clippy --all-targets -- -D warnings` reports 0 warnings.

**Verdict**: **APPROVE**

---

## 5. Verification Method

To independently verify these findings:

```powershell
# 1. Compilation check
cargo check

# 2. Run full test suite (228 passed, 0 failed, 8 pre-existing ignored)
cargo test

# 3. Run strict Clippy
cargo clippy --all-targets -- -D warnings

# 4. Target specific empirical challenge tests
cargo test -- tools::tests::mounting_preview
cargo test -- tools::tests::multiple_branch_changes_and_project_changes_tabs_coexist_without_collision
cargo test -- app::tests::app_turn_exit_auto_reload
cargo test -- app::tests::legacy_saved_state_ron_without_auto_refresh_deserializes_and_defaults_to_true
cargo test -- session::tests::sessions_without_auto_refresh_deserialize_cleanly_with_defaults
```
