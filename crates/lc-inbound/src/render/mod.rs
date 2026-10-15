//! Drawing a frame.

use lc_domain::ViewModel;
use lc_domain::state::{Geometry, Panel, Span as Selection};
use lc_domain::style::Token;
use lc_domain::view::{OverlayView, PanelBody, PanelTitle, Tab};
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

/// Half a cell, flush with the panel's outer edge: the thumb marks a position
/// without reading as a bar of its own beside the border it sits on.
const THUMB: &str = "▐";

/// What the frame just drawn turned out to be.
///
/// `geometry` travels into the domain as `Msg::Measured`, so it is guarded
/// against closing a redraw–measure–redraw loop (architecture.md §3). `map`
/// re-enters nothing — nobody reads it until a human moves a mouse — so it is
/// free to depend on the offsets `Geometry` may not
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    pub geometry: Geometry,
    pub map: FrameMap,
}

/// Draw one titled block per panel, and report what the frame turned out to
/// be.
///
/// The return value is the domain's only way to learn how tall a panel is and
/// how many lines the Ddl body wrapped to
/// (docs/superpowers/specs/2026-08-09-panel-scrolling-design.md §3).
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
    // The panels are context while a modal is up, not a place a key would
    // land, so they are drawn without the marks that say otherwise.
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

        // Not "the focused panel, drawn quietly": while a modal is up there
        // is no focused panel at all, and this one reads exactly as the other
        // three do. A style that merely muted the accent drew the title in
        // the border's colour, one step *below* the unfocused panels beside
        // it — the panel the user had just been in became the faintest thing
        // on screen.
        let is_focused_panel = panel.panel == view.focus && !dimmed;
        let (block, border, tabs) = panel_block(panel, area, is_focused_panel, beneath);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Every body kind, not only a grid: it is the panel's width, which is
        // what the domain needs in order to decide what would fit once a page
        // arrives (docs/superpowers/specs/2026-08-10-table-display-design.md §6).
        if panel.panel == Panel::Contents {
            grid_width = usize::from(inner.width);
        }

        // An overlay owns the frame: nothing beneath it shows a cursor, or it
        // blinks over the modal at a position that means nothing there.
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
            // `Blank` and `Notice` do not scroll, so what such a panel can
            // show is simply the room it was given.
            None => heights[index] = usize::from(inner.height),
        }

        // After the scrollbars, which run the length of the borders the
        // loader sits on: a panel waiting on a page it is still scrolled
        // across would otherwise have its scanner cut into by the track.
        if let Some(line) = crate::loader::line(panel.activity, beneath)
            && let Some(place) = crate::loader::placement(area)
        {
            frame.render_widget(line, place);
        }

        hits[index] = PanelHit::new(panel.panel, area, inner, tabs, body.as_ref());
    }

    let hint_bar = rects.bar();
    // The bar goes quiet with the panels: while a modal is up it is naming
    // keys the modal has already claimed, and the modal is listing them
    // itself.
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

/// What each overlay's own block would have room for in this frame.
///
/// A function of the terminal size alone, every field of it — which is the
/// invariant `Geometry` rests on (architecture.md §3). Gathered in one place
/// because each block is measured by the module that draws it, and there are
/// now seven of them.
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

/// The one overlay this frame is showing, if any — the modal over the
/// panels, and, for the two that carry a live search, the prompt on the hint
/// bar row after it (overlay search design §7). Answers with what each
/// wrapping overlay measured, all of it zero for the overlays that wrap
/// nothing.
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
        // Drawn on the hint bar row, not here: a prompt is a text field, not
        // a modal over the panels (design §5.2).
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
        Some(OverlayView::Health(view)) => crate::health::render(frame, view, theme),
        Some(OverlayView::Filter { column, query }) => {
            crate::filter::render(frame, column, query, theme);
        }
        Some(
            OverlayView::Drop(_)
            | OverlayView::Export(_)
            | OverlayView::Inspect(_)
            | OverlayView::Commit(_)
            | OverlayView::Discard { .. },
        ) => return render_measured(frame, view, theme),
        None => {}
    }

    Wrapped::default()
}

/// The overlays that report something back about what they drew: how far
/// their text wrapped, and where their buttons landed.
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

/// `rect` split at `taken` columns from its left edge, clamped to what `rect`
/// actually has, so a query wider than the bar cannot push the split past the
/// rect it is dividing.
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

/// The bordered block a panel is drawn in, the border style the scrollbars
/// that hang off it share, and where each tab of its title landed.
/// Draws a rule across `block` at `y`, meeting both of its borders.
///
/// `block` is the bordered rect, not its inside: a rule that stops short of
/// the frame it divides reads as a row of content. Painted straight onto the
/// buffer because that is the only way to reach the border columns a widget
/// is clipped out of, and before any scrollbar, which owns the right border
/// where the two meet.
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

/// Whether `overlay` is a modal that owns the frame outright.
///
/// The search prompt is not one: it is drawn on the hint bar row and reads
/// against the panel it filters, which has to keep saying which rows it is
/// filtering. The export form is not one either — it exports what the grid
/// beneath it has selected, and a form that dimmed its own subject would be
/// asking the user to confirm a selection it had stopped drawing.
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
            // A modal that owns the frame. Unlike the export form, which is
            // asking about the grid beneath it, a drop is asking about one
            // row its own title already names (drop design §2).
            | OverlayView::Drop(_)
            // The value it shows is copied out of the grid, and the title
            // names where from (cell inspector design §3).
            | OverlayView::Inspect(_)
            // Both ask about edits their own border counts, not about the
            // grid beneath (cell editing design §5, §6).
            | OverlayView::Commit(_)
            | OverlayView::Discard { .. },
        ) => true,
        // The filter modal is not one either, for the export form's reason:
        // it narrows the grid beneath it, and a modal that dimmed its own
        // subject would ask the user to filter something it had stopped
        // drawing (grid sort and filter design §3).
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
    // Right-aligned on the top border, and dropped whole when the border
    // cannot hold both: a truncated term is a lie about what is applied
    // (design §5.1). Drawn on every panel that has one, focused or not — the
    // marker is the only place a search that survives a focus change still
    // says so.
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

/// Where each tab of a title strip landed on the top border.
///
/// Walked in step with [`panel_title`], off the same `tabs`, `form` and `lead`
/// — the strip is measured where it is built, rather than measured a second
/// time from the panel's state, for the reason the row map is
/// (docs/superpowers/specs/2026-08-09-mouse-input-design.md §4).
///
/// Clipped at the right corner: a strip too wide for its border is drawn short
/// by `ratatui`, and a band running past what was drawn would claim border the
/// user can see is blank.
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

/// A count of characters as a count of terminal columns. Every tab name is
/// ASCII, and one wider than a `u16` cannot have been drawn.
fn columns(characters: usize) -> u16 {
    u16::try_from(characters).unwrap_or(u16::MAX)
}

/// The blank column between a title and the border either side of it.
///
/// `ratatui` gives a title none, so a name grows out of the corner it touches.
const TITLE_PAD: &str = " ";

/// The columns of drawn border a title is pushed along by, where the border
/// has them to spare.
///
/// Border glyphs rather than blanks: the gap is meant to read as the panel's
/// frame running on, not as a margin cut out of it
/// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §2). It is a ceiling
/// and not a cost, because the names come first — a sidebar on its 16-column
/// floor spends what it has left over on the inset and no more, rather than
/// shortening a tab to buy one.
const TITLE_INSET: usize = 2;

/// The horizontal glyph of the border the panels are drawn with — `ratatui`
/// puts it under a title, and a title that pads with it puts it back.
const BORDER_LINE: char = '─';

/// What each end of a `width`-column border has spare for an inset once the
/// title strip, a marker if one is drawn, their padding and the corners are
/// paid for.
///
/// The left end is served first: a title pushed off its corner while the
/// marker still sat on its own would read as two different alignments on one
/// border.
fn insets(width: u16, title: &str, marker: Option<&str>) -> (usize, usize) {
    let spare = usize::from(width).saturating_sub(spent(title, marker));
    let lead = TITLE_INSET.min(spare);

    (lead, TITLE_INSET.min(spare - lead))
}

/// `columns` of border, in the style the block draws its own with.
fn inset(columns: usize, border: Style) -> Span<'static> {
    Span::styled(BORDER_LINE.to_string().repeat(columns), border)
}

/// The `[1]-` prefix takes the same style as the active tab: it names the
/// panel, and the panel is what the active tab names too. The active tab takes
/// `Token::Focused`'s colour when its panel is focused, matching the border,
/// and drops one tier to `Token::Muted` when it is not — an unfocused panel's
/// active tab still names what it is showing, but not as loudly as body text.
/// An inactive tab sits a further tier down at `Token::Faint`, whether or not
/// its panel has focus: it names something the panel is not showing.
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

/// A title run that carries its own token, drawn on a border whose style it
/// inherits. The bold comes from [`Theme::focus_border`] and belongs to the
/// border and the panel's own name — a schema name is not emphasis, and a
/// title that bolds it says the name matters more than the row it came from
/// (vision.md §2).
fn schema_run(style: Style) -> Style {
    style.remove_modifier(Modifier::BOLD)
}

/// What a `width`-column border turned out to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drawn {
    strip: Strip,
    /// Whether the search marker survived beside the strip.
    marker: bool,
}

/// How much of a panel's title strip its border turned out to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strip {
    /// Every tab, under its full name.
    Full,
    /// Every tab, under the short names.
    Short,
    /// The tab being drawn, alone and whole.
    Active,
}

/// The tabs a `width`-column border can hold, and the names to draw them
/// under.
///
/// Three rungs, each giving up less than clipping would: a clipped strip says
/// the panel has a tab named `Table col`, which is a name no key reaches
/// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §4). Every tab stays
/// visible for as long as it can, because a tab nobody can see is one nobody
/// presses `2` for.
///
/// The whole ladder is walked with the marker before it is walked without
/// one, so a short name buys a border that still says it is filtered: a tree
/// drawn as if it were the cluster's is a claim about the cluster, where a
/// panel drawn under its short name has only given up standing context
/// (design §5.1).
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

/// What the title reads as on the border: the panel's number, then `tabs`
/// with their separators.
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

/// What `title`, a `marker` if one is drawn, the padding either side of each
/// and the two corners cost. The inset is not counted: it is decoration, and
/// both names say what the panel is actually showing.
fn spent(title: &str, marker: Option<&str>) -> usize {
    let marker = marker.map_or(0, |marker| {
        marker.chars().count() + 2 * TITLE_PAD.chars().count()
    });

    title.chars().count() + 2 * TITLE_PAD.chars().count() + marker + 2
}

/// The prompt of a live sift over an overlay's list, if one is up.
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

/// The query at the left of the bar row, `⏎ apply` / `esc cancel` at the
/// right of what it leaves — composed rather than drawn over the hints, which
/// `render` already put on `hint_bar` before this runs.
fn render_prompt(frame: &mut Frame, hint_bar: Rect, query: &str, view: &ViewModel, theme: Theme) {
    let line = Line::styled(format!(" /{query}"), theme.style(Token::HintAction));
    let taken = u16::try_from(line.width()).unwrap_or(hint_bar.width);
    let (left, right) = split_left(hint_bar, taken);

    frame.render_widget(line, left);
    frame.render_widget(crate::hints::line(&view.hints, right.width, theme), right);
    // `split_left` already clamps `left` to the bar's own bounds; a query
    // wide enough to fill it would otherwise place the cursor one column
    // past the frame's right edge.
    let cursor_x = (left.x + left.width).min(hint_bar.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, left.y));
}

/// Over the block's right border, so it spends no content width — and only
/// when something is hidden, so its absence says nothing is.
///
/// The track is the border glyph at `Token::Faint`, so it reads as part of the
/// border
/// rather than a second bar beside it; the thumb takes the border's own
/// (possibly focus-patched) style, so the scroll position — not the
/// unscrolled rest of the column — carries the stronger colour
/// (2026-08-11-panel-focus-contrast-design.md). `Scrollbar`'s default set is
/// `DOUBLE_VERTICAL`, whose `║` is what makes an unstyled scrollbar read as a
/// second border beside the first (design §3).
///
/// `ScrollbarState` normalises the thumb against `content_length - 1 +
/// viewport_length`, a model whose `position` is a selection index running over
/// `0..content_length`. Ours is a top-of-viewport offset, which stops at
/// `total - visible` — so that is the content length it is given, and
/// `viewport_content_length` is left alone: it is for a viewport item spanning
/// several track rows, not for this. Handing it `total` instead leaves the
/// thumb short of the bottom when the last row is already on screen.
///
/// Only called when the extent overflows, so the length below is at least one.
///
/// `pub(crate)` for the palette, which scrolls the same way and must not grow
/// a second copy of the arithmetic above.
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

/// The footer, and the horizontal track in what it leaves of the border left
/// of the loader (grid paging design §5).
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

/// Over `track`, the bottom border between the footer and the loader,
/// mirroring [`scrollbar`] on the right one, and only when a column is off
/// screen — so its absence says nothing is hidden
/// (docs/superpowers/specs/2026-08-10-table-display-design.md §9).
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
    // The thumb is parameterised with the track: `▐` is a full-height glyph,
    // which beside a `─` sitting at mid-height reads as a row of vertical bars
    // rather than as a thicker stretch of the same line.
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
