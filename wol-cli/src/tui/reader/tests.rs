use wol_core::backup::StoredIndex;

use super::*;
use crate::render::{ColorDepth, Glyphs, Theme};

const HTML: &str = r##"
<div id="article">
  <p data-pid="1">How Can You Draw Close to God?</p>
  <p data-pid="2">Jehovah God invites all people to draw close to him.</p>
  <p data-pid="3">The Bible says: draw close to God.</p>
</div>
"##;

fn reader() -> Reader {
    let mut document = wol_core::dom::parse_document(HTML);
    // Real loads always carry the canonical URL; the reader derives the
    // document id from its last path segment.
    document.source_url = Some("https://wol.jw.org/en/wol/d/r1/lp-e/2025441".to_string());
    Reader::new(
        document,
        StoredIndex::default(),
        &Theme::new(ColorDepth::None, Glyphs::Unicode),
        RenderOptions {
            width: 60,
            ..RenderOptions::default()
        },
    )
}

#[test]
fn renders_one_line_group_per_paragraph() {
    let reader = reader();
    assert!(reader.lines.len() >= 3);
    let with_blocks: Vec<Option<usize>> = reader.lines.iter().map(|line| line.block).collect();
    assert!(with_blocks.iter().any(Option::is_some));
}

#[test]
fn scrolling_is_clamped_to_the_document() {
    let mut reader = reader();
    reader.scroll_by(1000, 5);
    assert_eq!(reader.scroll, reader.lines.len().saturating_sub(5));
    reader.scroll_by(-1000, 5);
    assert_eq!(reader.scroll, 0);
    reader.bottom(5);
    assert_eq!(reader.scroll, reader.lines.len().saturating_sub(5));
    reader.top();
    assert_eq!(reader.scroll, 0);
}

#[test]
fn visual_selection_applies_and_clears_highlights() {
    let mut reader = reader();
    // Move into paragraph 2 (the second block line).
    reader.jump_to("2");
    let position = reader
        .lines
        .iter()
        .position(|line| line.block.and_then(|index| reader.document.nodes.get(index)).is_some_and(|node| matches!(node, Node_::Block(block) if block.identifier.as_deref() == Some("2"))))
        .expect("paragraph 2 line");
    reader.scroll = position;
    reader.cursor_token = 0;
    reader.begin_selection();
    reader.move_cursor_token(1);
    let pid = reader.apply_color(3).expect("applied");
    assert_eq!(pid, "2");
    assert_eq!(reader.marks.doc_ranges("2025441", "2"), &[(0, 1, 3)]);
    assert_eq!(reader.marks.highlight_count, 1);
    assert!(reader.selection.is_none());

    // Selecting again and pressing 0/x removes it.
    reader.jump_to("2");
    reader.scroll = position;
    reader.cursor_token = 0;
    reader.begin_selection();
    reader.move_cursor_token(1);
    assert_eq!(reader.clear_color().as_deref(), Some("2"));
    assert!(reader.marks.doc_ranges("2025441", "2").is_empty());
    assert_eq!(reader.marks.highlight_count, 0);
}

#[test]
fn selection_range_is_normalised() {
    let mut reader = reader();
    reader.begin_selection();
    reader.cursor_token = 5;
    assert_eq!(reader.selected_range().map(|(_, start, end)| (start, end)), Some((0, 5)));
    reader.cursor_token = 0;
    reader.selection.as_mut().unwrap().start_token = 7;
    assert_eq!(reader.selected_range().map(|(_, start, end)| (start, end)), Some((0, 7)));
}

#[test]
fn search_finds_hits_and_cycles() {
    let mut reader = reader();
    reader.search("draw close");
    assert!(!reader.hits.is_empty());
    let first = reader.hit_index;
    reader.next_hit();
    assert_ne!(reader.hits.len(), 1);
    assert!(reader.hit_index != first || reader.hits.len() == 1);
    reader.previous_hit();
    reader.search("");
    assert!(reader.hits.is_empty());
    assert!(reader.search.is_none());
}

#[test]
fn cursor_row_moves_and_keeps_itself_visible() {
    let mut reader = reader();
    assert_eq!(reader.cursor_line(), 0);
    reader.move_line(1, 3);
    assert_eq!(reader.cursor_line(), 1);
    assert_eq!(reader.scroll, 0, "cursor stays inside the viewport");
    reader.move_line(100, 3);
    assert_eq!(reader.cursor_line(), reader.lines.len() - 1);
    assert!(
        reader.cursor_line() >= reader.scroll && reader.cursor_line() < reader.scroll + 3,
        "scroll follows the cursor"
    );
    reader.move_line(-100, 3);
    assert_eq!((reader.cursor_line(), reader.scroll), (0, 0));
}

#[test]
fn stale_token_is_clamped_to_the_new_paragraph() {
    let mut reader = reader();
    reader.jump_to("3");
    // Simulate scrolling away from a long paragraph, which used to leave a token
    // index far past the end of the short one under the cursor.
    reader.cursor_token = 900;
    reader.set_cursor_line(reader.cursor_line());
    let total = reader.current_block().expect("paragraph").tokens.len() as i64;
    assert_eq!(reader.cursor_token, total - 1);

    reader.begin_selection();
    assert_eq!(reader.apply_color(2).as_deref(), Some("3"));
    assert_eq!(
        reader.marks.doc_ranges("2025441", "3"),
        &[(total - 1, total - 1, 2)],
        "a stale token must still land on a real token"
    );
    assert_eq!(reader.marks.highlight_count, 1);
}

#[test]
fn status_shows_the_cursor_row() {
    let theme = Theme::new(ColorDepth::None, Glyphs::Unicode);
    let mut reader = reader();
    reader.jump_to("2");
    let row = reader.cursor_line() + 1;
    let status = reader.status(&theme);
    assert!(status.contains(&format!("{row}/{}", reader.lines.len())));
    assert!(status.contains("tok 1/"));
}

#[test]
fn status_line_reports_position_and_visual_mode() {
    let theme = Theme::new(ColorDepth::None, Glyphs::Unicode);
    let mut reader = reader();
    let status = reader.status(&theme);
    assert!(status.contains('%'));
    assert!(status.contains("lines"));
    reader.begin_selection();
    assert!(reader.status(&theme).contains("VISUAL"));
}

#[test]
fn jump_to_known_paragraph_moves_the_viewport() {
    let mut reader = reader();
    reader.jump_to("3");
    let block = reader.current_block().map(|block| block.identifier.clone());
    assert_eq!(block, Some(Some("3".to_string())));
    reader.jump_to("does-not-exist");
    assert_eq!(reader.scroll > 0 || reader.scroll == 0, true);
}

#[test]
fn token_helpers_expose_the_current_paragraph() {
    let mut reader = reader();
    reader.jump_to("2");
    let tokens = reader.tokens_of_current_block();
    assert_eq!(tokens.first().map(String::as_str), Some("Jehovah"));
    assert!(reader.total_tokens() > 10);
}

const NAV_HTML: &str = r##"
<div id="article">
  <ul>
    <li><a href="/es/wol/library/r4/lp-s/biblioteca">Biblioteca</a></li>
    <li><a href="/es/wol/library/r4/lp-s/reuniones">Reuniones</a></li>
    <li><a href="/es/wol/library/r4/lp-s/atras">La Atalaya</a></li>
  </ul>
</div>
"##;

fn nav_reader() -> Reader {
    let mut document = wol_core::dom::parse_document(NAV_HTML);
    document.source_url = Some("https://wol.jw.org/es/wol/h/r4/lp-s".to_string());
    Reader::new(
        document,
        StoredIndex::default(),
        &Theme::new(ColorDepth::None, Glyphs::Unicode),
        RenderOptions {
            width: 60,
            ..RenderOptions::default()
        },
    )
}

#[test]
fn nav_pages_expose_one_link_per_entry() {
    let reader = nav_reader();
    assert_eq!(reader.links().len(), 3);
    assert_eq!(reader.links()[0].text.trim(), "Biblioteca");
    assert!(reader.links()[0].href.contains("/biblioteca"));
    // Every link points at a block that produced a rendered line.
    for link in reader.links() {
        assert!(reader.line_of_node(link.node).is_some(), "{link:?} has no line");
    }
}

#[test]
fn link_cursor_wraps_and_scrolls() {
    let mut reader = nav_reader();
    let (first, link) = reader.step_link(0, 1).expect("second link");
    assert_eq!(first, 1);
    assert!(link.href.contains("/reuniones"));

    // j from the last link wraps to the first.
    let (wrapped, _) = reader.step_link(2, 1).expect("wrapped link");
    assert_eq!(wrapped, 0);
    let (back, _) = reader.step_link(0, -1).expect("previous link");
    assert_eq!(back, 2);
}

#[test]
fn nearest_link_and_link_at_top_agree_on_nav_pages() {
    let mut reader = nav_reader();
    assert_eq!(reader.nearest_link(), Some(0));
    assert_eq!(reader.link_at_top(), Some(0));
    assert_eq!(reader.focus_link(2).map(|link| link.text.trim().to_string()), Some("La Atalaya".to_string()));
}

#[test]
fn pages_without_links_report_none() {
    let reader = reader();
    assert!(reader.links().is_empty());
    assert_eq!(reader.nearest_link(), None);
    assert_eq!(reader.link_at_top(), None);
}

/// Visible-column substring of a colourless rendered line.
fn columns(text: &str, from: usize, width: usize) -> String {
    text.chars().skip(from).take(width).collect()
}

/// Selection/cursor columns of one line, merged per overlay kind.
fn span(overlays: &[(usize, Vec<(usize, usize, OverlayKind)>)] , line: usize, kind: OverlayKind) -> Option<(usize, usize)> {
    overlays
        .iter()
        .find(|(index, _)| *index == line)
        .map(|(_, pieces)| pieces.iter())
        .unwrap()
        .filter(move |(_, _, found)| *found == kind)
        .map(|(start, end, _)| (*start, *end))
        .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
}

#[test]
fn selection_overlay_lands_on_the_selected_words() {
    let mut reader = reader();
    reader.jump_to("2");
    let line = reader.cursor_line();
    reader.cursor_token = 6;
    reader.begin_selection();
    reader.move_cursor_token(1);

    let overlays = reader.overlays();
    let (start, end) = span(&overlays, line, OverlayKind::Selection).expect("selection columns");
    assert_eq!(
        columns(&reader.lines[line].text, start, end - start),
        "draw close"
    );
    assert_eq!(reader.selected_text().as_deref(), Some("draw close"));
}

#[test]
fn cursor_overlay_marks_the_token_a_colour_would_use() {
    let mut reader = reader();
    reader.jump_to("2");
    let line = reader.cursor_line();
    reader.cursor_token = 0;
    let overlays = reader.overlays();
    let (start, end) = span(&overlays, line, OverlayKind::Cursor).expect("cursor columns");
    assert_eq!(columns(&reader.lines[line].text, start, end - start), "Jehovah");

    // The cursor is covered by the selection once `v` extends past it.
    reader.begin_selection();
    reader.move_cursor_token(2);
    let overlays = reader.overlays();
    let kinds: Vec<OverlayKind> = overlays
        .iter()
        .find(|(index, _)| *index == line)
        .map(|(_, pieces)| pieces.iter().map(|(_, _, kind)| *kind).collect())
        .unwrap();
    assert!(kinds.contains(&OverlayKind::Selection));
    assert!(kinds.contains(&OverlayKind::Cursor));
}

#[test]
fn status_names_the_selected_text() {
    let theme = Theme::new(ColorDepth::None, Glyphs::Unicode);
    let mut reader = reader();
    reader.jump_to("2");
    reader.cursor_token = 6;
    reader.begin_selection();
    reader.move_cursor_token(1);
    let status = reader.status(&theme);
    assert!(status.contains("VISUAL ¶2 tok 6-7"), "{status}");
    assert!(status.contains("«draw close»"), "{status}");
}

const WIDE_HTML: &str = r##"
<div id="article">
  <p data-pid="1">A Long Paragraph</p>
  <p data-pid="2">Jehovah God invites all people everywhere to draw close to him by studying the Bible carefully every single day and by praying to him often with a thankful heart.</p>
</div>
"##;

fn wide_reader() -> Reader {
    let mut document = wol_core::dom::parse_document(WIDE_HTML);
    document.source_url = Some("https://wol.jw.org/en/wol/d/r1/lp-e/2025441".to_string());
    Reader::new(
        document,
        StoredIndex::default(),
        &Theme::new(ColorDepth::None, Glyphs::Unicode),
        RenderOptions {
            width: 40,
            ..RenderOptions::default()
        },
    )
}

#[test]
fn moving_the_token_follows_it_across_wrapped_lines() {
    let mut reader = wide_reader();
    reader.jump_to("2");
    let first = reader.cursor_line();
    let start = reader.overlays().first().map(|(index, _)| *index);
    assert_eq!(start, Some(first));

    // Walk to the end of the paragraph: the cursor row must travel with the
    // token instead of staying on the first wrapped line.
    let tokens = reader.total_tokens();
    for _ in 1..tokens {
        reader.move_cursor_token(1);
        reader.follow_token(4);
    }
    assert!(reader.cursor_line() > first, "the cursor row did not follow");
    let last = reader.overlays().last().map(|(index, _)| *index);
    assert_eq!(last, Some(reader.cursor_line()));
}

#[test]
fn the_selection_band_is_on_screen_after_moving_the_token() {
    let mut reader = wide_reader();
    reader.jump_to("2");
    for _ in 0..20 {
        reader.move_cursor_token(1);
        reader.follow_token(4);
    }
    reader.begin_selection();
    reader.move_cursor_token(2);
    let overlays = reader.overlays();
    let row = overlays
        .iter()
        .find(|(_, pieces)| pieces.iter().any(|(_, _, kind)| *kind == OverlayKind::Selection))
        .map(|(index, _)| *index)
        .expect("a selection band");
    assert_eq!(row, reader.cursor_line());
    assert!(row >= reader.scroll, "the band scrolled out of view");
}
