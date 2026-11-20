use wol_core::backup::{NotesIndex, StoredIndex};

use super::*;
use crate::render::{ColorDepth, Glyphs, StyleRole, Theme};

const HTML: &str = r##"
<html><body>
  <div id="article">
    <p data-pid="1">How Can You Draw Close to God?</p>
    <p data-pid="2"><span class="parNum">2</span>Jehovah God invites all people to draw close to him.
      He is not distant or unapproachable.</p>
    <p data-pid="3">The Bible says: <b>“Draw close to God, and he will draw close to you.”</b></p>
  </div>
</body></html>
"##;

fn marks() -> StoredIndex {
    let mut marks = StoredIndex::default();
    let mut paragraph = std::collections::BTreeMap::new();
    // Paragraph 2 text is "Jehovah God invites all people to draw close to
    // him. He is not distant or unapproachable." — tokens 6..7 are
    // "draw"+"close" (the whitespace between them is not a token).
    paragraph.insert("2".to_string(), vec![(6, 7, 1)]);
    marks.docs.insert("2025441".to_string(), paragraph);
    marks.highlight_count = 1;
    marks.document_count = 1;
    marks
}

fn theme(depth: ColorDepth) -> Theme {
    Theme::new(depth, Glyphs::Unicode)
}

#[test]
fn highlights_land_on_the_expected_words() {
    let document = wol_core::dom::parse_document(HTML);
    let block = document.block("2").expect("paragraph 2");
    let runs = styled_runs(block, &[(6, 7, 1)], &theme(ColorDepth::None));
    let highlighted: String = runs
        .iter()
        .filter(|run| run.role == StyleRole::Highlight(1))
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(highlighted, "draw close");
    assert_eq!(highlighted, "draw close");
    // Token 5 is "to": a range starting there must include it.
    let wider = styled_runs(block, &[(5, 7, 1)], &theme(ColorDepth::None));
    let wider_highlighted: String = wider
        .iter()
        .filter(|run| run.role == StyleRole::Highlight(1))
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(wider_highlighted, "to draw close");
}

#[test]
fn truecolor_output_contains_background_sequences() {
    let document = wol_core::dom::parse_document(HTML);
    let lines = render_document(
        &document,
        &marks(),
        &theme(ColorDepth::TrueColor),
        RenderOptions::default(),
    );
    let joined = lines.join("\n");
    assert!(
        joined.contains("\x1b[48;2;255;227;107;38;2;16;20;26m"),
        "missing highlight background:\n{joined}"
    );
    assert!(joined.contains("\x1b[0m"), "missing reset");
    assert!(joined.contains("How Can You Draw Close to God?"));
}

#[test]
fn plain_output_has_no_escape_sequences() {
    let document = wol_core::dom::parse_document(HTML);
    let lines = render_document(
        &document,
        &marks(),
        &theme(ColorDepth::None),
        RenderOptions::default(),
    );
    let joined = lines.join("\n");
    assert!(!joined.contains('\x1b'));
    assert!(joined.contains("draw close"));
    // Gutter keeps paragraph numbers out of the prose.
    assert!(joined.contains("   2 │ Jehovah God invites"));
}

#[test]
fn golden_layout_at_80_and_120_columns() {
    let document = wol_core::dom::parse_document(HTML);
    for width in [80usize, 120] {
        let options = RenderOptions {
            width,
            ..RenderOptions::default()
        };
        let lines = render_document(&document, &marks(), &theme(ColorDepth::None), options);
        for line in lines {
            let visible = visible_width(&line);
            assert!(
                visible <= width.max(20),
                "line wider than {width} ({visible}): {line}"
            );
        }
    }
    let narrow = render_document(
        &document,
        &marks(),
        &theme(ColorDepth::None),
        RenderOptions {
            width: 60,
            ..RenderOptions::default()
        },
    );
    let wide = render_document(
        &document,
        &marks(),
        &theme(ColorDepth::None),
        RenderOptions {
            width: 120,
            ..RenderOptions::default()
        },
    );
    assert!(narrow.len() > wide.len(), "narrow terminals must wrap to more lines");
    // Stable snapshot of the uncoloured 80-column rendering.
    assert_eq!(narrow[0], format!("╭{}╮", "─".repeat(58)));
    assert_eq!(narrow[1].chars().next(), Some('│'));
    assert!(narrow[1].starts_with("│ How Can You Draw Close to God?"));
}

#[test]
fn ascii_mode_is_pure_ascii() {
    let document = wol_core::dom::parse_document(HTML);
    let lines = render_document(
        &document,
        &marks(),
        &Theme::new(ColorDepth::None, Glyphs::Ascii),
        RenderOptions {
            width: 80,
            ..RenderOptions::default()
        },
    );
    for line in lines {
        assert!(line.is_ascii(), "non-ASCII in ascii mode: {line}");
    }
}

#[test]
fn notes_are_rendered_in_a_gutter_box() {
    let document = wol_core::dom::parse_document(HTML);
    let mut marks = marks();
    marks.notes = NotesIndex::default();
    marks.notes.docs.insert(
        "2025441".to_string(),
        [(
            "2".to_string(),
            vec![wol_core::backup::NoteItem {
                content: "Remember this point for the meeting.".to_string(),
                title: None,
                note_id: None,
                guid: None,
                user_mark_id: None,
                location_id: None,
                last_modified: None,
                created: None,
                block_type: Some(1),
                block_identifier: Some(2),
            }],
        )]
        .into_iter()
        .collect(),
    );
    let lines = render_document(
        &document,
        &marks,
        &theme(ColorDepth::None),
        RenderOptions::default(),
    );
    let joined = lines.join("\n");
    assert!(joined.contains("✎ Note · ¶2"));
    assert!(joined.contains("Remember this point"));
}

#[test]
fn wrap_runs_never_exceeds_the_width() {
    let runs = vec![
        Run::new("alpha beta gamma ", StyleRole::Body),
        Run::new("delta-epsilon-zeta", StyleRole::Highlight(3)),
    ];
    for line in wrap_runs(&runs, 12) {
        assert!(visible_width(&line) <= 12, "{line}");
    }
    assert_eq!(wrap_runs(&[], 20), Vec::<String>::new());
}

#[test]
fn visible_width_ignores_ansi_sequences() {
    let painted = "\x1b[1;38;2;157;196;255mHeading\x1b[0m";
    assert_eq!(visible_width(painted), 7);
    assert_eq!(visible_width("ünïcodé"), 7);
}

#[test]
fn wrapped_highlight_keeps_its_style_on_every_line() {
    let document = wol_core::dom::parse_document(HTML);
    let block = document.block("2").expect("paragraph 2");
    let mut marks = marks();
    // Cover the whole paragraph, which is wider than 60 columns.
    marks.docs.insert(
        "2025441".to_string(),
        [("2".to_string(), vec![(0, 11, 1)])].into_iter().collect(),
    );
    let lines = render_block_lines(
        block,
        &marks,
        Some("2025441"),
        &theme(ColorDepth::TrueColor),
        RenderOptions {
            width: 60,
            ..RenderOptions::default()
        },
    );
    assert!(lines.len() > 1, "the paragraph must wrap");
    for line in &lines {
        assert!(
            line.text.contains("\x1b[48;2;255;227;107"),
            "highlight lost on a continuation line: {:?}",
            line.text
        );
    }
}

#[test]
fn fragments_report_the_columns_each_piece_occupies() {
    let document = wol_core::dom::parse_document(HTML);
    let block = document.block("2").expect("paragraph 2");
    let lines = render_block_lines(
        block,
        &marks(),
        Some("2025441"),
        &theme(ColorDepth::None),
        RenderOptions {
            width: 60,
            ..RenderOptions::default()
        },
    );
    let first = lines.first().expect("a rendered line");
    let plain: String = first.text.chars().collect();
    let columns = |from: usize, width: usize| -> String {
        plain.chars().skip(from).take(width).collect()
    };
    // The gutter is painted before the body, so the first fragment starts after it.
    assert!(columns(first.fragments[0].col, first.fragments[0].width).starts_with("Jehovah"));
    // The highlighted run carries its colourless markers, and the fragment's
    // columns include them — that is what makes the overlay land on real words.
    let highlighted = first
        .fragments
        .iter()
        .filter(|fragment| fragment.width > 0)
        .map(|fragment| columns(fragment.col, fragment.width))
        .find(|piece| piece.contains("draw"))
        .expect("highlighted fragment");
    assert_eq!(highlighted, "⟦draw close⟧");
}
