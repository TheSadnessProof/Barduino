# Handoff Report: Challenger M3-1 — Webview Live Preview & Artifact Integration Foundation

**Verdict**: **REQUEST_CHANGES**  
**Risk Assessment**: **HIGH**

---

## 1. Observation

1. **Compilation, Unit Tests, and Clippy Baseline**:
   - `cargo check`: compiled with 0 errors and 0 warnings.
   - `cargo test`: 228 passed; 0 failed; 8 pre-existing machine-dependent tests ignored; 0 measured.
   - `cargo clippy --all-targets -- -D warnings`: passed with 0 warnings.

2. **Empirical URL Conversion & Roundtrip Stress Testing (`src/preview.rs:20-82`)**:
   - Verified `path_to_file_url` and `file_url_to_path` against an adversarial battery of paths (`preview::tests::file_url_and_path_roundtrip_adversarial_unicode_and_symbols`):
     - **Spaces & escaping**: `C:\My Documents\Test Project 2026\index.html` -> `file:///C:/My%20Documents/Test%20Project%202026/index.html` (roundtrips cleanly).
     - **Special URL characters**: `C:\special\#notes\what?now\100%_done\file.xhtml` -> `#` encoded as `%23`, `?` as `%3F`, `%` as `%25` (roundtrips cleanly without premature query or fragment truncation).
     - **Already escaped sequences**: `C:\already%20escaped\my%25file.html` -> encodes `%` as `%25`, preventing double-decoding corruption (roundtrips to identical literal string).
     - **Unicode & Internationalization**: Accented Latin (`café`, `über`, `naïve`), Chinese (`C:\文档\测试\项目\index.html`), Russian Cyrillic (`C:\документы\отчет\страница.html`), and multi-byte Emojis (`C:\📁\🚀\design.svg`) all roundtrip cleanly without data loss.
     - **Windows Drive Letters**: Lowercase (`c:\...`) and uppercase (`C:\...`) drive letters preserved.
     - **UNC Prefixes**: Strips `\\?\` and `\\.\` cleanly, and preserves network share paths (`\\server\share\web\index.html` -> `file:////server/share/web/index.html` -> `\\server\share\web\index.html`).
   - Verified `file_url_to_path` malformed input handling (`preview::tests::file_url_to_path_handles_malformed_and_boundary_inputs`):
     - Non-file URLs (`not_a_file_url`, `http://...`, `https://...`) return `None` safely without panics.
     - Truncated percent sequences (`file:///C:/path/incomplete%`, `file:///C:/path/incomplete%2`) and invalid hex bytes (`file:///C:/path/invalid%ZZ/index.html`) degrade gracefully without panicking.

3. **Empirical Extension Classification Testing (`src/preview.rs:14-18`)**:
   - `is_previewable_web_path` correctly recognizes `.html`, `.htm`, `.svg`, `.xhtml` case-insensitively (`.HTML`, `.HTM`, `.SVG`, `.XHTML`).
   - Correctly rejects non-web extensions (`.rs`, `.js`, `.css`, `.png`, `.md`), extra extension characters (`.htmll`, `.html5`, `.svgz`), multiple dots (`index.html.bak`), dotfiles (`.gitignore`, `.html`), and extensionless paths (`Makefile`, `index`).

4. **CRITICAL BUG DISCOVERY: Corrupted Path Extraction & Broken Chat Preview (`src/preview.rs:106-139`, `src/chat.rs:864-865`, `src/chat.rs:1103-1108`)**:
   - In `src/preview.rs:106-127`:
     ```rust
     for line in detail.lines() {
         for part in line.split(" · ") {
             let part = part.trim();
             let clean = part.trim_matches(['"', '\'', '`', '(', ')', '[', ']']);
             let p = Path::new(clean);
             if is_previewable_web_path(p) {
                 let full = resolve_path(p, project_dir);
                 if !paths.contains(&full) {
                     paths.push(full);
                 }
             }
             for word in part.split_whitespace() { ... }
         }
     }
     ```
   - In Rust, `std::path::Path::new("create public/index.html").extension()` returns `Some("html")` because `extension()` looks exclusively at bytes after the final `.`, disregarding leading words, commands, and quotes.
   - When tested against real-world agent tool outputs in `extract_all_previewable_paths_across_diverse_provider_tool_formats`, `extract_all_previewable_paths_from_tool` produced the following verbatim outputs:
     1. **Codex `file_change` single file** (`detail: "create public/index.html"`):
        `["C:\\my_workspace\\create public/index.html", "C:\\my_workspace\\public/index.html"]`
        The verb `create` was prepended into a phantom path!
     2. **Codex `file_change` multiple files** (`detail: "create public/index.html, update assets/logo.svg"`):
        `["C:\\my_workspace\\create public/index.html, update assets/logo.svg", "C:\\my_workspace\\public/index.html", "C:\\my_workspace\\assets/logo.svg"]`
        Both file names, the comma, and verbs were concatenated into a single phantom path!
     3. **Claude `Write` action** (`detail: "Write \"views/home.html\""`):
        `["C:\\my_workspace\\Write \"views/home.html", "C:\\my_workspace\\views/home.html"]`
        The verb `Write` and quote were prepended into a phantom path!
     4. **Shell command with web URL** (`detail: "curl -s https://example.com/site.html"`):
        `["C:\\my_workspace\\curl -s https://example.com/site.html", "C:\\my_workspace\\https://example.com/site.html"]`
        Remote URLs in shell executions were resolved as local files joined to `project_dir`!
   - In `src/preview.rs:133-139`:
     ```rust
     pub fn previewable_path_from_tool(
         edit: Option<&FileEdit>,
         detail: &str,
         project_dir: &Path,
     ) -> Option<PathBuf> {
         extract_all_previewable_paths_from_tool(edit, detail, project_dir).into_iter().next()
     }
     ```
     Because `previewable_path_from_tool` calls `.into_iter().next()`, it returns the **first** path found — which is the corrupted phantom path!
     - In `src/chat.rs:864-865` & `1103-1108`, the `[👁 Preview]` button in the transcript passes `preview_path` to `ConversationAction::Preview(path)`.
     - In `src/app.rs:587-593`, `ConversationAction::Preview(path)` calls `preview::path_to_file_url(&path)` and mounts `file:///C:/my_workspace/create%20public/index.html`.
     - In WebView2, this causes an immediate **404 / file not found navigation error** when the user clicks `[👁 Preview]` on Codex or Claude tool calls.
   - In `src/session.rs:240-242` & `src/app.rs:779`, `session.previewable_artifacts()` is polluted with phantom paths, preventing `session.previewable_artifacts().contains(&path)` from correctly matching the actual edited file during turn-exit auto-reload checks.

---

## 2. Logic Chain

1. **Root Cause in `extract_all_previewable_paths_from_tool`**:
   - *From Observation 4*: `extract_all_previewable_paths_from_tool` evaluates `part` directly before splitting whitespace.
   - *From Observation 4*: A tool detail line without `" · "` is treated as a single `part` (e.g. `"create public/index.html"` or `"Write \"views/home.html\""`).
   - *From Observation 4*: `is_previewable_web_path` uses `Path::extension()`, which ignores preceding spaces and words. Therefore, `is_previewable_web_path(Path::new("create public/index.html"))` returns `true`.
   - *Therefore*: The entire multi-word command/summary is pushed into `paths` as the first item before individual words are checked.

2. **UI & Auto-Reload Failure Cascade**:
   - *From Observation 4*: `previewable_path_from_tool` returns the first item from `extract_all_previewable_paths_from_tool`.
   - *From Observation 4*: For all Codex tool calls (`edit` is `None` in `codex.rs:149`) and Claude command executions, the first item is a corrupted non-existent path containing command verbs and quotes.
   - *From Observation 4*: Clicking `[👁 Preview]` in the chat log navigates the browser to this corrupted path, resulting in a 404 navigation error.
   - *From Observation 4*: `extract_previewable_artifacts` similarly collects these corrupted paths into the session artifact list, breaking exact artifact matching for auto-refresh.

3. **Remote URL Confusion**:
   - *From Observation 4*: When a tool executes a command referencing an external URL (`curl https://example.com/site.html`), `clean` is `https://example.com/site.html`.
   - Because `clean` ends with `.html` and `clean.is_absolute()` is false on Windows/POSIX, `resolve_path` joins `project_dir` with the remote URL (`C:\project\https:\example.com\site.html`).
   - *Therefore*: Shell commands downloading or curling web pages surface false `[👁 Preview]` buttons that attempt to load non-existent local file URLs.

---

## 3. Caveats

- **Visual Interface Rendering**: As mandated by `AGENTS.md` Rule 3.1 and the `verifying-a-ui-change` skill, the embedded `wry::WebView` and native egui pixels were not observed visually. All behaviors have been verified programmatically via test suites and empirical assertion harnesses.
- **Webview Local Security Policies**: WebViews enforce local origin policies for `file://` URLs; relative asset loading (`<img>`, `<link>`) works normally when valid local file paths are mounted.

---

## 4. Conclusion

**Verdict**: **REQUEST_CHANGES**

Milestone 3 cannot be approved in its current state due to a critical bug in `src/preview.rs` that breaks the primary user-facing capability of the milestone:
1. **Broken Preview Button**: The `[👁 Preview]` button in the chat log generates broken, non-existent URLs (`create%20public/index.html`, `Write%20%22views/home.html`) for Codex and Claude tool entries, causing WebView2 to display 404 errors when clicked.
2. **Corrupted Multi-Change Summaries**: Multiple files in a single tool summary (`create a.html, update b.svg`) are concatenated into a single non-existent path.
3. **Remote URL False Positives**: Remote URLs in shell commands (`curl https://.../index.html`) are erroneously extracted as local file artifacts.
4. **Session Artifact Pollution**: `session.previewable_artifacts()` is polluted with phantom paths, impeding turn-exit auto-reload matching.

### Required Changes for Implementer / Worker:
In `src/preview.rs` (`extract_all_previewable_paths_from_tool`):
1. **Filter Remote URLs**: Skip any candidate string that contains `"://"` (e.g. `http://`, `https://`).
2. **Do Not Treat Multi-Word Strings as Single Paths**:
   - Only treat `clean` as a single candidate path if it does **not** contain internal quotes (`"` or `'`), commas (`,`), or command verbs, and only if it does not contain whitespace unless it represents an already-verified path or starts with a platform root/drive letter.
   - Alternatively and more robustly: extract paths by scanning whitespace tokens (and quoted substrings like `"views/home.html"`), trimming surrounding punctuation (quotes, parentheses, brackets, commas, semicolons), and verifying `is_previewable_web_path` on each clean token.
3. **Verify That `previewable_path_from_tool` Returns the Clean Path**:
   - For `"create public/index.html"`, it must return `Some(project.join("public/index.html"))`.
   - For `"create public/index.html, update assets/logo.svg"`, it must extract both paths cleanly without concatenating them.
   - For `"Write \"views/home.html\""`, it must return `Some(project.join("views/home.html"))`.

---

## 5. Verification Method

To independently reproduce and verify this finding:

```powershell
# 1. Run the empirical test in src/preview.rs asserting these failure modes
cargo test preview::tests::extract_all_previewable_paths_across_diverse_provider_tool_formats -- --nocapture

# 2. Run the full unit test suite (228 tests pass)
cargo test

# 3. Verify strict clippy compliance (0 warnings)
cargo clippy --all-targets -- -D warnings
```

### Invalidation Condition
This finding is resolved and invalidated when `extract_all_previewable_paths_from_tool(None, "create public/index.html", project)` returns strictly `vec![project.join("public/index.html")]`, `previewable_path_from_tool` returns `Some(project.join("public/index.html"))`, and remote URLs (`curl https://...`) are excluded from local file artifact lists.
