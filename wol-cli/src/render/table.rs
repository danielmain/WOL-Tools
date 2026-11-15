//! UTF-8 box-drawing tables that respect the terminal width.
//!
//! Used by `wol cache list`, `wol import`, `wol sync status`, and the TUI
//! sidebar so tabular data stays readable in 80 columns.

use unicode_width::UnicodeWidthStr;

use super::{Glyphs, StyleRole, Theme};

/// Build a table row from mixed `&str`/`String` values.
#[macro_export]
macro_rules! row {
    ($($cell:expr),* $(,)?) => {
        vec![$($cell.to_string()),*]
    };
}

pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    max_width: Option<usize>,
}

impl Table {
    pub fn new(headers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            headers: headers.into_iter().map(Into::into).collect(),
            rows: Vec::new(),
            max_width: None,
        }
    }

    pub fn max_width(mut self, width: usize) -> Self {
        self.max_width = Some(width);
        self
    }

    pub fn push<I, T>(&mut self, row: I)
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        self.rows.push(row.into_iter().map(Into::into).collect());
    }

    /// Convenience for rows whose cells have different `Into<String>` types.
    pub fn push_row(&mut self, cells: impl IntoIterator<Item = String>) {
        self.push(cells);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Number of data rows (without the header).
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Widest each column needs to be, truncated so the frame fits `max_width`.
    fn column_widths(&self, available: Option<usize>) -> Vec<usize> {
        let columns = self
            .headers
            .len()
            .max(self.rows.iter().map(Vec::len).max().unwrap_or(0));
        let mut widths = vec![0usize; columns];
        for (index, header) in self.headers.iter().enumerate() {
            widths[index] = widths[index].max(UnicodeWidthStr::width(header.as_str()));
        }
        for row in &self.rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(UnicodeWidthStr::width(cell.as_str()));
            }
        }
        // Frame overhead: one border char per column + one closing border.
        if let Some(available) = available {
            let overhead = columns * 3 + 1;
            let budget = available.saturating_sub(overhead);
            shrink(&mut widths, budget);
        }
        widths
    }

    pub fn render(&self, theme: &Theme) -> Vec<String> {
        if self.headers.is_empty() && self.rows.is_empty() {
            return Vec::new();
        }
        let widths = self.column_widths(self.max_width);
        let glyphs = GlyphSet::for_theme(theme);
        let mut lines = Vec::new();
        lines.push(self.border(theme, &glyphs, &widths, BorderKind::Top));
        if !self.headers.is_empty() {
            lines.push(self.row(theme, &glyphs, &widths, &self.headers, StyleRole::Accent));
            lines.push(self.border(theme, &glyphs, &widths, BorderKind::Middle));
        }
        for row in &self.rows {
            lines.push(self.row(theme, &glyphs, &widths, row, StyleRole::Body));
        }
        lines.push(self.border(theme, &glyphs, &widths, BorderKind::Bottom));
        lines
    }

    fn border(&self, theme: &Theme, glyphs: &GlyphSet, widths: &[usize], kind: BorderKind) -> String {
        let (left, join, right, horizontal) = match kind {
            BorderKind::Top => (glyphs.top_left, glyphs.top_join, glyphs.top_right, glyphs.horizontal),
            BorderKind::Middle => (glyphs.middle_left, glyphs.cross, glyphs.middle_right, glyphs.horizontal),
            BorderKind::Bottom => (glyphs.bottom_left, glyphs.bottom_join, glyphs.bottom_right, glyphs.horizontal),
        };
        let segments: Vec<String> = widths
            .iter()
            .map(|width| horizontal.to_string().repeat(width + 2))
            .collect();
        theme.paint(
            StyleRole::Dim,
            &format!("{left}{}{right}", segments.join(&join.to_string())),
        )
    }

    fn row(
        &self,
        theme: &Theme,
        glyphs: &GlyphSet,
        widths: &[usize],
        cells: &[String],
        role: StyleRole,
    ) -> String {
        let border = theme.paint(StyleRole::Dim, &glyphs.vertical.to_string());
        let rendered: Vec<String> = widths
            .iter()
            .enumerate()
            .map(|(index, width)| {
                let cell = cells.get(index).map(String::as_str).unwrap_or("");
                let truncated = truncate(cell, *width);
                let padding = width.saturating_sub(UnicodeWidthStr::width(truncated.as_str()));
                let padded = format!(" {truncated}{} ", " ".repeat(padding));
                if theme.depth == crate::render::ColorDepth::None {
                    padded
                } else {
                    theme.paint(role, &padded)
                }
            })
            .collect();
        format!("{border}{}{border}", rendered.join(&border))
    }
}

#[derive(Clone, Copy)]
enum BorderKind {
    Top,
    Middle,
    Bottom,
}

struct GlyphSet {
    top_left: char,
    top_join: char,
    top_right: char,
    middle_left: char,
    cross: char,
    middle_right: char,
    bottom_left: char,
    bottom_join: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
}

impl GlyphSet {
    fn for_theme(theme: &Theme) -> Self {
        match theme.glyphs {
            Glyphs::Unicode => Self {
                top_left: '┌',
                top_join: '┬',
                top_right: '┐',
                middle_left: '├',
                cross: '┼',
                middle_right: '┤',
                bottom_left: '└',
                bottom_join: '┴',
                bottom_right: '┘',
                horizontal: '─',
                vertical: '│',
            },
            Glyphs::Ascii => Self {
                top_left: '+',
                top_join: '+',
                top_right: '+',
                middle_left: '+',
                cross: '+',
                middle_right: '+',
                bottom_left: '+',
                bottom_join: '+',
                bottom_right: '+',
                horizontal: '-',
                vertical: '|',
            },
        }
    }
}

/// Trim the longest columns first until the total fits `budget`.
fn shrink(widths: &mut [usize], budget: usize) {
    let total: usize = widths.iter().sum();
    if total <= budget || widths.is_empty() {
        return;
    }
    let minimum = 3usize;
    let mut excess = total - budget;
    while excess > 0 {
        let Some((index, widest)) = widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > minimum)
            .max_by_key(|(_, width)| **width)
        else {
            break;
        };
        let cut = (*widest - minimum).min(excess);
        widths[index] = widest - cut;
        excess -= cut;
    }
}

fn truncate(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for character in text.chars() {
        let character_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width.saturating_sub(1) {
            break;
        }
        out.push(character);
        used += character_width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{ColorDepth, Glyphs, Theme};

    #[test]
    fn renders_unicode_grid() {
        let theme = Theme::new(ColorDepth::None, Glyphs::Unicode);
        let mut table = Table::new(["id", "title"]);
        table.push(["1", "Hello"]);
        let lines = table.render(&theme);
        assert_eq!(lines.len(), 5);
        assert!(lines[0].starts_with('┌'));
        assert!(lines[0].contains('┬'));
        assert!(lines[4].starts_with('└'));
        assert!(lines[1].contains("id"));
        assert!(lines[2].contains('┼'));
        assert!(lines[3].contains("Hello"));
    }

    #[test]
    fn renders_ascii_grid_when_requested() {
        let theme = Theme::new(ColorDepth::None, Glyphs::Ascii);
        let mut table = Table::new(["a"]);
        table.push(["b"]);
        let lines = table.render(&theme);
        assert!(lines.iter().all(|line| line.is_ascii()));
    }

    #[test]
    fn truncates_to_the_available_width() {
        let theme = Theme::new(ColorDepth::None, Glyphs::Unicode);
        let mut table = Table::new(["column"]).max_width(24);
        table.push(["a very long value that must be shortened"]);
        let lines = table.render(&theme);
        for line in &lines {
            assert!(
                crate::render::article::visible_width(line) <= 24,
                "{} is too wide",
                line
            );
        }
        assert!(lines[3].contains('…'));
    }

    #[test]
    fn empty_tables_render_nothing() {
        let theme = Theme::new(ColorDepth::None, Glyphs::Unicode);
        assert!(Table::new(Vec::<String>::new()).render(&theme).is_empty());
    }
}
