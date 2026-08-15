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

use lc_domain::grid::BAR;
use lc_domain::model::{ClusteringOrder, Setting};
use lc_domain::search::Search;
use lc_domain::state::Span;
use lc_domain::style::Token;
use lc_domain::view::{ColumnKind, DetailRow, Details, Group, IdentityKind};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span as TextSpan};
use ratatui::widgets::Paragraph;

use crate::body::Marked;
use crate::theme::Theme;

const ASCENDING: &str = " ↑";
const DESCENDING: &str = " ↓";
const INDENT: usize = 2;
/// A column's type falls to its own line under the name, one step further in
/// than a property's value so the two shapes stay distinguishable.
const TYPE_INDENT: usize = 4;
const ELLIPSIS: char = '…';
/// What separates one setting of a class-valued property from the next.
const BULLET: &str = "· ";

/// A body's lines, and where the selected row landed among them.
pub(crate) struct Laid {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) selection: Span,
    /// The row each line was wrapped from, in step with `lines`. `None` for the
    /// lines that belong to no row: the blanks between groups, and the section
    /// headings.
    ///
    /// The inverse of [`Laid::selection`], and needed for the same reason it
    /// is: this is the one panel whose rows do not stand one to a line, so a
    /// press cannot name a row by arithmetic on the line it landed on.
    pub(crate) rows: Vec<Option<usize>>,
}

/// The view model's Ddl body, as the renderer reads it.
///
/// A struct rather than five more parameters, the same reason `body.rs`'s
/// `GridView` and `TreeView` exist.
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
        laid.rows,
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
pub(crate) fn lay_out(
    details: &Details,
    selected: usize,
    marked: &Marked,
    width: usize,
    search: Option<&Search>,
    theme: Theme,
) -> Laid {
    // Every row reserves the bar's column, the Tables tree's way and the
    // Contents grid's, so nothing shifts sideways as the cursor moves
    // (docs/superpowers/specs/2026-08-10-table-display-design.md §4.2). A
    // heading is no cursor stop, so it opens at the margin and keeps the
    // column for its own text.
    let content = width.saturating_sub(usize::from(BAR));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut rows: Vec<Option<usize>> = Vec::new();
    let mut selection = Span::default();
    let mut row = 0;
    let mut previous: Option<Group> = None;

    for section in &details.sections {
        // The one row spent on separation, and only where it buys a split
        // between two of the panel's parts: what the rows are about, what the
        // table is, what it is configured to do.
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
            } else if marked.holds(row) {
                let band = theme.style(marked.band);
                drawn = drawn.into_iter().map(|line| patched(line, band)).collect();
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

/// `line` behind the bar column, marked where the line was wrapped from a row
/// the mark belongs to — every one of them, since the highlight covers every
/// one. Only a row's lines take it: a heading and a group's blank line cannot
/// be selected, so neither pays the column.
///
/// Prepended after the highlight is patched on, or the bar would be repainted
/// in the selection's own foreground and stop reading as a bar.
fn barred(mut line: Line<'static>, marked: bool, theme: Theme) -> Line<'static> {
    line.spans
        .insert(0, crate::body::selection_bar(marked, theme));
    line
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
/// Every row shape builds its spans through [`crate::highlight::spans`] now,
/// so a matched fragment can carry [`Token::Match`] distinct from the rest of
/// the line — the colour lives on the spans, not on the line as a whole.
/// Patching the line's own style too still matters: ratatui falls back to it
/// past the end of the widths a line's spans account for, which `patched`
/// itself has no way to know.
fn patched(mut line: Line<'static>, style: Style) -> Line<'static> {
    line.style = line.style.patch(style);
    for span in &mut line.spans {
        span.style = span.style.patch(style);
    }
    line
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

/// `Keyspace` or `Table` at the left, the name it carries at the right margin
/// — the shape a key column already uses, so the two read as one table down
/// the panel.
///
/// The label wears the icon and the colour the Tables tree gives that kind of
/// name, and the name itself is left plain: the label is what tells the two
/// rows apart, and two coloured runs on a line this narrow is one too many
/// (docs/superpowers/specs/2026-08-09-visual-polish-design.md §5.2).
fn identity_lines(
    kind: IdentityKind,
    name: &str,
    width: usize,
    search: Option<&Search>,
    theme: Theme,
) -> Vec<Line<'static>> {
    let icon = crate::icons::identity(kind.token());
    let label = prefixed(icon, kind.label(), width);
    let taken = count(&label);

    laid_out(&label, name, width)
        .into_iter()
        .enumerate()
        .map(|(index, text)| {
            if index == 0 {
                marked(&text, taken, theme.style(kind.token()), search, theme)
            } else {
                Line::from(crate::highlight::spans(
                    &text,
                    search,
                    Style::default(),
                    theme,
                ))
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

/// A column in the Columns tab: the key glyph in the two columns the other tab
/// spends on indentation, then the name, then the type at the margin.
///
/// The whole row wears the kind's colour rather than a marker alone. The glyph
/// only says whether the column is part of the primary key — which of the two
/// keys, and whether a non-key column is static, is what the colour carries,
/// and it can only carry it by covering the row
/// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §5).
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

/// `text` with its first `taken` characters lifted into a span of their own;
/// the rest split against `search` (the label is never matched — Identity
/// matches only `name`, search highlight design §4.2).
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
    spans.extend(crate::highlight::spans(
        &rest,
        search,
        Style::default(),
        theme,
    ));
    Line::from(spans)
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
    search: Option<&Search>,
    theme: Theme,
) -> Vec<Line<'static>> {
    let label = theme.style(Token::PropertyName);

    if value.is_empty() {
        return vec![Line::from(crate::highlight::spans(
            &truncate(name, width),
            search,
            label,
            theme,
        ))];
    }

    if let Some(gap) = gap(name, value, width) {
        let mut spans = crate::highlight::spans(name, search, label, theme);
        spans.push(TextSpan::styled(" ".repeat(gap), style));
        spans.extend(crate::highlight::spans(value, search, style, theme));
        return vec![Line::from(spans)];
    }

    std::iter::once(Line::from(crate::highlight::spans(
        &truncate(name, width),
        search,
        label,
        theme,
    )))
    .chain(
        wrap(value, width, INDENT)
            .into_iter()
            .map(|text| Line::from(crate::highlight::spans(&text, search, style, theme))),
    )
    .collect()
}

/// A class-valued property: the class where a plain property puts its value,
/// and the settings behind it in the colour that keeps the class the thing the
/// eye lands on.
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

    if let Some(gap) = gap(name, &value, width) {
        let mut spans = crate::highlight::spans(name, search, label, theme);
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

    // The class and the settings wrap as two blocks rather than as one run of
    // text: a wrapped line carries one style, and splitting the class across
    // lines is already the worst this panel does to a name it promises not to
    // abbreviate — colouring half of it faint on top of that would be worse.
    std::iter::once(Line::from(crate::highlight::spans(
        &truncate(name, width),
        search,
        label,
        theme,
    )))
    .chain(highlighted(
        wrap(class, width, INDENT),
        search,
        style,
        theme,
    ))
    .chain(highlighted(
        wrap(&tail, width, INDENT),
        search,
        faint,
        theme,
    ))
    .collect()
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

/// The settings run as it follows the class on a shared line: with the space
/// the class does not carry itself, and empty where there are no settings.
fn tail_at(tail: &str) -> String {
    if tail.is_empty() {
        String::new()
    } else {
        format!(" {tail}")
    }
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
    use lc_domain::view::{Heading, IdentityKind, Section};

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
            ["compression   LZ4Compressor · chunk 16kb"],
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
                "compaction",
                "  SizeTieredCompactionStrate",
                "  gy",
                "  · max 32 · min 4",
            ],
        );
    }

    #[test]
    fn a_class_with_no_settings_draws_the_class_alone() {
        assert_eq!(
            class_property("compression", "SnappyCompressor", &[], 30),
            ["compression   SnappyCompressor"],
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
            [format!("{}Keyspace         shop", crate::icons::KEYSPACE)]
        );
        assert_eq!(
            named(IdentityKind::Table, "orders", 23),
            [format!("{}Table          orders", crate::icons::TABLE)]
        );
    }

    #[test]
    fn a_name_too_wide_to_share_the_label_line_wraps_beneath_it() {
        assert_eq!(
            named(IdentityKind::Table, "orders_by_customer", 23),
            [
                format!("{}Table", crate::icons::TABLE),
                "    orders_by_customer".to_owned()
            ]
        );
    }

    #[test]
    fn only_the_label_of_an_identity_row_carries_the_kinds_colour() {
        let theme = Theme::default();
        let lines = identity_lines(IdentityKind::Keyspace, "shop", 23, None, theme);
        let keyspace = theme.style(Token::Keyspace).fg;

        assert!(keyspace.is_some(), "the token has to spend a colour");
        assert_eq!(
            lines[0].spans[0].content,
            format!("{}Keyspace", crate::icons::KEYSPACE)
        );
        assert_eq!(lines[0].spans[0].style.fg, keyspace);
        assert_eq!(
            lines[0].spans[1].style.fg, None,
            "the name is left alone, as a column's is beside its marker"
        );
    }

    #[test]
    fn a_panel_too_narrow_for_the_icon_keeps_the_whole_label() {
        assert_eq!(
            named(IdentityKind::Keyspace, "shop", 9),
            ["Keyspace", "    shop"],
            "the icon goes before a label this panel promises to draw whole"
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
            None,
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
            None,
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
    fn a_match_a_wrap_split_is_painted_on_neither_half() {
        // "one customer" straddles the wrap boundary: fragment one ends in
        // "one" and fragment two opens with "customer.", so neither drawn
        // fragment holds the whole term (search highlight design §3.1).
        let theme = Theme::default();
        let search = Search::new("one customer").expect("non-empty");
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
            ["comment", "  Order history for one", "  customer."],
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

    /// A heading is coloured by what it names and its rows by what they are.
    /// `Section` on the rows would repaint every option value in the heading's
    /// colour, which is why the two tokens are separate fields.
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
