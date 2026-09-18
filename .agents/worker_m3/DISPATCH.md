# Worker M3 Dispatch: Webview Live Preview & Artifact Integration Foundation

## Objective
Implement Milestone 3: Webview Live Preview & Artifact Integration Foundation in Viper.

## Context & Inputs
- User Request: `C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md` (under `## 2026-09-18T20:52:15Z`)
- Repository Rules: `C:\Users\ditob\Documents\viper\AGENTS.md`
- Project Architecture: `C:\Users\ditob\Documents\viper\.agents\orchestrator_1\PROJECT.md`
- Explorer Specification: `C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3\handoff.md`

## Files You Own Exclusively
- `src/preview.rs` (New module - remember to register `mod preview;` in `src/main.rs`)
- `src/browser.rs`
- `src/tools.rs`
- `src/chat.rs`
- `src/app.rs`
- `src/main.rs` (only to add `mod preview;`)

## Detailed Tasks
1. **Create `src/preview.rs`**:
   - Open module with a `//!` doc line.
   - Define `PREVIEWABLE_EXTENSIONS: &[&str] = &["html", "htm", "svg", "xhtml"]`.
   - Implement `pub fn is_previewable_web_path(path: &Path) -> bool`.
   - Implement pure-Rust `pub fn path_to_file_url(path: &Path) -> String`:
     - Normalizes backslashes to `/`.
     - Trims UNC `\\?\`.
     - Percent-encodes spaces (`%20`), `#` (`%23`), `?` (`%3F`), `%` (`%25`).
     - Ensures proper `file:///` prefix for Windows (`file:///C:/...`) and Unix (`file:///...`).
   - Implement `pub fn file_url_to_path(url: &str) -> Option<PathBuf>`.
   - Implement `pub fn extract_previewable_artifacts(entries: &[Entry], project_dir: &Path) -> Vec<PathBuf>` which scans `Entry::Tool` (both `edit` and `detail`) for previewable files and deduplicates.
2. **Browser Programmatic Reload & URL Normalization (`src/browser.rs`)**:
   - In `BrowserState` and `SavedBrowserState`, add `#[serde(default = "default_true")] pub auto_refresh: bool` with helper `fn default_true() -> bool { true }`.
   - Update `normalize_url`: recognize local paths (e.g. starting with drive letter `[a-zA-Z]:[/\\]`, starting with `file://`, or pointing to existing files) and route through `path_to_file_url`.
   - Add `pub fn reload(&mut self)` to `Browser`: sets `self.pending_reload = true;`.
   - In `Browser::ui`: if `self.pending_reload`, push `Command::Reload` to `commands` and set `self.pending_reload = false;`.
3. **Tools Panel Preview Mounting (`src/tools.rs`)**:
   - Implement `pub fn mount_preview(&mut self, url: &str, reload_if_loaded: bool)`:
     - Finds an existing `Tab::Browser` or creates a new one.
     - Sets address and activates tab.
   - Implement `pub fn active_browser_url(&self) -> Option<&str>`.
   - In `open_changes` and `open_branch_changes`, resolve tab deduplication collision (separate matching for `Source::Project` vs `Source::Branch`).
4. **Chat Integration & Action Routing (`src/chat.rs` & `src/app.rs`)**:
   - In `src/chat.rs`, add `ConversationAction::Preview(PathBuf)` to `ConversationAction`.
   - In `show_entry` for `Entry::Tool`:
     - If `edit` or `detail` references a previewable web artifact, render a clean `[👁 Preview]` button emitting `ConversationAction::Preview(path)`.
   - In `src/app.rs`:
     - Handle `ConversationAction::Preview(path)`: convert via `preview::path_to_file_url(&path)`, mount in `self.active_tools().mount_preview(&url, true)`, set `self.state.show_tools = true;`, and call `ui.ctx().request_repaint();`.
     - On `AgentEvent::Exited`: check if active browser tab is displaying a `file://` URL inside `session.working_dir()` or `session.project_dir`. If so and `auto_refresh` is true, call `self.browser.reload()` and request repaint.
5. **Unit & Integration Tests**:
   - Test `is_previewable_web_path` for html, htm, svg, xhtml vs rs, txt, bin.
   - Test `path_to_file_url` on Windows drive letter paths, unix paths, spaces, and special chars.
   - Test round-trip between `path_to_file_url` and `file_url_to_path`.
   - Test `extract_previewable_artifacts` from session tool entries.
   - Test `mount_preview` tab creation and tab switching.
   - Test `BrowserState` RON serialization backward compatibility with and without `auto_refresh`.
6. **Strict Quality Gates**:
   - `cargo check`
   - `cargo test`
   - `cargo clippy --all-targets -- -D warnings`
   - 0 errors, 0 warnings.
   - Do NOT run `cargo fmt`.
   - Do NOT add any dependencies to `Cargo.toml`.

## MANDATORY INTEGRITY WARNING
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

## 2026-09-18T21:23:47Z
You are Worker M3 implementing Milestone 3: Webview Live Preview & Artifact Integration Foundation.
Your working directory is: C:\Users\ditob\Documents\viper\.agents\worker_m3
Read your instructions in: C:\Users\ditob\Documents\viper\.agents\worker_m3\DISPATCH.md
Read the user request in: C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md
Read repository rules in: C:\Users\ditob\Documents\viper\AGENTS.md
Read the Explorer report in: C:\Users\ditob\Documents\viper\.agents\teamwork_preview_explorer_survey_3\handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Implement all tasks listed in DISPATCH.md.
Run:
- `cargo check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
Ensure all checks pass with 0 errors and 0 warnings.
Do NOT run `cargo fmt`.
Write your completion report in `C:\Users\ditob\Documents\viper\.agents\worker_m3\handoff.md` and message the orchestrator when finished.

