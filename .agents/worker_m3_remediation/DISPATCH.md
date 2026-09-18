# Worker M3 Remediation Dispatch: Fix Preview Artifact Extraction

## Objective
Remediate the critical defect identified by Challenger M3-1 in Milestone 3: `src/preview.rs` (`extract_all_previewable_paths_from_tool`).

## Context & Inputs
- User Request: `C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md`
- Repository Rules: `C:\Users\ditob\Documents\viper\AGENTS.md`
- Gate Status: `C:\Users\ditob\Documents\viper\.agents\orchestrator_1\GATE_STATUS.md`
- Challenger M3-1 Report: `C:\Users\ditob\Documents\viper\.agents\challenger_m3_1\handoff.md`

## Defect Summary
In `src/preview.rs:106-139`, `extract_all_previewable_paths_from_tool` evaluates multi-word lines and sentences as candidate paths before whitespace splitting. Because `Path::extension()` looks only after the final `.`, phrases like `"create public/index.html"` or `"Write \"views/home.html\""` or `"curl -s https://example.com/site.html"` are treated as valid web paths.
This causes:
1. The first path returned by `previewable_path_from_tool` is `C:\project\create public/index.html` instead of `public/index.html`.
2. When the user clicks `[👁 Preview]` in `chat.rs`, WebView2 attempts to load the phantom path with verbs prepended, resulting in an immediate 404 error.
3. Comma-separated files in tool summaries (e.g. `"create a.html, update b.svg"`) are concatenated into a single corrupted filename.
4. Remote URLs (`https://.../site.html`) in shell commands are erroneously joined to `project_dir` as local file artifacts.

## Required Fix in `src/preview.rs`
1. In `extract_all_previewable_paths_from_tool`:
   - Ignore any candidate containing `"://"` (remote web URLs like `http://` or `https://` should NEVER be treated as local file artifacts).
   - Do NOT treat multi-word phrases or lines containing spaces as single file paths unless they start with a drive letter (`C:\...`) or root `/` and represent a single path without command verbs.
   - For detail strings, split by whitespace and/or quotes/commas to isolate individual file path tokens.
   - Strip enclosing quotes (`"`, `'`, `` ` ``), parentheses, brackets, commas, semicolons, and colons from each token.
   - If a cleaned token has a previewable extension (`is_previewable_web_path`), resolve it relative to `project_dir` and add to `paths`.
   - Ensure `previewable_path_from_tool`:
     - For `"create public/index.html"`, returns `Some(project.join("public/index.html"))`.
     - For `"create public/index.html, update assets/logo.svg"`, extracts both `public/index.html` and `assets/logo.svg`.
     - For `"Write \"views/home.html\""`, returns `Some(project.join("views/home.html"))`.
     - For `"curl -s https://example.com/site.html"`, returns `None`.
2. Also check `src/chat.rs:601` per Reviewer M3-2's advisory finding: pass `session.working_dir()` rather than `&session.project_dir` to `show_entry` so worktree sessions resolve relative paths correctly.
3. Add unit tests asserting each of these cases.

## Quality Gates
- `cargo check`
- `cargo test` (all tests pass)
- `cargo clippy --all-targets -- -D warnings` (0 warnings)
- Do NOT run `cargo fmt`.
- Do NOT add dependencies to `Cargo.toml`.

## MANDATORY INTEGRITY WARNING

## 2026-09-18T21:39:02Z
You are Worker M3 Remediation.
Your working directory is: C:\Users\ditob\Documents\viper\.agents\worker_m3_remediation
Read your instructions in: C:\Users\ditob\Documents\viper\.agents\worker_m3_remediation\DISPATCH.md
Read the user request in: C:\Users\ditob\Documents\viper\.agents\ORIGINAL_REQUEST.md
Read repository rules in: C:\Users\ditob\Documents\viper\AGENTS.md
Read the Challenger finding in: C:\Users\ditob\Documents\viper\.agents\challenger_m3_1\handoff.md

MANDATORY INTEGRITY WARNING:
DO NOT CHEAT. All implementations must be genuine. DO NOT hardcode test results, create dummy/facade implementations, or circumvent the intended task. A teamwork_preview_auditor will independently verify your work. Integrity violations WILL be detected and your work WILL be rejected.

Implement the required fixes in `src/preview.rs` and `src/chat.rs`.
Verify with:
- `cargo check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
Ensure all checks pass with 0 errors and 0 warnings.
Do NOT run `cargo fmt`.
Write your completion report in `C:\Users\ditob\Documents\viper\.agents\worker_m3_remediation\handoff.md` and message the orchestrator when finished.

