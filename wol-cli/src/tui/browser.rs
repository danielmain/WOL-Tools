//! Two-pane browser for WOL's directory pages (Bible versions, library,
//! meetings, publication years).
//!
//! `wol_core::dom::parse_nav` turns such a page into sections (the left menu:
//! `Vida y Ministerio`, `Estudio de La Atalaya`, `Otras publicaciones`, …) and
//! their cards. This module owns the selection and flattening; rendering lives in
//! `tui::mod`, so the navigation maths stays unit-testable.

use wol_core::dom::{NavItem, NavPage, NavSection};

/// What a left-pane row represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A sub-category heading; not selectable.
    Section(usize),
    /// A card inside a sub-category.
    Item { section: usize, item: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub label: String,
    pub detail: Option<String>,
}

impl Row {
    pub fn is_selectable(&self) -> bool {
        matches!(self.kind, RowKind::Item { .. })
    }
}

/// Left menu + right content state for one directory page.
pub struct Browser {
    pub page: NavPage,
    pub rows: Vec<Row>,
    /// Index into [`Browser::rows`]; always points at a selectable row.
    pub selected: usize,
    /// Toolbar label of the section being browsed (`BIBLIA`, `PUBLICACIONES`,
    /// `REUNIONES`), matched from the page URL against WOL's own menu.
    pub channel: Option<String>,
}

impl Browser {
    /// `None` when the page has no usable entries (never show an empty browser).
    pub fn new(page: NavPage) -> Option<Self> {
        let mut rows = Vec::new();
        for (section_index, section) in page.sections.iter().enumerate() {
            if let Some(title) = section
                .title
                .clone()
                .filter(|title| !title.trim().is_empty())
            {
                rows.push(Row {
                    kind: RowKind::Section(section_index),
                    label: title,
                    detail: None,
                });
            }
            for (item_index, item) in section.items.iter().enumerate() {
                rows.push(Row {
                    kind: RowKind::Item {
                        section: section_index,
                        item: item_index,
                    },
                    label: item.label.clone(),
                    detail: item.detail.clone(),
                });
            }
        }
        let selected = rows.iter().position(Row::is_selectable)?;
        Some(Self {
            page,
            rows,
            selected,
            channel: None,
        })
    }

    /// Move `delta` cards forward/back, wrapping at both ends and skipping
    /// headings.
    pub fn step(&mut self, delta: isize) {
        if delta == 0 || self.rows.is_empty() {
            return;
        }
        let total = self.rows.len() as isize;
        let direction = delta.signum();
        let mut index = self.selected as isize;
        for _ in 0..delta.abs() {
            for _ in 0..total {
                index = (index + direction).rem_euclid(total);
                if self.rows[index as usize].is_selectable() {
                    break;
                }
            }
        }
        self.selected = index as usize;
    }

    pub fn first(&mut self) {
        if let Some(index) = self.rows.iter().position(Row::is_selectable) {
            self.selected = index;
        }
    }

    pub fn last(&mut self) {
        if let Some(index) = self.rows.iter().rposition(Row::is_selectable) {
            self.selected = index;
        }
    }

    /// `(section index, section, item index, item)` for the current selection.
    pub fn selected_item(&self) -> Option<(usize, &NavSection, usize, &NavItem)> {
        let (section_index, item_index) = match self.rows.get(self.selected)?.kind {
            RowKind::Item { section, item } => (section, item),
            RowKind::Section(_) => return None,
        };
        let section = self.page.sections.get(section_index)?;
        let item = section.items.get(item_index)?;
        Some((section_index, section, item_index, item))
    }

    /// Section that owns the selection (drives the right pane).
    pub fn selected_section(&self) -> Option<(usize, &NavSection, usize)> {
        let (section_index, section, item_index, _) = self.selected_item()?;
        Some((section_index, section, item_index))
    }

    /// 1-based position of the selection among all cards of the page.
    pub fn item_ordinal(&self) -> usize {
        self.rows
            .iter()
            .take(self.selected + 1)
            .filter(|row| row.is_selectable())
            .count()
    }

    pub fn item_count(&self) -> usize {
        self.page.item_count()
    }

    /// Heading of the browsed page (`Biblias en español`), never a section name.
    pub fn page_title(&self) -> Option<String> {
        self.page
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
    }

    /// Title shown in the window header: channel + page heading, de-duplicated.
    pub fn header_title(&self) -> Option<String> {
        match (self.channel.clone(), self.page_title()) {
            (Some(channel), Some(title)) if !channel.eq_ignore_ascii_case(&title) => {
                Some(format!("{channel} · {title}"))
            }
            (Some(channel), _) => Some(channel),
            (None, Some(title)) => Some(title),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> NavPage {
        NavPage {
            title: Some("Reuniones".into()),
            sections: vec![
                NavSection {
                    title: Some("Vida y Ministerio".into()),
                    items: vec![NavItem {
                        label: "16-22 de noviembre".into(),
                        detail: Some("Guía de actividades".into()),
                        href: "/es/wol/d/r4/lp-s/1".into(),
                    }],
                },
                NavSection {
                    title: Some("Estudio de La Atalaya".into()),
                    items: vec![NavItem {
                        label: "Predicamos las buenas noticias".into(),
                        detail: Some("La Atalaya 2026 | septiembre".into()),
                        href: "/es/wol/d/r4/lp-s/2".into(),
                    }],
                },
                NavSection {
                    title: None,
                    items: vec![
                        NavItem {
                            label: "Guía de actividades".into(),
                            detail: None,
                            href: "/es/wol/library/r4/lp-s/1".into(),
                        },
                        NavItem {
                            label: "Cantemos con gozo".into(),
                            detail: None,
                            href: "/es/wol/library/r4/lp-s/2".into(),
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn rows_flatten_headings_and_cards_in_order() {
        let browser = Browser::new(page()).expect("browser");
        let labels: Vec<&str> = browser.rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "Vida y Ministerio",
                "16-22 de noviembre",
                "Estudio de La Atalaya",
                "Predicamos las buenas noticias",
                "Guía de actividades",
                "Cantemos con gozo",
            ]
        );
        assert_eq!(browser.selected, 1, "the first card, not the heading");
        assert_eq!(browser.page_title().as_deref(), Some("Reuniones"));
        assert_eq!(browser.item_count(), 4);
    }

    #[test]
    fn stepping_skips_headings_and_wraps() {
        let mut browser = Browser::new(page()).unwrap();
        browser.step(1);
        assert_eq!(browser.selected_item().map(|(_, _, _, item)| item.label.as_str()), Some("Predicamos las buenas noticias"));
        browser.step(1);
        assert_eq!(browser.selected_item().map(|(_, _, _, item)| item.label.as_str()), Some("Guía de actividades"));
        browser.step(-1);
        assert_eq!(browser.selected_item().map(|(_, _, _, item)| item.label.as_str()), Some("Predicamos las buenas noticias"));
        browser.step(3);
        assert_eq!(browser.selected_item().map(|(_, _, _, item)| item.label.as_str()), Some("16-22 de noviembre"));
        browser.first();
        assert_eq!(browser.item_ordinal(), 1);
        browser.last();
        assert_eq!(browser.item_ordinal(), 4);
        assert_eq!(browser.selected_item().map(|(_, _, _, item)| item.label.as_str()), Some("Cantemos con gozo"));
    }

    #[test]
    fn the_right_pane_follows_the_selected_section() {
        let mut browser = Browser::new(page()).unwrap();
        let (section, _, item) = browser.selected_section().unwrap();
        assert_eq!(browser.page.sections[section].title.as_deref(), Some("Vida y Ministerio"));
        assert_eq!(item, 0);
        browser.step(1);
        let (section, _, _) = browser.selected_section().unwrap();
        assert_eq!(browser.page.sections[section].title.as_deref(), Some("Estudio de La Atalaya"));
    }

    #[test]
    fn empty_pages_do_not_become_browsers() {
        assert!(Browser::new(NavPage::default()).is_none());
        assert!(Browser::new(NavPage {
            title: Some("Empty".into()),
            sections: vec![NavSection {
                title: Some("Nothing".into()),
                items: Vec::new(),
            }],
        })
        .is_none());
    }
}
