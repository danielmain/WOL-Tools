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
    for node in &document.nodes {
        if let Node_::Block(block) = node {
            for piece in &block.pieces {
                if let Some(href) = &piece.href {
                    links.push(Link {
                        href: href.clone(),
                        text: slice_utf16(&block.text, piece.start, piece.end),
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
        collect_text(element, &Style::default(), &mut sink, self.warnings);

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
    fn push(&mut self, value: &str, style: &Style) {
        // Whitespace-only nodes never affect token coordinates (the tokenizer
        // skips them), so dropping them keeps pieces text-free of separators
        // without shifting any token index.
        if value.trim().is_empty() {
            return;
        }
        let start = tokenize::utf16_len_of(&self.text);
        self.text.push_str(value);
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
        // a.cl and .fn skipped, normal links kept.
        assert_eq!(third.text, "See  and link.");
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
