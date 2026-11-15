//! Scrollable reader view: rendered lines, cursor token, visual selection, search.
//!
//! Kept free of terminal I/O so every behaviour (scrolling, selection, token
//! math, search) is unit-testable; `tui/mod.rs` only maps key events onto it.

use wol_core::backup::{Span, StoredIndex};
use wol_core::dom::{Block, BlockKind, Document, Node_};
use wol_core::highlight;

use crate::render::article::{guess_document_id, RenderOptions};
use crate::render::Theme;

/// One rendered line plus the paragraph it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    pub block: Option<usize>,
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
    pub scroll: usize,
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
            })
            .collect();
        self.scroll = self.scroll.min(self.lines.len().saturating_sub(1));
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
        self.scroll
    }

    fn line_block(&self, line: usize) -> Option<usize> {
        self.lines.get(line).and_then(|entry| entry.block)
    }

    pub fn scroll_by(&mut self, delta: isize, viewport: usize) {
        let max = self.lines.len().saturating_sub(viewport.max(1));
        let next = self.scroll as isize + delta;
        self.scroll = next.clamp(0, max as isize) as usize;
    }

    pub fn scroll_to(&mut self, line: usize) {
        self.scroll = line.min(self.lines.len().saturating_sub(1));
    }

    pub fn top(&mut self) {
        self.scroll = 0;
    }

    pub fn bottom(&mut self, viewport: usize) {
        self.scroll = self.lines.len().saturating_sub(viewport.max(1));
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
    /// Anchors to the paragraph under the viewport top; when that line is a
    /// header/rule, the next paragraph is used so `v` always works.
    pub fn begin_selection(&mut self) {
        let Some(index) = self.active_block_index() else {
            return;
        };
        self.selection = Some(Selection {
            block: index,
            start_token: self.cursor_token,
            end_token: self.cursor_token,
        });
    }

    /// Nearest paragraph at or after the viewport top.
    pub fn active_block_index(&self) -> Option<usize> {
        if let Some(index) = self.line_block(self.current_line()) {
            return Some(index);
        }
        self.lines
            .iter()
            .skip(self.current_line())
            .find_map(|line| line.block)
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

    /// Status line: document, position, highlight count, search state.
    pub fn status(&self, theme: &Theme) -> String {
        let position = if self.lines.is_empty() {
            0
        } else {
            ((self.scroll + 1) * 100 / self.lines.len()).min(100)
        };
        let document_id = self.document_id.clone().unwrap_or_else(|| "?".into());
        let mut parts = vec![
            format!("doc {document_id}"),
            format!("{position}%"),
            format!("{} lines", self.lines.len()),
            format!("{} tokens", self.total_tokens()),
            format!("{} marks", self.marks.highlight_count),
        ];
        if let Some(search) = &self.search {
            parts.push(format!(
                "/{search} {}/{}",
                if self.hits.is_empty() { 0 } else { self.hit_index + 1 },
                self.hits.len()
            ));
        }
        if let Some(selection) = &self.selection {
            let (start, end) = if selection.start_token <= selection.end_token {
                (selection.start_token, selection.end_token)
            } else {
                (selection.end_token, selection.start_token)
            };
            parts.push(format!("VISUAL {start}-{end}"));
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

#[cfg(test)]
mod tests;
