# Handoff Report: Webview Live Preview & Artifact Integration Foundation (Requirement R3)

## 1. Observation

### 1.1 Embedded WebView Architecture (`src/browser.rs`, `src/tools.rs`, `src/app.rs`)
- **Shared Singleton WebView**:
  - In `src/app.rs` line 135, `ViperApp` owns a single `browser: Browser` instance and a per-session map `tools: BTreeMap<u64, Tools>` (line 133).
  - In `src/session.rs` line 116, each `Session` struct owns its persistent state `pub browser: BrowserState`.
  - In `src/tools.rs` line 27-30, `Tab::Browser { number: u64, state: BrowserState }` holds the tab's state. When drawn in `tools.ui` (lines 370–388):
    ```rust
    if state.address.is_empty() && !page.address.is_empty() {
        *state = page.clone();
    }
    let page_visible = !egui::Popup::is_any_open(ui.ctx());
    match browser.ui((id, *number), state, ui, frame, page_visible) {
        BrowserAction::Attach(elements) => action = ToolsAction::Attach(elements),
        BrowserAction::Send(elements) => action = ToolsAction::Send(elements),
        BrowserAction::None => {}
    }
    *page = state.clone();
    ```
  - In `src/browser.rs` lines 422–433:
    `native::NativeBrowser::create(ui.ctx(), frame, &url)` lazily initializes the `wry::WebView` child window over the egui allocated rect.
  - In `src/browser.rs` lines 271–275:
    ```rust
    let wanted = normalize_url(&state.address);
    if !self.loaded.is_empty() && self.loaded != wanted {
        commands.push(Command::Load(wanted));
    }
    ```
    If `self.loaded == wanted`, no `Command::Load` is issued.
  - In `src/browser.rs` lines 776–780:
    ```rust
    Command::Reload => self.webview.reload(),
    Command::Load(url) => self.webview.load_url(&url),
    ```
    Only the manual reload button `⟳` (line 309) currently emits `Command::Reload`. There is no programmatic reload method on `Browser`.

### 1.2 URL Normalization & Local File Limitations (`src/browser.rs`)
- In `src/browser.rs` lines 639–650:
  ```rust
  fn normalize_url(input: &str) -> String {
      let input = input.trim();
      if input.is_empty() {
          "about:blank".to_owned()
      } else if input.contains("://") || input.starts_with("about:") {
          input.to_owned()
      } else if input.starts_with("localhost") || input.starts_with("127.0.0.1") || input.starts_with("[::1]") {
          format!("http://{input}")
      } else {
          format!("https://{input}")
      }
  }
  ```
  - While `file:///...` passes through untouched because `input.contains("://")` is true, raw filesystem paths such as `C:\work\index.html` or `/tmp/preview.html` or relative paths like `index.html` are mistakenly prefixed with `https://`, resulting in `https://C:\work\index.html` which fails to load.
  - Neither `url` nor any URI encoding crate is in `Cargo.toml`. Per Rule 3.4 of `AGENTS.md`, no dependencies may be added.

### 1.3 Agent Artifact Generation Across Providers (`src/agent.rs`, `src/claude.rs`, `src/codex.rs`, `src/antigravity.rs`)
- **Claude** (`src/claude.rs` lines 92–96, 190–214):
  - Invocations of `Edit`, `Write`, and `MultiEdit` tools produce `AgentEvent::ToolUse { name, detail, edit: Some(FileEdit) }`.
  - `FileEdit` (`src/line_diff.rs` lines 16–25) explicitly stores `pub path: String`, `pub old: String`, and `pub new: String`.
- **Codex** (`src/codex.rs` lines 148–152, 283, 336–350):
  - `file_change` items provide changed files via `changes(item)` (`format!("{kind} {path}")`), placed in `detail`.
- **Antigravity** (`src/antigravity.rs` lines 172–176, 187–200):
  - Invocations of `write_to_file` and `replace_file_content` place the file path in parameters (`TargetFile`, `AbsolutePath`), which `tool_detail` puts into `detail`.
- **Session History** (`src/session.rs` lines 22–28, 281–284):
  - `Entry::Tool { name, detail, edit: Option<FileEdit> }` stores completed and ongoing tool invocations in the session transcript.

### 1.4 Post-Turn Lifecycle in App (`src/app.rs`)
- In `src/app.rs` lines 696–702:
  ```rust
  // The agent may have edited files, so any diff of its folder is out of date.
  if let AgentEvent::Exited { .. } = &event {
      for panel in self.tools.values_mut() {
          panel.refresh_changes(&session.project_dir, ctx);
      }
      self.sidebar.invalidate(&session.project_dir);
  }
  ```
  `AgentEvent::Exited` is already the central trigger that invalidates working tree diffs and refreshes the Changes panel.

### 1.5 Conversation UI Actions (`src/chat.rs`)
- In `src/chat.rs` lines 45–49:
  ```rust
  pub enum ConversationAction {
      None,
      ChangeFolder,
      SelectProvider(Provider),
  }
  ```
  `show_entry` renders `Entry::Tool` via `tool_row` (lines 917–948) and `edit_view` (lines 950–975). Currently, there are no action buttons for tools or file edits to open in the browser.

---

## 2. Logic Chain

### Step 1: WebView Lifecycle & Invariant Analysis
- Observation 1.1 shows that `wry::WebView` is a singleton native window attached to the application window.
- The webview only issues a `Command::Load(wanted)` when `self.loaded != wanted`.
- When an agent modifies an existing file on disk that is already displayed in the webview (e.g. `index.html`), `state.address` and `self.loaded` remain unchanged. Because `self.loaded == wanted`, `Browser::ui` will not navigate or reload.
- Therefore, for live preview to reflect file edits on disk, `Browser` must expose a reload mechanism (e.g. `browser.reload()`), setting a pending reload flag that issues `Command::Reload`.

### Step 2: Pure-Rust URL & File Path Conversion
- Observation 1.2 shows that `normalize_url` does not recognize local filesystem paths, corrupting them into `https://` URLs.
- In addition, `Cargo.toml` has no URI library, so file-to-URL conversion must be performed in pure Rust:
  - `path_to_file_url(path: &Path) -> String`:
    - Normalizes path separators to `/`.
    - Trims verbatim UNC prefixes (`\\?\`).
    - Ensures absolute paths (joining relative paths with `project_dir` / `cwd`).
    - Standardizes the scheme:
      - Windows: `file:///C:/path/to/file.html` (three slashes followed by drive letter).
      - Unix: `file:///path/to/file.html`.
    - Percent-encodes reserved characters: space (`%20`), `#` (`%23`), `?` (`%3F`), `%` (`%25`).
  - `file_url_to_path(url: &str) -> Option<PathBuf>`:
    - Strips `file:///` (or `file://`).
    - On Windows, strips leading `/` before drive letters (`/C:` -> `C:`).
    - Percent-decodes `%20` etc.
  - Updating `normalize_url`:
    If the trimmed input starts with a drive letter (e.g., `[a-zA-Z]:[/\\]`), starts with `/`, starts with `file://`, or points to an existing file on disk, route through `path_to_file_url`.

### Step 3: Previewable Artifact Detection
- Based on web standards and agent behaviors (Observation 1.3), previewable artifacts are files that the native browser engine can render directly without an external build server:
  - Extensions: `.html`, `.htm`, `.svg`, `.xhtml`.
  - In addition, local dev server URLs output in agent text or tool output (`http://localhost:\d+`, `http://127.0.0.1:\d+`) are recognized as live web preview targets.
- Detection can be extracted from three levels:
  1. `FileEdit.path` (available directly in Claude turns and stored in `Entry::Tool`).
  2. `Entry::Tool` details / parameters (for Codex and Antigravity file operations).
  3. `Session::entries` scan:
     `extract_previewable_artifacts(entries: &[Entry], project_dir: &Path) -> Vec<PathBuf>`:
     Scans session entries in reverse chronological order, resolves paths against `project_dir`, and returns deduplicated previewable file paths.

### Step 4: Live Preview Mounting Logic
- In `src/tools.rs`, implement `mount_preview(&mut self, url: &str, reload_if_loaded: bool)`:
  - Finds an existing `Tab::Browser` or creates a new one (`Tab::Browser { number, state: BrowserState { address: url, .. } }`).
  - If a browser tab exists:
    - If `tab.state.address == url`: triggers a reload if `reload_if_loaded` is true.
    - If `tab.state.address != url`: sets `tab.state.address = url.to_owned()`.
  - Sets `self.active` to that browser tab.
- In `src/browser.rs`:
  - Add `pub fn reload(&mut self)` which sets `self.pending_reload = true`.
  - In `Browser::ui`, when `self.pending_reload` is true, push `Command::Reload` to `commands` and reset the flag.

### Step 5: One-Click UI Integration
- **In Chat (`src/chat.rs`)**:
  - Extend `ConversationAction`:
    ```rust
    pub enum ConversationAction {
        None,
        ChangeFolder,
        SelectProvider(Provider),
        Preview(PathBuf),
    }
    ```
  - In `show_entry` for `Entry::Tool`:
    - When `edit.path` or `detail` points to a previewable artifact (`.html`, `.svg`, etc.), render a compact `[👁 Preview]` button in `tool_row` or `edit_view`.
    - Clicking it returns `ConversationAction::Preview(full_path)`.
  - In `src/app.rs` (`chat_area`):
    - Handle `ConversationAction::Preview(path)`:
      - Convert path to `file://` URL via `path_to_file_url(&path)`.
      - Mount preview in active session tools: `self.active_tools().mount_preview(&url, true)`.
      - Expand tools panel: `self.state.show_tools = true;`.
      - Call `ui.ctx().request_repaint()`.
- **In Tools Panel (`src/tools.rs`)**:
  - In `add_menu` (`+` popup):
    - If `session.previewable_artifacts()` has files, list:
      `Preview <filename>` (e.g. `Preview index.html`, `Preview icon.svg`).
  - In `Browser` tab toolbar:
    - When viewing a `file://` URL, show an active preview indicator: `Live Preview: <filename>` and an "Auto-refresh" toggle.

### Step 6: Auto-Refresh Mechanism
- In `BrowserState`, add `pub auto_refresh: bool` (default: `true`).
- In `src/app.rs` line 696:
  When `AgentEvent::Exited` arrives:
  - Check whether the session's active tools panel has a browser tab showing a `file://` URL.
  - If the active browser tab's URL corresponds to a file within `session.project_dir` and `auto_refresh` is true:
    - Call `browser.reload()`.
    - Request repaint via `ctx.request_repaint()`.
- This guarantees that when an agent modifies an HTML or SVG artifact during a turn, the preview in the right panel updates immediately without manual user intervention.

### Step 7: Architecture Design & Layout Compliance
- Module placement options:
  - Option A: Create `src/preview.rs` opening with `//! Live preview and web artifact integration...` containing detection, URL formatting, and artifact extraction, re-exported or used by `browser.rs`, `tools.rs`, and `chat.rs`.
  - Option B: Place URL and preview helpers directly in `src/browser.rs`.
  - Recommendation: Option A (`src/preview.rs`) is cleanest because it keeps artifact detection and path-to-URL conversion as pure, testable domain logic independent of `wry` and `egui`, fulfilling the `verifying-a-ui-change` skill guideline.
- Serialization backward compatibility:
  - Ensure `SavedBrowserState` has `#[serde(default)]` on `auto_refresh`.
  - Existing RON files deserialize without error.

---

## 3. Caveats

1. **Local File Script & CORS Restrictions in WebViews**:
   - Modern browser engines (Edge WebView2 on Windows and WKWebView on macOS) enforce strict cross-origin policies on `file://` URLs. Fetching external files or resources via relative `fetch()` or `XMLHttpRequest` across different directories may be blocked by browser security policies. Standard assets (`<img src="...">`, `<link rel="stylesheet">`, `<script src="...">`) within the same directory tree load normally.
2. **Complex Multi-File Framework Applications**:
   - Framework projects (React/Vue/Next.js/Vite) requiring build steps or dev servers cannot be rendered directly from raw `App.tsx` or `.vue` files. For those, live preview mounts the dev server URL (e.g. `http://localhost:5173`) rather than a `file://` URL.
3. **Headless Testing Inability to Verify Actual Pixel Rendering**:
   - As mandated by AGENTS.md rule 3.1 and `verifying-a-ui-change` skill, automated unit tests cannot launch or take screenshots of `wry::WebView`. All verification must rely on testing the pure data-flow functions (URL conversion, path parsing, artifact detection, mounting state transitions) rather than visual confirmation.
4. **Platform URL Variations**:
   - Windows drive letters require three slashes (`file:///C:/...`), whereas POSIX paths also use three slashes (`file:///home/...`). Tests must validate both platforms explicitly.

---

## 4. Conclusion

The Webview Live Preview & Artifact Integration Foundation (Requirement R3) can be implemented cleanly, with zero new dependencies and complete backward compatibility, across the following components:

### Technical Design Specifications

1. **`src/preview.rs` (New Module)**:
   ```rust
   //! Live preview and web artifact integration: detects previewable files (HTML, SVG,
   //! templates) created by agents, mounts them as file:// URLs, and manages auto-refresh.

   use std::path::{Path, PathBuf};
   use crate::session::Entry;

   pub const PREVIEWABLE_EXTENSIONS: &[&str] = &["html", "htm", "svg", "xhtml"];

   /// Whether a file path can be previewed directly in the embedded browser.
   pub fn is_previewable_web_path(path: &Path) -> bool {
       path.extension()
           .and_then(|ext| ext.to_str())
           .map(|ext| PREVIEWABLE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
           .unwrap_or(false)
   }

   /// Converts a local file path to a valid file:// URL.
   pub fn path_to_file_url(path: &Path) -> String {
       let normalized = path.to_string_lossy().replace('\\', "/");
       let trimmed = normalized.strip_prefix("//?/").unwrap_or(&normalized);
       let mut encoded = String::new();
       for ch in trimmed.chars() {
           match ch {
               ' ' => encoded.push_str("%20"),
               '#' => encoded.push_str("%23"),
               '?' => encoded.push_str("%3F"),
               '%' => encoded.push_str("%25"),
               _ => encoded.push(ch),
           }
       }
       if encoded.starts_with('/') {
           format!("file://{encoded}")
       } else {
           format!("file:///{encoded}")
       }
   }

   /// Converts a file:// URL back to a local filesystem PathBuf.
   pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
       let stripped = url.strip_prefix("file://")?;
       let path_part = if let Some(rest) = stripped.strip_prefix('/') {
           // On Windows, "/C:/foo" -> "C:/foo"
           if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
               rest
           } else {
               stripped
           }
       } else {
           stripped
       };
       let decoded = percent_decode(path_part);
       Some(PathBuf::from(decoded.replace('/', std::path::MAIN_SEPARATOR_STR)))
   }

   fn percent_decode(input: &str) -> String { ... }

   /// Scans session entries to find all generated or edited web artifacts.
   pub fn extract_previewable_artifacts(entries: &[Entry], project_dir: &Path) -> Vec<PathBuf> {
       let mut artifacts = Vec::new();
       for entry in entries.iter().rev() {
           if let Entry::Tool { edit, detail, .. } = entry {
               if let Some(edit) = edit {
                   let p = Path::new(&edit.path);
                   if is_previewable_web_path(p) {
                       let full = if p.is_absolute() { p.to_path_buf() } else { project_dir.join(p) };
                       if !artifacts.contains(&full) {
                           artifacts.push(full);
                       }
                   }
               }
               // Also check detail for tools without explicit FileEdit
               ...
           }
       }
       artifacts
   }
   ```

2. **`src/browser.rs` Enhancements**:
   - Update `BrowserState`:
     ```rust
     #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
     #[serde(from = "SavedBrowserState")]
     pub struct BrowserState {
         pub address: String,
         pub viewport: Viewport,
         pub size: [u32; 2],
         pub auto_refresh: bool,
     }
     ```
   - Update `SavedBrowserState` with `#[serde(default)] pub auto_refresh: bool` to maintain backward compatibility.
   - Update `normalize_url`: recognize local paths and convert via `path_to_file_url`.
   - Add `pub fn reload(&mut self)` to `Browser`, issuing `Command::Reload`.

3. **`src/tools.rs` Enhancements**:
   - Add `pub fn mount_preview(&mut self, url: &str, reload_if_loaded: bool)`:
     Mounts the target URL into an existing or new `Tab::Browser` and sets it active.
   - Add `pub fn refresh_preview_if_active(&mut self, dir: &Path, ctx: &egui::Context)`:
     Reloads the active browser tab if it is displaying a file inside `dir`.
   - Update `add_menu` to display available previewable artifacts.

4. **`src/chat.rs` & `src/app.rs` Enhancements**:
   - Add `ConversationAction::Preview(PathBuf)` in `chat.rs`.
   - In `show_entry` for `Entry::Tool`, display a `[👁 Preview]` button next to previewable artifacts.
   - In `src/app.rs`:
     - Handle `ConversationAction::Preview(path)` by mounting in `active_tools()` and expanding the tools panel.
     - On `AgentEvent::Exited`, call `active_tools().refresh_preview_if_active(&session.project_dir, ctx)`.

---

## 5. Verification Method

### Test Suite Execution
1. Run standard unit tests:
   ```powershell
   cargo test
   ```
2. Run strict clippy verification:
   ```powershell
   cargo clippy --all-targets -- -D warnings
   ```
3. Run project check:
   ```powershell
   cargo check
   ```

### Specific Unit Tests to Implement
All test names must be complete descriptive sentences per `AGENTS.md`:
- `fn recognizes_html_svg_and_xhtml_as_previewable_artifacts()`
- `fn ignores_non_web_extensions_like_rust_or_binary_files()`
- `fn converts_windows_absolute_path_to_file_url()`
- `fn converts_unix_absolute_path_to_file_url()`
- `fn handles_spaces_and_special_characters_in_file_url_encoding()`
- `fn roundtrips_path_to_file_url_and_back()`
- `fn extracts_previewable_artifacts_from_session_entries()`
- `fn mounting_preview_opens_or_switches_to_browser_tab()`
- `fn mounting_same_preview_twice_requests_reload_instead_of_duplicate_tab()`
- `fn auto_refresh_reloads_browser_when_watched_file_is_modified()`
- `fn browser_state_with_auto_refresh_preserves_ron_roundtrip()`

### Invalidation Conditions
- Any change requiring an external dependency in `Cargo.toml`.
- Any modification causing existing RON serialized sessions to fail loading.
- Any change that drives the global mouse, captures screens, or leaves background processes running.
