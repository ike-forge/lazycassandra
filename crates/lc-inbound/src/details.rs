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
use lc_domain::style::Token;
use lc_domain::view::{DetailRow, Details, Group, Section};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
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
) -> crate::body::Extent {
    let lines = lines(details, usize::from(area.width), theme);
    let total = lines.len();
    // `Paragraph::scroll` is u16, and a body past 65535 lines is unreachable;
    // the extent keeps the offset itself, so the thumb stays right regardless.
    let scroll = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);

    crate::body::Extent {
        position: offset,
        total,
        visible: usize::from(area.height),
    }
}

/// Every line the body occupies at `width`.
///
/// Also what the frame reports back as `Geometry::ddl_lines`, so the domain
/// can bound an offset over text it cannot wrap itself (design §2).
pub(crate) fn lines(details: &Details, width: usize, theme: Theme) -> Vec<Line<'static>> {
    let mut lines = vec![Line::raw(truncate(&details.heading, width)), Line::raw("")];
    for (index, section) in details.sections.iter().enumerate() {
        // The one row spent on separation, and only where it buys the split
        // between what the table is and what it is configured to do. A section
        // that opens the panel already has the heading's blank line above it.
        if index > 0 && section.group == Group::Settings {
            lines.push(Line::raw(""));
        }
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

    std::iter::once(Line::styled(truncate(&section.heading, width), style))
        .chain(
            section
                .rows
                .iter()
                .flat_map(|row| row_lines(row, width, style, theme)),
        )
        .collect()
}

fn row_lines(row: &DetailRow, width: usize, style: Style, theme: Theme) -> Vec<Line<'static>> {
    match row {
        DetailRow::Column {
            name,
            order,
            cql_type,
        } => column_lines(name, *order, cql_type, width)
            .into_iter()
            .map(|text| Line::styled(text, style))
            .collect(),
        DetailRow::Property { name, value } => property_lines(name, value, width, style, theme),
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

    match gap(&left, cql_type, width) {
        Some(gap) => vec![format!("{left}{}{cql_type}", " ".repeat(gap))],
        None => std::iter::once(truncate(&left, width))
            .chain(wrap(cql_type, width, TYPE_INDENT))
            .collect(),
    }
}

/// A property on one line when its name and value both fit, with the value at
/// the right margin; name over indented value when they do not.
///
/// The margin is per row rather than a column shared by the whole section: a
/// column is set by the widest name, and one long name then pushes every other
/// value past the edge — `bloom_filter_fp_chance` puts it at 23, where
/// `gc_grace_seconds` and `864000` need 29 of a 28-column panel
/// (doc/specs/2026-08-09-visual-polish-design.md §5.2).
///
/// Which properties get one line therefore follows from their length. `comment`
/// and `compaction` wrap because they are long, not because they are named.
fn property_lines(
    name: &str,
    value: &str,
    width: usize,
    style: Style,
    theme: Theme,
) -> Vec<Line<'static>> {
    let label = theme.style(Token::PropertyName);

    if value.is_empty() {
        return vec![Line::from(Span::styled(truncate(name, width), label))];
    }

    if let Some(gap) = gap(name, value, width) {
        return vec![Line::from(vec![
            Span::styled(name.to_owned(), label),
            Span::styled(format!("{}{value}", " ".repeat(gap)), style),
        ])];
    }

    std::iter::once(Line::from(Span::styled(truncate(name, width), label)))
        .chain(
            wrap(value, width, INDENT)
                .into_iter()
                .map(|text| Line::styled(text, style)),
        )
        .collect()
}

/// The spaces that put `right` at the margin of a `width`-column line already
/// holding `left`, or `None` when the two cannot share a line with a gap
/// between them.
fn gap(left: &str, right: &str, width: usize) -> Option<usize> {
    match width.checked_sub(count(left) + count(right)) {
        Some(gap) if gap > 0 => Some(gap),
        _ => None,
    }
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
    use lc_domain::style::Token;

    use super::*;

    /// The text of each line, with the styling dropped.
    fn texts(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    fn property(name: &str, value: &str, width: usize) -> Vec<String> {
        texts(&property_lines(
            name,
            value,
            width,
            Style::new(),
            Theme::TerminalDefault,
        ))
    }

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
    fn a_property_short_enough_for_one_line_puts_its_value_at_the_margin() {
        assert_eq!(
            property("gc_grace_seconds", "864000", 23),
            ["gc_grace_seconds 864000"]
        );
        assert_eq!(property("cdc", "false", 23), ["cdc               false"]);
    }

    #[test]
    fn values_on_one_line_end_in_the_same_column() {
        let rows = [
            property("default_time_to_live", "0", 28),
            property("gc_grace_seconds", "864000", 28),
            property("cdc", "false", 28),
        ];

        for row in &rows {
            assert_eq!(row.len(), 1, "{row:?} should have fitted one line");
            assert_eq!(row[0].chars().count(), 28, "{row:?} is off the margin");
        }
    }

    #[test]
    fn a_property_too_long_for_one_line_puts_its_value_indented_beneath_it() {
        assert_eq!(
            property("gc_grace_seconds", "864000", 20),
            ["gc_grace_seconds", "  864000"]
        );
    }

    #[test]
    fn a_property_name_is_the_only_part_of_its_row_that_is_highlighted() {
        let one_line = property_lines(
            "gc_grace_seconds",
            "864000",
            23,
            Style::new(),
            Theme::TerminalDefault,
        );
        let accent = Theme::TerminalDefault.style(Token::PropertyName).fg;

        assert!(accent.is_some(), "the token has to spend a colour");
        assert_eq!(one_line[0].spans[0].style.fg, accent);
        assert_eq!(
            one_line[0].spans[1].style.fg, None,
            "the value is left alone: two treatments on one narrow line is one too many"
        );

        // The wrapped shape marks the same thing the same way.
        let wrapped = property_lines(
            "comment",
            "Order history for one customer.",
            23,
            Style::new(),
            Theme::TerminalDefault,
        );
        assert_eq!(wrapped[0].spans[0].style.fg, accent);
        assert_eq!(wrapped[1].spans[0].style.fg, None);
    }

    #[test]
    fn a_long_value_wraps_rather_than_losing_its_tail() {
        assert_eq!(
            property("comment", "Order history for one customer.", 23),
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
            property("memtable_flush_period_in_ms", "0", 20),
            ["memtable_flush_peri…", "  0"]
        );
    }

    #[test]
    fn a_panel_with_no_room_for_a_value_still_names_the_property() {
        assert_eq!(property("cdc", "false", 2), ["c…"]);
    }

    #[test]
    fn a_property_with_an_empty_value_is_just_its_name() {
        assert_eq!(property("comment", "", 23), ["comment"]);
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
