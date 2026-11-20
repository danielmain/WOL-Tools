//! Scrollable reader view: rendered lines, cursor token, visual selection, search.
//!
//! Kept free of terminal I/O so every behaviour (scrolling, selection, token
//! math, search) is unit-testable; `tui/mod.rs` only maps key events onto it.

use wol_core::backup::{Span, StoredIndex};
use wol_core::dom::{Block, BlockKind, Document, Link, Node_};
use wol_core::highlight;
use wol_core::tokenize;

use unicode_width::UnicodeWidthChar;

use crate::render::article::{guess_document_id, utf16_window_to_chars, Fragment, RenderOptions};
use crate::render::Theme;

/// One rendered line plus the paragraph it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub block: Option<usize>,
    /// Where each piece of the paragraph sits on this line (screen columns).
    pub fragments: Vec<Fragment>,
}

/// A window of a paragraph's text, in `char` offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharRange {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

/// What a painted column range stands for (the TUI picks the styling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    /// The visual selection (`v` … arrow keys).
    Selection,
    /// The token the cursor is on.
    Cursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub block: usize,
    pub start_token: i64,
    pub end_token: i64,
}

#[derive(Debug, Clone)]
pub struct Reader {
    pub document: Document,
    pub marks: StoredIndex,
    pub document_id: Option<String>,
    pub lines: Vec<Line>,
    /// First rendered line shown in the viewport.
    pub scroll: usize,
    /// Row the cursor sits on (nvim-style cursorline), always inside the viewport.
    pub cursor_line: usize,
    pub cursor_token: i64,
    pub selection: Option<Selection>,
    pub search: Option<String>,
    pub hits: Vec<usize>,
    pub hit_index: usize,
}

impl Reader {
    pub fn new(document: Document, marks: StoredIndex, theme: &Theme, options: RenderOptions) -> Self {
        let document_id = guess_document_id(&document, &marks);
        let mut reader = Self {
            document,
            marks,
            document_id,
            lines: Vec::new(),
            scroll: 0,
            cursor_line: 0,
            cursor_token: 0,
            selection: None,
            search: None,
            hits: Vec::new(),
            hit_index: 0,
        };
        reader.rebuild(theme, options);
        reader
    }

    pub fn rebuild(&mut self, theme: &Theme, options: RenderOptions) {
        let rendered =
            crate::render::article::render_document_lines(&self.document, &self.marks, theme, options);
        self.lines = rendered
            .into_iter()
            .map(|line| Line {
                text: line.text,
                block: line.node,
                fragments: line.fragments,
            })
            .collect();
        self.scroll = self.scroll.min(self.lines.len().saturating_sub(1));
        self.cursor_line = self.cursor_line.min(self.lines.len().saturating_sub(1));
    }

    /// Row the cursor is on, clamped to the rendered document.
    pub fn cursor_line(&self) -> usize {
        self.cursor_line.min(self.lines.len().saturating_sub(1))
    }

    /// Put the cursor on `line`, keeping the token cursor valid for its block.
    pub fn set_cursor_line(&mut self, line: usize) {
        self.cursor_line = line.min(self.lines.len().saturating_sub(1));
        self.clamp_token_to_block();
        if let Some(selection) = self.selection.as_mut() {
            selection.end_token = self.cursor_token;
        }
    }

    /// Move the cursor one row down/up, scrolling only when it leaves the viewport.
    pub fn move_line(&mut self, delta: isize, viewport: usize) {
        let last = self.lines.len().saturating_sub(1) as isize;
        let next = (self.cursor_line as isize + delta).clamp(0, last) as usize;
        let changed_block = self.line_block(next) != self.line_block(self.cursor_line);
        self.cursor_line = next;
        if changed_block {
            // Ranges never span paragraphs, so a selection cannot follow the cursor.
            self.selection = None;
            self.cursor_token = 0;
        }
        self.clamp_token_to_block();
        if let Some(selection) = self.selection.as_mut() {
            selection.end_token = self.cursor_token;
        }
        self.follow_cursor(viewport);
    }

    /// Scroll just enough to keep the cursor row visible.
    fn follow_cursor(&mut self, viewport: usize) {
        let height = viewport.max(1);
        if self.cursor_line < self.scroll {
            self.scroll = self.cursor_line;
        } else if self.cursor_line >= self.scroll + height {
            self.scroll = self.cursor_line + 1 - height;
        }
        let max = self.lines.len().saturating_sub(height);
        self.scroll = self.scroll.min(max);
    }

    /// Move the cursor row to the line that shows the token under the cursor.
    ///
    /// `h`/`l` used to move the token without touching `cursor_line`, so on a
    /// paragraph wider than the viewport the cursor row stayed on the first
    /// wrapped line: `v` started selecting on line 1, the band was painted off
    /// screen, and the gutter indicator landed on the wrong row. Following the
    /// token keeps "where the cursor is" and "what the cursor is on" in sync.
    pub fn follow_token(&mut self, viewport: usize) {
        let Some(block) = self.active_block_index() else {
            return;
        };
        let Some(row) = self.token_line(block) else {
            return;
        };
        self.cursor_line = row;
        self.follow_cursor(viewport);
    }

    /// The rendered line that shows the token under the cursor, if any.
    fn token_line(&self, block: usize) -> Option<usize> {
        let parsed = self.block_at(block)?;
        let window = tokenize::token_range_to_chars(&parsed.tokens, self.cursor_token, self.cursor_token)?;
        let (start, end) = utf16_window_to_chars(&parsed.text, window)?;
        self.lines.iter().enumerate().find_map(|(index, line)| {
            (line.block == Some(block)
                && line
                    .fragments
                    .iter()
                    .any(|fragment| fragment.start < end && start < fragment.end))
                .then_some(index)
        })
    }

    /// Keep the token cursor inside the paragraph under the cursor row.
    ///
    /// Scrolling used to leave `cursor_token` pointing into a previous, longer
    /// paragraph; `v` + `1`-`6` then stored a range that covered no text, so the
    /// highlight silently never appeared. Clamping here makes the first highlight
    /// always land on a real token.
    fn clamp_token_to_block(&mut self) {
        let Some(block) = self.current_block() else {
            self.cursor_token = 0;
            return;
        };
        let total = block.tokens.len() as i64;
        self.cursor_token = if total == 0 { 0 } else { self.cursor_token.clamp(0, total - 1) };
    }

    pub fn current_block(&self) -> Option<&Block> {
        let index = self.active_block_index()?;
        self.document
            .nodes
            .get(index)
            .and_then(|node| match node {
                Node_::Block(block) => Some(block),
                _ => None,
            })
    }

    fn current_line(&self) -> usize {
        self.cursor_line()
    }

    fn line_block(&self, line: usize) -> Option<usize> {
        self.lines.get(line).and_then(|entry| entry.block)
    }

    /// The `Block` a node index points at.
    fn block_at(&self, index: usize) -> Option<&Block> {
        match self.document.nodes.get(index)? {
            Node_::Block(block) => Some(block),
            _ => None,
        }
    }

    /// Text window covered by the inclusive token range `start..=end`.
    ///
    /// Token ranges are UTF-16 based (like the extension); the renderer and the
    /// TUI work in `char` offsets, so the window is converted here.
    fn token_chars(&self, index: usize, start: i64, end: i64) -> Option<CharRange> {
        let block = self.block_at(index)?;
        let window = tokenize::token_range_to_chars(&block.tokens, start, end)?;
        let (start, end) = utf16_window_to_chars(&block.text, window)?;
        Some(CharRange {
            block: index,
            start,
            end,
        })
    }

    /// Paragraph text covered by the visual selection.
    pub fn selection_chars(&self) -> Option<CharRange> {
        let (block, start, end) = self.selected_range()?;
        self.token_chars(block, start, end)
    }

    /// Paragraph text covered by the token under the cursor.
    pub fn cursor_chars(&self) -> Option<CharRange> {
        let block = self.active_block_index()?;
        self.token_chars(block, self.cursor_token, self.cursor_token)
    }

    /// Screen columns the selection and the token cursor occupy, per line.
    ///
    /// Lines are already wrapped and painted (gutter, highlight markers), so the
    /// columns come from the fragments the renderer recorded — the overlay lands
    /// on exactly the words a highlight would cover.
    pub fn overlays(&self) -> Vec<(usize, Vec<(usize, usize, OverlayKind)>)> {
        let wanted: [(Option<CharRange>, OverlayKind); 2] = [
            (self.selection_chars(), OverlayKind::Selection),
            (self.cursor_chars(), OverlayKind::Cursor),
        ];
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let block = line.block?;
                let text = self.block_at(block)?.text.as_str();
                let pieces: Vec<(usize, usize, OverlayKind)> = wanted
                    .iter()
                    .filter_map(|(range, kind)| {
                        let range = (*range)?;
                        (range.block == block).then_some((range, *kind))
                    })
                    .flat_map(|(range, kind)| {
                        line.fragments.iter().filter_map(move |fragment| {
                            let begin = range.start.max(fragment.start);
                            let finish = range.end.min(fragment.end);
                            if begin >= finish {
                                return None;
                            }
                            let head = columns(text, fragment.start, begin);
                            let span = columns(text, begin, finish);
                            Some((fragment.col + head, fragment.col + head + span, kind))
                        })
                    })
                    .collect();
                if pieces.is_empty() {
                    None
                } else {
                    Some((index, pieces))
                }
            })
            .collect()
    }

    /// Plain text of the current selection, for the status line.
    pub fn selected_text(&self) -> Option<String> {
        let range = self.selection_chars()?;
        let text = self.block_at(range.block)?.text.as_str();
        let taken: String = text.chars().skip(range.start).take(range.end - range.start).collect();
        let trimmed = taken.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed.chars().take(40).collect())
    }

    /// Page-style scrolling (mouse wheel, `less`-like paging) that keeps the
    /// cursor row inside the new viewport.
    #[allow(dead_code)]
    pub fn scroll_by(&mut self, delta: isize, viewport: usize) {
        let height = viewport.max(1);
        let max = self.lines.len().saturating_sub(height);
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, max as isize) as usize;
        // The cursor follows the screen instead of being left behind above it.
        let bottom = (self.scroll + height - 1).min(self.lines.len().saturating_sub(1));
        let line = self.cursor_line().clamp(self.scroll, bottom.max(self.scroll));
        self.set_cursor_line(line);
    }

    pub fn scroll_to(&mut self, line: usize) {
        let line = line.min(self.lines.len().saturating_sub(1));
        self.scroll = line;
        self.set_cursor_line(line);
    }

    pub fn top(&mut self) {
        self.scroll = 0;
        self.set_cursor_line(0);
    }

    pub fn bottom(&mut self, viewport: usize) {
        self.scroll = self.lines.len().saturating_sub(viewport.max(1));
        self.set_cursor_line(self.lines.len().saturating_sub(1));
    }

    /// Move the token cursor within the paragraph under the viewport top.
    pub fn move_cursor_token(&mut self, delta: i64) {
        // Inside a selection, the cursor belongs to the selected block.
        let selection_block = self.selection.as_ref().map(|selection| selection.block);
        let Some(block) = selection_block
            .and_then(|index| self.document.nodes.get(index))
            .and_then(|node| match node {
                Node_::Block(block) => Some(block),
                _ => None,
            })
            .or_else(|| self.current_block())
        else {
            return;
        };
        let total = block.tokens.len() as i64;
        if total == 0 {
            return;
        }
        self.cursor_token = (self.cursor_token + delta).clamp(0, total - 1);
        if let Some(selection) = self.selection.as_mut() {
            selection.end_token = self.cursor_token;
        }
        // A selection belongs to the block its anchor lives in.
    }

    /// Jump to the next paragraph, keeping the cursor at its first token.
    pub fn move_block(&mut self, delta: isize) {
        let blocks = self.block_indices();
        if blocks.is_empty() {
            return;
        }
        let current = self
            .active_block_index()
            .and_then(|index| blocks.iter().position(|candidate| *candidate == index))
            .unwrap_or(0);
        let next = (current as isize + delta).clamp(0, blocks.len() as isize - 1) as usize;
        let target = blocks[next];
        if let Some(line) = self.lines.iter().position(|entry| entry.block == Some(target)) {
            self.scroll_to(line);
        }
        self.cursor_token = 0;
    }

    fn block_indices(&self) -> Vec<usize> {
        self.document
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| match node {
                Node_::Block(block) if has_tokens(block) => Some(index),
                _ => None,
            })
            .collect()
    }

    /// Start a visual selection at the cursor token.
    ///
    /// Anchors to the paragraph under the cursor row; when that line is a
    /// header/rule, the nearest text paragraph is used so `v` always works.
    pub fn begin_selection(&mut self) {
        let Some(index) = self.active_block_index() else {
            return;
        };
        self.clamp_token_to_block();
        if self.current_block().is_none_or(|block| block.tokens.is_empty()) {
            return;
        }
        self.selection = Some(Selection {
            block: index,
            start_token: self.cursor_token,
            end_token: self.cursor_token,
        });
    }

    /// Nearest text paragraph at the cursor row, else after it, else before it.
    pub fn active_block_index(&self) -> Option<usize> {
        let has_text = |index: usize| {
            self.document
                .nodes
                .get(index)
                .is_some_and(|node| matches!(node, Node_::Block(block) if !block.tokens.is_empty()))
        };
        let line = self.current_line();
        if let Some(index) = self.line_block(line).filter(|index| has_text(*index)) {
            return Some(index);
        }
        self.lines
            .iter()
            .skip(line)
            .find_map(|entry| entry.block.filter(|index| has_text(*index)))
            .or_else(|| {
                self.lines[..line.min(self.lines.len())]
                    .iter()
                    .rev()
                    .find_map(|entry| entry.block.filter(|index| has_text(*index)))
            })
    }

    pub fn cancel_selection(&mut self) {
        self.selection = None;
    }

    /// Normalised inclusive token range of the current selection.
    ///
    /// The anchor is `start_token`; the live cursor is the moving end, so the
    /// selection always reflects where the user is right now.
    pub fn selected_range(&self) -> Option<(usize, i64, i64)> {
        let selection = self.selection.as_ref()?;
        let (start, end) = if selection.start_token <= self.cursor_token {
            (selection.start_token, self.cursor_token)
        } else {
            (self.cursor_token, selection.start_token)
        };
        Some((selection.block, start, end))
    }

    /// Apply a colour to the selection; returns the block identifier touched.
    ///
    /// `None` means "nothing happened": no selection, or no document id yet
    /// (the page was parsed without a canonical URL).
    pub fn apply_color(&mut self, color: i64) -> Option<String> {
        let (block_index, start, end) = self.selected_range()?;
        let (identifier, kind) = {
            let node = self.document.nodes.get(block_index)?;
            match node {
                Node_::Block(block) => (block.identifier.clone()?, block.kind),
                _ => return None,
            }
        };
        let span: Span = (start, end, highlight::safe_color(color));
        match kind {
            BlockKind::Verse => {
                let key = self
                    .marks
                    .bible
                    .keys()
                    .next()
                    .cloned()
                    .unwrap_or_default();
                let verses = self.marks.bible.entry(key).or_default();
                let existing = verses.get(&identifier).cloned().unwrap_or_default();
                verses.insert(identifier.clone(), highlight::add_range(&existing, span));
            }
            _ => {
                let Some(document_id) = self.document_id.clone() else {
                    return None;
                };
                let paragraphs = self.marks.docs.entry(document_id).or_default();
                let existing = paragraphs.get(&identifier).cloned().unwrap_or_default();
                paragraphs.insert(identifier.clone(), highlight::add_range(&existing, span));
            }
        }
        self.marks = wol_core::backup::recount(&self.marks);
        self.selection = None;
        Some(identifier)
    }

    /// Remove highlights under the selection.
    pub fn clear_color(&mut self) -> Option<String> {
        let (block_index, start, end) = self.selected_range()?;
        let identifier = match self.document.nodes.get(block_index)? {
            Node_::Block(block) => block.identifier.clone()?,
            _ => return None,
        };
        if let Some(document_id) = self.document_id.clone() {
            if let Some(paragraphs) = self.marks.docs.get_mut(&document_id) {
                if let Some(spans) = paragraphs.get_mut(&identifier) {
                    *spans = highlight::remove_range(spans, (start, end));
                    if spans.is_empty() {
                        paragraphs.remove(&identifier);
                    }
                }
                if paragraphs.is_empty() {
                    self.marks.docs.remove(&document_id);
                }
            }
        }
        self.marks = wol_core::backup::recount(&self.marks);
        self.selection = None;
        Some(identifier)
    }

    /// Search rendered text; stores line indices of every hit.
    pub fn search(&mut self, needle: &str) {
        let needle = needle.trim().to_string();
        if needle.is_empty() {
            self.search = None;
            self.hits.clear();
            return;
        }
        let lowercase = needle.to_lowercase();
        self.hits = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.text.to_lowercase().contains(&lowercase))
            .map(|(index, _)| index)
            .collect();
        self.hit_index = 0;
        self.search = Some(needle);
        if let Some(first) = self.hits.first() {
            self.scroll_to(*first);
        }
    }

    pub fn next_hit(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        self.hit_index = (self.hit_index + 1) % self.hits.len();
        self.scroll_to(self.hits[self.hit_index]);
    }

    pub fn previous_hit(&mut self) {
        if self.hits.is_empty() {
            return;
        }
        self.hit_index = if self.hit_index == 0 {
            self.hits.len() - 1
        } else {
            self.hit_index - 1
        };
        self.scroll_to(self.hits[self.hit_index]);
    }

    /// Visible window of lines.
    pub fn viewport(&self, height: usize) -> &[Line] {
        let end = (self.scroll + height).min(self.lines.len());
        let start = self.scroll.min(end);
        &self.lines[start..end]
    }

    /// Text of the token under the cursor, for the status line.
    pub fn cursor_token_text(&self) -> Option<String> {
        let block = self.current_block()?;
        let token = block
            .tokens
            .get(usize::try_from(self.cursor_token).ok()?)?;
        let units: Vec<u16> = block.text.encode_utf16().collect();
        let text = String::from_utf16_lossy(
            &units[token.start.min(units.len())..token.end.min(units.len())],
        );
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some(text.chars().take(24).collect())
    }

    /// Status line: document, position, highlight count, search state.
    pub fn status(&self, theme: &Theme) -> String {
        let position = if self.lines.is_empty() {
            0
        } else {
            ((self.cursor_line() + 1) * 100 / self.lines.len()).min(100)
        };
        let document_id = self.document_id.clone().unwrap_or_else(|| "?".into());
        let mut parts = vec![
            format!("doc {document_id}"),
            format!(
                "{}/{} lines",
                if self.lines.is_empty() { 0 } else { self.cursor_line() + 1 },
                self.lines.len()
            ),
            format!("{position}%"),
            format!("{} tokens", self.total_tokens()),
            format!("{} marks", self.marks.highlight_count),
        ];
        if let Some(block) = self.current_block() {
            if !block.tokens.is_empty() {
                parts.push(format!(
                    "tok {}/{}",
                    self.cursor_token + 1,
                    block.tokens.len()
                ));
                if let Some(text) = self.cursor_token_text() {
                    parts.push(format!("«{text}»"));
                }
            }
        }
        if let Some(search) = &self.search {
            parts.push(format!(
                "/{search} {}/{}",
                if self.hits.is_empty() { 0 } else { self.hit_index + 1 },
                self.hits.len()
            ));
        }
        if let Some(selection) = &self.selection {
            let (start, end) = if selection.start_token <= self.cursor_token {
                (selection.start_token, self.cursor_token)
            } else {
                (self.cursor_token, selection.start_token)
            };
            let span = format!("VISUAL ¶{} tok {start}-{end}", self.current_pid().unwrap_or_default());
            parts.push(match self.selected_text() {
                Some(text) => format!("{span} «{text}»"),
                None => span,
            });
        } else if let Some(text) = self.cursor_token_text() {
            // Without a selection the status still says what `v` + a colour would mark.
            parts.push(format!("cursor «{text}»"));
        }
        let _ = theme;
        parts.join("  ·  ")
    }

    /// Colour legend used by the picker overlay.
    pub fn legend(&self, theme: &Theme) -> Vec<String> {
        crate::render::highlight::legend(theme)
    }

    /// First non-empty paragraph identifier (note target).
    pub fn current_pid(&self) -> Option<String> {
        self.current_block().and_then(|block| block.identifier.clone())
    }

    /// Auto-scroll so `pid` is visible.
    pub fn jump_to(&mut self, pid: &str) {
        let Some(index) = self
            .document
            .nodes
            .iter()
            .position(|node| matches!(node, Node_::Block(block) if block.identifier.as_deref() == Some(pid)))
        else {
            return;
        };
        if let Some(line) = self.lines.iter().position(|entry| entry.block == Some(index)) {
            self.scroll_to(line);
            self.cursor_token = 0;
        }
    }

    /// Every link on the page, in document order.
    pub fn links(&self) -> &[Link] {
        &self.document.links
    }

    /// First line rendered for `node`, or `None` when it produced no text.
    pub fn line_of_node(&self, node: usize) -> Option<usize> {
        self.lines
            .iter()
            .position(|entry| entry.block == Some(node))
    }

    /// Scroll to a link and return it.
    pub fn focus_link(&mut self, index: usize) -> Option<Link> {
        let link = self.document.links.get(index)?.clone();
        if let Some(line) = self.line_of_node(link.node) {
            self.scroll_to(line);
        }
        Some(link)
    }

    /// Index of the first link in the block under the viewport top.
    pub fn link_at_top(&self) -> Option<usize> {
        let block = self.active_block_index()?;
        self.document.links.iter().position(|link| link.node == block)
    }

    /// Index of the link closest to the current scroll position (for `Tab`).
    pub fn nearest_link(&self) -> Option<usize> {
        let target = self.scroll;
        self.document
            .links
            .iter()
            .enumerate()
            .filter_map(|(index, link)| {
                self.line_of_node(link.node)
                    .map(|line| (index, line.abs_diff(target)))
            })
            .min_by_key(|(_, distance)| *distance)
            .map(|(index, _)| index)
    }

    /// Move the link cursor by `delta`, scrolling to the new link.
    ///
    /// Wraps around the ends so `j` on the last link returns to the top.
    pub fn step_link(&mut self, index: usize, delta: isize) -> Option<(usize, Link)> {
        let total = self.document.links.len();
        if total == 0 {
            return None;
        }
        let next = (index as isize + delta).rem_euclid(total as isize) as usize;
        let link = self.focus_link(next)?;
        Some((next, link))
    }

    /// Highlighted runs for the paragraph under the cursor (for the footnote drawer).
    pub fn tokens_of_current_block(&self) -> Vec<String> {
        let Some(block) = self.current_block() else {
            return Vec::new();
        };
        let units: Vec<u16> = block.text.encode_utf16().collect();
        block
            .tokens
            .iter()
            .map(|token| String::from_utf16_lossy(&units[token.start.min(units.len())..token.end.min(units.len())]))
            .collect()
    }

    /// Token range of the whole document, useful for tests.
    pub fn total_tokens(&self) -> usize {
        self.document
            .nodes
            .iter()
            .filter_map(|node| match node {
                Node_::Block(block) => Some(block.tokens.len()),
                _ => None,
            })
            .sum()
    }
}

fn has_tokens(block: &Block) -> bool {
    !block.tokens.is_empty()
}

/// Terminal columns of the `char` window `start..end` of `text`.
fn columns(text: &str, start: usize, end: usize) -> usize {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|character| UnicodeWidthChar::width(character).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests;
