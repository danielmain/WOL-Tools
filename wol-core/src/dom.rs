//! HTML → token-aligned document model.
//!
//! WOL article pages are server-rendered: titles are `p[data-pid="1"]`, study
//! paragraphs are `p[data-pid="N"]`, Bible verses are `span.v[id^="v<book>-…"]`.
//! [`parse_document`] reproduces the text-collection rules of
//! `wol-highlights/lib/highlight.js` (same ignore list, same order) so that the
//! token indices computed here line up 1:1 with the ranges stored in `wolMarks`.
//!
//! Everything here is pure: `&str` in, [`Document`] out — no I/O, no network.

use std::collections::BTreeMap;

use scraper::{ElementRef, Html, Node, Selector};

use crate::tokenize::{self, Token};

/// The ignore list from `highlight.js` (`SKIP_ANCESTOR` plus link rules).
const SKIP_CLASSES: &[&str] = &["parNum", "pageNum", "fn", "rt", "rp"];
const SKIP_LINK_CLASSES: &[&str] = &["vl", "cl"];

/// One measured block of text inside a paragraph/verse, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPiece {
    /// UTF-16 offsets of this piece inside [`Block::text`].
    pub start: usize,
    pub end: usize,
    pub bold: bool,
    pub italic: bool,
    pub href: Option<String>,
}

/// A paragraph (`p[data-pid]`) or a Bible verse (`span.v`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Concatenated visible text, exactly as the extension tokenizes it.
    pub text: String,
    /// Tokens of [`Block::text`].
    pub tokens: Vec<Token>,
    pub pieces: Vec<TextPiece>,
    /// `data-pid` for paragraphs, the verse id for Bible verses.
    pub identifier: Option<String>,
    pub kind: BlockKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockKind {
    Title,
    Paragraph,
    Verse,
    Heading(u8),
    Caption,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub href: String,
    pub text: String,
    /// Index into [`Document::nodes`] of the block this link lives in, so a UI
    /// can scroll to it (`links()[i].node` → rendered line).
    pub node: usize,
}

/// A textarea (`.gen-field`) whose value comes from `InputField` rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputField {
    pub tag: Option<String>,
    pub label: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub src: String,
    pub alt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node_ {
    Block(Block),
    InputField(InputField),
    Image(Image),
    Rule,
}

/// Parsed `#article` (or `<body>` when no article container exists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub title: Option<String>,
    /// Canonical URL the HTML was fetched from (set by the caller).
    pub source_url: Option<String>,
    pub breadcrumb: Vec<String>,
    pub nodes: Vec<Node_>,
    /// `data-pid` (or verse id) → index into [`Document::nodes`].
    pub blocks: BTreeMap<String, usize>,
    pub links: Vec<Link>,
    /// Non-fatal parse issues (unknown markup is skipped, never fatal).
    pub warnings: Vec<String>,
}

impl Document {
    /// Every `p[data-pid]` block in document order.
    pub fn paragraphs(&self) -> impl Iterator<Item = (String, &Block)> {
        self.blocks.iter().filter_map(move |(pid, index)| {
            match self.nodes.get(*index) {
                Some(Node_::Block(block)) if block.kind != BlockKind::Verse => {
                    Some((pid.clone(), block))
                }
                _ => None,
            }
        })
    }

    /// Find a block by identifier (`data-pid` or verse id).
    pub fn block(&self, identifier: &str) -> Option<&Block> {
        self.blocks
            .get(identifier)
            .and_then(|index| match self.nodes.get(*index) {
                Some(Node_::Block(block)) => Some(block),
                _ => None,
            })
    }

    pub fn text_len(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| match node {
                Node_::Block(block) => block.text.len(),
                Node_::InputField(field) => field.value.len(),
                _ => 0,
            })
            .sum()
    }
}

#[derive(Debug, Clone, Default)]
struct Style {
    bold: bool,
    italic: bool,
    href: Option<String>,
}

/// Parser entry point. Never fails: malformed HTML is normalised by html5ever.
pub fn parse_document(html: &str) -> Document {
    let fragment = Html::parse_document(html);
    let mut warnings = Vec::new();

    let root = find_article(&fragment).unwrap_or_else(|| fragment.root_element());
    let mut collector = Collector {
        warnings: &mut warnings,
    };
    let nodes = collector.walk_children(root);
    let mut document = Document {
        title: None,
        source_url: None,
        breadcrumb: extract_breadcrumb(&fragment),
        nodes,
        blocks: BTreeMap::new(),
        links: Vec::new(),
        warnings,
    };
    finalize(&mut document);
    document
}

fn finalize(document: &mut Document) {
    let mut links = Vec::new();
    for (index, node) in document.nodes.iter().enumerate() {
        if let Node_::Block(block) = node {
            if let Some(identifier) = &block.identifier {
                match document.blocks.get(identifier) {
                    Some(_) => document
                        .warnings
                        .push(format!("duplicate block identifier {identifier}")),
                    None => {
                        document.blocks.insert(identifier.clone(), index);
                    }
                }
            }
            if block.kind == BlockKind::Title && document.title.is_none() {
                let trimmed = block.text.trim();
                if !trimmed.is_empty() {
                    document.title = Some(trimmed.to_string());
                }
            }
        }
    }
    // Links are collected after the fact so heading anchors and footnotes show up.
    for (index, node) in document.nodes.iter().enumerate() {
        if let Node_::Block(block) = node {
            for piece in &block.pieces {
                if let Some(href) = &piece.href {
                    links.push(Link {
                        href: href.clone(),
                        text: slice_utf16(&block.text, piece.start, piece.end),
                        node: index,
                    });
                }
            }
        }
    }
    document.links = links;
}

fn slice_utf16(text: &str, start: usize, end: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let clamped_end = end.min(units.len());
    let clamped_start = start.min(clamped_end);
    String::from_utf16_lossy(&units[clamped_start..clamped_end])
}

fn find_article(fragment: &Html) -> Option<ElementRef<'_>> {
    for selector in ["#article", "article", "main"] {
        if let Ok(selector) = Selector::parse(selector) {
            if let Some(element) = fragment.select(&selector).next() {
                return Some(element);
            }
        }
    }
    None
}

fn extract_breadcrumb(fragment: &Html) -> Vec<String> {
    let Ok(selector) = Selector::parse("#publicationNavigation .pub, #publicationNavigation li, .breadcrumb a, nav .pub")
    else {
        return Vec::new();
    };
    fragment
        .select(&selector)
        .map(|element| element.text().collect::<String>().trim().to_string())
        .filter(|text| !text.is_empty())
        .take(6)
        .collect()
}

struct Collector<'a> {
    warnings: &'a mut Vec<String>,
}

impl<'a> Collector<'a> {
    fn walk_children(&mut self, root: ElementRef<'_>) -> Vec<Node_> {
        let mut nodes = Vec::new();
        self.walk(root, &mut nodes);
        nodes
    }

    fn walk(&mut self, element: ElementRef<'_>, out: &mut Vec<Node_>) {
        for child in element.children() {
            let Some(child_element) = ElementRef::wrap(child) else {
                continue;
            };
            self.visit(child_element, out);
        }
    }

    fn visit(&mut self, element: ElementRef<'_>, out: &mut Vec<Node_>) {
        let name = element.value().name();
        match name {
            "script" | "style" | "noscript" | "rt" | "rp" => return,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse::<u8>().unwrap_or(1);
                if let Some(block) = self.collect_block(element, BlockKind::Heading(level)) {
                    out.push(Node_::Block(block));
                }
                return;
            }
            "p" => {
                if let Some(block) = self.collect_paragraph(element) {
                    out.push(Node_::Block(block));
                }
                return;
            }
            "span" if element.value().attr("class").is_some_and(|value| {
                value.split_whitespace().any(|class| class == "v")
            }) =>
            {
                if let Some(block) = self.collect_block(element, BlockKind::Verse) {
                    out.push(Node_::Block(block));
                }
                return;
            }
            "textarea" => {
                out.push(Node_::InputField(self.collect_input_field(element)));
                return;
            }
            "img" => {
                let src = element.value().attr("src").unwrap_or_default().to_string();
                if !src.is_empty() {
                    out.push(Node_::Image(Image {
                        src,
                        alt: element.value().attr("alt").unwrap_or_default().to_string(),
                    }));
                }
                return;
            }
            "hr" => {
                out.push(Node_::Rule);
                return;
            }
            "figcaption" => {
                if let Some(block) = self.collect_block(element, BlockKind::Caption) {
                    out.push(Node_::Block(block));
                }
                return;
            }
            "blockquote" | "table" | "li" | "dt" | "dd" | "section" | "div" | "aside"
            | "figure" | "article" | "main" | "details" | "summary" | "header" | "footer"
            | "nav" | "form" | "fieldset" | "label" | "td" | "th" | "tr" | "thead" | "tbody"
            | "ol" | "ul" | "dl" | "center" | "body" | "html" => {
                self.walk(element, out);
            }
            _ => {
                // Inline-ish wrappers (`strong`, `em`, `a`, `sup`, …) that appear at
                // block level still carry readable text, so keep it.
                if element.children().any(|child| child.value().is_text()) {
                    if let Some(block) = self.collect_block(element, BlockKind::Paragraph) {
                        if !block.text.trim().is_empty() {
                            out.push(Node_::Block(block));
                        }
                    }
                } else {
                    self.walk(element, out);
                }
            }
        }
    }

    fn collect_paragraph(&mut self, element: ElementRef<'_>) -> Option<Block> {
        let pid = element.value().attr("data-pid").map(str::to_string);
        let class = element.value().attr("class").unwrap_or_default();
        if class.split_whitespace().any(|c| c == "parNum" || c == "pageNum" || c == "fn") {
            return None;
        }
        let kind = if pid.as_deref() == Some("1") {
            BlockKind::Title
        } else {
            BlockKind::Paragraph
        };
        let block = self.collect_block_with_id(element, kind, pid)?;
        if block.text.trim().is_empty() {
            return None;
        }
        Some(block)
    }

    fn collect_block(&mut self, element: ElementRef<'_>, kind: BlockKind) -> Option<Block> {
        let identifier = match kind {
            BlockKind::Verse => element.value().attr("id").map(str::to_string),
            _ => element.value().attr("data-pid").map(str::to_string),
        };
        self.collect_block_with_id(element, kind, identifier)
    }

    fn collect_block_with_id(
        &mut self,
        element: ElementRef<'_>,
        kind: BlockKind,
        identifier: Option<String>,
    ) -> Option<Block> {
        let mut sink = TextSink::default();
        // Seed the style from the block element itself so a block that *is* a
        // link (`<li><a href>…</a></li>` on WOL navigation pages) keeps its
        // `href`; otherwise nav pages render as unlinked text.
        let base = style_for(&element, &Style::default());
        collect_text(element, &base, &mut sink, self.warnings);
        sink.trim_end();

        if sink.text.is_empty() && sink.pieces.is_empty() {
            return None;
        }
        let tokens = tokenize::tokenize(&sink.text);
        Some(Block {
            text: sink.text,
            tokens,
            pieces: sink.pieces,
            identifier,
            kind,
        })
    }

    fn collect_input_field(&self, element: ElementRef<'_>) -> InputField {
        let tag = element
            .value()
            .attr("data-pid")
            .or_else(|| element.value().attr("data-rel-pid"))
            .or_else(|| element.value().attr("name"))
            .map(str::to_string);
        let own_text = element.text().collect::<String>();
        let value = if own_text.trim().is_empty() {
            element.value().attr("value").unwrap_or_default().to_string()
        } else {
            own_text
        };
        InputField {
            tag,
            label: element.value().attr("aria-label").map(str::to_string),
            value: value.trim().to_string(),
        }
    }
}

#[derive(Default)]
struct TextSink {
    text: String,
    pieces: Vec<TextPiece>,
}

impl TextSink {
    /// Drop trailing whitespace (WOL cards end with indentation before `</a>`).
    /// Tokens are unaffected — whitespace never forms one.
    fn trim_end(&mut self) {
        let kept_bytes = self.text.trim_end().len();
        if kept_bytes == self.text.len() {
            return;
        }
        let kept_units = tokenize::utf16_len_of(&self.text[..kept_bytes]);
        self.text.truncate(kept_bytes);
        for piece in &mut self.pieces {
            piece.end = piece.end.min(kept_units);
        }
        self.pieces.retain(|piece| piece.start < piece.end);
    }

    /// Append one text node.
    ///
    /// Whitespace is collapsed to single spaces (and the block never starts
    /// with one). WOL navigation cards are pretty-printed with newlines and
    /// deep indentation inside the anchor, which would otherwise wrap into
    /// blank lines. This cannot shift token coordinates: the tokenizer skips
    /// whitespace entirely, so token indices are identical either way.
    fn push(&mut self, value: &str, style: &Style) {
        if value.is_empty() {
            return;
        }
        let mut collapsed = String::with_capacity(value.len());
        let mut previous_space = false;
        for character in value.chars() {
            if character.is_whitespace() {
                if !previous_space {
                    collapsed.push(' ');
                }
                previous_space = true;
            } else {
                collapsed.push(character);
                previous_space = false;
            }
        }
        if self.text.is_empty() {
            // No leading separator at the start of a block.
            let trimmed = collapsed.trim_start();
            if trimmed.is_empty() {
                return;
            }
            collapsed = trimmed.to_string();
        } else if collapsed.starts_with(' ') && self.text.ends_with(' ') {
            collapsed.remove(0);
        }
        if collapsed.is_empty() {
            return;
        }
        let start = tokenize::utf16_len_of(&self.text);
        self.text.push_str(&collapsed);
        let end = tokenize::utf16_len_of(&self.text);
        self.pieces.push(TextPiece {
            start,
            end,
            bold: style.bold,
            italic: style.italic,
            href: style.href.clone(),
        });
    }
}

/// Classes split on whitespace; the borrow lives as long as the document.
fn class_list<'a>(element: &ElementRef<'a>) -> Vec<&'a str> {
    element
        .value()
        .attr("class")
        .map(|value| value.split_whitespace().collect())
        .unwrap_or_default()
}

fn should_skip(element: &ElementRef<'_>) -> bool {
    let classes = class_list(element);
    if classes.iter().any(|class| SKIP_CLASSES.contains(class)) {
        return true;
    }
    let name = element.value().name();
    if name == "a" {
        if classes.iter().any(|class| SKIP_LINK_CLASSES.contains(class)) {
            return true;
        }
        let is_plus_reference = classes.contains(&"b")
            && element.text().collect::<String>().trim() == "+";
        if is_plus_reference {
            return true;
        }
    }
    false
}

fn style_for(element: &ElementRef<'_>, parent: &Style) -> Style {
    let name = element.value().name();
    let classes = class_list(element);
    let mut style = parent.clone();
    if matches!(name, "b" | "strong") {
        style.bold = true;
    }
    if matches!(name, "i" | "em") {
        style.italic = true;
    }
    if name == "a" {
        if let Some(href) = element.value().attr("href") {
            style.href = Some(href.to_string());
        }
        if classes.contains(&"b") {
            style.bold = true;
        }
    }
    style
}

fn collect_text(
    element: ElementRef<'_>,
    style: &Style,
    sink: &mut TextSink,
    warnings: &mut Vec<String>,
) {
    for child in element.children() {
        match child.value() {
            Node::Text(text) => sink.push(&text.text, style),
            Node::Element(inner) => {
                let is_br = inner.name() == "br";
                if is_br {
                    sink.push(" ", style);
                    continue;
                }
                let Some(child_element) = ElementRef::wrap(child) else {
                    continue;
                };
                if should_skip(&child_element) {
                    continue;
                }
                let child_style = style_for(&child_element, style);
                let name = child_element.value().name();
                let is_block_level = matches!(
                    name,
                    "div" | "p" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                );
                if is_block_level && !sink.text.is_empty() && !sink.text.ends_with(' ') {
                    sink.push(" ", style);
                }
                collect_text(child_element, &child_style, sink, warnings);
            }
            _ => {}
        }
    }
    if element.value().name() == "a" && element.value().attr("title").is_none() {
        // Links without a title are still links; nothing to warn about.
    }
    let _ = warnings;
}

/// One entry of WOL's top menu bar (`#menuBar`): Bible, Publications, Meetings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    /// The element id, e.g. `menuBible`.
    pub id: String,
    /// Visible label, already in the page's language (`BIBLIA`, `PUBLICACIONES`, …).
    pub label: String,
    /// Site-relative href, e.g. `/es/wol/binav/r4/lp-s`.
    pub href: String,
}

/// The site-wide search box (`form.searchForm`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchBox {
    /// Form action, e.g. `/es/wol/qt/r4/lp-s`.
    pub action: String,
    /// Localised placeholder, e.g. `Introducir búsqueda`.
    pub placeholder: String,
}

/// Navigation chrome shared by every WOL page.
///
/// Deserialised straight out of the server-rendered HTML, so a CLI dashboard can
/// show the same toolbar the web UI does, in the same language, without hard-coded
/// per-language URL rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chrome {
    pub menu: Vec<MenuEntry>,
    pub search: Option<SearchBox>,
    /// Language switcher (`#siteBanner .language a`).
    pub language: Option<MenuEntry>,
    /// Preferences/gear entry (`#menuToolsPreferences a`).
    pub preferences: Option<MenuEntry>,
    /// Logo link back to the home page.
    pub home: Option<String>,
    /// Site title (`BIBLIOTECA EN LÍNEA Watchtower`).
    pub title: Option<String>,
}

/// The “Watchtower ONLINE LIBRARY” welcome panel of a home page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Welcome {
    pub brand: Option<String>,
    pub title: Option<String>,
    pub greeting: Option<String>,
    pub about: Option<String>,
    pub notice: Option<String>,
    pub announcement: Option<String>,
    pub message: Option<String>,
}

impl Welcome {
    pub fn is_empty(&self) -> bool {
        self == &Welcome::default()
    }
}

/// One day of the daily-text widget (`#dailyText .tabContent`).
///
/// WOL renders three days into every home page (yesterday, today, tomorrow);
/// [`parse_daily_text`] picks the one matching `today` so a reader does not have
/// to scroll past the other two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyText {
    /// `data-date` exactly as WOL wrote it (`2026-11-16T00:00:00.000Z`).
    pub date: Option<String>,
    pub document: Document,
    pub previous: Option<String>,
    pub today: Option<String>,
    pub next: Option<String>,
    /// Accessible labels (“día anterior”, “Hoy”, “día siguiente”).
    pub label_previous: Option<String>,
    pub label_today: Option<String>,
    pub label_next: Option<String>,
}

/// Parse the toolbar / search / language chrome out of any WOL page.
pub fn parse_chrome(html: &str) -> Chrome {
    let fragment = Html::parse_document(html);
    let mut chrome = Chrome {
        home: href_of(&fragment, "#siteBanner .title a[href]"),
        title: text_of(&fragment, "#siteBanner .titleBig"),
        language: menu_entry(&fragment, ".language a[href]", "language"),
        preferences: menu_entry(&fragment, "#menuToolsPreferences", "preferences"),
        search: search_box(&fragment),
        menu: menu_entries(&fragment),
    };
    // A page whose menu only has the hidden search arrows has no toolbar.
    chrome.menu.retain(|entry| !entry.href.trim().is_empty() && entry.href != "#");
    chrome
}

/// Parse the welcome panel of a home page (localised text included).
pub fn parse_welcome(html: &str) -> Option<Welcome> {
    let fragment = Html::parse_document(html);
    let welcome = Welcome {
        brand: text_of(&fragment, "#welcome .siteTitleLine1"),
        title: text_of(&fragment, "#welcome .siteTitleLine2"),
        greeting: text_of(&fragment, "#welcome .siteGreeting"),
        about: text_of(&fragment, "#welcome .siteAbout"),
        notice: text_of(&fragment, "#welcome .siteParentLink"),
        announcement: text_of(&fragment, "#welcome .announcementTitle"),
        message: text_of(&fragment, "#welcome .announcementMessage"),
    };
    (!welcome.is_empty()).then_some(welcome)
}

/// Pick one day out of the daily-text widget.
///
/// `today` is a `YYYY-MM-DD` prefix in the reader's local time; when it matches
/// no section (stale cached page, timezone edge) the middle section is used —
/// WOL always renders yesterday, today, tomorrow in that order.
pub fn parse_daily_text(html: &str, today: Option<&str>) -> Option<DailyText> {
    let fragment = Html::parse_document(html);
    let selector = Selector::parse("#dailyText .tabContent[data-date]").ok()?;
    let sections: Vec<ElementRef<'_>> = fragment.select(&selector).collect();
    if sections.is_empty() {
        return None;
    }
    let prefix = today.map(str::trim).filter(|value| !value.is_empty());
    let index = prefix
        .and_then(|prefix| {
            sections.iter().position(|section| {
                section
                    .value()
                    .attr("data-date")
                    .is_some_and(|date| date.starts_with(prefix))
            })
        })
        .unwrap_or(if sections.len() >= 3 { 1 } else { 0 });
    let section = sections.get(index)?;
    let date = section.value().attr("data-date").map(str::to_string);

    let mut document = parse_document(&section.inner_html());
    // The daily text always links to its own publication (`/wol/d/…`); using it
    // as the source URL gives `guess_document_id` the right `documentId`, so
    // highlights and notes land in the same `wolMarks` bucket as the extension.
    document.source_url = document
        .links
        .iter()
        .find(|link| link.href.contains("/wol/d/"))
        .map(|link| {
            link.href
                .split('#')
                .next()
                .unwrap_or(&link.href)
                .trim_end_matches('?')
                .to_string()
        });

    let (previous, label_previous) = nav_entry(&fragment, "#footerPrevDay a");
    let (today, label_today) = nav_entry(&fragment, "#navigationDailyTextToday a");
    let (next, label_next) = nav_entry(&fragment, "#footerNextDay a");

    Some(DailyText {
        date,
        document,
        previous,
        today,
        next,
        label_previous,
        label_today,
        label_next,
    })
}

fn menu_entries(fragment: &Html) -> Vec<MenuEntry> {
    let Ok(items) = Selector::parse("#menuBar li[id]") else {
        return Vec::new();
    };
    let Ok(anchors) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    let Ok(labels) = Selector::parse(".label") else {
        return Vec::new();
    };
    fragment
        .select(&items)
        .filter_map(|item| {
            let anchor = item.select(&anchors).next()?;
            let href = anchor.value().attr("href")?.to_string();
            let id = item.value().attr("id").unwrap_or_default().to_string();
            let label = anchor
                .select(&labels)
                .next()
                .map(|label| collapsed_text(label))
                .filter(|label| !label.is_empty())
                .or_else(|| {
                    anchor
                        .value()
                        .attr("aria-label")
                        .map(str::to_string)
                })
                .or_else(|| item.value().attr("aria-label").map(str::to_string))
                .unwrap_or_else(|| id.clone());
            Some(MenuEntry { id, label, href })
        })
        .collect()
}

/// Menu entry from an element that either is the anchor or contains one.
fn menu_entry(fragment: &Html, selector: &str, id: &str) -> Option<MenuEntry> {
    let element = first(fragment, selector)?;
    let anchor = if element.value().name() == "a" {
        element
    } else {
        element.select(&Selector::parse("a[href]").ok()?).next()?
    };
    let href = anchor.value().attr("href").map(str::to_string)?;
    let label = text_of_element(element, ".label")
        .or_else(|| {
            anchor
                .value()
                .attr("aria-label")
                .or_else(|| element.value().attr("aria-label"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| id.to_string());
    Some(MenuEntry {
        id: id.to_string(),
        label,
        href,
    })
}

fn search_box(fragment: &Html) -> Option<SearchBox> {
    let form = first(fragment, "form.searchForm")?;
    let action = form.value().attr("action").unwrap_or_default().to_string();
    if action.is_empty() {
        return None;
    }
    let placeholder = first(fragment, "form.searchForm input.searchField")
        .and_then(|field| field.value().attr("placeholder"))
        .unwrap_or("Search")
        .to_string();
    Some(SearchBox { action, placeholder })
}

fn nav_entry(fragment: &Html, selector: &str) -> (Option<String>, Option<String>) {
    let Some(anchor) = first(fragment, selector) else {
        return (None, None);
    };
    let href = anchor.value().attr("href").map(str::to_string);
    let label = text_of_element(anchor, ".navContent")
        .or_else(|| anchor.value().attr("aria-label").map(str::to_string));
    (href, label)
}

fn first<'a>(fragment: &'a Html, selector: &str) -> Option<ElementRef<'a>> {
    let selector = Selector::parse(selector).ok()?;
    fragment.select(&selector).next()
}

fn href_of(fragment: &Html, selector: &str) -> Option<String> {
    first(fragment, selector)
        .and_then(|element| element.value().attr("href"))
        .map(str::to_string)
}

fn text_of(fragment: &Html, selector: &str) -> Option<String> {
    let element = first(fragment, selector)?;
    let text = collapsed_text(element);
    (!text.is_empty()).then_some(text)
}

fn text_of_element(element: ElementRef<'_>, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    let text = collapsed_text(element.select(&selector).next()?);
    (!text.is_empty()).then_some(text)
}

/// Text of one element with runs of whitespace collapsed to single spaces.
fn collapsed_text(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// One selectable entry of a WOL directory page (`ul.directory li a[href]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    /// Card title (`Biblias`, `Génesis`, `16-22 de noviembre`).
    pub label: String,
    /// Card subtitle/year (`Guía de actividades 2026 | noviembre`, `2019`).
    pub detail: Option<String>,
    /// Site-relative href.
    pub href: String,
}

/// A group of directory entries under one heading (`Vida y Ministerio`, …).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavSection {
    pub title: Option<String>,
    pub items: Vec<NavItem>,
}

/// A WOL directory page (Bible versions, library, meetings, publication years).
///
/// These pages are pure navigation: headings plus grids of link cards. The CLI
/// renders them as a two-pane browser (sub-categories on the left, their cards on
/// the right) instead of a long list of links.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavPage {
    /// Page heading (`Biblias en español`, `Reuniones`).
    pub title: Option<String>,
    pub sections: Vec<NavSection>,
}

impl NavPage {
    pub fn item_count(&self) -> usize {
        self.sections.iter().map(|section| section.items.len()).sum()
    }
}

/// Parse a directory page's navigation tree, or `None` when the page is not one.
///
/// Only pages with a `#materialNav` / `.materialNav` container qualify (articles,
/// Bible chapters and search results have none), so the reader is never replaced
/// by a browser on a real document.
pub fn parse_nav(html: &str) -> Option<NavPage> {
    let fragment = Html::parse_document(html);
    let article = find_article(&fragment).unwrap_or_else(|| fragment.root_element());
    let selector = Selector::parse("#materialNav, .materialNav").ok()?;
    let containers: Vec<ElementRef<'_>> = article.select(&selector).collect();
    if containers.is_empty() {
        return None;
    }
    let mut sections: Vec<NavSection> = Vec::new();
    for container in containers {
        // Nested containers are reached by the outer walk; starting at both would
        // duplicate every card into a second section.
        let nested = container.ancestors().any(|ancestor| match ancestor.value() {
            Node::Element(element) => {
                element.name() == "materialNav"
                    || element
                        .attr("class")
                        .is_some_and(|class| class.split_whitespace().any(|c| c == "materialNav"))
            }
            _ => false,
        });
        if nested {
            continue;
        }
        let mut current = None;
        walk_nav(container, &mut sections, &mut current);
    }
    if sections.first().is_some_and(|section| section.title.is_none()) {
        let title = first(&fragment, "#article h1, article h1, #article h2, article h2")
            .map(collapsed_text)
            .filter(|title| !title.is_empty());
        if let Some(section) = sections.first_mut() {
            section.title = title;
        }
    }
    // The same card can appear in two nav containers (thumbnail + title links).
    let mut seen = std::collections::BTreeSet::new();
    for section in &mut sections {
        section.items.retain(|item| seen.insert(item.href.clone()));
    }
    sections.retain(|section| !section.items.is_empty());
    if sections.is_empty() {
        return None;
    }
    let title = first(&fragment, "#article h1, article h1")
        .map(collapsed_text)
        .filter(|title| !title.is_empty());
    Some(NavPage { title, sections })
}

fn walk_nav(element: ElementRef<'_>, sections: &mut Vec<NavSection>, current: &mut Option<String>) {
    for child in element.children() {
        let Some(child) = ElementRef::wrap(child) else {
            continue;
        };
        match child.value().name() {
            "h1" | "h2" | "h3" | "h4" => {
                let title = collapsed_text(child);
                if !title.is_empty() {
                    *current = Some(title);
                }
            }
            "ul" | "ol" => {
                let items = nav_items(child);
                if items.is_empty() {
                    continue;
                }
                push_nav_section(sections, current.clone(), items);
            }
            _ => walk_nav(child, sections, current),
        }
    }
}

fn push_nav_section(sections: &mut Vec<NavSection>, title: Option<String>, items: Vec<NavItem>) {
    if let Some(last) = sections.last_mut() {
        // Consecutive lists under one heading belong to the same sub-category.
        if last.title == title {
            last.items.extend(items);
            return;
        }
    }
    sections.push(NavSection { title, items });
}

fn nav_items(list: ElementRef<'_>) -> Vec<NavItem> {
    let Ok(anchors) = Selector::parse("a[href]") else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for child in list.children() {
        let Some(li) = ElementRef::wrap(child).filter(|li| li.value().name() == "li") else {
            continue;
        };
        let Some(anchor) = li.select(&anchors).next() else {
            continue;
        };
        let href = anchor.value().attr("href").unwrap_or_default().trim();
        if href.is_empty() || href.starts_with('#') || href.starts_with("javascript:") {
            continue;
        }
        let label = text_of_element(li, ".cardLine1")
            .or_else(|| text_of_element(li, ".title"))
            .or_else(|| {
                let text = collapsed_text(anchor);
                (!text.is_empty()).then_some(text)
            })
            .or_else(|| {
                let images = Selector::parse("img[alt]").ok()?;
                li.select(&images)
                    .next()
                    .and_then(|image| image.value().attr("alt"))
                    .map(str::to_string)
            });
        let Some(label) = label.filter(|label| !label.is_empty()) else {
            continue;
        };
        let detail = text_of_element(li, ".cardLine2")
            .or_else(|| text_of_element(li, ".cardTitleDetail"))
            .filter(|detail| !detail.is_empty());
        items.push(NavItem {
            label,
            detail,
            href: href.to_string(),
        });
    }
    items
}

/// Convenience: text of one block with a `data-pid`, ready to tokenize.
pub fn block_text(html: &str, pid: &str) -> Option<Block> {
    parse_document(html)
        .block(pid)
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r##"
    <html><body>
      <div id="publicationNavigation">
        <span class="pub">Watchtower Study</span>
      </div>
      <div id="article">
        <p data-pid="1">Paragraph <strong>one</strong> title</p>
        <h2>Subheading</h2>
        <p data-pid="2"><span class="parNum">2</span>God is <em>love</em><a class="b" href="#fn1">+</a>.</p>
        <p data-pid="3">See <a class="cl" href="#x">cross-ref</a> and <a href="/wol/d/42">link</a>.<sup class="fn">footnote</sup></p>
        <blockquote><p>The Lord is my shepherd.</p></blockquote>
        <div class="gen-field"><textarea data-pid="2">my answer</textarea></div>
        <img src="wol://es/img/figure.jpg" alt="Figure 1">
      </div>
    </body></html>
    "##;

    #[test]
    fn collects_paragraph_text_with_ignore_rules() {
        let document = parse_document(ARTICLE);
        let first = document.block("1").expect("title paragraph");
        assert_eq!(first.kind, BlockKind::Title);
        assert_eq!(first.text, "Paragraph one title");
        assert_eq!(document.title.as_deref(), Some("Paragraph one title"));

        let second = document.block("2").expect("paragraph 2");
        // .parNum skipped, the "+" reference skipped, punctuation kept.
        assert_eq!(second.text, "God is love.");
        assert_eq!(second.tokens.len(), tokenize::tokenize("God is love.").len());

        let third = document.block("3").expect("paragraph 3");
        // a.cl and .fn skipped, normal links kept, whitespace collapsed.
        assert_eq!(third.text, "See and link.");
    }

    #[test]
    fn keeps_style_pieces_for_bold_italic_and_links() {
        let document = parse_document(ARTICLE);
        let first = document.block("1").unwrap();
        let bold = first
            .pieces
            .iter()
            .find(|piece| piece.bold)
            .expect("bold piece");
        assert_eq!(slice_utf16(&first.text, bold.start, bold.end), "one");

        let second = document.block("2").unwrap();
        assert!(second.pieces.iter().any(|piece| piece.italic));
        assert!(document
            .links
            .iter()
            .any(|link| link.href == "/wol/d/42" && link.text == "link"));
    }

    #[test]
    fn extracts_input_fields_and_images() {
        let document = parse_document(ARTICLE);
        let field = document
            .nodes
            .iter()
            .find_map(|node| match node {
                Node_::InputField(field) => Some(field),
                _ => None,
            })
            .expect("textarea");
        assert_eq!(field.tag.as_deref(), Some("2"));
        assert_eq!(field.value, "my answer");

        assert!(document.nodes.iter().any(|node| matches!(
            node,
            Node_::Image(image) if image.src.ends_with("figure.jpg") && image.alt == "Figure 1"
        )));
    }

    #[test]
    fn verse_spans_use_their_id_as_identifier() {
        let html = r#"<div id="article"><div class="verse">
            <span class="v" id="v-2-5-9-1">He said: <b>“Go!”</b></span>
            <span class="v" id="v-2-5-9-2">Then they went.</span>
        </div></div>"#;
        let document = parse_document(html);
        let first = document.block("v-2-5-9-1").expect("verse 9 part 1");
        assert_eq!(first.kind, BlockKind::Verse);
        assert_eq!(first.text, "He said: “Go!”");
        assert!(document.block("v-2-5-9-2").is_some());
    }

    #[test]
    fn breadcrumb_and_headings_are_preserved() {
        let document = parse_document(ARTICLE);
        assert_eq!(document.breadcrumb, vec!["Watchtower Study".to_string()]);
        assert!(document.nodes.iter().any(|node| matches!(
            node,
            Node_::Block(block) if block.kind == BlockKind::Heading(2) && block.text == "Subheading"
        )));
    }

    #[test]
    fn empty_and_malformed_html_do_not_panic() {
        assert!(parse_document("").nodes.is_empty());
        let broken = r#"<div id="article"><p data-pid="1">unclosed <b>bold"#;
        let document = parse_document(broken);
        assert_eq!(document.block("1").map(|block| block.text.as_str()), Some("unclosed bold"));
    }

    const HOME: &str = r##"
    <html><body>
      <div id="siteBanner">
        <div class="title"><a href="/es/wol/h/r4/lp-s">
          <span class="titleBig">BIBLIOTECA EN LÍNEA Watchtower</span>
        </a></div>
        <div class="language"><a href="/es/wol/li/r4/lp-s"><span class="label">español</span></a></div>
      </div>
      <form class="searchForm" role="search" method="GET" action="/es/wol/qt/r4/lp-s">
        <input class="searchField" name="q" placeholder="Introducir búsqueda" />
      </form>
      <ul id="menuBar" class="menu" role="menubar">
        <li id="menuBible" aria-label="Biblia"><a href="/es/wol/binav/r4/lp-s" role="menuitem">
          <span class="label">BIBLIA</span></a></li>
        <li id="menuPublications" aria-label="publicaciones"><a href="/es/wol/library/r4/lp-s/biblioteca">
          <span class="label">PUBLICACIONES</span></a></li>
        <li id="menuToday" aria-label="reuniones"><a href="/es/wol/meetings/r4/lp-s">
          <span class="label">REUNIONES</span></a></li>
        <li id="menuSearchHitNext" class="hidden"><a href="#"><span class="icon"></span></a></li>
      </ul>
      <div id="toolbarMenu"><ul class="menu">
        <li id="menuToolsPreferences" class="menuButton iconOnly menuRight" aria-label="configuración">
          <a href="/es/wol/pref/r4/lp-s?url=%2Fes%2Fwol%2Fh" aria-label="configuración"><span class="icon"></span></a>
        </li>
      </ul></div>
      <article id="article">
        <div id="welcome">
          <div class="siteTitle">BIBLIOTECA EN LÍNEA Watchtower</div>
          <div class="siteTitleLine1 light">Watchtower</div>
          <div class="siteTitleLine2 dark">BIBLIOTECA EN LÍNEA</div>
          <div class="siteGreeting">¡Bienvenido!</div>
          <div class="siteAbout">En esta página podrá consultar publicaciones.</div>
          <div class="siteParentLink">Para descargar, visite <a href="https://www.jw.org">jw.org</a>.</div>
          <div class="announcementTitle">Anuncio</div>
          <div class="announcementMessage">Nuevos idiomas: Ba, Masana.</div>
        </div>
        <div id="dailyText">
          <div id="todayNav"><ul>
            <li id="footerPrevDay"><a href="/es/wol/h/r4/lp-s/2026/11/15" aria-label="día anterior"><span class="icon navContent"></span></a></li>
            <li id="navigationDailyTextToday" class="todayNav"><a href="/es/wol/h/r4/lp-s"><span class="navContent">Hoy</span></a></li>
            <li id="footerNextDay"><a href="/es/wol/h/r4/lp-s/2026/11/17" aria-label="día siguiente"><span class="icon navContent"></span></a></li>
          </ul></div>
          <div class="articlePositioner">
            <div class="tabContent" data-date="2026-11-15T00:00:00.000Z">
              <h2 data-pid="44">Domingo 15 de noviembre</h2>
              <p data-pid="45">Ayer.</p>
              <a href="/es/wol/d/r4/lp-s/1102026210?#h=44:0-47:0">Examinemos las Escrituras</a>
            </div>
            <div class="tabContent" data-date="2026-11-16T00:00:00.000Z">
              <h2 data-pid="47">Lunes 16 de noviembre</h2>
              <p data-pid="48">Hoy.</p>
            </div>
            <div class="tabContent" data-date="2026-11-17T00:00:00.000Z">
              <h2 data-pid="50">Martes 17 de noviembre</h2>
              <p data-pid="51">Mañana.</p>
            </div>
          </div>
        </div>
      </article>
    </body></html>
    "##;

    #[test]
    fn parses_the_toolbar_search_and_language_chrome() {
        let chrome = parse_chrome(HOME);
        let labels: Vec<&str> = chrome.menu.iter().map(|entry| entry.label.as_str()).collect();
        assert_eq!(labels, vec!["BIBLIA", "PUBLICACIONES", "REUNIONES"]);
        assert_eq!(chrome.menu[0].href, "/es/wol/binav/r4/lp-s");
        assert_eq!(chrome.menu[2].id, "menuToday");

        let search = chrome.search.expect("search box");
        assert_eq!(search.action, "/es/wol/qt/r4/lp-s");
        assert_eq!(search.placeholder, "Introducir búsqueda");

        assert_eq!(chrome.language.expect("language").label, "español");
        assert_eq!(chrome.preferences.expect("preferences").label, "configuración");
        assert_eq!(chrome.home.as_deref(), Some("/es/wol/h/r4/lp-s"));
        assert_eq!(chrome.title.as_deref(), Some("BIBLIOTECA EN LÍNEA Watchtower"));
    }

    #[test]
    fn pages_without_a_menu_bar_have_empty_chrome() {
        let chrome = parse_chrome(ARTICLE);
        assert!(chrome.menu.is_empty());
        assert!(chrome.search.is_none());
    }

    #[test]
    fn parses_the_localised_welcome_panel() {
        let welcome = parse_welcome(HOME).expect("welcome panel");
        assert_eq!(welcome.brand.as_deref(), Some("Watchtower"));
        assert_eq!(welcome.title.as_deref(), Some("BIBLIOTECA EN LÍNEA"));
        assert_eq!(welcome.greeting.as_deref(), Some("¡Bienvenido!"));
        assert_eq!(welcome.notice.as_deref(), Some("Para descargar, visite jw.org."));
        assert_eq!(welcome.announcement.as_deref(), Some("Anuncio"));
        assert!(parse_welcome(ARTICLE).is_none());
    }

    #[test]
    fn picks_the_daily_text_day_matching_the_reader_date() {
        let daily = parse_daily_text(HOME, Some("2026-11-16")).expect("daily text");
        assert_eq!(daily.date.as_deref(), Some("2026-11-16T00:00:00.000Z"));
        assert_eq!(daily.document.block("48").map(|block| block.text.as_str()), Some("Hoy."));
        assert_eq!(daily.label_today.as_deref(), Some("Hoy"));
        assert_eq!(daily.label_next.as_deref(), Some("día siguiente"));
        assert_eq!(daily.next.as_deref(), Some("/es/wol/h/r4/lp-s/2026/11/17"));
        // The ``/wol/d/…`` link of the neighbouring day is not inherited.
        assert_eq!(daily.document.source_url, None);

        let other = parse_daily_text(HOME, Some("2026-11-15")).expect("daily text");
        assert_eq!(other.document.block("45").map(|block| block.text.as_str()), Some("Ayer."));
        // That section does link to the daily-text publication: keep it as the id source.
        assert_eq!(
            other.document.source_url.as_deref(),
            Some("/es/wol/d/r4/lp-s/1102026210")
        );
    }

    #[test]
    fn daily_text_falls_back_to_the_middle_day() {
        let daily = parse_daily_text(HOME, None).expect("daily text");
        assert_eq!(daily.document.block("48").map(|block| block.text.as_str()), Some("Hoy."));
        let stale = parse_daily_text(HOME, Some("1999-01-01")).expect("daily text");
        assert_eq!(stale.document.block("48").map(|block| block.text.as_str()), Some("Hoy."));
        assert!(parse_daily_text(ARTICLE, None).is_none());
    }

    const MEETINGS: &str = r##"
    <html><body><article id="article">
      <div id="materialNav"><nav>
        <h2>Vida y Ministerio</h2>
        <ul class="directory navCard">
          <li><a href="/es/wol/d/r4/lp-s/202026403">
            <div class="cardTitleBlock">
              <div class="cardLine1">16-22 de noviembre</div>
              <div class="cardLine2">Guía de actividades 2026 | noviembre</div>
            </div>
          </a></li>
        </ul>
        <h2>Estudio de La Atalaya</h2>
        <ul class="directory navCard">
          <li><a href="/es/wol/d/r4/lp-s/2026562">
            <div class="cardLine1">Predicamos las buenas noticias</div>
            <div class="cardLine2">La Atalaya (estudio) 2026 | septiembre</div>
          </a></li>
        </ul>
        <h2>Otras publicaciones</h2>
        <ul class="directory navCard">
          <li><a href="/es/wol/d/r4/lp-s/1"><div class="cardLine1">Guía de actividades</div></a></li>
          <li><a href="/es/wol/d/r4/lp-s/2"><div class="cardLine1">Cantemos con gozo</div></a></li>
        </ul>
      </nav></div>
    </article></body></html>
    "##;

    const LIBRARY: &str = r##"
    <html><body><article id="article">
      <h1>Biblioteca</h1>
      <div id="materialNav" role="navigation"><nav>
        <ul class="directory navCard">
          <li><a href="/es/wol/library/r4/lp-s/biblioteca/biblias">
            <div class="cardLine1">Biblias</div></a></li>
          <li><a href="/es/wol/library/r4/lp-s/biblioteca/glosario">
            <div class="cardLine1">Glosario</div></a></li>
          <li><a href="#"><div class="cardLine1">ignored anchor</div></a></li>
        </ul>
      </nav></div>
    </article></body></html>
    "##;

    #[test]
    fn parses_directory_pages_into_title_sections() {
        let nav = parse_nav(MEETINGS).expect("meetings page is a directory");
        let titles: Vec<&str> = nav
            .sections
            .iter()
            .map(|section| section.title.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(
            titles,
            vec!["Vida y Ministerio", "Estudio de La Atalaya", "Otras publicaciones"]
        );
        assert_eq!(nav.item_count(), 4);
        let first = &nav.sections[0].items[0];
        assert_eq!(first.label, "16-22 de noviembre");
        assert_eq!(
            first.detail.as_deref(),
            Some("Guía de actividades 2026 | noviembre")
        );
        assert_eq!(first.href, "/es/wol/d/r4/lp-s/202026403");
        assert_eq!(nav.sections[2].items[1].label, "Cantemos con gozo");
    }

    #[test]
    fn single_list_directories_use_the_page_heading() {
        let nav = parse_nav(LIBRARY).expect("library page is a directory");
        assert_eq!(nav.title.as_deref(), Some("Biblioteca"));
        assert_eq!(nav.sections.len(), 1);
        assert_eq!(nav.sections[0].title.as_deref(), Some("Biblioteca"));
        // `href="#"` placeholders never become menu entries.
        let labels: Vec<&str> = nav.sections[0]
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Biblias", "Glosario"]);
        assert!(nav.sections[0].items[0].detail.is_none());
    }

    #[test]
    fn articles_are_not_directory_pages() {
        assert!(parse_nav(ARTICLE).is_none());
        assert!(parse_nav(HOME).is_none());
        assert!(parse_nav("<article><p data-pid=\"1\">Hi</p></article>").is_none());
    }

    #[test]
    fn unknown_containers_still_surface_their_text() {
        let html = r#"<div id="article"><foo>Hello</foo></div>"#;
        let document = parse_document(html);
        let text: String = document
            .nodes
            .iter()
            .filter_map(|node| match node {
                Node_::Block(block) => Some(block.text.clone()),
                _ => None,
            })
            .collect();
        assert!(text.contains("Hello"));
    }
}
