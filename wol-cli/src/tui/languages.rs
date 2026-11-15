//! First-run / `Ctrl-l` language picker.
//!
//! WOL exposes ~1000 languages; the picker keeps a filter string and the
//! indices of the matching entries into [`wol_core::languages::all_languages`],
//! so every keystroke is a pure `filter -> Vec<usize>` recomputation.

use wol_core::languages::{all_languages, LanguageInfo};

/// How many entries the popup shows at once.
pub const PAGE: usize = 12;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguagePicker {
    pub filter: String,
    pub selected: usize,
    pub matches: Vec<usize>,
}

impl LanguagePicker {
    pub fn new() -> Self {
        let mut picker = Self::default();
        picker.refresh();
        picker
    }

    /// Recompute matches, keeping the selection in range.
    pub fn refresh(&mut self) {
        let needle = self.filter.trim().to_lowercase();
        self.matches = all_languages()
            .iter()
            .enumerate()
            .filter(|(_, language)| needle.is_empty() || matches(language, &needle))
            .map(|(index, _)| index)
            .collect();
        if self.matches.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.matches.len() - 1);
        }
    }

    pub fn push(&mut self, character: char) {
        self.filter.push(character);
        self.selected = 0;
        self.refresh();
    }

    pub fn pop(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.refresh();
    }

    pub fn clear(&mut self) {
        self.filter.clear();
        self.selected = 0;
        self.refresh();
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.matches.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.matches.len() - 1;
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, last as isize) as usize;
    }

    pub fn selected_language(&self) -> Option<&'static LanguageInfo> {
        let index = *self.matches.get(self.selected)?;
        all_languages().get(index)
    }

    /// Rows to draw: `(language, is_selected)` in display order.
    pub fn visible(&self) -> Vec<(&'static LanguageInfo, bool)> {
        if self.matches.is_empty() {
            return Vec::new();
        }
        let start = (self.selected / PAGE) * PAGE;
        let end = (start + PAGE).min(self.matches.len());
        (start..end)
            .filter_map(|position| {
                let index = *self.matches.get(position)?;
                let language = all_languages().get(index)?;
                Some((language, position == self.selected))
            })
            .collect()
    }

    /// `1-based selected / total` for the status line.
    pub fn position(&self) -> (usize, usize) {
        (
            if self.matches.is_empty() { 0 } else { self.selected + 1 },
            self.matches.len(),
        )
    }
}

fn matches(language: &LanguageInfo, needle: &str) -> bool {
    language.name.to_lowercase().contains(needle)
        || language.locale.to_lowercase().contains(needle)
        || language.lib.to_lowercase().contains(needle)
        || language.symbol.to_lowercase().contains(needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_with_every_language_listed() {
        let picker = LanguagePicker::new();
        assert_eq!(picker.matches.len(), all_languages().len());
        assert_eq!(picker.position(), (1, all_languages().len()));
    }

    #[test]
    fn filters_by_name_locale_and_symbol() {
        let mut picker = LanguagePicker::new();
        picker.push('s');
        picker.push('p');
        picker.push('a');
        let spanish = picker
            .matches
            .iter()
            .filter_map(|index| all_languages().get(*index))
            .find(|language| language.locale == "es");
        assert!(spanish.is_some(), "Spanish must match the filter \"spa\"");

        picker.clear();
        picker.push('e');
        picker.push('s');
        assert!(picker
            .matches
            .iter()
            .filter_map(|index| all_languages().get(*index))
            .any(|language| language.locale == "es"));
    }

    #[test]
    fn selection_clamps_and_scrolls_in_pages() {
        let mut picker = LanguagePicker::new();
        picker.move_by(-5);
        assert_eq!(picker.selected, 0);
        for _ in 0..PAGE + 3 {
            picker.move_by(1);
        }
        assert_eq!(picker.selected, PAGE + 3);
        let rows = picker.visible();
        assert_eq!(rows.len(), PAGE);
        assert_eq!(rows.last().map(|(_, selected)| *selected), Some(false));
        // The page window follows the selection.
        assert!(picker
            .visible()
            .iter()
            .any(|(_, selected)| *selected));
    }

    #[test]
    fn backspace_widens_the_match_set() {
        let mut picker = LanguagePicker::new();
        picker.push('x');
        picker.push('x');
        let narrow = picker.matches.len();
        picker.pop();
        assert!(picker.matches.len() >= narrow);
        picker.pop();
        assert_eq!(picker.matches.len(), all_languages().len());
    }

    #[test]
    fn unmatched_filter_yields_no_selection() {
        let mut picker = LanguagePicker::new();
        for character in "zzzzzz".chars() {
            picker.push(character);
        }
        assert!(picker.matches.is_empty());
        assert_eq!(picker.position(), (0, 0));
        assert!(picker.selected_language().is_none());
        assert!(picker.visible().is_empty());
    }

    #[test]
    fn default_language_home_matches_the_cli_expectation() {
        let spanish = wol_core::languages::find_by_locale("es").expect("Spanish table entry");
        assert_eq!(spanish.region, 4);
        assert_eq!(spanish.home_url("wol.jw.org"), "https://wol.jw.org/es/wol/h/r4/lp-s");
    }
}
