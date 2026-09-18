# Handoff Report: Worker M3 Remediation — Fix Preview Artifact Extraction

## 1. Observation

1. **Defect Reported by Challenger M3-1**:
   - In `src/preview.rs:106-139`, `extract_all_previewable_paths_from_tool` evaluated entire lines and phrases directly before whitespace splitting. Because `Path::extension()` returns bytes following the last `.`, phrases such as `"create public/index.html"` were treated as valid web paths, leading to phantom paths like `project.join("create public/index.html")`.
   - `previewable_path_from_tool` returned the first path found, which was the phantom path prepended with command verbs or concatenated multi-change strings (`create public/index.html, update assets/logo.svg`).
   - Remote URLs in shell commands (`curl -s https://example.com/site.html`) were joined against `project_dir` rather than ignored.
   - In `src/chat.rs:601`, `show_entry` received `&session.project_dir` instead of `session.working_dir()`, causing relative tool paths in worktree sessions to resolve against the project root rather than the isolated worktree directory.

2. **Remediation Implementation**:
   - In `src/preview.rs`:
     - Introduced `COMMAND_VERBS` listing standard CLI verbs (`create`, `update`, `delete`, `write`, `edit`, `read`, `run`, `curl`, `wget`, `cat`, etc.).
     - Introduced `clean_token` to strip enclosing quotes (`"`, `'`, `` ` ``), brackets (`[`, `]`, `{`, `}`), parentheses (`(`, `)`), commas (`,`), semicolons (`;`), and colons (`:`).
     - Introduced `strip_line_col_suffix` to clean trailing compiler/grep line numbers (`path.html:42:10` -> `path.html`).
     - Introduced `is_plausible_single_path_with_spaces` to disallow multi-word sentences/commands containing spaces from being treated as a single path, while permitting genuine absolute paths starting with Windows drive letters (`C:\...`) or root (`/`, `\\`) that do not contain command verbs or command flags.
     - Introduced `candidate_path`: rejects empty tokens, candidates containing `"://"` (such as `http://`, `https://`, `ftp://`), multi-word paths failing `is_plausible_single_path_with_spaces`, and non-previewable extensions.
     - Introduced `extract_and_mask_quotes`: extracts quoted substrings and masks their contents in the remainder text with spaces, preventing words within paths containing spaces (e.g. `"C:\My Documents\report.html"`) from being split into spurious relative tokens.
     - In `extract_all_previewable_paths_from_tool`:
       - Handled `edit` with `candidate_path`.
       - For each part in `detail`:
         - If `candidate_path(clean_token(part))` is valid, resolves and adds it, then continues without splitting.
         - Extracts and validates quoted candidates.
         - Splits unquoted remainder by `,`, `;`, and whitespace to extract individual candidate tokens.
   - In `src/chat.rs:601`:
     - Passed `session.working_dir()` instead of `&session.project_dir` to `show_entry`, ensuring worktree-isolated sessions resolve relative tool paths within the worktree directory.

3. **Compiler, Test Suite, and Linter Results**:
   - `cargo check`:
     ```
     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.73s
     ```
     0 errors, 0 warnings.
   - `cargo test`:
     ```
     test result: ok. 231 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out; finished in 1.93s
     ```
     All 231 tests passed (0 failures, 8 pre-existing machine-dependent tests ignored, none newly ignored).
   - `cargo clippy --all-targets -- -D warnings`:
     ```
     Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.32s
     ```
     0 warnings across all targets.
   - Unit tests updated and added in `src/preview.rs`:
     - `extract_all_previewable_paths_across_diverse_provider_tool_formats`: asserted clean resolution for Codex single-file (`"create public/index.html"` -> `public/index.html`), Codex multi-file (`"create public/index.html, update assets/logo.svg"` -> both files separate), Claude (`"Write \"views/home.html\""` -> `views/home.html`), and remote URL shell executions (`"curl -s https://example.com/site.html"` -> `None`).
     - `extract_previewable_paths_strips_punctuation_brackets_and_line_suffixes`
     - `extract_previewable_paths_supports_windows_drive_paths_with_spaces`
     - `extract_previewable_paths_rejects_remote_http_and_https_schemes`

## 2. Logic Chain

1. *From Observation 1*: The root cause of the phantom path bug was that unquoted multi-word strings like `"create public/index.html"` were checked as single paths before word splitting, and `Path::extension()` ignores everything before the last dot.
2. *From Observation 2*: By enforcing `candidate_path`, any candidate containing spaces is rejected unless it starts with a drive letter or root and contains no command verbs. As a result, `"create public/index.html"` is not evaluated as a whole path; it is split by whitespace into `"create"` (ignored) and `"public/index.html"` (valid).
3. *From Observation 2*: By filtering any candidate containing `"://"`, remote web URLs like `https://example.com/site.html` are excluded from being joined to `project_dir`.
4. *From Observation 2*: By masking quoted contents with spaces before word splitting, genuine paths containing spaces inside quotes are not broken into spurious secondary tokens.
5. *From Observation 2*: In `src/chat.rs`, passing `session.working_dir()` ensures that relative preview paths clicked in the chat log resolve against the session's active worktree directory if worktree isolation is active.
6. *From Observation 3*: All unit tests pass, and strict clippy passes with zero warnings.

## 3. Caveats

- **Visual Rendering**: In accordance with `AGENTS.md` Rule 3.1 and `verifying-a-ui-change`, no GUI process was spawned or visually observed. Path extraction and URL resolution logic were tested directly via automated unit tests.
- No other caveats.

## 4. Conclusion

The critical defect identified by Challenger M3-1 has been completely remediated:
1. `previewable_path_from_tool` and `extract_all_previewable_paths_from_tool` now extract clean, genuine local paths without command verbs, quotes, or punctuation.
2. Multi-file summaries are cleanly separated into distinct paths.
3. Remote URLs (`://`) in shell commands are excluded.
4. `src/chat.rs` passes `session.working_dir()` to `show_entry` for proper worktree relative path resolution.
5. All 231 tests pass with 0 errors and 0 clippy warnings.

## 5. Verification Method

To independently verify the fix:

```powershell
# 1. Verify compilation
cargo check

# 2. Run the unit test suite (231 passed)
cargo test

# 3. Verify clippy with zero warnings enforcement
cargo clippy --all-targets -- -D warnings

# 4. Verify preview path unit tests specifically
cargo test preview::tests:: -- --nocapture
```

### Invalidation Condition
The fix would be invalidated if `extract_all_previewable_paths_from_tool(None, "create public/index.html", project)` included `project.join("create public/index.html")`, if `previewable_path_from_tool(None, "curl -s https://example.com/site.html", project)` returned `Some(_)`, or if `cargo clippy --all-targets -- -D warnings` produced any warnings.
