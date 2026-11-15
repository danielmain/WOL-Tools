//! Toggleable sidebar: cached documents, table of contents, and search hits.

use wol_core::backup::StoredIndex;
use wol_core::cache::CacheEntry;
use wol_core::dom::{BlockKind, Document};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Cached,
    Contents,
    Highlights,
}

impl SidebarTab {
    pub fn next(self) -> Self {
        match self {
            SidebarTab::Cached => SidebarTab::Contents,
            SidebarTab::Contents => SidebarTab::Highlights,
            SidebarTab::Highlights => SidebarTab::Cached,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            SidebarTab::Cached => "Cached",
            SidebarTab::Contents => "Contents",
            SidebarTab::Highlights => "Highlights",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarItem {
    pub label: String,
    /// Where selecting the item navigates (`None` = jump within the page).
    pub target: Option<String>,
    /// Paragraph/verse identifier for in-page jumps.
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Sidebar {
    pub visible: bool,
    pub tab: SidebarTab,
    pub selected: usize,
    pub scroll: usize,
}

impl Default for SidebarTab {
    fn default() -> Self {
        SidebarTab::Cached
    }
}

impl Sidebar {
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn next_tab(&mut self) {
        self.tab = self.tab.next();
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn select(&mut self, index: usize, len: usize) {
        if len == 0 {
            self.selected = 0;
            return;
        }
        self.selected = index.min(len - 1);
        self.scroll = self.scroll.min(self.selected);
    }

    pub fn items(
        &self,
        cached: &[CacheEntry],
        document: Option<&Document>,
        marks: &StoredIndex,
    ) -> Vec<SidebarItem> {
        match self.tab {
            SidebarTab::Cached => cached
                .iter()
                .map(|entry| SidebarItem {
                    label: pretty_url(entry.url.as_str()),
                    target: Some(entry.url.as_str().to_string()),
                    anchor: None,
                })
                .collect(),
            SidebarTab::Contents => document
                .map(|document| {
                    document
                        .nodes
                        .iter()
                        .filter_map(|node| match node {
                            wol_core::dom::Node_::Block(block)
                                if matches!(block.kind, BlockKind::Heading(_)) =>
                            {
                                Some(SidebarItem {
                                    label: format!("  {}", block.text.trim()),
                                    target: None,
                                    anchor: block.identifier.clone(),
                                })
                            }
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            SidebarTab::Highlights => {
                let mut items = Vec::new();
                for (document_id, paragraphs) in &marks.docs {
                    for (pid, spans) in paragraphs {
                        items.push(SidebarItem {
                            label: format!("¶{pid}  {} mark(s) · {document_id}", spans.len()),
                            target: None,
                            anchor: Some(pid.clone()),
                        });
                    }
                }
                items
            }
        }
    }
}

/// `https://wol.jw.org/en/wol/d/r1/lp-e/2025441` → `en · 2025441`
pub fn pretty_url(url: &str) -> String {
    let path = url.split('#').next().unwrap_or(url);
    let stripped = path
        .strip_prefix("https://")
        .or_else(|| path.strip_prefix("http://"))
        .or_else(|| path.strip_prefix("wol://"))
        .unwrap_or(path);
    // First segment may be the host (`wol.jw.org`) or the language (`es`).
    let mut segments: Vec<&str> = stripped.split('/').filter(|segment| !segment.is_empty()).collect();
    if segments.first().is_some_and(|segment| segment.contains('.')) {
        segments.remove(0);
    }
    match (segments.first(), segments.last()) {
        (Some(language), Some(id)) if segments.len() > 1 => format!("{language} · {id}"),
        (Some(single), _) => (*single).to_string(),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_cycle() {
        assert_eq!(SidebarTab::Cached.next(), SidebarTab::Contents);
        assert_eq!(SidebarTab::Contents.next(), SidebarTab::Highlights);
        assert_eq!(SidebarTab::Highlights.next(), SidebarTab::Cached);
    }

    #[test]
    fn selection_is_clamped() {
        let mut sidebar = Sidebar::default();
        sidebar.select(10, 3);
        assert_eq!(sidebar.selected, 2);
        sidebar.select(5, 0);
        assert_eq!(sidebar.selected, 0);
    }

    #[test]
    fn urls_are_shortened() {
        assert_eq!(
            pretty_url("https://wol.jw.org/en/wol/d/r1/lp-e/2025441"),
            "en · 2025441"
        );
        assert_eq!(pretty_url("wol://es/wol/h/r4/lp-s"), "es · lp-s");
    }

    #[test]
    fn contents_are_derived_from_headings() {
        let document = wol_core::dom::parse_document(
            r#"<div id="article"><h2>First</h2><p data-pid="2">Body</p><h3>Second</h3></div>"#,
        );
        let mut sidebar = Sidebar::default();
        sidebar.tab = SidebarTab::Contents;
        let items = sidebar.items(&[], Some(&document), &StoredIndex::default());
        assert_eq!(items.len(), 2);
        assert!(items[0].label.contains("First"));
        assert!(items[1].label.contains("Second"));
    }
}
