//! Drawing a [`Details`] body about twenty columns wide. Names are never
//! abbreviated and values never truncated: there is no way to expand them, so
//! lines wrap instead. Widths count `char`s; a wide character misaligns by a
//! column, which ratatui clips.

use lc_domain::grid::BAR;
use lc_domain::model::{ClusteringOrder, Setting};
use lc_domain::search::Search;
use lc_domain::style::Token;
use lc_domain::view::{ColumnKind, DetailRow, Details, Group, IdentityKind};
use lc_domain::viewport::Span;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span as TextSpan};
use ratatui::widgets::Paragraph;

use crate::body::{Marked, barred, patched};
use crate::theme::Theme;

const ASCENDING: &str = " ↑";
const DESCENDING: &str = " ↓";
const INDENT: usize = 2;
/// A wrapped value falls to its own line, one step in from the name.
const TYPE_INDENT: usize = 4;
const ELLIPSIS: char = '…';
const BULLET: &str = "· ";

pub(crate) struct Laid {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selection: Span,
    /// The inverse of [`Laid::selection`]: rows here span several lines, so a
    /// press cannot name one by arithmetic.
    pub(crate) rows: Vec<Option<usize>>,
}

#[derive(Clone, Copy)]
pub(crate) struct DetailsView<'a> {
    pub(crate) details: &'a Details,
    pub(crate) selected: usize,
    pub(crate) offset: usize,
    pub(crate) marked: &'a Marked,
    pub(crate) search: Option<&'a Search>,
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &DetailsView,
    theme: Theme,
) -> (crate::body::Extent, Span, Vec<Option<usize>>) {
    let DetailsView {
        details,
        selected,
        offset,
        marked,
        search,
    } = *view;

    let laid = lay_out(
        details,
        selected,
        marked,
        usize::from(area.width),
        search,
        theme,
    );
    let total = laid.lines.len();
    // `Paragraph::scroll` is u16; the extent keeps the true offset, so the
    // thumb stays right.
    let scroll = u16::try_from(offset).unwrap_or(u16::MAX);

    frame.render_widget(Paragraph::new(laid.lines).scroll((scroll, 0)), area);

    (
        crate::body::Extent {
            position: offset,
            total,
            visible: usize::from(area.height),
        },
        laid.selection,
        laid.rows,
    )
}

/// Measured over every line, never only those scrolled into view: an offset
/// must not feed back into the measurement (architecture.md §3).
pub(crate) fn lay_out(
    details: &Details,
    selected: usize,
    marked: &Marked,
    width: usize,
    search: Option<&Search>,
    theme: Theme,
) -> Laid {
    // Every row reserves the bar's column so nothing shifts; a heading is no
    // cursor stop and keeps it.
    let content = width.saturating_sub(usize::from(BAR));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut rows: Vec<Option<usize>> = Vec::new();
    let mut selection = Span::default();
    let mut row = 0;
    let mut previous: Option<Group> = None;

    for section in &details.sections {
        // One blank line between the panel's parts.
        if previous.is_some_and(|group| group != section.group) {
            lines.push(Line::raw(""));
            rows.push(None);
        }
        previous = Some(section.group);

        let style = section
            .token
            .map_or_else(Style::new, |token| theme.style(token));

        if let Some(heading) = &section.heading {
            let icon = crate::icons::section(heading.token, section.group);
            lines.push(Line::styled(
                truncate(&prefixed(icon, &heading.text, width), width),
                theme.style(heading.token),
            ));
            rows.push(None);
        }

        for detail in &section.rows {
            let mut drawn = row_lines(detail, content, style, search, theme);

            // Band first, cursor highlight over it: the anchor is both cursor
            // and part of what was yanked.
            if marked.holds(row) {
                let band = theme.style(marked.band);
                drawn = drawn.into_iter().map(|line| patched(line, band)).collect();
            }

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

            rows.extend(std::iter::repeat_n(Some(row), drawn.len()));
            lines.extend(drawn);
            row += 1;
        }
    }

    let lines = lines
        .into_iter()
        .zip(&rows)
        .map(|(line, row)| match row {
            Some(row) => barred(line, *row == selected || marked.holds(*row), theme),
            None => line,
        })
        .collect();

    Laid {
        lines,
        selection,
        rows,
    }
}

/// The icon is dropped before the name is abbreviated.
fn prefixed(icon: &str, text: &str, width: usize) -> String {
    if icon.chars().count() + text.chars().count() > width {
        return text.to_owned();
    }

    format!("{icon}{text}")
}

fn row_lines(
    row: &DetailRow,
    width: usize,
    style: Style,
    search: Option<&Search>,
    theme: Theme,
) -> Vec<Line<'static>> {
    match row {
        DetailRow::KeyColumn {
            name,
            order,
            cql_type,
        } => key_column_lines(name, *order, cql_type, width)
            .into_iter()
            .map(|text| Line::from(crate::highlight::spans(&text, search, style, theme)))
            .collect(),
        DetailRow::Column {
            name,
            cql_type,
            kind,
        } => column_lines(name, cql_type, *kind, width, search, theme),
        DetailRow::Property { name, value } => {
            property_lines(name, value, width, style, search, theme)
        }
        DetailRow::ClassProperty {
            name,
            class,
            settings,
            ..
        } => class_property_lines(name, class, settings, width, style, search, theme),
        DetailRow::Identity { kind, name } => identity_lines(*kind, name, width, search, theme),
    }
}

/// Shaped like a key column. Both runs wear the tree's colour for that kind: a
/// schema name reads as one wherever it is written.
fn identity_lines(
    kind: IdentityKind,
    name: &str,
    width: usize,
    search: Option<&Search>,
    theme: Theme,
) -> Vec<Line<'static>> {
    let icon = crate::icons::identity(kind.token());
    let label = format!(
        "{}{}",
        " ".repeat(INDENT),
        prefixed(icon, kind.label(), width.saturating_sub(INDENT))
    );
    let taken = count(&label);

    laid_out(&label, name, width)
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            let style = theme.style(kind.token());

            if index == 0 {
                marked(&text, taken, style, search, theme)
            } else {
                Line::from(crate::highlight::spans(&text, search, style, theme))
            }
        })
        .collect()
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

/// The whole row wears the kind's colour: the glyph only says key or not.
fn column_lines(
    name: &str,
    cql_type: &str,
    kind: ColumnKind,
    width: usize,
    search: Option<&Search>,
    theme: Theme,
) -> Vec<Line<'static>> {
    let marker = if kind.is_key() {
        crate::icons::KEY
    } else {
        &" ".repeat(INDENT)
    };
    let style = theme.style(kind.token());

    laid_out(&format!("{marker}{name}"), cql_type, width)
        .into_iter()
        .map(|text| Line::from(crate::highlight::spans(&text, search, style, theme)))
        .collect()
}

/// Labels are never matched.
fn marked(
    text: &str,
    taken: usize,
    style: Style,
    search: Option<&Search>,
    theme: Theme,
) -> Line<'static> {
    let mark: String = text.chars().take(taken).collect();
    let rest: String = text.chars().skip(taken).collect();

    let mut spans = vec![TextSpan::styled(mark, style)];
    spans.extend(crate::highlight::spans(&rest, search, style, theme));
    Line::from(spans)
}

fn laid_out(left: &str, right: &str, width: usize) -> Vec<String> {
    match gap(left, right, width) {
        Some(gap) => vec![format!("{left}{}{right}", " ".repeat(gap))],
        None => std::iter::once(truncate(left, width))
            .chain(crate::wrap::plain(right, width, TYPE_INDENT))
            .collect(),
    }
}

/// Per-row margin, not a section column: one long name would push every value
/// past the edge.
fn property_lines(
    name: &str,
    value: &str,
    width: usize,
    style: Style,
    search: Option<&Search>,
    theme: Theme,
) -> Vec<Line<'static>> {
    let label = theme.style(Token::PropertyName);

    if value.is_empty() {
        return vec![name_line(name, width, search, label, theme)];
    }

    if let Some(gap) = gap_after_name(name, value, width) {
        let mut spans = name_spans(name, search, label, theme);
        spans.push(TextSpan::styled(" ".repeat(gap), style));
        spans.extend(crate::highlight::spans(value, search, style, theme));
        return vec![Line::from(spans)];
    }

    std::iter::once(name_line(name, width, search, label, theme))
        .chain(
            crate::wrap::plain(value, width, TYPE_INDENT)
                .into_iter()
                .map(|text| Line::from(crate::highlight::spans(&text, search, style, theme))),
        )
        .collect()
}

fn class_property_lines(
    name: &str,
    class: &str,
    settings: &[Setting],
    width: usize,
    style: Style,
    search: Option<&Search>,
    theme: Theme,
) -> Vec<Line<'static>> {
    let label = theme.style(Token::PropertyName);
    let faint = theme.style(Token::Faint);
    let tail = settings
        .iter()
        .map(|setting| format!("{BULLET}{} {}", setting.label, setting.value))
        .collect::<Vec<String>>()
        .join(" ");

    let value = format!("{class}{}", tail_at(&tail));

    if let Some(gap) = gap_after_name(name, &value, width) {
        let mut spans = name_spans(name, search, label, theme);
        spans.push(TextSpan::styled(" ".repeat(gap), style));
        spans.extend(crate::highlight::spans(class, search, style, theme));
        spans.extend(crate::highlight::spans(
            &tail_at(&tail),
            search,
            faint,
            theme,
        ));
        return vec![Line::from(spans)];
    }

    // Class and settings wrap as separate blocks: a wrapped line carries one
    // style.
    std::iter::once(name_line(name, width, search, label, theme))
        .chain(highlighted(
            crate::wrap::plain(class, width, TYPE_INDENT),
            search,
            style,
            theme,
        ))
        .chain(highlighted(
            crate::wrap::plain(&tail, width, TYPE_INDENT),
            search,
            faint,
            theme,
        ))
        .collect()
}

/// One step in from the heading, where a key column's name sits.
fn name_spans(
    name: &str,
    search: Option<&Search>,
    label: Style,
    theme: Theme,
) -> Vec<TextSpan<'static>> {
    let mut spans = vec![TextSpan::styled(" ".repeat(INDENT), label)];
    spans.extend(crate::highlight::spans(name, search, label, theme));
    spans
}

fn name_line(
    name: &str,
    width: usize,
    search: Option<&Search>,
    label: Style,
    theme: Theme,
) -> Line<'static> {
    let mut spans = vec![TextSpan::styled(" ".repeat(INDENT), label)];
    spans.extend(crate::highlight::spans(
        &truncate(name, width.saturating_sub(INDENT)),
        search,
        label,
        theme,
    ));
    Line::from(spans)
}

fn gap_after_name(name: &str, value: &str, width: usize) -> Option<usize> {
    gap(&format!("{}{name}", " ".repeat(INDENT)), value, width)
}

fn highlighted(
    texts: Vec<String>,
    search: Option<&Search>,
    style: Style,
    theme: Theme,
) -> impl Iterator<Item = Line<'static>> {
    texts
        .into_iter()
        .map(move |text| Line::from(crate::highlight::spans(&text, search, style, theme)))
}

/// Empty where there are no settings.
fn tail_at(tail: &str) -> String {
    if tail.is_empty() {
        String::new()
    } else {
        format!(" {tail}")
    }
}

/// `None` when the two cannot share a line with a gap.
fn gap(left: &str, right: &str, width: usize) -> Option<usize> {
    match width.checked_sub(count(left) + count(right)) {
        Some(gap) if gap > 0 => Some(gap),
        _ => None,
    }
}

/// The one place `…` appears: a single token wider than the panel.
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
    use lc_domain::selection::{Range, Ranges};
    use lc_domain::style::Token;
    use lc_domain::view::{Heading, IdentityKind, Section};

    use super::*;

    fn texts(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(ToString::to_string).collect()
    }

    fn property(name: &str, value: &str, width: usize) -> Vec<String> {
        texts(&property_lines(
            name,
            value,
            width,
            Style::new(),
            None,
            Theme::default(),
        ))
    }

    fn class_property(
        name: &str,
        class: &str,
        settings: &[(&'static str, &str)],
        width: usize,
    ) -> Vec<String> {
        let settings: Vec<Setting> = settings
            .iter()
            .map(|(label, value)| Setting {
                label,
                value: (*value).to_owned(),
            })
            .collect();

        texts(&class_property_lines(
            name,
            class,
            &settings,
            width,
            Style::new(),
            None,
            Theme::default(),
        ))
    }

    #[test]
    fn a_class_property_puts_its_class_and_settings_at_the_right_margin() {
        assert_eq!(
            class_property("compression", "LZ4Compressor", &[("chunk", "16kb")], 40),
            ["  compression LZ4Compressor · chunk 16kb"],
            "the whole value is pushed right, the way a plain property's is"
        );
    }

    #[test]
    fn a_class_property_too_wide_for_one_line_drops_its_settings_beneath_the_class() {
        assert_eq!(
            class_property(
                "compaction",
                "SizeTieredCompactionStrategy",
                &[("max", "32"), ("min", "4")],
                28,
            ),
            [
                "  compaction",
                "    SizeTieredCompactionStra",
                "    tegy",
                "    · max 32 · min 4",
            ],
        );
    }

    #[test]
    fn a_class_with_no_settings_draws_the_class_alone() {
        assert_eq!(
            class_property("compression", "SnappyCompressor", &[], 30),
            ["  compression SnappyCompressor"],
        );
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
        texts(&column_lines(
            name,
            cql_type,
            kind,
            width,
            None,
            Theme::default(),
        ))
    }

    #[test]
    fn a_listed_key_column_wears_the_key_glyph_where_the_other_tab_indents() {
        assert_eq!(
            listed("customer_id", "uuid", ColumnKind::Partition, 23),
            [format!("{}customer_id      uuid", crate::icons::KEY)]
        );
        assert_eq!(
            listed("order_date", "date", ColumnKind::Clustering, 23),
            [format!("{}order_date       date", crate::icons::KEY)],
            "the same glyph: which key it is is what the colour says"
        );
        assert_eq!(
            listed("region", "text", ColumnKind::Static, 23),
            ["  region           text"]
        );
        assert_eq!(
            listed("status", "text", ColumnKind::Regular, 23),
            ["  status           text"],
            "a column outside the key earns no glyph, and the names still line up"
        );
    }

    #[test]
    fn a_listed_column_carries_its_kinds_colour_across_the_whole_row() {
        let theme = Theme::default();

        for (kind, token) in [
            (ColumnKind::Partition, Token::PartitionKey),
            (ColumnKind::Clustering, Token::ClusteringKey),
            (ColumnKind::Static, Token::StaticColumn),
            (ColumnKind::Regular, Token::RegularColumn),
        ] {
            let lines = column_lines("customer_id", "uuid", kind, 23, None, theme);
            let colour = theme.style(token).fg;

            assert!(
                lines[0].spans.iter().all(|span| span.style.fg == colour),
                "{kind:?} must read as its own kind end to end"
            );
        }
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
            ["  warehouse_identifier", "    frozen<list<text>>"]
        );
    }

    fn named(kind: IdentityKind, name: &str, width: usize) -> Vec<String> {
        texts(&identity_lines(kind, name, width, None, Theme::default()))
    }

    #[test]
    fn an_identity_row_puts_the_name_at_the_margin_behind_a_labelled_icon() {
        assert_eq!(
            named(IdentityKind::Keyspace, "shop", 23),
            [format!("  {}Keyspace       shop", crate::icons::KEYSPACE)]
        );
        assert_eq!(
            named(IdentityKind::Table, "orders", 23),
            [format!("  {}Table        orders", crate::icons::TABLE)]
        );
    }

    #[test]
    fn a_name_too_wide_to_share_the_label_line_wraps_beneath_it() {
        assert_eq!(
            named(IdentityKind::Table, "orders_by_customer", 23),
            [
                format!("  {}Table", crate::icons::TABLE),
                "    orders_by_customer".to_owned()
            ]
        );
    }

    #[test]
    fn an_identity_row_carries_the_kinds_colour_over_both_its_halves() {
        let theme = Theme::default();
        let lines = identity_lines(IdentityKind::Keyspace, "shop", 23, None, theme);
        let keyspace = theme.style(Token::Keyspace).fg;

        assert!(keyspace.is_some(), "the token has to spend a colour");
        assert_eq!(
            lines[0].spans[0].content,
            format!("  {}Keyspace", crate::icons::KEYSPACE)
        );

        for span in &lines[0].spans {
            assert_eq!(
                span.style.fg, keyspace,
                "{:?} lost the colour",
                span.content
            );
        }
    }

    #[test]
    fn a_wrapped_identity_name_keeps_the_kinds_colour() {
        let theme = Theme::default();
        let lines = identity_lines(IdentityKind::Table, "orders_by_customer", 23, None, theme);
        let table = theme.style(Token::Table).fg;

        assert!(lines.len() > 1, "the name has to have wrapped");
        for line in &lines {
            for span in &line.spans {
                assert_eq!(span.style.fg, table, "{:?} lost the colour", span.content);
            }
        }
    }

    #[test]
    fn a_panel_too_narrow_for_the_icon_keeps_the_whole_label() {
        assert_eq!(
            named(IdentityKind::Keyspace, "shop", 9),
            ["  Keyspa…", "    shop"],
            "the icon goes before a label this panel promises to draw whole"
        );
    }

    #[test]
    fn a_property_short_enough_for_one_line_puts_its_value_at_the_margin() {
        assert_eq!(
            property("gc_grace_seconds", "864000", 25),
            ["  gc_grace_seconds 864000"]
        );
        assert_eq!(property("cdc", "false", 25), ["  cdc               false"]);
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
            ["  gc_grace_seconds", "    864000"]
        );
    }

    #[test]
    fn a_property_name_is_the_only_part_of_its_row_that_is_highlighted() {
        let one_line = property_lines(
            "gc_grace_seconds",
            "864000",
            25,
            Style::new(),
            None,
            Theme::default(),
        );
        let accent = Theme::default().style(Token::PropertyName).fg;

        assert!(accent.is_some(), "the token has to spend a colour");
        assert_eq!(one_line[0].spans[1].style.fg, accent);
        assert_eq!(
            one_line[0].spans[3].style.fg, None,
            "the value is left alone: two treatments on one narrow line is one too many"
        );

        // The wrapped shape marks the same way.
        let wrapped = property_lines(
            "comment",
            "Order history for one customer.",
            23,
            Style::new(),
            None,
            Theme::default(),
        );
        assert_eq!(wrapped[0].spans[1].style.fg, accent);
        assert_eq!(wrapped[1].spans[0].style.fg, None);
    }

    #[test]
    fn a_long_value_wraps_rather_than_losing_its_tail() {
        assert_eq!(
            property("comment", "Order history for one customer.", 23),
            ["  comment", "    Order history for", "    one customer."]
        );
    }

    #[test]
    fn a_match_a_wrap_split_is_painted_on_neither_half() {
        // "for one" straddles the wrap, so no fragment holds the whole term.
        let theme = Theme::default();
        let search = Search::new("for one").expect("non-empty");
        let style = theme.style(Token::Faint);

        let lines = property_lines(
            "comment",
            "Order history for one customer.",
            23,
            style,
            Some(&search),
            theme,
        );

        assert_eq!(
            texts(&lines),
            ["  comment", "    Order history for", "    one customer."],
            "the wrap itself does not move for a search"
        );

        let matched = theme.style(Token::Match).fg;
        for line in &lines[1..] {
            assert!(
                line.spans.iter().all(|span| span.style.fg != matched),
                "a match a wrap split in half is painted on neither half: {line:?}"
            );
        }
    }

    #[test]
    fn a_name_wider_than_the_panel_is_the_one_thing_that_gets_an_ellipsis() {
        assert_eq!(
            property("memtable_flush_period_in_ms", "0", 20),
            ["  memtable_flush_pe…", "    0"]
        );
    }

    #[test]
    fn a_panel_with_no_room_for_a_value_still_names_the_property() {
        assert_eq!(property("cdc", "false", 4), ["  c…"]);
    }

    #[test]
    fn a_property_with_an_empty_value_is_just_its_name() {
        assert_eq!(property("comment", "", 23), ["  comment"]);
    }

    #[test]
    fn lay_out_reports_the_span_of_the_row_it_highlighted() {
        let details = Details {
            sections: vec![
                Section {
                    heading: None,
                    group: Group::Identity,
                    token: None,
                    rows: vec![DetailRow::Identity {
                        kind: IdentityKind::Table,
                        name: "orders".to_owned(),
                    }],
                },
                Section {
                    heading: Some(Heading {
                        text: "Partition Key".to_owned(),
                        token: Token::PartitionKey,
                    }),
                    group: Group::Shape,
                    token: Some(Token::PartitionKey),
                    rows: vec![DetailRow::KeyColumn {
                        name: "customer_id".to_owned(),
                        order: None,
                        cql_type: "uuid".to_owned(),
                    }],
                },
                Section {
                    heading: Some(Heading {
                        text: "Options".to_owned(),
                        token: Token::Section,
                    }),
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

        assert_eq!(
            lay_out(&details, 0, &Marked::none(), 28, None, Theme::default()).selection,
            Span { first: 0, lines: 1 },
            "the identity row opens the body, so nothing precedes it"
        );

        let laid = lay_out(&details, 1, &Marked::none(), 28, None, Theme::default());

        assert_eq!(
            laid.selection,
            Span { first: 3, lines: 1 },
            "identity, blank, section heading, then the first column"
        );

        let laid = lay_out(
            &details,
            details.row_count() - 1,
            &Marked::none(),
            28,
            None,
            Theme::default(),
        );
        assert!(laid.selection.lines > 1, "the comment wraps");
        assert_eq!(
            laid.selection.first + laid.selection.lines,
            laid.lines.len(),
            "the last row's span ends where the body does"
        );
    }

    fn two_column_details() -> Details {
        Details {
            sections: vec![Section {
                heading: Some(Heading {
                    text: "Columns".to_owned(),
                    token: Token::Section,
                }),
                group: Group::Shape,
                token: None,
                rows: vec![
                    DetailRow::Column {
                        name: "id".to_owned(),
                        cql_type: "uuid".to_owned(),
                        kind: ColumnKind::Partition,
                    },
                    DetailRow::Column {
                        name: "email".to_owned(),
                        cql_type: "text".to_owned(),
                        kind: ColumnKind::Regular,
                    },
                ],
            }],
        }
    }

    #[test]
    fn the_cursor_row_inside_a_range_keeps_the_band_under_its_highlight() {
        let theme = Theme::default();
        let marked = Marked {
            ranges: Ranges::of(Range::new(0, 1)),
            band: Token::Flash,
        };

        let laid = lay_out(&two_column_details(), 0, &marked, 28, None, theme);

        let flash = theme.style(Token::Flash);
        let selected = theme.style(Token::Selected);
        let cursor = &laid.lines[1];

        assert!(
            cursor.style.add_modifier.contains(
                flash
                    .add_modifier
                    .iter()
                    .next()
                    .expect("the flash is a modifier")
            ),
            "the anchor lost the afterglow the rest of the range kept"
        );
        assert_eq!(cursor.style.bg, selected.bg, "and still says where it is");
    }

    #[test]
    fn a_heading_naming_no_schema_concept_is_the_only_part_of_its_group_in_the_section_colour() {
        let theme = Theme::default();
        let details = Details {
            sections: vec![Section {
                heading: Some(Heading {
                    text: "Options".to_owned(),
                    token: Token::Section,
                }),
                group: Group::Settings,
                token: None,
                rows: vec![DetailRow::Property {
                    name: "gc_grace_seconds".to_owned(),
                    value: "864000".to_owned(),
                }],
            }],
        };

        let laid = lay_out(&details, usize::MAX, &Marked::none(), 28, None, theme);
        let section = theme.style(Token::Section);

        assert_eq!(laid.lines[0].style, section);
        assert!(
            laid.lines[1]
                .spans
                .iter()
                .all(|span| span.style.fg != section.fg),
            "the section colour must not bleed onto the rows"
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
