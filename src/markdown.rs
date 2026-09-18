//! Custom markdown parsing and rich rendering for agent and user chat messages.
//!
//! Claude Code and Codex format their output with clean typography, compact headings,
//! styled code containers with language badges and copy buttons, inline code pills,
//! and provider-colored bullet lists and blockquotes. This module turns markdown text
//! into those exact visuals using egui's LayoutJob and Frame primitives.

use eframe::egui;

use crate::agent::Provider;

/// The signature terracotta/coral of Anthropic's Claude.
const CLAUDE_CORAL: egui::Color32 = egui::Color32::from_rgb(217, 119, 87);
/// The signature emerald green of OpenAI's Codex.
const CODEX_GREEN: egui::Color32 = egui::Color32::from_rgb(16, 163, 127);
/// The signature blue of Google's Antigravity.
const AGY_BLUE: egui::Color32 = egui::Color32::from_rgb(66, 133, 244);

/// Returns the primary accent colour for an agent CLI.
pub fn provider_color(provider: Provider) -> egui::Color32 {
    match provider {
        Provider::Claude => CLAUDE_CORAL,
        Provider::Codex => CODEX_GREEN,
        Provider::Antigravity => AGY_BLUE,
    }
}

/// A parsed block of markdown content.
#[derive(Debug, PartialEq, Clone)]
pub enum Block<'a> {
    Heading { level: usize, text: &'a str },
    Code { lang: &'a str, code: String },
    List { ordered: bool, items: Vec<String> },
    Blockquote(String),
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
    Rule,
    Paragraph(String),
}

/// Parses a markdown string into structured visual blocks.
///
/// Fenced code blocks that have not yet closed (common during streaming) are
/// kept as code blocks rather than discarded or broken up into raw text.
pub fn parse_blocks(input: &str) -> Vec<Block<'_>> {
    let mut blocks = Vec::new();
    let mut lines = input.lines().peekable();

    while let Some(&line) = lines.peek() {
        let trimmed = line.trim();

        // 1. Code block fence: ``` or ~~~
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let fence_char = trimmed.chars().next().unwrap_or('`');
            let fence = if fence_char == '`' { "```" } else { "~~~" };
            let lang = trimmed.trim_start_matches(fence_char).trim();
            lines.next(); // consume opening fence
            let mut code_lines = Vec::new();
            while let Some(&code_line) = lines.peek() {
                if code_line.trim().starts_with(fence) {
                    lines.next(); // consume closing fence
                    break;
                }
                code_lines.push(code_line);
                lines.next();
            }
            blocks.push(Block::Code {
                lang,
                code: code_lines.join("\n"),
            });
            continue;
        }

        // 2. Blank line: skip
        if trimmed.is_empty() {
            lines.next();
            continue;
        }

        // 3. Headings: #, ##, ###, ####, #####, ######
        if let Some((level, text)) = parse_heading_line(trimmed) {
            lines.next();
            blocks.push(Block::Heading { level, text });
            continue;
        }

        // 4. Horizontal rule: ---, ***, ___
        if is_horizontal_rule(trimmed) {
            lines.next();
            blocks.push(Block::Rule);
            continue;
        }

        // 5. Table: line contains '|' and the subsequent line is a delimiter |---|---|
        if is_table_start(&lines)
            && let Some(table) = parse_table(&mut lines)
        {
            blocks.push(table);
            continue;
        }

        // 6. Blockquote: lines starting with '>'
        if trimmed.starts_with('>') {
            let mut quote_lines = Vec::new();
            while let Some(&qline) = lines.peek() {
                let qtrimmed = qline.trim();
                if qtrimmed.starts_with('>') {
                    let text = qtrimmed.strip_prefix('>').unwrap_or("").trim_start();
                    quote_lines.push(text);
                    lines.next();
                } else if !qtrimmed.is_empty()
                    && !quote_lines.is_empty()
                    && !qtrimmed.starts_with('#')
                    && !qtrimmed.starts_with("```")
                    && !qtrimmed.starts_with("~~~")
                {
                    quote_lines.push(qtrimmed);
                    lines.next();
                } else {
                    break;
                }
            }
            blocks.push(Block::Blockquote(quote_lines.join(" ")));
            continue;
        }

        // 7. List: - , * , + , or 1. , 2. 
        if let Some((ordered, first_item)) = parse_list_item_start(trimmed) {
            let mut items = vec![first_item.to_owned()];
            lines.next();
            while let Some(&next_line) = lines.peek() {
                let ntrimmed = next_line.trim();
                if ntrimmed.is_empty() {
                    break;
                }
                if let Some((is_ord, item_text)) = parse_list_item_start(ntrimmed)
                    && is_ord == ordered
                {
                    items.push(item_text.to_owned());
                    lines.next();
                    continue;
                }
                // Multi-line list item continuation (indented)
                if next_line.starts_with("  ") || next_line.starts_with('\t') {
                    if let Some(last) = items.last_mut() {
                        last.push(' ');
                        last.push_str(ntrimmed);
                    }
                    lines.next();
                    continue;
                }
                break;
            }
            blocks.push(Block::List { ordered, items });
            continue;
        }

        // 8. Paragraph: collect lines until blank line, heading, code fence, list, or blockquote
        let mut para_lines = Vec::new();
        while let Some(&pline) = lines.peek() {
            let ptrimmed = pline.trim();
            if ptrimmed.is_empty()
                || ptrimmed.starts_with("```")
                || ptrimmed.starts_with("~~~")
                || parse_heading_line(ptrimmed).is_some()
                || is_horizontal_rule(ptrimmed)
                || ptrimmed.starts_with('>')
                || parse_list_item_start(ptrimmed).is_some()
                || is_table_start(&lines)
            {
                break;
            }
            para_lines.push(ptrimmed);
            lines.next();
        }
        if !para_lines.is_empty() {
            blocks.push(Block::Paragraph(para_lines.join(" ")));
        }
    }

    blocks
}

fn parse_heading_line(line: &str) -> Option<(usize, &str)> {
    let mut count = 0;
    for ch in line.chars() {
        if ch == '#' {
            count += 1;
        } else {
            break;
        }
    }
    if (1..=6).contains(&count) && line[count..].starts_with(' ') {
        Some((count, line[count..].trim()))
    } else {
        None
    }
}

fn is_horizontal_rule(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() >= 3 {
        let first = chars[0];
        (first == '-' || first == '*' || first == '_') && chars.iter().all(|&c| c == first)
    } else {
        false
    }
}

fn parse_list_item_start(line: &str) -> Option<(bool, &str)> {
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return Some((false, &line[2..]));
    }
    let dot_idx = line.find('.')?;
    if dot_idx > 0
        && dot_idx <= 4
        && line[..dot_idx].chars().all(|c| c.is_ascii_digit())
        && line[dot_idx..].starts_with(". ")
    {
        return Some((true, &line[dot_idx + 2..]));
    }
    None
}

fn is_table_start<'a>(lines: &std::iter::Peekable<std::str::Lines<'a>>) -> bool {
    let mut clone = lines.clone();
    let Some(first) = clone.next() else { return false };
    if !first.contains('|') {
        return false;
    }
    if let Some(second) = clone.peek() {
        let s = second.trim();
        s.contains('|') && s.chars().all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace())
    } else {
        false
    }
}

fn parse_table<'a>(lines: &mut std::iter::Peekable<std::str::Lines<'a>>) -> Option<Block<'a>> {
    let header_line = lines.next()?;
    let _delimiter_line = lines.next()?;
    let headers: Vec<String> = header_line
        .split('|')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    let mut rows = Vec::new();
    while let Some(&row_line) = lines.peek() {
        let trimmed = row_line.trim();
        if !trimmed.contains('|') || trimmed.is_empty() {
            break;
        }
        let cells: Vec<String> = trimmed
            .split('|')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        if !cells.is_empty() {
            rows.push(cells);
        }
        lines.next();
    }
    Some(Block::Table { headers, rows })
}

/// An inline span of formatted text.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum InlineSpan<'a> {
    Text(&'a str),
    Bold(&'a str),
    Italic(&'a str),
    BoldItalic(&'a str),
    Code(&'a str),
    Link { text: &'a str, url: &'a str },
    Strikethrough(&'a str),
}

/// Parses inline markdown elements such as bold, italics, inline code, and links.
pub fn parse_inline(text: &str) -> Vec<InlineSpan<'_>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        // 1. Inline code: `code`
        if remaining.starts_with('`') {
            if let Some(end) = remaining[1..].find('`') {
                spans.push(InlineSpan::Code(&remaining[1..=end]));
                remaining = &remaining[end + 2..];
                continue;
            } else {
                // Unclosed backtick at end (common while streaming)
                spans.push(InlineSpan::Code(&remaining[1..]));
                break;
            }
        }

        // 2. Bold + Italic: ***text***
        if remaining.starts_with("***")
            && let Some(end) = remaining[3..].find("***")
        {
            spans.push(InlineSpan::BoldItalic(&remaining[3..3 + end]));
            remaining = &remaining[3 + end + 3..];
            continue;
        }

        // 3. Bold: **text** or __text__
        if remaining.starts_with("**")
            && let Some(end) = remaining[2..].find("**")
        {
            spans.push(InlineSpan::Bold(&remaining[2..2 + end]));
            remaining = &remaining[2 + end + 2..];
            continue;
        }
        if remaining.starts_with("__")
            && let Some(end) = remaining[2..].find("__")
        {
            spans.push(InlineSpan::Bold(&remaining[2..2 + end]));
            remaining = &remaining[2 + end + 2..];
            continue;
        }

        // 4. Strikethrough: ~~text~~
        if remaining.starts_with("~~")
            && let Some(end) = remaining[2..].find("~~")
        {
            spans.push(InlineSpan::Strikethrough(&remaining[2..2 + end]));
            remaining = &remaining[2 + end + 2..];
            continue;
        }

        // 5. Italic: *text* or _text_ (ensuring not double)
        if remaining.starts_with('*')
            && !remaining.starts_with("**")
            && let Some(end) = remaining[1..].find('*')
            && end > 0
        {
            spans.push(InlineSpan::Italic(&remaining[1..=end]));
            remaining = &remaining[end + 2..];
            continue;
        }
        if remaining.starts_with('_')
            && !remaining.starts_with("__")
            && let Some(end) = remaining[1..].find('_')
            && end > 0
        {
            spans.push(InlineSpan::Italic(&remaining[1..=end]));
            remaining = &remaining[end + 2..];
            continue;
        }

        // 6. Link: [text](url)
        if remaining.starts_with('[')
            && let Some(closing_bracket) = remaining.find(']')
            && remaining[closing_bracket + 1..].starts_with('(')
            && let Some(closing_paren) = remaining[closing_bracket + 2..].find(')')
        {
            let link_text = &remaining[1..closing_bracket];
            let url = &remaining[closing_bracket + 2..closing_bracket + 2 + closing_paren];
            spans.push(InlineSpan::Link { text: link_text, url });
            remaining = &remaining[closing_bracket + 2 + closing_paren + 1..];
            continue;
        }

        // Normal plain text: advance until next special character
        let next_special = remaining[1..]
            .find(['`', '*', '_', '~', '['])
            .map(|idx| idx + 1)
            .unwrap_or(remaining.len());

        spans.push(InlineSpan::Text(&remaining[..next_special]));
        remaining = &remaining[next_special..];
    }

    spans
}

/// Formats inline spans into an egui LayoutJob with font, weight, and background pills.
pub fn layout_inline(
    spans: &[InlineSpan<'_>],
    dark_mode: bool,
    base_font: egui::FontId,
    base_color: egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();

    for span in spans {
        match *span {
            InlineSpan::Text(s) => {
                job.append(
                    s,
                    0.0,
                    egui::TextFormat {
                        font_id: base_font.clone(),
                        color: base_color,
                        ..Default::default()
                    },
                );
            }
            InlineSpan::Bold(s) => {
                job.append(
                    s,
                    0.0,
                    egui::TextFormat {
                        font_id: base_font.clone(),
                        color: if dark_mode {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_rgb(20, 20, 25)
                        },
                        extra_letter_spacing: 0.15,
                        ..Default::default()
                    },
                );
            }
            InlineSpan::Italic(s) => {
                job.append(
                    s,
                    0.0,
                    egui::TextFormat {
                        font_id: base_font.clone(),
                        color: base_color,
                        italics: true,
                        ..Default::default()
                    },
                );
            }
            InlineSpan::BoldItalic(s) => {
                job.append(
                    s,
                    0.0,
                    egui::TextFormat {
                        font_id: base_font.clone(),
                        color: if dark_mode {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::from_rgb(20, 20, 25)
                        },
                        italics: true,
                        ..Default::default()
                    },
                );
            }
            InlineSpan::Strikethrough(s) => {
                let weak_color = if dark_mode {
                    egui::Color32::from_rgb(140, 142, 150)
                } else {
                    egui::Color32::from_rgb(130, 134, 142)
                };
                job.append(
                    s,
                    0.0,
                    egui::TextFormat {
                        font_id: base_font.clone(),
                        color: weak_color,
                        strikethrough: egui::Stroke::new(1.0, weak_color),
                        ..Default::default()
                    },
                );
            }
            InlineSpan::Code(s) => {
                // Clean inline code pill styling like Claude Code and Codex
                let code_font = egui::FontId::new((base_font.size * 0.92).max(11.0), egui::FontFamily::Monospace);
                let text_color = if dark_mode {
                    egui::Color32::from_rgb(245, 185, 155)
                } else {
                    egui::Color32::from_rgb(175, 60, 35)
                };
                let bg_color = if dark_mode {
                    egui::Color32::from_rgb(44, 46, 54)
                } else {
                    egui::Color32::from_rgb(234, 237, 242)
                };
                job.append(
                    s,
                    0.0,
                    egui::TextFormat {
                        font_id: code_font,
                        color: text_color,
                        background: bg_color,
                        expand_bg: 2.0,
                        ..Default::default()
                    },
                );
            }
            InlineSpan::Link { text, .. } => {
                let link_color = if dark_mode {
                    egui::Color32::from_rgb(110, 175, 255)
                } else {
                    egui::Color32::from_rgb(30, 110, 210)
                };
                job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id: base_font.clone(),
                        color: link_color,
                        underline: egui::Stroke::new(1.0, link_color),
                        ..Default::default()
                    },
                );
            }
        }
    }

    job
}

/// Token kinds for lightweight code syntax highlighting.
#[derive(Debug, PartialEq, Clone, Copy)]
enum TokenKind {
    Keyword,
    Type,
    Plain,
}

/// Fast syntax highlighter generating an egui LayoutJob for code blocks.
pub fn highlight_code(code: &str, lang: &str, dark_mode: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font_id = egui::FontId::new(13.0, egui::FontFamily::Monospace);

    let keyword_color = if dark_mode {
        egui::Color32::from_rgb(198, 140, 215) // purple / lavender
    } else {
        egui::Color32::from_rgb(160, 40, 160)
    };
    let type_color = if dark_mode {
        egui::Color32::from_rgb(78, 201, 176) // teal / cyan
    } else {
        egui::Color32::from_rgb(16, 120, 105)
    };
    let string_color = if dark_mode {
        egui::Color32::from_rgb(150, 200, 130) // soft sage green
    } else {
        egui::Color32::from_rgb(55, 135, 45)
    };
    let comment_color = if dark_mode {
        egui::Color32::from_rgb(115, 125, 135) // muted gray
    } else {
        egui::Color32::from_rgb(135, 142, 150)
    };
    let number_color = if dark_mode {
        egui::Color32::from_rgb(209, 154, 102) // warm amber
    } else {
        egui::Color32::from_rgb(180, 95, 30)
    };
    let plain_color = if dark_mode {
        egui::Color32::from_rgb(220, 222, 228)
    } else {
        egui::Color32::from_rgb(40, 42, 48)
    };

    let is_hash_comment_lang = matches!(
        lang.to_lowercase().as_str(),
        "python" | "py" | "bash" | "sh" | "shell" | "zsh" | "toml" | "yaml" | "yml" | "dockerfile"
    );

    for (line_idx, line) in code.lines().enumerate() {
        if line_idx > 0 {
            job.append(
                "\n",
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: plain_color,
                    line_height: Some(18.0),
                    ..Default::default()
                },
            );
        }

        let mut chars = line.char_indices().peekable();
        while let Some(&(idx, ch)) = chars.peek() {
            // Line comment check
            if (ch == '#' && is_hash_comment_lang)
                || (ch == '/' && line[idx..].starts_with("//"))
            {
                let comment_text = &line[idx..];
                job.append(
                    comment_text,
                    0.0,
                    egui::TextFormat {
                        font_id: font_id.clone(),
                        color: comment_color,
                        line_height: Some(18.0),
                        italics: true,
                        ..Default::default()
                    },
                );
                break;
            }

            // String literals: "...", '...', `...`
            if ch == '"' || ch == '\'' || ch == '`' {
                let quote = ch;
                chars.next();
                let mut escaped = false;
                let mut end_idx = line.len();
                while let Some(&(s_idx, s_ch)) = chars.peek() {
                    chars.next();
                    if escaped {
                        escaped = false;
                    } else if s_ch == '\\' {
                        escaped = true;
                    } else if s_ch == quote {
                        end_idx = s_idx + s_ch.len_utf8();
                        break;
                    }
                }
                let str_text = &line[idx..end_idx];
                job.append(
                    str_text,
                    0.0,
                    egui::TextFormat {
                        font_id: font_id.clone(),
                        color: string_color,
                        line_height: Some(18.0),
                        ..Default::default()
                    },
                );
                continue;
            }

            // Numbers: digits or hex
            if ch.is_ascii_digit() && (idx == 0 || !line[..idx].ends_with(|c: char| c.is_alphanumeric() || c == '_')) {
                let mut num_end = idx;
                while let Some(&(n_idx, n_ch)) = chars.peek() {
                    if n_ch.is_ascii_alphanumeric() || n_ch == '.' || n_ch == '_' {
                        num_end = n_idx + n_ch.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                job.append(
                    &line[idx..num_end],
                    0.0,
                    egui::TextFormat {
                        font_id: font_id.clone(),
                        color: number_color,
                        line_height: Some(18.0),
                        ..Default::default()
                    },
                );
                continue;
            }

            // Words (keywords, types, identifiers)
            if ch.is_alphabetic() || ch == '_' {
                let mut word_end = idx;
                while let Some(&(w_idx, w_ch)) = chars.peek() {
                    if w_ch.is_alphanumeric() || w_ch == '_' {
                        word_end = w_idx + w_ch.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let word = &line[idx..word_end];
                let kind = classify_word(word);
                let color = match kind {
                    TokenKind::Keyword => keyword_color,
                    TokenKind::Type => type_color,
                    _ => plain_color,
                };
                job.append(
                    word,
                    0.0,
                    egui::TextFormat {
                        font_id: font_id.clone(),
                        color,
                        line_height: Some(18.0),
                        ..Default::default()
                    },
                );
                continue;
            }

            // Punctuation and whitespace
            chars.next();
            job.append(
                &line[idx..idx + ch.len_utf8()],
                0.0,
                egui::TextFormat {
                    font_id: font_id.clone(),
                    color: plain_color,
                    line_height: Some(18.0),
                    ..Default::default()
                },
            );
        }
    }

    job
}

fn classify_word(word: &str) -> TokenKind {
    const KEYWORDS: &[&str] = &[
        "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "for", "while", "loop", "if",
        "else", "match", "return", "use", "mod", "async", "await", "def", "class", "import", "from",
        "const", "function", "var", "export", "default", "select", "where", "insert", "update",
        "delete", "type", "case", "break", "continue", "in", "as", "self", "super", "crate", "yield",
        "try", "catch", "throw", "finally", "new", "typeof", "instanceof", "static", "interface",
        "extends", "implements", "package", "print", "echo", "val", "fun", "public", "private",
        "protected", "class", "void", "override",
    ];

    const TYPES: &[&str] = &[
        "String", "str", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
        "bool", "char", "usize", "isize", "Option", "Some", "None", "Result", "Ok", "Err", "Vec",
        "Box", "Arc", "Rc", "Mutex", "Path", "PathBuf", "Self", "true", "false", "null", "nil",
        "undefined", "NaN", "int", "float", "boolean", "any", "unknown", "never", "number",
    ];

    if KEYWORDS.contains(&word) {
        TokenKind::Keyword
    } else if TYPES.contains(&word) || (word.starts_with(|c: char| c.is_ascii_uppercase()) && word.chars().any(|c| c.is_ascii_lowercase())) {
        TokenKind::Type
    } else {
        TokenKind::Plain
    }
}

/// Renders a code block in a dark, bordered card with language badge and copy button.
pub fn code_block_ui(ui: &mut egui::Ui, id_salt: egui::Id, lang: &str, code: &str) {
    let dark_mode = ui.visuals().dark_mode;
    let background = if dark_mode {
        egui::Color32::from_rgb(20, 21, 25)
    } else {
        egui::Color32::from_rgb(245, 246, 249)
    };
    let border_color = if dark_mode {
        egui::Color32::from_rgb(46, 48, 56)
    } else {
        egui::Color32::from_rgb(222, 225, 230)
    };

    ui.add_space(4.0);
    egui::Frame::new()
        .fill(background)
        .stroke(egui::Stroke::new(1.0, border_color))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Header bar: Language label + Copy button
            ui.horizontal(|ui| {
                let display_lang = if lang.trim().is_empty() {
                    "CODE"
                } else {
                    lang.trim()
                }
                .to_uppercase();

                ui.label(
                    egui::RichText::new(display_lang)
                        .monospace()
                        .size(11.0)
                        .strong()
                        .color(if dark_mode {
                            egui::Color32::from_rgb(140, 145, 160)
                        } else {
                            egui::Color32::from_rgb(110, 115, 125)
                        }),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let copy_id = id_salt.with("copy_btn");
                    let copied_at: Option<f64> = ui.ctx().data(|d| d.get_temp(copy_id));
                    let is_copied = copied_at.map(|t| ui.input(|i| i.time - t) < 2.0).unwrap_or(false);
                    let label = if is_copied { "✓ Copied" } else { "Copy" };
                    if ui.small_button(label).on_hover_text("Copy code block to clipboard").clicked() {
                        ui.ctx().copy_text(code.to_owned());
                        ui.ctx().data_mut(|d| d.insert_temp(copy_id, ui.input(|i| i.time)));
                    }
                });
            });

            ui.add_space(4.0);

            // Body: scrollable horizontally so long lines never wrap messily
            egui::ScrollArea::horizontal()
                .id_salt(id_salt.with("code_scroll"))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let job = highlight_code(code, lang, dark_mode);
                    ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
                });
        });
    ui.add_space(4.0);
}

fn heading_ui(ui: &mut egui::Ui, level: usize, text: &str, provider_accent: egui::Color32) {
    let (size, top_space, bottom_space, underline) = match level {
        1 => (17.5, 14.0, 6.0, true),
        2 => (16.0, 10.0, 4.0, false),
        3 => (14.5, 8.0, 3.0, false),
        _ => (13.5, 6.0, 2.0, false),
    };

    ui.add_space(top_space);

    let mut heading_text = egui::RichText::new(text).size(size).strong();
    if level == 1 {
        heading_text = heading_text.color(if ui.visuals().dark_mode {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(20, 20, 25)
        });
    }
    ui.label(heading_text);

    if underline {
        ui.add_space(2.0);
        let stroke = egui::Stroke::new(1.5, provider_accent.gamma_multiply(0.4));
        let rect = ui.available_rect_before_wrap();
        ui.painter().hline(rect.x_range(), ui.cursor().top(), stroke);
    }

    ui.add_space(bottom_space);
}

fn list_ui(
    ui: &mut egui::Ui,
    _id_salt: egui::Id,
    ordered: bool,
    items: &[String],
    provider_accent: egui::Color32,
    dark_mode: bool,
) {
    ui.add_space(2.0);
    for (idx, item) in items.iter().enumerate() {
        ui.horizontal_top(|ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.x = 6.0;

            if ordered {
                let num_label = format!("{}.", idx + 1);
                ui.add(egui::Label::new(egui::RichText::new(num_label).strong().small()));
            } else {
                ui.add(egui::Label::new(egui::RichText::new("•").size(14.0).color(provider_accent)));
            }

            let spans = parse_inline(item);
            let font = egui::FontId::new(14.0, egui::FontFamily::Proportional);
            let color = ui.visuals().text_color();
            let job = layout_inline(&spans, dark_mode, font, color);
            ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Wrap));
        });
        ui.add_space(2.0);
    }
    ui.add_space(2.0);
}

fn blockquote_ui(ui: &mut egui::Ui, text: &str, provider_accent: egui::Color32, dark_mode: bool) {
    ui.add_space(3.0);
    let border_color = provider_accent.gamma_multiply(0.7);
    let bg_color = provider_accent.gamma_multiply(if dark_mode { 0.08 } else { 0.05 });

    egui::Frame::new()
        .fill(bg_color)
        .stroke(egui::Stroke::NONE)
        .corner_radius(4.0)
        .inner_margin(egui::Margin { left: 12, right: 10, top: 6, bottom: 6 })
        .show(ui, |ui| {
            let left_top = ui.min_rect().left_top();
            let left_bottom = ui.min_rect().left_bottom();
            ui.painter().line_segment([left_top, left_bottom], egui::Stroke::new(3.0, border_color));

            let spans = parse_inline(text);
            let font = egui::FontId::new(14.0, egui::FontFamily::Proportional);
            let color = ui.visuals().weak_text_color();
            let mut job = layout_inline(&spans, dark_mode, font, color);
            for section in &mut job.sections {
                section.format.italics = true;
            }
            ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Wrap));
        });
    ui.add_space(3.0);
}

fn table_ui(ui: &mut egui::Ui, headers: &[String], rows: &[Vec<String>], dark_mode: bool) {
    if headers.is_empty() {
        return;
    }
    ui.add_space(4.0);

    let border_color = if dark_mode {
        egui::Color32::from_rgb(50, 52, 60)
    } else {
        egui::Color32::from_rgb(215, 218, 224)
    };

    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, border_color))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::same(0))
        .show(ui, |ui| {
            egui::Grid::new(ui.id().with("markdown_table"))
                .striped(true)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for header in headers {
                        let spans = parse_inline(header);
                        let font = egui::FontId::new(13.5, egui::FontFamily::Proportional);
                        let color = ui.visuals().strong_text_color();
                        let job = layout_inline(&spans, dark_mode, font, color);
                        ui.add(egui::Label::new(job));
                    }
                    ui.end_row();

                    for row in rows {
                        for cell in row {
                            let spans = parse_inline(cell);
                            let font = egui::FontId::new(13.0, egui::FontFamily::Proportional);
                            let color = ui.visuals().text_color();
                            let job = layout_inline(&spans, dark_mode, font, color);
                            ui.add(egui::Label::new(job));
                        }
                        ui.end_row();
                    }
                });
        });
    ui.add_space(4.0);
}

fn paragraph_ui(ui: &mut egui::Ui, text: &str, dark_mode: bool) {
    let spans = parse_inline(text);
    let has_links = spans.iter().any(|s| matches!(s, InlineSpan::Link { .. }));

    if !has_links {
        let font = egui::FontId::new(14.5, egui::FontFamily::Proportional);
        let color = ui.visuals().text_color();
        let job = layout_inline(&spans, dark_mode, font, color);
        ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Wrap));
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for span in &spans {
                match span {
                    InlineSpan::Link { text, url } => {
                        if ui.link(*text).on_hover_text(*url).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::same_tab(*url));
                        }
                    }
                    _ => {
                        let single = [*span];
                        let font = egui::FontId::new(14.5, egui::FontFamily::Proportional);
                        let color = ui.visuals().text_color();
                        let job = layout_inline(&single, dark_mode, font, color);
                        ui.add(egui::Label::new(job));
                    }
                }
            }
        });
    }
    ui.add_space(4.0);
}

/// Renders markdown blocks (used for user messages and general markdown blocks).
pub fn render_plain_or_markdown(ui: &mut egui::Ui, id_salt: egui::Id, text: &str, dark_mode: bool) {
    let blocks = parse_blocks(text);

    for (b_idx, block) in blocks.into_iter().enumerate() {
        let block_id = id_salt.with(("block", b_idx));
        match block {
            Block::Heading { level, text } => heading_ui(ui, level, text, ui.visuals().selection.bg_fill),
            Block::Code { lang, code } => code_block_ui(ui, block_id, lang, &code),
            Block::List { ordered, items } => list_ui(ui, block_id, ordered, &items, ui.visuals().text_color(), dark_mode),
            Block::Blockquote(quote) => blockquote_ui(ui, &quote, ui.visuals().weak_text_color(), dark_mode),
            Block::Table { headers, rows } => table_ui(ui, &headers, &rows, dark_mode),
            Block::Rule => {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
            }
            Block::Paragraph(p) => {
                paragraph_ui(ui, &p, dark_mode);
            }
        }
    }
}

/// Renders an agent turn with Claude Code / Codex header, model badge, copy button,
/// and formatted markdown body.
pub fn show_agent_turn(
    ui: &mut egui::Ui,
    id_salt: egui::Id,
    text: &str,
    provider: Provider,
    model: Option<&str>,
    is_streaming: bool,
    show_header: bool,
) {
    let accent = provider_color(provider);
    let dark_mode = ui.visuals().dark_mode;

    if show_header {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // Provider bullet / icon
            ui.label(egui::RichText::new("●").size(13.0).color(accent));

            // Provider title
            ui.label(egui::RichText::new(provider.short_name()).strong());

            // Model info
            if let Some(m) = model {
                ui.label(egui::RichText::new(format!("· {m}")).small().weak());
            }

            // Status or Copy button
            if is_streaming {
                ui.label(egui::RichText::new("responding…").small().italics().weak());
            } else {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let copy_id = id_salt.with("turn_copy");
                    let copied_at: Option<f64> = ui.ctx().data(|d| d.get_temp(copy_id));
                    let is_copied = copied_at.map(|t| ui.input(|i| i.time - t) < 2.0).unwrap_or(false);
                    let label = if is_copied { "✓ Copied" } else { "Copy" };
                    if ui.small_button(label).on_hover_text("Copy response").clicked() {
                        ui.ctx().copy_text(text.to_owned());
                        ui.ctx().data_mut(|d| d.insert_temp(copy_id, ui.input(|i| i.time)));
                    }
                });
            }
        });
        ui.add_space(4.0);
    } else {
        ui.add_space(4.0);
    }

    let blocks = parse_blocks(text);
    let num_blocks = blocks.len();

    for (b_idx, block) in blocks.into_iter().enumerate() {
        let block_id = id_salt.with(("block", b_idx));
        match block {
            Block::Heading { level, text } => heading_ui(ui, level, text, accent),
            Block::Code { lang, code } => code_block_ui(ui, block_id, lang, &code),
            Block::List { ordered, items } => list_ui(ui, block_id, ordered, &items, accent, dark_mode),
            Block::Blockquote(quote) => blockquote_ui(ui, &quote, accent, dark_mode),
            Block::Table { headers, rows } => table_ui(ui, &headers, &rows, dark_mode),
            Block::Rule => {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
            }
            Block::Paragraph(p) => {
                if is_streaming && b_idx + 1 == num_blocks {
                    let mut p_with_cursor = p;
                    p_with_cursor.push_str(" ▊");
                    paragraph_ui(ui, &p_with_cursor, dark_mode);
                } else {
                    paragraph_ui(ui, &p, dark_mode);
                }
            }
        }
    }

    if is_streaming && num_blocks == 0 {
        ui.label(egui::RichText::new("▊").color(accent));
    }

    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_blocks_are_extracted_with_their_language() {
        let input = "Here is the code:\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\nDone.";
        let blocks = parse_blocks(input);
        assert_eq!(blocks.len(), 3, "paragraph, code block, and conclusion");
        let [Block::Paragraph(intro), Block::Code { lang, code }, Block::Paragraph(outro)] = &blocks[..] else {
            panic!("expected intro, code block, outro");
        };
        assert_eq!(*intro, "Here is the code:");
        assert_eq!(*lang, "rust");
        assert_eq!(code, "fn main() {\n    println!(\"hi\");\n}");
        assert_eq!(*outro, "Done.");
    }

    #[test]
    fn unclosed_code_blocks_during_streaming_are_preserved() {
        let input = "Streaming reply:\n```python\nprint('hello world')";
        let blocks = parse_blocks(input);
        let [Block::Paragraph(intro), Block::Code { lang, code }] = &blocks[..] else {
            panic!("unclosed fence should still yield a code block");
        };
        assert_eq!(*intro, "Streaming reply:");
        assert_eq!(*lang, "python");
        assert_eq!(code, "print('hello world')");
    }

    #[test]
    fn headings_are_parsed_by_level() {
        let input = "# Heading 1\n## Heading 2\n### Heading 3\nNot a # heading";
        let blocks = parse_blocks(input);
        let [Block::Heading { level: 1, text: h1 }, Block::Heading { level: 2, text: h2 }, Block::Heading { level: 3, text: h3 }, Block::Paragraph(p)] =
            &blocks[..]
        else {
            panic!("expected three headings and one paragraph");
        };
        assert_eq!(*h1, "Heading 1");
        assert_eq!(*h2, "Heading 2");
        assert_eq!(*h3, "Heading 3");
        assert_eq!(*p, "Not a # heading");
    }

    #[test]
    fn inline_code_is_parsed_with_pill_formatting() {
        let spans = parse_inline("Use `cargo check` and `cargo test` for speed.");
        let [InlineSpan::Text(t1), InlineSpan::Code(c1), InlineSpan::Text(t2), InlineSpan::Code(c2), InlineSpan::Text(t3)] =
            &spans[..]
        else {
            panic!("expected text and code spans");
        };
        assert_eq!(*t1, "Use ");
        assert_eq!(*c1, "cargo check");
        assert_eq!(*t2, " and ");
        assert_eq!(*c2, "cargo test");
        assert_eq!(*t3, " for speed.");
    }

    #[test]
    fn lists_group_contiguous_items() {
        let input = "- First item\n- Second item\n  with continuation\n- Third item";
        let blocks = parse_blocks(input);
        let [Block::List { ordered: false, items }] = &blocks[..] else {
            panic!("expected a single unordered list");
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], "First item");
        assert_eq!(items[1], "Second item with continuation");
        assert_eq!(items[2], "Third item");
    }

    #[test]
    fn tables_extract_headers_and_rows() {
        let input = "| Name | Role |\n|---|---|\n| Claude | Agent |\n| User | Human |";
        let blocks = parse_blocks(input);
        let [Block::Table { headers, rows }] = &blocks[..] else {
            panic!("expected a parsed table");
        };
        assert_eq!(headers, &["Name", "Role"]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], &["Claude", "Agent"]);
        assert_eq!(rows[1], &["User", "Human"]);
    }

    #[test]
    fn code_syntax_is_highlighted_for_common_languages() {
        let rust_code = "fn main() {\n    let count: usize = 42;\n    // comment\n}";
        let job = highlight_code(rust_code, "rust", true);
        assert!(!job.sections.is_empty(), "layout job has highlighted sections");
        assert_eq!(job.text, rust_code);
    }
}
