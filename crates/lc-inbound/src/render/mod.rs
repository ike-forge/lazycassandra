//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::geometry::Geometry;
use lc_domain::panel::Panel;
use lc_domain::style::Token;
use lc_domain::view::{OverlayView, PanelBody, PanelTitle, Tab};
use lc_domain::viewport::Span as Selection;
use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::body::Extent;
use crate::hit::{Band, FrameMap, Modal, PanelHit};
use crate::layout::{Shell, layout, queries_height};
use crate::theme::Theme;

/// Half a cell flush with the edge, so the thumb is not a second border.
const THUMB: &str = "▐";

/// `geometry` re-enters the domain, so it must not close a redraw-measure loop
/// (architecture.md §3). `map` re-enters nothing, so it may depend on offsets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    pub geometry: Geometry,
    pub map: FrameMap,
}

/// The domain's only way to learn panel heights and how Ddl wrapped.
pub fn render(frame: &mut Frame, view: &ViewModel, theme: Theme) -> Rendered {
    let ground = theme.ground();
    if ground != Style::new() {
        let area = frame.area();
        frame.buffer_mut().set_style(area, ground);
    }

    let room = Room::of(frame.area());
    let rects = match layout(frame.area(), queries_height(view)) {
        Shell::Panels(rects) => rects,
        Shell::TooSmall(area) => {
            crate::too_small::render(frame, area);
            return Rendered::default();
        }
    };
    // Under a modal the panels are context, drawn without focus marks.
    let dimmed = dims_the_panels(view.overlay.as_ref());
    let beneath = if dimmed { theme.dimmed() } else { theme };
    let mut heights = [0; 4];
    let mut ddl_lines = 0;
    let mut ddl_selection = Selection::default();
    let mut log_lines = 0;
    let mut log_selection = Selection::default();
    let mut grid_width = 0;
    let mut hits: [PanelHit; 4] = std::array::from_fn(|_| PanelHit::default());

    for (index, panel) in view.panels.iter().enumerate() {
        let area = rects.get(panel.panel);

        // No focused panel under a modal; muting only the accent made the last
        // panel the faintest on screen.
        let is_focused_panel = panel.panel == view.focus && !dimmed;
        let (block, border, tabs) = panel_block(panel, area, is_focused_panel, beneath);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Every body kind: the domain needs the width before a page arrives.
        if panel.panel == Panel::Contents {
            grid_width = usize::from(inner.width);
        }

        // Under an overlay no panel shows a cursor.
        let focused = panel.panel == view.focus && view.overlay.is_none();

        let chrome = crate::body::Chrome {
            inner,
            block: area,
            border,
        };
        let body = crate::body::render(
            frame,
            chrome,
            &panel.body,
            panel.search.as_ref(),
            focused,
            panel.activity,
            beneath,
        );

        let columns = body.as_ref().and_then(|drawn| drawn.columns.as_ref());
        bottom_border(frame, area, panel, columns, border, beneath);

        match &body {
            Some(drawn) => {
                heights[index] = drawn.extent.visible;
                if panel.panel == Panel::Ddl {
                    ddl_lines = drawn.extent.total;
                    ddl_selection = drawn.selection;
                }
                if matches!(panel.body, PanelBody::Log { .. }) {
                    log_lines = drawn.extent.total;
                    log_selection = drawn.selection;
                }
                if drawn.extent.overflows() {
                    scrollbar(frame, area, &drawn.extent, border, beneath);
                }
            }
            // `Blank` and `Notice` do not scroll.
            None => heights[index] = usize::from(inner.height),
        }

        // After the scrollbars, which share the border with the loader.
        if let Some(line) = crate::loader::line(panel.activity, beneath)
            && let Some(place) = crate::loader::placement(area)
        {
            frame.render_widget(line, place);
        }

        hits[index] = PanelHit::new(panel.panel, area, inner, tabs, body.as_ref());
    }

    let hint_bar = rects.bar();
    // Under a modal the bar names keys the modal has claimed.
    frame.render_widget(
        crate::hints::line(&view.hints, hint_bar.width, beneath),
        hint_bar,
    );

    let wrapped = render_overlay(frame, view, hint_bar, theme);

    if let Some(toast) = &view.toast {
        crate::toast::render(frame, toast.severity, &toast.message, theme);
    }

    Rendered {
        geometry: Geometry::new(heights, ddl_lines, room.command_rows)
            .with_ddl_selection(ddl_selection)
            .with_log(log_lines, log_selection)
            .with_export_fits(room.export_fits)
            .with_drop_fits(room.drop_fits)
            .with_grid_width(grid_width)
            .with_theme_rows(room.theme_rows)
            .with_connection_rows(room.connection_rows)
            .with_history_rows(room.history_rows)
            .with_health_rows(room.health_rows)
            .with_inspector(
                room.inspector_rows,
                wrapped.inspector_lines,
                wrapped.inspector_selection,
            )
            .with_commit(room.commit_rows, wrapped.commit_lines),
        map: FrameMap::new(hits).with_modal(wrapped.modal),
    }
}

/// Every field a function of terminal size alone (architecture.md §3).
struct Room {
    command_rows: usize,
    theme_rows: usize,
    connection_rows: usize,
    history_rows: usize,
    health_rows: usize,
    export_fits: bool,
    drop_fits: bool,
    inspector_rows: usize,
    commit_rows: usize,
}

impl Room {
    fn of(area: Rect) -> Self {
        Self {
            command_rows: crate::palette::visible_rows(area, lc_domain::commands::row_count(None)),
            theme_rows: crate::themes::visible_rows(area),
            connection_rows: crate::connections::visible_rows(area),
            history_rows: crate::history::visible_rows(area),
            health_rows: crate::health::visible_rows(area),
            export_fits: crate::export::fits(area),
            drop_fits: crate::drop::fits(area),
            inspector_rows: crate::inspect::visible_rows(area),
            commit_rows: crate::commit::visible_rows(area),
        }
    }
}

/// Zero for the overlays that wrap nothing.
#[derive(Default)]
struct Wrapped {
    inspector_lines: usize,
    inspector_selection: Selection,
    commit_lines: usize,
    modal: Option<Modal>,
}

fn render_overlay(frame: &mut Frame, view: &ViewModel, hint_bar: Rect, theme: Theme) -> Wrapped {
    match &view.overlay {
        Some(OverlayView::Message(message) | OverlayView::Fatal(message)) => {
            crate::overlay::render(frame, message, theme);
        }
        Some(OverlayView::Commands {
            rows,
            selected,
            sift,
        }) => {
            crate::palette::render(
                frame,
                rows,
                lc_domain::commands::row_count(None),
                *selected,
                sift.as_ref().and_then(|sift| sift.search.as_ref()),
                theme,
            );
            render_sift(frame, hint_bar, sift.as_ref(), view, theme);
        }
        // A prompt is a text field on the hint bar row, not a modal.
        Some(OverlayView::Search { query }) => {
            render_prompt(frame, hint_bar, query, view, theme);
        }
        Some(OverlayView::Themes {
            labels,
            total,
            selected,
            sift,
        }) => {
            crate::themes::render(
                frame,
                labels,
                *total,
                *selected,
                sift.as_ref().and_then(|sift| sift.search.as_ref()),
                theme,
            );
            render_sift(frame, hint_bar, sift.as_ref(), view, theme);
        }
        Some(OverlayView::Connections {
            rows,
            matched,
            selected,
            sift,
            config_home,
        }) => {
            crate::connections::render(
                frame,
                rows,
                matched,
                *selected,
                sift.as_ref().and_then(|sift| sift.search.as_ref()),
                config_home.as_deref(),
                theme,
            );
            render_sift(frame, hint_bar, sift.as_ref(), view, theme);
        }
        Some(OverlayView::History {
            labels,
            selected,
            sift,
        }) => {
            crate::history::render(
                frame,
                labels,
                *selected,
                sift.as_ref().and_then(|sift| sift.search.as_ref()),
                theme,
            );
            render_sift(frame, hint_bar, sift.as_ref(), view, theme);
        }
        Some(OverlayView::Diagnostics { rows, selected }) => {
            crate::diagnostics::render(frame, rows, *selected, theme);
        }
        Some(OverlayView::Filter { column, query }) => {
            crate::filter::render(frame, column, query, theme);
        }
        Some(
            OverlayView::Drop(_)
            | OverlayView::Export(_)
            | OverlayView::Health(_)
            | OverlayView::Inspect(_)
            | OverlayView::Commit(_)
            | OverlayView::Discard { .. },
        ) => return render_measured(frame, view, theme),
        None => {}
    }

    Wrapped::default()
}

fn render_measured(frame: &mut Frame, view: &ViewModel, theme: Theme) -> Wrapped {
    let pressed = view.pressed;

    match &view.overlay {
        Some(OverlayView::Drop(view)) => Wrapped {
            modal: crate::drop::render(frame, view, pressed, theme),
            ..Wrapped::default()
        },
        Some(OverlayView::Export(view)) => Wrapped {
            modal: crate::export::render(frame, view, pressed, theme),
            ..Wrapped::default()
        },
        Some(OverlayView::Health(view)) => Wrapped {
            modal: crate::health::render(frame, view, theme),
            ..Wrapped::default()
        },
        Some(OverlayView::Inspect(view)) => {
            let (inspector_lines, inspector_selection) = crate::inspect::render(frame, view, theme);

            Wrapped {
                inspector_lines,
                inspector_selection,
                ..Wrapped::default()
            }
        }
        Some(OverlayView::Commit(view)) => {
            let (commit_lines, modal) = crate::commit::render(frame, view, pressed, theme);

            Wrapped {
                commit_lines,
                modal,
                ..Wrapped::default()
            }
        }
        Some(OverlayView::Discard { message }) => Wrapped {
            modal: crate::commit::render_discard(frame, message, pressed, theme),
            ..Wrapped::default()
        },
        _ => Wrapped::default(),
    }
}

/// Clamped, so a wide query cannot split past the rect.
fn split_left(rect: Rect, taken: u16) -> (Rect, Rect) {
    let taken = taken.min(rect.width);

    (
        Rect {
            width: taken,
            ..rect
        },
        Rect {
            x: rect.x + taken,
            width: rect.width - taken,
            ..rect
        },
    )
}

/// Spans `block`'s borders: a rule short of the frame reads as content. Painted
/// to the buffer to reach the border columns, before any scrollbar.
pub(crate) fn rule(frame: &mut Frame, block: Rect, y: u16, style: Style) {
    for x in block.x..block.right() {
        let symbol = if x == block.x {
            symbols::line::VERTICAL_RIGHT
        } else if x == block.right() - 1 {
            symbols::line::VERTICAL_LEFT
        } else {
            symbols::line::HORIZONTAL
        };

        if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
            cell.set_symbol(symbol).set_style(style);
        }
    }
}

/// Not the search prompt, export form or filter: each refers to the panel
/// beneath, which must keep drawing what it is about.
fn dims_the_panels(overlay: Option<&OverlayView>) -> bool {
    match overlay {
        Some(
            OverlayView::Message(_)
            | OverlayView::Fatal(_)
            | OverlayView::Commands { .. }
            | OverlayView::Themes { .. }
            | OverlayView::Connections { .. }
            | OverlayView::History { .. }
            | OverlayView::Diagnostics { .. }
            | OverlayView::Health(_)
            // A drop names its own subject.
            | OverlayView::Drop(_)
            // Copied out of the grid; the title says where from.
            | OverlayView::Inspect(_)
            // Both count their own edits.
            | OverlayView::Commit(_)
            | OverlayView::Discard { .. },
        ) => true,
        Some(OverlayView::Search { .. } | OverlayView::Export(_) | OverlayView::Filter { .. })
        | None => false,
    }
}

fn panel_block(
    panel: &lc_domain::view::PanelView,
    area: Rect,
    is_focused_panel: bool,
    theme: Theme,
) -> (Block<'_>, Style, Vec<Band<usize>>) {
    let border = if is_focused_panel {
        theme.focus_border()
    } else {
        theme.border_style()
    };
    let prefix = panel.title.prefix();
    // Dropped whole when it does not fit: a truncated term lies about what is
    // applied. On every panel that has one, focused or not.
    let marker = panel
        .search
        .as_ref()
        .map(|search| format!("/{}", search.as_str()));
    let (tabs, drawn) = drawn_tabs(&panel.title, area.width, marker.as_deref());
    let form = drawn.strip;
    let strip = strip(&prefix, &tabs, form);
    let marker = marker.filter(|_| drawn.marker);
    let (lead, trail) = insets(area.width, &strip, marker.as_deref());

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(panel_title(
            &prefix,
            &tabs,
            form,
            is_focused_panel,
            theme,
            border,
            lead,
        ));

    if let Some(marker) = marker {
        block = block.title(
            Line::from(vec![
                Span::styled(
                    format!("{TITLE_PAD}{marker}{TITLE_PAD}"),
                    theme.style(Token::Muted),
                ),
                inset(trail, border),
            ])
            .right_aligned(),
        );
    }

    (block, border, tab_bands(&prefix, &tabs, form, lead, area))
}

/// Measured where the strip is built. Clipped at the corner: ratatui draws an
/// over-wide strip short.
fn tab_bands(
    prefix: &str,
    tabs: &[&Tab],
    form: Strip,
    lead: usize,
    area: Rect,
) -> Vec<Band<usize>> {
    let limit = area.right().saturating_sub(1);
    let mut start = area.x.saturating_add(1).saturating_add(columns(
        lead + TITLE_PAD.chars().count() + prefix.chars().count(),
    ));

    let mut bands = Vec::new();

    for (index, tab) in tabs.iter().enumerate() {
        if index > 0 {
            start = start.saturating_add(columns(lc_domain::view::TAB_SEPARATOR.chars().count()));
        }

        let width = columns(match form {
            Strip::Short => tab.short().chars().count(),
            Strip::Full | Strip::Active => tab.text().chars().count(),
        });
        let end = start.saturating_add(width).min(limit);

        if start < end {
            bands.push(Band {
                start,
                end,
                names: index,
            });
        }

        start = start.saturating_add(width);
    }

    bands
}

/// Tab names are ASCII; one wider than `u16` cannot have been drawn.
fn columns(characters: usize) -> u16 {
    u16::try_from(characters).unwrap_or(u16::MAX)
}

/// ratatui gives a title no padding.
const TITLE_PAD: &str = " ";

/// Border glyphs, so the gap reads as frame. A ceiling: names come first.
const TITLE_INSET: usize = 2;

/// ratatui draws it under a title; padding with it puts it back.
const BORDER_LINE: char = '─';

/// The left end first, so the title and the marker keep one alignment.
fn insets(width: u16, title: &str, marker: Option<&str>) -> (usize, usize) {
    let spare = usize::from(width).saturating_sub(spent(title, marker));
    let lead = TITLE_INSET.min(spare);

    (lead, TITLE_INSET.min(spare - lead))
}

fn inset(columns: usize, border: Style) -> Span<'static> {
    Span::styled(BORDER_LINE.to_string().repeat(columns), border)
}

/// The prefix shares the active tab's style. Active is `Focused` when focused,
/// else `Muted`; inactive tabs are `Faint`.
fn panel_title<'a>(
    prefix: &str,
    tabs: &[&'a Tab],
    form: Strip,
    is_focused_panel: bool,
    theme: Theme,
    border: Style,
    lead: usize,
) -> Line<'a> {
    let separator = theme.style(Token::Faint);
    let inactive = theme.style(Token::Faint);
    let active = if is_focused_panel {
        theme.style(Token::Focused)
    } else {
        theme.style(Token::Muted)
    };

    let mut spans = vec![
        inset(lead, border),
        Span::raw(TITLE_PAD),
        Span::styled(prefix.to_owned(), active),
    ];

    for (index, tab) in tabs.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(lc_domain::view::TAB_SEPARATOR, separator));
        }

        let style = if tab.active { active } else { inactive };
        let segments = match form {
            Strip::Short => tab.short_segments(),
            Strip::Full | Strip::Active => tab.segments(),
        };

        for segment in segments {
            let run = segment
                .token
                .map_or(style, |token| schema_run(theme.style(token)));

            spans.push(Span::styled(segment.text.clone(), run));
        }
    }

    spans.push(Span::raw(TITLE_PAD));
    Line::from(spans)
}

/// Schema names drop the border's bold: a name is not emphasis.
fn schema_run(style: Style) -> Style {
    style.remove_modifier(Modifier::BOLD)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drawn {
    strip: Strip,
    marker: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strip {
    Full,
    Short,
    Active,
}

/// Each rung gives up less than clipping, which would name a tab no key
/// reaches. The ladder is walked with the marker first: losing standing context
/// beats hiding a filter.
fn drawn_tabs<'a>(
    title: &'a PanelTitle,
    width: u16,
    marker: Option<&str>,
) -> (Vec<&'a Tab>, Drawn) {
    let all: Vec<&Tab> = title.tabs().iter().collect();
    let prefix = title.prefix();

    for beside in [marker, None] {
        for form in [Strip::Full, Strip::Short] {
            if spent(&strip(&prefix, &all, form), beside) <= usize::from(width) {
                return (
                    all,
                    Drawn {
                        strip: form,
                        marker: beside.is_some(),
                    },
                );
            }
        }
    }

    (
        all.into_iter().filter(|tab| tab.active).collect(),
        Drawn {
            strip: Strip::Active,
            marker: false,
        },
    )
}

fn strip(prefix: &str, tabs: &[&Tab], form: Strip) -> String {
    let joined = tabs
        .iter()
        .map(|tab| match form {
            Strip::Short => tab.short(),
            Strip::Full | Strip::Active => tab.text(),
        })
        .collect::<Vec<_>>()
        .join(lc_domain::view::TAB_SEPARATOR);

    format!("{prefix}{joined}")
}

/// The inset is decoration and not counted.
fn spent(title: &str, marker: Option<&str>) -> usize {
    let marker = marker.map_or(0, |marker| {
        marker.chars().count() + 2 * TITLE_PAD.chars().count()
    });

    title.chars().count() + 2 * TITLE_PAD.chars().count() + marker + 2
}

fn render_sift(
    frame: &mut Frame,
    hint_bar: Rect,
    sift: Option<&lc_domain::view::SiftView>,
    view: &ViewModel,
    theme: Theme,
) {
    if let Some(sift) = sift {
        render_prompt(frame, hint_bar, &sift.query, view, theme);
    }
}

/// Composed beside the hints already on `hint_bar`.
fn render_prompt(frame: &mut Frame, hint_bar: Rect, query: &str, view: &ViewModel, theme: Theme) {
    let line = Line::styled(format!(" /{query}"), theme.style(Token::HintAction));
    let taken = u16::try_from(line.width()).unwrap_or(hint_bar.width);
    let (left, right) = split_left(hint_bar, taken);

    frame.render_widget(line, left);
    frame.render_widget(crate::hints::line(&view.hints, right.width, theme), right);
    // Otherwise a full-width query puts the cursor past the edge.
    let cursor_x = (left.x + left.width).min(hint_bar.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, left.y));
}

/// Over the right border, only when something is hidden. Track is the faint
/// border glyph; the thumb takes the border's style. `ScrollbarState` models a
/// selection index, so it gets `total - visible` as content length, or the
/// thumb stops short of the bottom. `pub(crate)` for the palette.
pub(crate) fn scrollbar(
    frame: &mut Frame,
    area: Rect,
    extent: &Extent,
    border: Style,
    theme: Theme,
) {
    bar(
        frame,
        area,
        extent,
        ScrollbarOrientation::VerticalRight,
        border,
        theme,
    );
}

fn bottom_border(
    frame: &mut Frame,
    area: Rect,
    panel: &lc_domain::view::PanelView,
    columns: Option<&Extent>,
    border: Style,
    theme: Theme,
) {
    let limit = crate::loader::placement(area).map_or(area.right().saturating_sub(1), |place| {
        place.x.saturating_sub(crate::loader::INSET)
    });
    let start = panel
        .footer
        .as_ref()
        .map_or(area.x.saturating_add(1), |footer| {
            crate::footer::render(frame, area, limit, footer, theme)
        });

    if let Some(columns) = columns.filter(|columns| columns.overflows()) {
        let track = Rect {
            x: start,
            y: area.bottom().saturating_sub(1),
            width: limit.saturating_sub(start),
            height: 1,
        };
        horizontal_scrollbar(frame, track, columns, border, theme);
    }
}

/// Only when a column is off screen.
fn horizontal_scrollbar(
    frame: &mut Frame,
    track: Rect,
    extent: &Extent,
    border: Style,
    theme: Theme,
) {
    bar(
        frame,
        track,
        extent,
        ScrollbarOrientation::HorizontalBottom,
        border,
        theme,
    );
}

fn bar(
    frame: &mut Frame,
    area: Rect,
    extent: &Extent,
    orientation: ScrollbarOrientation,
    border: Style,
    theme: Theme,
) {
    // `▐` beside a mid-height `─` reads as vertical bars.
    let (track, thumb, margin) = match orientation {
        // The caller has already cut the track off the border.
        ScrollbarOrientation::HorizontalBottom | ScrollbarOrientation::HorizontalTop => (
            symbols::line::HORIZONTAL,
            symbols::line::THICK_HORIZONTAL,
            Margin {
                horizontal: 0,
                vertical: 0,
            },
        ),
        _ => (
            symbols::line::VERTICAL,
            THUMB,
            Margin {
                horizontal: 0,
                vertical: 1,
            },
        ),
    };
    let mut state =
        ScrollbarState::new(extent.total.saturating_sub(extent.visible)).position(extent.position);

    frame.render_stateful_widget(
        // No arrows: they would land on the block's corners.
        Scrollbar::new(orientation)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some(track))
            .track_style(theme.style(Token::Faint))
            .thumb_symbol(thumb)
            .thumb_style(border),
        area.inner(margin),
        &mut state,
    );
}

#[cfg(test)]
mod tests;
