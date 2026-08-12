//! Drawing a [`Details`] body in a panel about twenty columns wide.
//!
//! Names are never abbreviated and values are never truncated: there is no
//! expansion mechanism yet, so an abbreviation would owe the reader a decoder
//! and a truncation would hide data permanently
//! (docs/superpowers/specs/2026-08-09-table-metadata-design.md §6). Wrapping spends lines
//! instead, which is why the option ordering carries so much weight.
//!
//! Widths are counted in `char`s. Every identifier this panel draws is ASCII;
//! a comment carrying wide characters misaligns by a column, which `ratatui`
//! clips rather than corrupts, and a `unicode-width` dependency is not worth
//! that.
//!
//! A row's selection spans as many lines as it wrapped to
//! (docs/superpowers/specs/2026-08-10-ddl-selection-design.md §5.2), so `lay_out` reports
//! back where it drew the highlight as a [`Span`] over the wrapped lines,
//! rather than the row index the cursor carries in. `List` and `Table` need a
//! guard against a selection scrolled out of view, because a stateful widget
//! insists on scrolling one it cannot see back into view; `Paragraph` carries
//! no selection of its own, and only ever draws the lines it is scrolled to,
//! clipping the rest — so a highlight above or below the offset is simply
//! never drawn, and there is nothing here to guard.

use lc_domain::model::ClusteringOrder;
use lc_domain::state::Span;
use lc_domain::style::Token;
use lc_domain::view::{ColumnKind, DetailRow, Details, Group};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span as TextSpan};
use ratatui::widgets::Paragraph;

use crate::theme::Theme;

const ASCENDING: &str = " ↑";
const DESCENDING: &str = " ↓";
const INDENT: usize = 2;
/// A column's type falls to its own line under the name, one step further in
/// than a property's value so the two shapes stay distinguishable.
const TYPE_INDENT: usize = 4;
const ELLIPSIS: char = '…';

/// A body's lines, and where the selected row landed among them.
pub(crate) struct Laid {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selection: Span,
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    details: &Details,
    selected: usize,
    offset: usize,
    theme: Theme,
) -> (crate::body::Extent, Span) {
    let laid = lay_out(details, selected, usize::from(area.width), theme);
    let total = laid.lines.len();
    // `Paragraph::scroll` is u16, and a body past 65535 lines is unreachable;
    // the extent keeps the offset itself, so the thumb stays right regardless.
    let scroll = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(laid.lines).scroll((scroll, 0)), area);

    (
        crate::body::Extent {
            position: offset,
            total,
            visible: usize::from(area.height),
        },
        laid.selection,
    )
}

/// Every line the body occupies at `width`, with the selected row highlighted
/// across all of its own.
///
/// The span is what the frame reports back as `Geometry::ddl_selection`, so
/// the domain can bound an offset over text it cannot wrap itself
/// (docs/superpowers/specs/2026-08-10-ddl-selection-design.md §5.4). It is measured
/// against every line this function produces, never against the ones a
/// caller happens to scroll into view — an offset must never feed back into
/// what is measured, or a redraw could chase its own tail
/// (architecture.md §3).
pub(crate) fn lay_out(details: &Details, selected: usize, width: usize, theme: Theme) -> Laid {
    // Centred and bold: the heading names what every row below it belongs to,
    // and left-aligned in the body's own colour it read as the first of them
    // (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §7). The colour is
    // the one the Tables tree already spends on that kind of name, so the panel
    // and the tree agree about what a table looks like.
    let heading = Line::styled(
        truncate(
            &prefixed(
                crate::icons::heading(details.heading_token),
                &details.heading,
                width,
            ),
            width,
        ),
        theme
            .style(details.heading_token)
            .add_modifier(Modifier::BOLD),
    )
    .centered();
    let mut lines = vec![heading, Line::raw("")];
    let mut selection = Span::default();
    let mut row = 0;

    for (index, section) in details.sections.iter().enumerate() {
        // The one row spent on separation, and only where it buys the split
        // between what the table is and what it is configured to do. A section
        // that opens the panel already has the heading's blank line above it.
        if index > 0 && section.group == Group::Settings {
            lines.push(Line::raw(""));
        }

        let style = section
            .token
            .map_or_else(Style::new, |token| theme.style(token));

        let icon = crate::icons::section(section.token);
        lines.push(Line::styled(
            truncate(&prefixed(icon, &section.heading, width), width),
            style,
        ));

        for detail in &section.rows {
            let mut drawn = row_lines(detail, width, style, theme);

            if row == selected {
                let highlight = theme.style(Token::Selected);
                drawn = drawn
                    .into_iter()
                    .map(|line| patched(line, highlight))
                    .collect();
                selection = Span {
                    first: lines.len(),
                    lines: drawn.len(),
                };
            }

            lines.extend(drawn);
            row += 1;
        }
    }

    Laid { lines, selection }
}

/// `text` behind its icon, or bare where the two together would push the name
/// past `width`.
///
/// The name comes first: an icon says what a heading is, and the heading
/// already says it in words — so the two columns it spends are given up rather
/// than paid for out of a name this panel promises never to abbreviate
/// (docs/superpowers/specs/2026-08-12-nerd-font-icons-design.md §4).
fn prefixed(icon: &str, text: &str, width: usize) -> String {
    if icon.chars().count() + text.chars().count() > width {
        return text.to_owned();
    }

    format!("{icon}{text}")
}

/// `style` patched onto both the line's own style and each of its spans'.
///
/// A column line carries its colour on the [`Line`] itself (`Line::styled`
/// with an unstyled span), while a property line with a name and a value puts
/// it on the spans instead, so the name and value can differ
/// (`property_lines`). Patching only one of the two would lose the token
/// colour for whichever row shape uses the other.
fn patched(mut line: Line<'static>, style: Style) -> Line<'static> {
    line.style = line.style.patch(style);
    for span in &mut line.spans {
        span.style = span.style.patch(style);
    }
    line
}

fn row_lines(row: &DetailRow, width: usize, style: Style, theme: Theme) -> Vec<Line<'static>> {
    match row {
        DetailRow::KeyColumn {
            name,
            order,
            cql_type,
        } => key_column_lines(name, *order, cql_type, width)
            .into_iter()
            .map(|text| Line::styled(text, style))
            .collect(),
        DetailRow::Column {
            name,
            cql_type,
            kind,
        } => column_lines(name, cql_type, *kind, width, theme),
        DetailRow::Property { name, value } => property_lines(name, value, width, style, theme),
    }
}

fn key_column_lines(
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

    laid_out(
        &format!("{}{name}{arrow}", " ".repeat(INDENT)),
        cql_type,
        width,
    )
}

/// A column in the Table columns tab: its kind in the two columns the other
/// tab spends on indentation, then the name, then the type at the margin.
///
/// The marker carries the colour and the name does not, for the reason a
/// property's value is left alone: two coloured runs on a line this narrow is
/// one too many (visual polish design §5.2).
fn column_lines(
    name: &str,
    cql_type: &str,
    kind: ColumnKind,
    width: usize,
    theme: Theme,
) -> Vec<Line<'static>> {
    let marker = kind.marker();
    let mut lines = laid_out(&format!("{marker:INDENT$} {name}"), cql_type, width)
        .into_iter()
        .map(Line::raw)
        .collect::<Vec<Line<'static>>>();

    if let Some(first) = lines.first_mut() {
        *first = marked(first, INDENT, theme.style(kind.token()));
    }

    lines
}

/// `line` with its first `taken` characters lifted into a span of their own.
fn marked(line: &Line<'static>, taken: usize, style: Style) -> Line<'static> {
    let text = line.to_string();
    let mark: String = text.chars().take(taken).collect();
    let rest: String = text.chars().skip(taken).collect();

    Line::from(vec![TextSpan::styled(mark, style), TextSpan::raw(rest)])
}

/// `left` with `right` at the margin when the two fit, `right` wrapped beneath
/// it when they do not.
fn laid_out(left: &str, right: &str, width: usize) -> Vec<String> {
    match gap(left, right, width) {
        Some(gap) => vec![format!("{left}{}{right}", " ".repeat(gap))],
        None => std::iter::once(truncate(left, width))
            .chain(wrap(right, width, TYPE_INDENT))
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
/// (docs/superpowers/specs/2026-08-09-visual-polish-design.md §5.2).
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
        return vec![Line::from(TextSpan::styled(truncate(name, width), label))];
    }

    if let Some(gap) = gap(name, value, width) {
        return vec![Line::from(vec![
            TextSpan::styled(name.to_owned(), label),
            TextSpan::styled(format!("{}{value}", " ".repeat(gap)), style),
        ])];
    }

    std::iter::once(Line::from(TextSpan::styled(truncate(name, width), label)))
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
    use lc_domain::view::Section;

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
            Theme::default(),
        ))
    }

    #[test]
    fn a_column_puts_its_type_at_the_right_margin() {
        assert_eq!(
            key_column_lines("customer_id", None, "uuid", 23),
            ["  customer_id      uuid"]
        );
    }

    #[test]
    fn a_clustering_column_carries_its_direction_beside_its_name() {
        assert_eq!(
            key_column_lines("order_date", Some(ClusteringOrder::Descending), "date", 23),
            ["  order_date ↓     date"]
        );
        assert_eq!(
            key_column_lines("order_id", Some(ClusteringOrder::Ascending), "timeuuid", 23),
            ["  order_id ↑   timeuuid"]
        );
    }

    #[test]
    fn a_column_too_wide_for_one_line_drops_its_type_beneath_the_name() {
        assert_eq!(
            key_column_lines("warehouse_identifier", None, "frozen<list<text>>", 23),
            ["  warehouse_identifier", "    frozen<list<text>>"]
        );
    }

    fn listed(name: &str, cql_type: &str, kind: ColumnKind, width: usize) -> Vec<String> {
        texts(&column_lines(name, cql_type, kind, width, Theme::default()))
    }

    #[test]
    fn a_listed_column_wears_its_kind_where_the_other_tab_indents() {
        assert_eq!(
            listed("customer_id", "uuid", ColumnKind::Partition, 23),
            ["PK customer_id     uuid"]
        );
        assert_eq!(
            listed("order_date", "date", ColumnKind::Clustering, 23),
            ["CK order_date      date"]
        );
        assert_eq!(
            listed("region", "text", ColumnKind::Static, 23),
            ["S  region          text"]
        );
        assert_eq!(
            listed("status", "text", ColumnKind::Regular, 23),
            ["   status          text"],
            "the common case earns no ink, and the names still line up"
        );
    }

    #[test]
    fn only_the_kind_marker_carries_the_kinds_colour() {
        let theme = Theme::default();
        let lines = column_lines("customer_id", "uuid", ColumnKind::Partition, 23, theme);
        let partition = theme.style(Token::PartitionKey).fg;

        assert!(partition.is_some(), "the token has to spend a colour");
        assert_eq!(lines[0].spans[0].content, "PK");
        assert_eq!(lines[0].spans[0].style.fg, partition);
        assert_eq!(
            lines[0].spans[1].style.fg, None,
            "two coloured runs on a line this narrow is one too many"
        );
    }

    #[test]
    fn a_listed_column_too_wide_for_one_line_drops_its_type_beneath_the_name() {
        assert_eq!(
            listed(
                "warehouse_identifier",
                "frozen<list<text>>",
                ColumnKind::Regular,
                23
            ),
            ["   warehouse_identifier", "    frozen<list<text>>"]
        );
    }

    #[test]
    fn the_heading_is_centred_and_bold_in_the_colour_of_what_it_names() {
        let theme = Theme::default();
        let details = Details {
            heading: "shop.orders".to_owned(),
            heading_token: Token::Table,
            sections: Vec::new(),
        };

        let heading = lay_out(&details, 0, 28, theme).lines.remove(0);

        assert_eq!(heading.alignment, Some(ratatui::layout::Alignment::Center));
        assert_eq!(heading.style.fg, theme.style(Token::Table).fg);
        assert!(heading.style.add_modifier.contains(Modifier::BOLD));
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
            Theme::default(),
        );
        let accent = Theme::default().style(Token::PropertyName).fg;

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
            Theme::default(),
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
    fn lay_out_reports_the_span_of_the_row_it_highlighted() {
        let details = Details {
            heading: "shop.orders".to_owned(),
            heading_token: Token::Table,
            sections: vec![
                Section {
                    heading: "Partition Key".to_owned(),
                    group: Group::Shape,
                    token: Some(Token::PartitionKey),
                    rows: vec![DetailRow::KeyColumn {
                        name: "customer_id".to_owned(),
                        order: None,
                        cql_type: "uuid".to_owned(),
                    }],
                },
                Section {
                    heading: "Options".to_owned(),
                    group: Group::Settings,
                    token: None,
                    rows: vec![DetailRow::Property {
                        name: "comment".to_owned(),
                        value: "Order history for one customer, spanning several \
                                wrapped lines once it is drawn at this width."
                            .to_owned(),
                    }],
                },
            ],
        };

        let laid = lay_out(&details, 0, 28, Theme::default());

        assert_eq!(
            laid.selection,
            Span { first: 3, lines: 1 },
            "heading, blank, section heading, then the first column"
        );

        let laid = lay_out(&details, details.row_count() - 1, 28, Theme::default());
        assert!(laid.selection.lines > 1, "the comment wraps");
        assert_eq!(
            laid.selection.first + laid.selection.lines,
            laid.lines.len(),
            "the last row's span ends where the body does"
        );
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
