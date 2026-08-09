//! Drawing a [`Details`] body in a panel about twenty columns wide.
//!
//! Names are never abbreviated and values are never truncated: there is no
//! expansion mechanism yet, so an abbreviation would owe the reader a decoder
//! and a truncation would hide data permanently
//! (doc/specs/2026-08-09-table-metadata-design.md §6). Wrapping spends lines
//! instead, which is why the option ordering carries so much weight.
//!
//! Widths are counted in `char`s. Every identifier this panel draws is ASCII;
//! a comment carrying wide characters misaligns by a column, which `ratatui`
//! clips rather than corrupts, and a `unicode-width` dependency is not worth
//! that.

use lc_domain::model::ClusteringOrder;
use lc_domain::view::{DetailRow, Details, Section};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::theme::Theme;

const ASCENDING: &str = " ↑";
const DESCENDING: &str = " ↓";
const INDENT: usize = 2;
/// A column's type falls to its own line under the name, one step further in
/// than a property's value so the two shapes stay distinguishable.
const TYPE_INDENT: usize = 4;
const ELLIPSIS: char = '…';

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    details: &Details,
    offset: usize,
    theme: Theme,
) {
    let lines = lines(details, usize::from(area.width), theme);
    let offset = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)), area);
}

/// Every line the body occupies at `width`.
///
/// Also what the frame reports back as `Geometry::ddl_lines`, so the domain
/// can bound an offset over text it cannot wrap itself (design §2).
pub(crate) fn lines(details: &Details, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw(truncate(&details.heading, width)), Line::raw("")];
    for section in &details.sections {
        lines.extend(section_lines(section, width, theme));
    }

    lines
}

/// A heading and its rows, with no blank line of their own: the heading is
/// unindented and styled, which separates the groups without spending a line
/// on each in a panel that has few to spare.
fn section_lines(section: &Section, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let style = section
        .token
        .map_or_else(Style::new, |token| theme.style(token));

    std::iter::once(truncate(&section.heading, width))
        .chain(section.rows.iter().flat_map(|row| row_lines(row, width)))
        .map(|text| Line::styled(text, style))
        .collect()
}

fn row_lines(row: &DetailRow, width: usize) -> Vec<String> {
    match row {
        DetailRow::Column {
            name,
            order,
            cql_type,
        } => column_lines(name, *order, cql_type, width),
        DetailRow::Property { name, value } => property_lines(name, value, width),
    }
}

fn column_lines(
    name: &str,
    order: Option<ClusteringOrder>,
    cql_type: &str,
    width: usize,
) -> Vec<String> {
    let arrow = match order {
        Some(ClusteringOrder::Ascending) => ASCENDING,
        Some(ClusteringOrder::Descending) => DESCENDING,
        None => "",
    };
    let left = format!("{}{name}{arrow}", " ".repeat(INDENT));

    match width.checked_sub(count(&left) + count(cql_type)) {
        Some(gap) if gap > 0 => vec![format!("{left}{}{cql_type}", " ".repeat(gap))],
        _ => std::iter::once(truncate(&left, width))
            .chain(wrap(cql_type, width, TYPE_INDENT))
            .collect(),
    }
}

fn property_lines(name: &str, value: &str, width: usize) -> Vec<String> {
    std::iter::once(truncate(name, width))
        .chain(wrap(value, width, INDENT))
        .collect()
}

/// `text` broken across lines of at most `width` columns, each prefixed with
/// `indent` spaces. A word with no room to fit is broken mid-word rather than
/// dropped — `compaction`'s class name is one unbreakable 60-character token.
fn wrap(text: &str, width: usize, indent: usize) -> Vec<String> {
    let room = width.saturating_sub(indent);
    if room == 0 {
        return Vec::new();
    }
    let prefix = " ".repeat(indent);

    let mut lines = Vec::new();
    let mut current = String::new();

    for chunk in text.split_whitespace().flat_map(|word| broken(word, room)) {
        if current.is_empty() {
            current = chunk;
        } else if count(&current) + 1 + count(&chunk) <= room {
            current.push(' ');
            current.push_str(&chunk);
        } else {
            lines.push(format!("{prefix}{current}"));
            current = chunk;
        }
    }

    if !current.is_empty() {
        lines.push(format!("{prefix}{current}"));
    }
    lines
}

fn broken(word: &str, room: usize) -> Vec<String> {
    word.chars()
        .collect::<Vec<char>>()
        .chunks(room)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

/// The one place `…` appears: a single token wider than the panel, which
/// nothing else can be done with.
fn truncate(text: &str, width: usize) -> String {
    if count(text) <= width {
        return text.to_owned();
    }

    match width.checked_sub(1) {
        Some(0) | None => String::new(),
        Some(room) => text.chars().take(room).chain([ELLIPSIS]).collect(),
    }
}

fn count(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_puts_its_type_at_the_right_margin() {
        assert_eq!(
            column_lines("customer_id", None, "uuid", 23),
            ["  customer_id      uuid"]
        );
    }

    #[test]
    fn a_clustering_column_carries_its_direction_beside_its_name() {
        assert_eq!(
            column_lines("order_date", Some(ClusteringOrder::Descending), "date", 23),
            ["  order_date ↓     date"]
        );
        assert_eq!(
            column_lines("order_id", Some(ClusteringOrder::Ascending), "timeuuid", 23),
            ["  order_id ↑   timeuuid"]
        );
    }

    #[test]
    fn a_column_too_wide_for_one_line_drops_its_type_beneath_the_name() {
        assert_eq!(
            column_lines("warehouse_identifier", None, "frozen<list<text>>", 23),
            ["  warehouse_identifier", "    frozen<list<text>>"]
        );
    }

    #[test]
    fn a_property_puts_its_value_indented_on_the_next_line() {
        assert_eq!(
            property_lines("gc_grace_seconds", "864000", 23),
            ["gc_grace_seconds", "  864000"]
        );
    }

    #[test]
    fn a_long_value_wraps_rather_than_losing_its_tail() {
        assert_eq!(
            property_lines("comment", "Order history for one customer.", 23),
            ["comment", "  Order history for one", "  customer."]
        );
    }

    #[test]
    fn a_value_with_no_break_in_it_is_broken_anyway() {
        // The compaction class name arrives as one 60-character token.
        assert_eq!(
            wrap(
                "org.apache.cassandra.db.compaction.SizeTieredCompactionStrategy",
                23,
                2
            ),
            [
                "  org.apache.cassandra.",
                "  db.compaction.SizeTie",
                "  redCompactionStrategy",
            ]
        );
    }

    #[test]
    fn a_name_wider_than_the_panel_is_the_one_thing_that_gets_an_ellipsis() {
        assert_eq!(
            property_lines("memtable_flush_period_in_ms", "0", 20),
            ["memtable_flush_peri…", "  0"]
        );
    }

    #[test]
    fn a_panel_with_no_room_for_a_value_still_names_the_property() {
        assert_eq!(property_lines("cdc", "false", 2), ["c…"]);
    }

    #[test]
    fn a_property_with_an_empty_value_is_just_its_name() {
        assert_eq!(property_lines("comment", "", 23), ["comment"]);
    }

    #[test]
    fn text_that_fits_is_left_exactly_alone() {
        assert_eq!(truncate("shop", 23), "shop");
        assert_eq!(truncate("shop", 4), "shop");
        assert_eq!(truncate("shop", 3), "sh…");
        assert_eq!(truncate("shop", 1), "");
        assert_eq!(truncate("shop", 0), "");
    }
}
