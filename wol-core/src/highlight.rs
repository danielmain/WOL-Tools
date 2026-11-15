//! Highlight range algebra — the pure core of `highlight.js`.
//!
//! Stored ranges are `[startToken, endToken, colorIndex]` with an **inclusive**
//! `endToken`. Adding a range either merges same-colour neighbours or trims the
//! overlapped ranges so the result is always disjoint and sorted — exactly what
//! `addHighlightRange()` / `removeHighlightRange()` do in the extension, which is
//! what keeps CLI edits byte-compatible with browser edits.

use crate::backup::Span;

/// Clamp a colour index into JW Library's 1–6 palette (`0` = no colour),
/// mirroring `const safeColor = color >= 1 && color <= 6 ? color : 0`.
pub fn safe_color(color: i64) -> i64 {
    if (1..=6).contains(&color) {
        color
    } else {
        0
    }
}

/// Add or recolour `new_range`, returning a sorted, disjoint range list.
pub fn add_range(ranges: &[Span], new_range: Span) -> Vec<Span> {
    let (new_start, new_end, new_color) = new_range;
    let mut result = Vec::with_capacity(ranges.len() + 1);
    let mut start = new_start;
    let mut end = new_end;

    for &(range_start, range_end, color) in ranges {
        if range_end < start || range_start > end {
            result.push((range_start, range_end, color));
            continue;
        }
        if color == new_color {
            start = start.min(range_start);
            end = end.max(range_end);
            continue;
        }
        if range_start < start {
            result.push((range_start, start - 1, color));
        }
        if range_end > end {
            result.push((end + 1, range_end, color));
        }
    }
    result.push((start, end, new_color));
    result.sort_by_key(|(a, b, _)| (*a, *b));
    result
}

/// Remove every stored range inside `removal`, splitting surviving edges.
pub fn remove_range(ranges: &[Span], removal: (i64, i64)) -> Vec<Span> {
    let (remove_start, remove_end) = removal;
    let mut result = Vec::with_capacity(ranges.len() + 1);
    for &(start, end, color) in ranges {
        if end < remove_start || start > remove_end {
            result.push((start, end, color));
            continue;
        }
        if start < remove_start {
            result.push((start, remove_start - 1, color));
        }
        if end > remove_end {
            result.push((remove_end + 1, end, color));
        }
    }
    result.sort_by_key(|(a, b, _)| (*a, *b));
    result
}

/// Colour covering `token`, if any.
pub fn color_at(ranges: &[Span], token: i64) -> Option<i64> {
    ranges
        .iter()
        .find(|(start, end, _)| *start <= token && token <= *end)
        .map(|(_, _, color)| *color)
}

/// Total number of tokens covered by `ranges` (overlaps counted once).
pub fn covered_tokens(ranges: &[Span]) -> usize {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|(start, _, _)| *start);
    let mut total = 0i64;
    let mut cursor = i64::MIN;
    for (start, end, _) in sorted {
        if end < start {
            continue;
        }
        let from = start.max(cursor.saturating_add(1));
        if end >= from {
            total += end - from + 1;
            cursor = end;
        }
    }
    usize::try_from(total).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_a_disjoint_range_keeps_both() {
        let ranges = vec![(0, 2, 1)];
        assert_eq!(add_range(&ranges, (5, 7, 2)), vec![(0, 2, 1), (5, 7, 2)]);
    }

    #[test]
    fn adding_same_colour_merges_neighbours() {
        let ranges = vec![(0, 4, 1), (5, 9, 1)];
        assert_eq!(add_range(&ranges, (3, 6, 1)), vec![(0, 9, 1)]);
    }

    #[test]
    fn adding_a_new_colour_inside_splits_the_old_range() {
        let ranges = vec![(0, 9, 1)];
        assert_eq!(
            add_range(&ranges, (3, 5, 2)),
            vec![(0, 2, 1), (3, 5, 2), (6, 9, 1)]
        );
    }

    #[test]
    fn adding_a_new_colour_over_the_left_edge_trims_it() {
        let ranges = vec![(4, 9, 1)];
        assert_eq!(add_range(&ranges, (0, 6, 3)), vec![(0, 6, 3), (7, 9, 1)]);
    }

    #[test]
    fn adding_a_new_colour_over_the_right_edge_trims_it() {
        let ranges = vec![(0, 5, 1)];
        assert_eq!(add_range(&ranges, (3, 9, 3)), vec![(0, 2, 1), (3, 9, 3)]);
    }

    #[test]
    fn removing_splits_and_drops() {
        let ranges = vec![(0, 9, 1), (12, 14, 2)];
        assert_eq!(remove_range(&ranges, (3, 5)), vec![(0, 2, 1), (6, 9, 1), (12, 14, 2)]);
        assert_eq!(remove_range(&ranges, (0, 9)), vec![(12, 14, 2)]);
        assert!(remove_range(&ranges, (0, 99)).is_empty());
    }

    #[test]
    fn color_indices_are_clamped_to_the_palette() {
        assert_eq!(safe_color(0), 0);
        assert_eq!(safe_color(3), 3);
        assert_eq!(safe_color(6), 6);
        assert_eq!(safe_color(7), 0);
        assert_eq!(safe_color(-2), 0);
    }

    #[test]
    fn colors_are_looked_up_and_counted() {
        let ranges = vec![(0, 4, 1), (5, 9, 2)];
        assert_eq!(color_at(&ranges, 0), Some(1));
        assert_eq!(color_at(&ranges, 9), Some(2));
        assert_eq!(color_at(&ranges, 10), None);
        assert_eq!(covered_tokens(&ranges), 10);
        assert_eq!(covered_tokens(&[(0, 4, 1), (2, 6, 2)]), 7);
    }
}
