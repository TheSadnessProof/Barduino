//! Web artifact preview and file URL conversion: detects previewable files (HTML, SVG,
//! XHTML) created by agents, converts between filesystem paths and file:// URLs, and
//! extracts previewable artifacts from session histories.

use std::path::{Path, PathBuf};

use crate::line_diff::FileEdit;
use crate::session::Entry;

/// File extensions that can be rendered directly by the embedded webview without a build step.
pub const PREVIEWABLE_EXTENSIONS: &[&str] = &["html", "htm", "svg", "xhtml"];

/// Whether a file path can be previewed directly in the embedded browser.
pub fn is_previewable_web_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| PREVIEWABLE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Converts a local filesystem path to a standard `file:///` URL with percent-encoding.
pub fn path_to_file_url(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let trimmed = normalized
        .strip_prefix("//?/")
        .or_else(|| normalized.strip_prefix("//./"))
        .unwrap_or(&normalized);
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

/// Converts a `file://` URL back to a local filesystem [`PathBuf`].
pub fn file_url_to_path(url: &str) -> Option<PathBuf> {
    let stripped = url.strip_prefix("file://")?;
    let path_part = if let Some(rest) = stripped.strip_prefix('/') {
        // On Windows, "/C:/foo" -> "C:/foo"
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' && rest.as_bytes()[0].is_ascii_alphabetic() {
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

/// Decodes percent-encoded character sequences like `%20` or `%23`.
fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::new();
    let input_bytes = input.as_bytes();
    let mut i = 0;
    while i < input_bytes.len() {
        if input_bytes[i] == b'%'
            && i + 2 < input_bytes.len()
            && let (Some(h1), Some(h2)) = (
                (input_bytes[i + 1] as char).to_digit(16),
                (input_bytes[i + 2] as char).to_digit(16),
            )
        {
            bytes.push((h1 * 16 + h2) as u8);
            i += 3;
            continue;
        }
        bytes.push(input_bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Common command verbs and shell tools found in agent tool details.
const COMMAND_VERBS: &[&str] = &[
    "create", "update", "delete", "write", "edit", "read", "run", "curl", "wget",
    "cat", "echo", "python", "node", "bash", "sh", "git", "powershell", "cmd", "type",
    "diff", "patch", "fetch", "open", "show",
];

/// Trims enclosing quotes, brackets, parentheses, braces, commas, semicolons, and colons.
fn clean_token(token: &str) -> &str {
    token.trim_matches(['"', '\'', '`', '(', ')', '[', ']', '{', '}', ',', ';', ':'])
}

/// Strips trailing line or line:column suffixes such as `:42` or `:10:5` from compiler/grep output.
fn strip_line_col_suffix(s: &str) -> &str {
    let mut cur = s;
    while let Some((file, num)) = cur.rsplit_once(':') {
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            cur = file;
        } else {
            break;
        }
    }
    cur
}

/// Returns true if a string with spaces is an absolute filesystem path starting with a drive
/// letter or root, without command verbs, command flags, quotes, or list separators.
fn is_plausible_single_path_with_spaces(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty()
        || s.contains('"')
        || s.contains('\'')
        || s.contains('`')
        || s.contains(',')
        || s.contains(';')
        || s.contains("://")
    {
        return false;
    }
    let is_drive = s.len() >= 3
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s.as_bytes()[1] == b':'
        && (s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/');
    let is_root = s.starts_with('/') || s.starts_with(r"\\");
    if !is_drive && !is_root {
        return false;
    }
    for word in s.split_whitespace() {
        if word.starts_with('-') {
            return false;
        }
        let lower = word.to_ascii_lowercase();
        let trimmed = lower.trim_matches(['"', '\'', '`', '(', ')', '[', ']', '{', '}', ',', ';', ':']);
        if COMMAND_VERBS.contains(&trimmed) {
            return false;
        }
        if let Some(cmd) = trimmed
            .strip_suffix(".exe")
            .or_else(|| trimmed.strip_suffix(".bat"))
            .or_else(|| trimmed.strip_suffix(".cmd"))
            && COMMAND_VERBS.contains(&cmd)
        {
            return false;
        }
    }
    true
}

/// Validates whether a token represents a candidate local web artifact path.
fn candidate_path(clean: &str) -> Option<&str> {
    if clean.is_empty() || clean.contains("://") {
        return None;
    }
    if clean.contains(' ') && !is_plausible_single_path_with_spaces(clean) {
        return None;
    }
    let stripped = strip_line_col_suffix(clean);
    if is_previewable_web_path(Path::new(stripped)) {
        Some(stripped)
    } else {
        None
    }
}

/// Extracts all substrings bounded by matching quotation marks and returns them along with
/// a version of the input string where quoted contents are replaced by spaces.
fn extract_and_mask_quotes(s: &str) -> (Vec<&str>, String) {
    let mut candidates = Vec::new();
    let mut masked = s.as_bytes().to_vec();
    for &quote in b"\"'`" {
        let mut in_quote = false;
        let mut start = 0;
        for i in 0..masked.len() {
            if masked[i] == quote {
                if in_quote {
                    if start + 1 < i {
                        candidates.push(&s[start + 1..i]);
                        for b in &mut masked[start + 1..i] {
                            *b = b' ';
                        }
                    }
                    in_quote = false;
                } else {
                    in_quote = true;
                    start = i;
                }
            }
        }
    }
    let masked_str = String::from_utf8_lossy(&masked).into_owned();
    (candidates, masked_str)
}

/// Resolves a relative or absolute path against a project directory.
fn resolve_path(p: &Path, project_dir: &Path) -> PathBuf {
    if p.is_absolute() || project_dir.as_os_str().is_empty() {
        p.to_path_buf()
    } else {
        project_dir.join(p)
    }
}

/// Extracts all previewable web artifact paths referenced by a tool's edit or detail string.
pub fn extract_all_previewable_paths_from_tool(
    edit: Option<&FileEdit>,
    detail: &str,
    project_dir: &Path,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(edit) = edit
        && let Some(valid) = candidate_path(&edit.path)
    {
        let full = resolve_path(Path::new(valid), project_dir);
        if !paths.contains(&full) {
            paths.push(full);
        }
    }
    for line in detail.lines() {
        for part in line.split(" · ") {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            // If the entire trimmed part is already a valid single path (e.g. "public/index.html"
            // or "C:\My Documents\index.html"), record it directly without splitting.
            if let Some(valid) = candidate_path(clean_token(part)) {
                let full = resolve_path(Path::new(valid), project_dir);
                if !paths.contains(&full) {
                    paths.push(full);
                }
                continue;
            }

            // Extract quoted tokens (e.g. Write "views/home.html" or "C:\My Documents\page.html"),
            // masking the quoted contents with spaces so their internal words are not re-parsed.
            let (quoted_candidates, unquoted_text) = extract_and_mask_quotes(part);
            for quoted in quoted_candidates {
                if let Some(valid) = candidate_path(clean_token(quoted)) {
                    let full = resolve_path(Path::new(valid), project_dir);
                    if !paths.contains(&full) {
                        paths.push(full);
                    }
                }
            }

            // Comma-separated or whitespace-separated tokens in unquoted text
            // (e.g. "create a.html, update b.svg")
            for subpart in unquoted_text.split([',', ';']) {
                for word in subpart.split_whitespace() {
                    if let Some(valid) = candidate_path(clean_token(word)) {
                        let full = resolve_path(Path::new(valid), project_dir);
                        if !paths.contains(&full) {
                            paths.push(full);
                        }
                    }
                }
            }
        }
    }
    paths
}

/// Extracts the first previewable artifact path from a tool call, if any.
pub fn previewable_path_from_tool(
    edit: Option<&FileEdit>,
    detail: &str,
    project_dir: &Path,
) -> Option<PathBuf> {
    extract_all_previewable_paths_from_tool(edit, detail, project_dir).into_iter().next()
}

/// Scans session entries to find all generated or edited web artifacts (HTML, SVG, XHTML).
///
/// Returns deduplicated paths, with the most recent artifacts first.
pub fn extract_previewable_artifacts(entries: &[Entry], project_dir: &Path) -> Vec<PathBuf> {
    let mut artifacts = Vec::new();
    for entry in entries.iter().rev() {
        if let Entry::Tool { edit, detail, .. } = entry {
            for path in extract_all_previewable_paths_from_tool(edit.as_ref(), detail, project_dir) {
                if !artifacts.contains(&path) {
                    artifacts.push(path);
                }
            }
        }
    }
    artifacts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_html_svg_and_xhtml_as_previewable_artifacts() {
        assert!(is_previewable_web_path(Path::new("index.html")));
        assert!(is_previewable_web_path(Path::new("demo.HTM")));
        assert!(is_previewable_web_path(Path::new("icons/logo.svg")));
        assert!(is_previewable_web_path(Path::new("nested/path/page.xhtml")));
    }

    #[test]
    fn ignores_non_web_extensions_like_rust_or_binary_files() {
        assert!(!is_previewable_web_path(Path::new("src/main.rs")));
        assert!(!is_previewable_web_path(Path::new("notes.txt")));
        assert!(!is_previewable_web_path(Path::new("app.bin")));
        assert!(!is_previewable_web_path(Path::new("README.md")));
        assert!(!is_previewable_web_path(Path::new("data.json")));
        assert!(!is_previewable_web_path(Path::new("image.png")));
        assert!(!is_previewable_web_path(Path::new("archive.zip")));
        assert!(!is_previewable_web_path(Path::new("no_extension")));
    }

    #[test]
    fn converts_windows_absolute_path_to_file_url() {
        let p = Path::new(r"C:\work\project\index.html");
        let url = path_to_file_url(p);
        assert_eq!(url, "file:///C:/work/project/index.html");
    }

    #[test]
    fn converts_unix_absolute_path_to_file_url() {
        let p = Path::new("/var/www/site/index.html");
        let url = path_to_file_url(p);
        assert_eq!(url, "file:///var/www/site/index.html");
    }

    #[test]
    fn handles_spaces_and_special_characters_in_file_url_encoding() {
        let p = Path::new(r"C:\My Documents\test #1 ? & %100\report.html");
        let url = path_to_file_url(p);
        assert_eq!(url, "file:///C:/My%20Documents/test%20%231%20%3F%20&%20%25100/report.html");
    }

    #[test]
    fn roundtrips_path_to_file_url_and_back() {
        #[cfg(windows)]
        {
            let original = PathBuf::from(r"C:\Users\test\docs\page.html");
            let url = path_to_file_url(&original);
            let restored = file_url_to_path(&url).expect("should parse file URL");
            assert_eq!(restored, original);
        }

        #[cfg(not(windows))]
        {
            let original = PathBuf::from("/home/user/docs/page.html");
            let url = path_to_file_url(&original);
            let restored = file_url_to_path(&url).expect("should parse file URL");
            assert_eq!(restored, original);
        }
    }

    #[test]
    fn extracts_previewable_artifacts_from_session_entries() {
        let project = Path::new(r"C:\work\demo");
        let entries = vec![
            Entry::Tool {
                name: "write_to_file".into(),
                detail: "public/index.html".into(),
                edit: None,
            },
            Entry::Tool {
                name: "replace_file_content".into(),
                detail: "src/main.rs".into(),
                edit: None,
            },
            Entry::Tool {
                name: "Edit".into(),
                detail: "Update icon".into(),
                edit: Some(FileEdit::new("assets/logo.svg", "<svg></svg>", "<svg width='20'></svg>")),
            },
            Entry::Tool {
                name: "write_to_file".into(),
                detail: "public/index.html".into(),
                edit: None,
            },
        ];

        let artifacts = extract_previewable_artifacts(&entries, project);
        // Newest first, deduplicated
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0], project.join("public/index.html"));
        assert_eq!(artifacts[1], project.join("assets/logo.svg"));
    }

    #[test]
    fn file_url_and_path_roundtrip_adversarial_unicode_and_symbols() {
        #[cfg(windows)]
        let test_paths = [
            r"C:\work\project\index.html",
            r"c:\lowercase_drive\page.htm",
            r"C:\My Documents\Test Project 2026\index.html",
            r"C:\special\#notes\what?now\100%_done\file.xhtml",
            r"C:\already%20escaped\my%25file.html",
            r"C:\documents\café\über\naïve.html",
            r"C:\文档\测试\项目\index.html",
            r"C:\документы\отчет\страница.html",
            r"C:\📁\🚀\design.svg",
            r"\\server\share\web\index.html",
        ];

        #[cfg(not(windows))]
        let test_paths = [
            "/var/www/site/index.html",
            "/home/user/My Documents/index.html",
            "/tmp/#notes/what?now/100%_done/file.xhtml",
            "/tmp/already%20escaped/my%25file.html",
            "/tmp/documents/café/über/naïve.html",
            "/tmp/文档/测试/项目/index.html",
            "/tmp/документы/отчет/страница.html",
            "/tmp/📁/🚀/design.svg",
        ];

        for path_str in test_paths {
            let original = PathBuf::from(path_str);
            let url = path_to_file_url(&original);
            assert!(url.starts_with("file://"), "URL must have file:// scheme: {url}");
            assert!(!url.contains(' '), "URL must not contain unescaped spaces: {url}");
            assert!(!url.contains('#'), "URL must not contain unescaped hash: {url}");
            assert!(!url.contains('?'), "URL must not contain unescaped question mark: {url}");
            let restored = file_url_to_path(&url).expect("should parse back from file URL");
            assert_eq!(restored, original, "Path failed roundtrip: original={path_str}, url={url}");
        }
    }

    #[test]
    fn file_url_to_path_handles_malformed_and_boundary_inputs() {
        assert_eq!(file_url_to_path("not_a_file_url"), None);
        assert_eq!(file_url_to_path("http://example.com/index.html"), None);
        assert_eq!(file_url_to_path("https://example.com/index.html"), None);

        // Boundary URLs
        assert!(file_url_to_path("file://").is_some());
        assert!(file_url_to_path("file:///").is_some());

        // Incomplete percent encodings must not panic
        let broken = file_url_to_path("file:///C:/path/incomplete%");
        assert!(broken.is_some());
        let broken2 = file_url_to_path("file:///C:/path/incomplete%2");
        assert!(broken2.is_some());
        let invalid_hex = file_url_to_path("file:///C:/path/invalid%ZZ/index.html");
        assert!(invalid_hex.is_some());
    }

    #[test]
    fn is_previewable_web_path_edge_cases_and_boundaries() {
        // Valid web extensions
        assert!(is_previewable_web_path(Path::new("index.html")));
        assert!(is_previewable_web_path(Path::new("INDEX.HTML")));
        assert!(is_previewable_web_path(Path::new("page.htm")));
        assert!(is_previewable_web_path(Path::new("PAGE.HTM")));
        assert!(is_previewable_web_path(Path::new("logo.svg")));
        assert!(is_previewable_web_path(Path::new("LOGO.SVG")));
        assert!(is_previewable_web_path(Path::new("app.xhtml")));
        assert!(is_previewable_web_path(Path::new("APP.XHTML")));

        // Invalid extensions
        assert!(!is_previewable_web_path(Path::new("index.htmll")));
        assert!(!is_previewable_web_path(Path::new("index.html5")));
        assert!(!is_previewable_web_path(Path::new("logo.svgz")));
        assert!(!is_previewable_web_path(Path::new("script.js")));
        assert!(!is_previewable_web_path(Path::new("style.css")));
        assert!(!is_previewable_web_path(Path::new("image.png")));
        assert!(!is_previewable_web_path(Path::new("lib.rs")));

        // Multiple dots
        assert!(is_previewable_web_path(Path::new("my.component.index.html")));
        assert!(!is_previewable_web_path(Path::new("index.html.bak")));
        assert!(!is_previewable_web_path(Path::new("index.html.old")));

        // No extension or dotfiles
        assert!(!is_previewable_web_path(Path::new("index")));
        assert!(!is_previewable_web_path(Path::new("Makefile")));
        assert!(!is_previewable_web_path(Path::new(".gitignore")));
    }

    #[test]
    fn path_to_file_url_handles_unc_prefixes() {
        let unc_question = Path::new(r"\\?\C:\project\page.html");
        let url = path_to_file_url(unc_question);
        assert_eq!(url, "file:///C:/project/page.html");

        let unc_dot = Path::new(r"\\.\C:\project\page.html");
        let url_dot = path_to_file_url(unc_dot);
        assert_eq!(url_dot, "file:///C:/project/page.html");
    }

    #[test]
    fn extract_all_previewable_paths_across_diverse_provider_tool_formats() {
        let project = Path::new(r"C:\my_workspace");

        // Codex file_change output: "create public/index.html"
        let codex_res = extract_all_previewable_paths_from_tool(None, "create public/index.html", project);
        assert_eq!(
            codex_res,
            vec![project.join("public/index.html")],
            "Verb 'create' must not be prepended; extracts only clean path"
        );

        // Codex multiple changes: "create public/index.html, update assets/logo.svg"
        let codex_multi = extract_all_previewable_paths_from_tool(
            None,
            "create public/index.html, update assets/logo.svg",
            project,
        );
        assert_eq!(
            codex_multi,
            vec![
                project.join("public/index.html"),
                project.join("assets/logo.svg")
            ],
            "Multi-file comma-separated list must be cleanly split into separate paths"
        );

        // Claude format: Write "views/home.html"
        let claude_res = extract_all_previewable_paths_from_tool(None, "Write \"views/home.html\"", project);
        assert_eq!(
            claude_res,
            vec![project.join("views/home.html")],
            "Claude Write verb and surrounding quotes must be stripped"
        );

        // Shell execution: "curl -s https://example.com/site.html"
        let web_url_res = extract_all_previewable_paths_from_tool(
            None,
            "curl -s https://example.com/site.html",
            project,
        );
        assert_eq!(
            web_url_res,
            Vec::<PathBuf>::new(),
            "Remote URLs in shell commands must never be extracted as local file paths"
        );

        // What the UI [👁 Preview] button actually receives:
        assert_eq!(
            previewable_path_from_tool(None, "create public/index.html", project),
            Some(project.join("public/index.html")),
            "Chat preview button receives the genuine local path"
        );
        assert_eq!(
            previewable_path_from_tool(None, "create public/index.html, update assets/logo.svg", project),
            Some(project.join("public/index.html")),
        );
        assert_eq!(
            previewable_path_from_tool(None, "Write \"views/home.html\"", project),
            Some(project.join("views/home.html")),
        );
        assert_eq!(
            previewable_path_from_tool(None, "curl -s https://example.com/site.html", project),
            None,
        );
    }

    #[test]
    fn extract_previewable_paths_strips_punctuation_brackets_and_line_suffixes() {
        let project = Path::new(r"C:\my_workspace");

        let bracketed = extract_all_previewable_paths_from_tool(None, "view [public/index.html]", project);
        assert_eq!(bracketed, vec![project.join("public/index.html")]);

        let parenthesized = extract_all_previewable_paths_from_tool(None, "check (views/page.xhtml);", project);
        assert_eq!(parenthesized, vec![project.join("views/page.xhtml")]);

        let with_line_col = extract_all_previewable_paths_from_tool(None, "error in assets/logo.svg:42:10", project);
        assert_eq!(with_line_col, vec![project.join("assets/logo.svg")]);
    }

    #[test]
    fn extract_previewable_paths_supports_windows_drive_paths_with_spaces() {
        let project = Path::new(r"C:\my_workspace");

        let drive_with_spaces = extract_all_previewable_paths_from_tool(None, r"C:\My Documents\index.html", project);
        assert_eq!(drive_with_spaces, vec![PathBuf::from(r"C:\My Documents\index.html")]);

        let quoted_in_command = extract_all_previewable_paths_from_tool(
            None,
            r#"Write "C:\My Documents\report.html""#,
            project,
        );
        assert_eq!(quoted_in_command, vec![PathBuf::from(r"C:\My Documents\report.html")]);
    }

    #[test]
    fn extract_previewable_paths_rejects_remote_http_and_https_schemes() {
        let project = Path::new(r"C:\my_workspace");

        let http = extract_all_previewable_paths_from_tool(None, "open http://localhost:8080/index.html", project);
        assert!(http.is_empty(), "http:// scheme must be rejected");

        let https = extract_all_previewable_paths_from_tool(None, "fetch https://example.org/assets/logo.svg", project);
        assert!(https.is_empty(), "https:// scheme must be rejected");

        let ftp = extract_all_previewable_paths_from_tool(None, "download ftp://files.org/demo.xhtml", project);
        assert!(ftp.is_empty(), "ftp:// scheme must be rejected");
    }
}
