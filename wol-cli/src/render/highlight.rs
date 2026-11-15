//! Token ranges → ANSI-coloured spans.
//!
//! Thin layer over [`super::article::styled_runs`] plus the colour legend used by
//! `wol read --legend` and the TUI colour picker.

use super::{HighlightColor, StyleRole, Theme};

/// One legend entry: swatch + label for a highlight colour.
pub fn legend(theme: &Theme) -> Vec<String> {
    (1..=6)
        .map(|color| {
            let swatch = theme.paint(StyleRole::Highlight(color), &format!(" {color} "));
            format!("{swatch} {}", HighlightColor::label(color))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{ColorDepth, Glyphs};

    #[test]
    fn legend_lists_every_colour() {
        let theme = Theme::new(ColorDepth::None, Glyphs::Unicode);
        let legend = legend(&theme);
        assert_eq!(legend.len(), 6);
        assert!(legend[0].contains("yellow"));
        assert!(legend[5].contains("purple"));
    }

    #[test]
    fn colour_labels_are_the_extension_names() {
        let labels: Vec<&str> = (1..=6).map(HighlightColor::label).collect();
        assert_eq!(labels, vec!["yellow", "green", "blue", "pink", "orange", "purple"]);
        assert_eq!(HighlightColor::label(0), "none");
        assert_eq!(HighlightColor::label(7), "none");
    }
}
