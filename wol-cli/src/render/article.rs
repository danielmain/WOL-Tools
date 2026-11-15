//! HTML DOM → styled, width-aware terminal lines.
//!
//! The renderer consumes [`wol_core::dom::Document`] plus a [`wol_core::backup::StoredIndex`]
//! and rewrites each paragraph as one *styled run* per highlight (or per
//! emphasis/link run), then word-wraps to the terminal width. Because runs are
//! derived from UTF-16 char windows, a highlight always lands on exactly the
//! same words the browser extension would paint.

use unicode_width::UnicodeWidthChar;

use wol_core::backup::{NoteItem, StoredIndex};
use wol_core::dom::{Block, BlockKind, Document, InputField, Node_};
use wol_core::highlight;
use wol_core::tokenize;

use super::{StyleRole, Theme};

/// A piece of text that shares one visual style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub role: StyleRole,
}

impl Run {
    pub fn new(text: impl Into<String>, role: StyleRole) -> Self {
        Self {
            text: text.into(),
            role,
        }
    }
}

/// Options shared by the plain renderer and the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    pub width: usize,
    pub gutter: bool,
    pub show_notes: bool,
    pub show_footnotes: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 100,
            gutter: true,
            show_notes: true,
            show_footnotes: true,
        }
    }
}

/// Replace typographic characters that have plain ASCII equivalents.
///
/// Token coordinates are UTF-16 based, so folding one BMP character to another
/// leaves every token index unchanged — `--ascii` stays mark-compatible.
pub fn fold_ascii(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => '"',
            '\u{2013}' | '\u{2014}' | '\u{2015}' | '\u{2212}' => '-',
            '\u{2026}' => '.',
            '\u{00A0}' | '\u{2007}' | '\u{2009}' | '\u{200A}' | '\u{202F}' => ' ',
            '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}' | '\u{2008}'
            | '\u{2000}' | '\u{2001}' | '\u{3000}' => ' ',
            '\u{2022}' | '\u{00B7}' => '*',
            '\u{00AB}' | '\u{00BB}' => '"',
            '\u{2192}' => '>',
            other => other,
        })
        .collect()
}

/// One rendered line and the `Document::nodes` index it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedLine {
    pub text: String,
    pub node: Option<usize>,
}

/// Render a whole document to printable lines (colour included).
pub fn render_document(
    document: &Document,
    marks: &StoredIndex,
    theme: &Theme,
    options: RenderOptions,
) -> Vec<String> {
    render_document_lines(document, marks, theme, options)
        .into_iter()
        .map(|line| line.text)
        .collect()
}

/// Like [`render_document`], but keeps the node each line belongs to.
pub fn render_document_lines(
    document: &Document,
    marks: &StoredIndex,
    theme: &Theme,
    options: RenderOptions,
) -> Vec<RenderedLine> {
    let mut out: Vec<RenderedLine> = Vec::new();
    out.extend(
        render_header(document, marks, theme, options)
            .into_iter()
            .map(|text| RenderedLine { text, node: None }),
    );

    let document_id = guess_document_id(document, marks);
    let mut footnotes: Vec<String> = Vec::new();

    for (index, node) in document.nodes.iter().enumerate() {
        let push = |lines: Vec<String>, out: &mut Vec<RenderedLine>| {
            out.extend(lines.into_iter().map(|text| RenderedLine {
                text,
                node: Some(index),
            }));
        };
        match node {
            Node_::Block(block) => {
                push(
                    render_block(block, marks, document_id.as_deref(), theme, options),
                    &mut out,
                );
                if options.show_footnotes {
                    footnotes.extend(collect_footnotes(block));
                }
            }
            Node_::InputField(field) => push(render_input_field(field, theme, options), &mut out),
            Node_::Image(image) => {
                let caption = if image.alt.is_empty() {
                    image.src.clone()
                } else {
                    image.alt.clone()
                };
                push(
                    vec![format!(
                        "  {}",
                        theme.paint(StyleRole::Caption, &format!("[ image: {caption} ]"))
                    )],
                    &mut out,
                );
            }
            Node_::Rule => push(vec![rule(theme, options.width, '─')], &mut out),
        }
    }

    if options.show_notes {
        let notes = render_notes(document, marks, document_id.as_deref(), theme, options);
        if !notes.is_empty() {
            out.push(RenderedLine {
                text: String::new(),
                node: None,
            });
            out.extend(notes.into_iter().map(|text| RenderedLine { text, node: None }));
        }
    }
    if options.show_footnotes && !footnotes.is_empty() {
        out.push(RenderedLine {
            text: String::new(),
            node: None,
        });
        out.push(RenderedLine {
            text: theme.paint(StyleRole::Accent, "References"),
            node: None,
        });
        for footnote in footnotes {
            out.push(RenderedLine {
                text: format!("  {}", theme.paint(StyleRole::Dim, &footnote)),
                node: None,
            });
        }
    }
    if theme.glyphs == super::Glyphs::Ascii {
        out.into_iter()
            .map(|line| RenderedLine {
                text: fold_ascii(&line.text),
                node: line.node,
            })
            .collect()
    } else {
        out
    }
}

/// Frame with title, breadcrumb and cache status.
pub fn render_header(
    document: &Document,
    marks: &StoredIndex,
    theme: &Theme,
    options: RenderOptions,
) -> Vec<String> {
    let borders = theme.border();
    let width = options.width.max(20);
    let title = document
        .title
        .clone()
        .unwrap_or_else(|| "(untitled)".to_string());
    let breadcrumb = document.breadcrumb.join(" › ");
    let separator = if theme.glyphs == super::Glyphs::Ascii { "|" } else { "·" };
    let status = format!(
        "{} highlights {separator} {} notes",
        marks.highlight_count, marks.note_count
    );

    let mut lines = Vec::new();
    lines.push(theme.paint(
        StyleRole::Accent,
        &format!(
            "{}{}{}",
            borders.top_left,
            borders.horizontal.to_string().repeat(width.saturating_sub(2)),
            borders.top_right
        ),
    ));
    for line in wrap_text(&title, width.saturating_sub(4)) {
        lines.push(frame_line(&theme.paint(StyleRole::Title, &line), &theme.paint(StyleRole::Accent, &borders.vertical.to_string()), width, borders.vertical));
    }
    if !breadcrumb.is_empty() {
        for line in wrap_text(&breadcrumb, width.saturating_sub(4)) {
            lines.push(frame_line(&theme.paint(StyleRole::Dim, &line), &theme.paint(StyleRole::Accent, &borders.vertical.to_string()), width, borders.vertical));
        }
    }
    for line in wrap_text(&status, width.saturating_sub(4)) {
        lines.push(frame_line(&theme.paint(StyleRole::Caption, &line), &theme.paint(StyleRole::Accent, &borders.vertical.to_string()), width, borders.vertical));
    }
    lines.push(theme.paint(
        StyleRole::Accent,
        &format!(
            "{}{}{}",
            borders.bottom_left,
            borders.horizontal.to_string().repeat(width.saturating_sub(2)),
            borders.bottom_right
        ),
    ));
    lines.push(String::new());
    lines
}

/// `│ content<pad>│` totalling exactly `width` columns (colour-aware).
fn frame_line(content: &str, border: &str, width: usize, vertical: char) -> String {
    // Borders take two columns, the leading space takes one.
    let padding = width.saturating_sub(visible_width(content) + 4);
    let text = format!(" {content}{} ", " ".repeat(padding));
    let _ = vertical;
    format!("{border}{text}{border}")
}

/// Render one paragraph/verse/heading.
pub fn render_block(
    block: &Block,
    marks: &StoredIndex,
    document_id: Option<&str>,
    theme: &Theme,
    options: RenderOptions,
) -> Vec<String> {
    let ranges = document_id
        .map(|id| match block.kind {
            BlockKind::Verse => block
                .identifier
                .as_deref()
                .map(|verse| marks.bible_ranges(&bible_key_for(marks, id), verse).to_vec())
                .unwrap_or_default(),
            _ => block
                .identifier
                .as_deref()
                .map(|pid| marks.doc_ranges(id, pid).to_vec())
                .unwrap_or_default(),
        })
        .unwrap_or_default();

    let runs = styled_runs(block, &ranges, theme);
    let prefix = block_prefix(block, theme, options);
    let indent = visible_width(&prefix);
    let body_width = options.width.saturating_sub(indent).max(20);

    // Highlight runs are painted up-front, so wrapping happens on styled text.
    let painted: Vec<Run> = runs
        .into_iter()
        .map(|run| Run {
            text: paint_run(&run.text, run.role, theme),
            role: run.role,
        })
        .collect();
    let mut lines = Vec::new();
    for (index, line) in wrap_runs(&painted, body_width).into_iter().enumerate() {
        let lead = if index == 0 { prefix.clone() } else { " ".repeat(indent) };
        lines.push(format!("{lead}{line}"));
    }
    if block.kind == BlockKind::Heading(1) {
        lines.push(rule(theme, options.width, '━'));
    }
    lines
}

/// Paint one run. Without colour, highlights fall back to marker brackets so the
/// reader still sees which text is highlighted.
fn paint_run(text: &str, role: StyleRole, theme: &Theme) -> String {
    if theme.depth != crate::render::ColorDepth::None {
        return theme.paint(role, text);
    }
    match role {
        StyleRole::Highlight(_) => {
            let (open, close) = highlight_markers(theme.glyphs);
            format!("{open}{text}{close}")
        }
        StyleRole::Title | StyleRole::Heading(_) => text.to_ascii_uppercase(),
        _ => text.to_string(),
    }
}

fn highlight_markers(glyphs: crate::render::Glyphs) -> (&'static str, &'static str) {
    match glyphs {
        crate::render::Glyphs::Unicode => ("⟦", "⟧"),
        crate::render::Glyphs::Ascii => ("[", "]"),
    }
}

fn bible_key_for(marks: &StoredIndex, _id: &str) -> String {
    // Bible keys are discovered from the index itself; the CLI resolves the
    // active chapter before rendering, so match by suffix when ambiguous.
    marks.bible.keys().next().cloned().unwrap_or_default()
}

fn block_prefix(block: &Block, theme: &Theme, options: RenderOptions) -> String {
    let bar = if theme.glyphs == super::Glyphs::Ascii { "|" } else { "│" };
    match block.kind {
        BlockKind::Title => String::new(),
        BlockKind::Heading(level) => {
            let label = match (level <= 1, theme.glyphs) {
                (true, super::Glyphs::Ascii) => "== ",
                (false, super::Glyphs::Ascii) => "- ",
                (true, super::Glyphs::Unicode) => "━━ ",
                (false, super::Glyphs::Unicode) => "─ ",
            };
            format!("  {}", theme.paint(StyleRole::Heading(level), label))
        }
        BlockKind::Caption => "  ".to_string(),
        BlockKind::Verse => {
            let number = block
                .identifier
                .as_deref()
                .and_then(|id| id.trim_start_matches('v').split(['-', '_']).next_back())
                .unwrap_or("")
                .to_string();
            let heavy = if theme.glyphs == super::Glyphs::Ascii { "|" } else { "┃" };
            let label = if number.is_empty() { format!("{heavy} ") } else { format!("{number:>3} {heavy} ") };
            theme.paint(StyleRole::Scripture, &label)
        }
        BlockKind::Paragraph => {
            // Navigation pages (library index, year lists) are just anchors:
            // render them as a compact bulleted link list instead of pretending
            // they are numbered study paragraphs.
            if is_nav_link(block) {
                let bullet = if theme.glyphs == super::Glyphs::Ascii { "- " } else { "• " };
                return format!("  {}", theme.paint(StyleRole::Link, bullet));
            }
            if !options.gutter {
                return "    ".to_string();
            }
            match block.identifier.as_deref() {
                Some(pid) if !pid.is_empty() && pid != "1" => {
                    theme.paint(StyleRole::Dim, &format!("{pid:>4} {bar} "))
                }
                _ => format!("    {bar} "),
            }
        }
    }
}

/// A paragraph that is nothing but one or more links (WOL navigation pages).
pub fn is_nav_link(block: &Block) -> bool {
    block.kind == BlockKind::Paragraph
        && block
            .identifier
            .as_deref()
            .is_none_or(|identifier| identifier.is_empty())
        && !block.pieces.is_empty()
        && block.pieces.iter().all(|piece| piece.href.is_some())
}

/// Turn a block into styled runs: highlights win over emphasis, emphasis over body.
pub fn styled_runs(block: &Block, ranges: &[(i64, i64, i64)], theme: &Theme) -> Vec<Run> {
    let _ = theme;
    let total_units = tokenize::utf16_len_of(&block.text);

    if total_units == 0 {
        return Vec::new();
    }

    // colour[i] = highlight colour covering UTF-16 unit i (0 = none).
    let mut colour = vec![0i64; total_units];
    for (start_token, end_token, color) in ranges {
        if let Some(window) = tokenize::token_range_to_chars(&block.tokens, *start_token, *end_token) {
            let end = window.end.min(total_units);
            for slot in colour.iter_mut().take(end).skip(window.start) {
                *slot = highlight::safe_color(*color);
            }
        }
    }

    let units: Vec<u16> = block.text.encode_utf16().collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut cursor = 0usize;
    while cursor < total_units {
        let piece = block.pieces.iter().find(|piece| piece.start <= cursor && cursor < piece.end);
        let chunk_end = piece
            .map(|piece| piece.end.min(total_units))
            .unwrap_or(total_units);
        let chunk_end = chunk_end.max(cursor + 1);
        let role = match piece {
            Some(piece) if piece.href.is_some() => StyleRole::Link,
            _ => StyleRole::Body,
        };
        // Split the chunk wherever the colour changes.
        let mut segment_start = cursor;
        while segment_start < chunk_end {
            let current = colour[segment_start];
            let mut segment_end = segment_start + 1;
            while segment_end < chunk_end && colour[segment_end] == current {
                segment_end += 1;
            }
            let text = String::from_utf16_lossy(&units[segment_start..segment_end]);
            let role = if current > 0 {
                StyleRole::Highlight(current)
            } else {
                role
            };
            push_run(&mut runs, text, role);
            segment_start = segment_end;
        }
        cursor = chunk_end;
    }
    runs
}

fn push_run(runs: &mut Vec<Run>, text: String, role: StyleRole) {
    if text.is_empty() {
        return;
    }
    match runs.last_mut() {
        Some(last) if last.role == role => last.text.push_str(&text),
        _ => runs.push(Run::new(text, role)),
    }
}

/// Word-wrap runs to `width` visible columns (ANSI-aware via [`Theme::paint`]).
pub fn wrap_runs(runs: &[Run], width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for run in runs {
        for word in split_keep_spaces(&run.text) {
            let word_width = visible_width(&word);
            if word_width == 0 {
                current.push_str(&run.text.get(..0).unwrap_or(""));
                continue;
            }
            let is_space = word.chars().all(char::is_whitespace);
            if word_width > width && !is_space {
                // A single token longer than the line: hard-split it.
                if current_width > 0 {
                    lines.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                for chunk in chunk_by_width(&word, width) {
                    if visible_width(&chunk) >= width {
                        lines.push(chunk);
                    } else {
                        current_width = visible_width(&chunk);
                        current = chunk;
                    }
                }
                continue;
            }
            if current_width + word_width > width && current_width > 0 {
                if is_space {
                    continue;
                }
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            if is_space && current_width == 0 {
                continue;
            }
            current.push_str(&word);
            current_width += word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Split `text` into pieces no wider than `width` columns.
fn chunk_by_width(text: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut buffer = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width && !buffer.is_empty() {
            chunks.push(std::mem::take(&mut buffer));
            used = 0;
        }
        buffer.push(character);
        used += character_width;
    }
    if !buffer.is_empty() {
        chunks.push(buffer);
    }
    chunks
}

fn split_keep_spaces(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut buffer = String::new();
    let mut buffer_is_space = None;
    for character in text.chars() {
        let is_space = character.is_whitespace();
        match buffer_is_space {
            Some(flag) if flag == is_space => buffer.push(character),
            Some(_) => {
                parts.push(std::mem::take(&mut buffer));
                buffer.push(character);
                buffer_is_space = Some(is_space);
            }
            None => {
                buffer.push(character);
                buffer_is_space = Some(is_space);
            }
        }
    }
    if !buffer.is_empty() {
        parts.push(buffer);
    }
    parts
}

/// Plain (unstyled) word wrap, used for headers and notes.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word_width = visible_width(word);
        if !current.is_empty() && visible_width(&current) + 1 + word_width > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Visible columns of `text`, ignoring ANSI SGR sequences.
pub fn visible_width(text: &str) -> usize {
    let mut width = 0usize;
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character == '\x1b' {
            // Skip until the closing 'm' of the SGR sequence.
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
            continue;
        }
        width += UnicodeWidthChar::width(character).unwrap_or(0);
    }
    width
}

fn rule(theme: &Theme, width: usize, glyph: char) -> String {
    let glyph = if theme.glyphs == super::Glyphs::Ascii {
        if glyph == '━' {
            '='
        } else {
            '-'
        }
    } else {
        glyph
    };
    theme.paint(StyleRole::Accent, &glyph.to_string().repeat(width))
}

fn render_input_field(field: &InputField, theme: &Theme, options: RenderOptions) -> Vec<String> {
    let borders = theme.border();
    let width = options.width.max(20);
    let label = match (&field.label, &field.tag) {
        (Some(label), _) if !label.is_empty() => label.clone(),
        (_, Some(tag)) => format!("field {tag}"),
        _ => "field".to_string(),
    };
    let mut lines = vec![format!(
        "  {}{}{}{}",
        theme.paint(StyleRole::Note, &borders.top_left.to_string()),
        theme.paint(StyleRole::Note, &format!("{} {label} ", borders.horizontal.to_string().repeat(2))),
        theme.paint(
            StyleRole::Note,
            &borders.horizontal.to_string().repeat(width.saturating_sub(label.len() + 8))
        ),
        theme.paint(StyleRole::Note, &borders.top_right.to_string())
    )];
    if field.value.trim().is_empty() {
        lines.push(format!(
            "  {} {}",
            theme.paint(StyleRole::Note, &borders.vertical.to_string()),
            theme.paint(StyleRole::Dim, "(empty)")
        ));
    } else {
        for line in wrap_text(&field.value, width.saturating_sub(8)) {
            lines.push(format!(
                "  {} {}",
                theme.paint(StyleRole::Note, &borders.vertical.to_string()),
                theme.paint(StyleRole::Body, &line)
            ));
        }
    }
    lines.push(format!(
        "  {}{}{}",
        theme.paint(StyleRole::Note, &borders.bottom_left.to_string()),
        theme.paint(
            StyleRole::Note,
            &borders.horizontal.to_string().repeat(width.saturating_sub(4))
        ),
        theme.paint(StyleRole::Note, &borders.bottom_right.to_string())
    ));
    lines
}

fn render_notes(
    document: &Document,
    marks: &StoredIndex,
    document_id: Option<&str>,
    theme: &Theme,
    options: RenderOptions,
) -> Vec<String> {
    let Some(document_id) = document_id else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    let borders = theme.border();
    for (pid, _) in document.paragraphs() {
        for note in marks.doc_notes(document_id, &pid) {
            lines.extend(note_box(&pid, note, theme, options, &borders));
        }
    }
    lines
}

fn note_box(
    pid: &str,
    note: &NoteItem,
    theme: &Theme,
    options: RenderOptions,
    borders: &super::BorderSet,
) -> Vec<String> {
    let width = options.width.max(20);
    let title = note
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| format!("Note · ¶{pid}"));
    let mut lines = vec![format!(
        "  {} {} {}",
        theme.paint(StyleRole::Note, &borders.top_left.to_string()),
        theme.paint(StyleRole::Note, &format!("✎ {title}")),
        theme.paint(
            StyleRole::Note,
            &borders.horizontal.to_string().repeat(width.saturating_sub(title.len() + 6))
        )
    )];
    let body = note.content.clone();
    for line in wrap_text(&body, width.saturating_sub(6)) {
        lines.push(format!(
            "  {} {}",
            theme.paint(StyleRole::Note, &borders.vertical.to_string()),
            theme.paint(StyleRole::Note, &line)
        ));
    }
    lines.push(format!(
        "  {}{}",
        theme.paint(StyleRole::Note, &borders.bottom_left.to_string()),
        theme.paint(
            StyleRole::Note,
            &borders.horizontal.to_string().repeat(width.saturating_sub(3))
        )
    ));
    lines
}

/// Footnotes / cross-reference markers found in a block.
pub fn collect_footnotes(block: &Block) -> Vec<String> {
    let mut found = Vec::new();
    let lower = block.text.to_ascii_lowercase();
    for marker in ["see also", "compare", "cross-reference", "footnote"] {
        if lower.contains(marker) {
            found.push(format!("¶{}: {marker}", block.identifier.clone().unwrap_or_default()));
        }
    }
    found
}

/// Best-guess document id for a parsed page, preferring ids already in `wolMarks`.
pub fn guess_document_id(document: &Document, marks: &StoredIndex) -> Option<String> {
    // WOL document pages end in the numeric document id:
    // `/<lang>/wol/d/r<region>/lp-<lang>/<documentId>`. Trust it even when the
    // document has no marks yet — that is how the first highlight is created.
    if let Some(canonical) = document.source_url.as_deref() {
        let path = canonical.split(['?', '#']).next().unwrap_or(canonical);
        if path.contains("/wol/d/") {
            if let Some(tail) = path.rsplit('/').next() {
                if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
                    return Some(tail.to_string());
                }
            }
        }
    }
    // Fallback: the unique marks document that shares a paragraph id.
    let mut candidates: Vec<&String> = Vec::new();
    for (pid, _) in document.paragraphs() {
        if pid.is_empty() {
            continue;
        }
        for key in marks.docs.keys() {
            if marks.docs[key].contains_key(&pid) && !candidates.contains(&key) {
                candidates.push(key);
            }
        }
    }
    match candidates.len() {
        1 => candidates.first().map(|key| (*key).clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
