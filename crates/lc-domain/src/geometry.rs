//! What the last frame measured: the renderer's one report back into the
//! domain (architecture.md §3).

use crate::panel::Panel;
use crate::viewport::Span;

/// What the last frame turned out to be.
///
/// The renderer's one report back into the domain
/// (docs/superpowers/specs/2026-08-09-panel-scrolling-design.md §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    heights: [usize; 4],
    ddl_lines: usize,
    ddl_selection: Span,
    log_lines: usize,
    log_selection: Span,
    command_rows: usize,
    /// Whether the frame is big enough for the renderer to draw the export
    /// form. A function of the terminal size alone — the form is a fixed
    /// block — so it satisfies §3's rule that geometry never depends on the
    /// offsets it bounds (export design §2).
    export_fits: bool,
    /// Whether the frame is big enough for the renderer to draw the drop
    /// confirmation. Measured apart from [`Self::export_fits`] because the
    /// two blocks are different sizes, and each gate has to read its own
    /// (drop design §1).
    drop_fits: bool,
    /// Character columns the Contents grid's body has, gutter included. A
    /// function of the terminal size alone, which is what keeps §3's
    /// measurement loop open (docs/superpowers/specs/2026-08-10-table-display-design.md §6).
    grid_width: usize,
    /// Rows the theme picker's own block could show in this frame, measured
    /// the same way `command_rows` measures the palette's — its own room,
    /// not a borrowed one (configuration design §9).
    theme_rows: usize,
    /// Rows the connection picker's own block could show, measured the way
    /// `theme_rows` measures the theme picker's. Its own, because the block
    /// is wider — it draws a host list beside each name — so a frame that
    /// fits one picker need not fit the other
    /// (connection picker design §7).
    connection_rows: usize,
    /// Rows the history list could show, measured like `theme_rows`. The
    /// block scrolls, so unlike the pickers' it is a ceiling and not a
    /// refusal threshold (query history design §3).
    history_rows: usize,
    /// Rows the health block's body could show. Sized from the terminal
    /// alone, like the inspector's.
    health_rows: usize,
    /// Rows the inspector's body could show, and the lines its open value
    /// wrapped to. The block is sized from the terminal alone, so neither
    /// depends on the offset they bound (cell inspector design §5).
    inspector_rows: usize,
    inspector_lines: usize,
    /// Where the frame wrapped the inspector's cursor line to, which is what
    /// its offset follows (cell inspector design §5).
    inspector_selection: Span,
    /// Rows the commit confirmation's body has, and the lines its statement
    /// wrapped to. Measured the way the inspector's are, and for the same
    /// reason: the block is sized from the terminal alone, so neither depends
    /// on the offset they bound (cell editing design §5).
    commit_rows: usize,
    commit_lines: usize,
}

impl Geometry {
    /// `heights` in [`Panel::ALL`] order, each counted in the unit that panel
    /// scrolls in — tree rows, grid rows, or wrapped lines — and net of what
    /// the panel spends on chrome, such as the grid's header row.
    #[must_use]
    pub const fn new(heights: [usize; 4], ddl_lines: usize, command_rows: usize) -> Self {
        Self {
            heights,
            ddl_lines,
            ddl_selection: Span { first: 0, lines: 0 },
            log_lines: 0,
            log_selection: Span { first: 0, lines: 0 },
            command_rows,
            export_fits: false,
            drop_fits: false,
            grid_width: 0,
            theme_rows: 0,
            connection_rows: 0,
            history_rows: 0,
            health_rows: 0,
            inspector_rows: 0,
            inspector_lines: 0,
            inspector_selection: Span { first: 0, lines: 0 },
            commit_rows: 0,
            commit_lines: 0,
        }
    }

    /// Rows of content `panel` can show.
    #[must_use]
    pub const fn height(self, panel: Panel) -> usize {
        self.heights[match panel {
            Panel::Tables => 0,
            Panel::Ddl => 1,
            Panel::Contents => 2,
            Panel::Queries => 3,
        }]
    }

    /// Lines the Ddl body wrapped to.
    ///
    /// The one length the domain cannot count for itself: wrapping needs a
    /// width, and width-dependent layout is the renderer's business. Tree and
    /// grid rows it counts, and must keep counting — two sources of truth for
    /// a row count is the drift `tree.rs`'s single flattening prevents.
    #[must_use]
    pub const fn ddl_lines(self) -> usize {
        self.ddl_lines
    }

    /// Character columns the Contents grid can draw into.
    #[must_use]
    pub const fn grid_width(self) -> usize {
        self.grid_width
    }

    /// Rows the command palette would show at this frame size, whether or not
    /// one is open.
    ///
    /// Zero means it does not fit, which is what makes `?` inert rather than
    /// opening a palette nothing draws
    /// (docs/superpowers/specs/2026-08-09-command-palette-design.md §5.1).
    #[must_use]
    pub const fn command_rows(self) -> usize {
        self.command_rows
    }

    /// Whether the export form fits the frame, whether or not one is open.
    ///
    /// False means nothing would draw it, which is what makes `ctrl-e` inert
    /// and closes a form the frame shrank past — the rule the palette follows
    /// through [`Self::command_rows`], applied to a block with its own size.
    /// The threshold is the renderer's own (`lc-inbound::export::fits`), so
    /// the domain never spells the form's dimensions and the two cannot
    /// diverge (export design §2).
    #[must_use]
    pub const fn export_fits(self) -> bool {
        self.export_fits
    }

    /// Whether the frame could draw the export form.
    ///
    /// Split from [`Self::new`] for [`Self::with_ddl_selection`]'s reason.
    #[must_use]
    pub const fn with_export_fits(self, export_fits: bool) -> Self {
        Self {
            export_fits,
            ..self
        }
    }

    #[must_use]
    pub const fn drop_fits(self) -> bool {
        self.drop_fits
    }

    /// Rows of body the commit confirmation could show. Zero is what makes
    /// `ctrl-s` inert and closes an open confirmation, the gate
    /// `inspector_rows` is for `Enter`.
    #[must_use]
    pub const fn commit_rows(self) -> usize {
        self.commit_rows
    }

    #[must_use]
    pub const fn commit_lines(self) -> usize {
        self.commit_lines
    }

    #[must_use]
    pub const fn with_commit(self, commit_rows: usize, commit_lines: usize) -> Self {
        Self {
            commit_rows,
            commit_lines,
            ..self
        }
    }

    /// Whether the frame could draw the drop confirmation. Split from
    /// [`Self::new`] for [`Self::with_export_fits`]'s reason.
    #[must_use]
    pub const fn with_drop_fits(self, drop_fits: bool) -> Self {
        Self { drop_fits, ..self }
    }

    /// The span the frame drew the Ddl selection at.
    ///
    /// Split from [`Self::new`] because almost every caller is a test that has
    /// no selection to describe, and a fourth positional `Span::default()` at
    /// each of them says nothing.
    #[must_use]
    pub const fn with_ddl_selection(self, ddl_selection: Span) -> Self {
        Self {
            ddl_selection,
            ..self
        }
    }

    /// How wide the frame drew the Contents grid's body.
    ///
    /// Split from [`Self::new`] for [`Self::with_ddl_selection`]'s reason: a
    /// test that is not about the grid's width has nothing to say here, and a
    /// positional zero at each of them says nothing either.
    #[must_use]
    pub const fn with_grid_width(self, grid_width: usize) -> Self {
        Self { grid_width, ..self }
    }

    /// Rows the theme picker's own block would show at this frame size,
    /// whether or not one is open.
    ///
    /// Zero means it does not fit, which is what makes `ctrl-t` refuse to
    /// open a picker nothing draws (design §9).
    #[must_use]
    pub const fn theme_rows(self) -> usize {
        self.theme_rows
    }

    /// Split from [`Self::new`] for [`Self::with_ddl_selection`]'s reason: a
    /// test that opens no picker has nothing to say here.
    #[must_use]
    pub const fn with_theme_rows(self, theme_rows: usize) -> Self {
        Self { theme_rows, ..self }
    }

    /// Rows the connection picker's own block would show at this frame size,
    /// whether or not one is open.
    ///
    /// Zero means it does not fit, which is what makes the picker refuse to
    /// open over a list nothing would draw. An empty list is the exception
    /// the opener makes rather than one this measurement makes: guidance is
    /// sized as a message, not as rows (connection picker design §7).
    #[must_use]
    pub const fn connection_rows(self) -> usize {
        self.connection_rows
    }

    /// Split from [`Self::new`] for [`Self::with_ddl_selection`]'s reason: a
    /// test that opens no picker has nothing to say here.
    #[must_use]
    pub const fn with_connection_rows(self, connection_rows: usize) -> Self {
        Self {
            connection_rows,
            ..self
        }
    }

    /// Rows the history list would show at this frame size, whether or not one
    /// is open. Zero means not even its borders fit.
    #[must_use]
    pub const fn history_rows(self) -> usize {
        self.history_rows
    }

    #[must_use]
    pub const fn with_history_rows(self, history_rows: usize) -> Self {
        Self {
            history_rows,
            ..self
        }
    }

    /// Rows the health block would show at this frame size, whether or not
    /// one is open. Zero means it does not fit, which makes `H` inert.
    #[must_use]
    pub const fn health_rows(self) -> usize {
        self.health_rows
    }

    #[must_use]
    pub const fn with_health_rows(self, health_rows: usize) -> Self {
        Self {
            health_rows,
            ..self
        }
    }

    /// Rows the inspector's body would show at this frame size, whether or
    /// not one is open. Zero means it does not fit, which is what makes
    /// `Enter` over the grid inert (design §5).
    #[must_use]
    pub const fn inspector_rows(self) -> usize {
        self.inspector_rows
    }

    /// Lines the open inspector's value wrapped to; zero with none open.
    #[must_use]
    pub const fn inspector_lines(self) -> usize {
        self.inspector_lines
    }

    /// Where the frame drew the inspector's cursor line.
    #[must_use]
    pub const fn inspector_selection(self) -> Span {
        self.inspector_selection
    }

    /// Split from [`Self::new`] for [`Self::with_ddl_selection`]'s reason: a
    /// test that opens no inspector has nothing to say here.
    #[must_use]
    pub const fn with_inspector(self, rows: usize, lines: usize, selection: Span) -> Self {
        Self {
            inspector_rows: rows,
            inspector_lines: lines,
            inspector_selection: selection,
            ..self
        }
    }

    /// Where the Ddl panel drew its selected row.
    ///
    /// The second thing only the renderer can produce, for the same reason as
    /// [`Self::ddl_lines`]: it is a function of the wrapping.
    #[must_use]
    pub const fn ddl_selection(self) -> Span {
        self.ddl_selection
    }

    /// Lines the Logs tab's trail wrapped to.
    ///
    /// The one length the domain cannot count for itself, for the reason
    /// [`Self::ddl_lines`] cannot: wrapping needs a width (query log design
    /// §5.4).
    #[must_use]
    pub const fn log_lines(self) -> usize {
        self.log_lines
    }

    /// Where the Logs tab drew its selected entry.
    #[must_use]
    pub const fn log_selection(self) -> Span {
        self.log_selection
    }

    /// Split from [`Self::new`] for [`Self::with_ddl_selection`]'s reason: a
    /// test that opens no Logs tab has nothing to say here.
    #[must_use]
    pub const fn with_log(self, lines: usize, selection: Span) -> Self {
        Self {
            log_lines: lines,
            log_selection: selection,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_reports_each_panels_own_height() {
        let geometry = Geometry::new([8, 14, 20, 1], 37, 0);

        assert_eq!(geometry.height(Panel::Tables), 8);
        assert_eq!(geometry.height(Panel::Ddl), 14);
        assert_eq!(geometry.height(Panel::Contents), 20);
        assert_eq!(geometry.height(Panel::Queries), 1);
        assert_eq!(geometry.ddl_lines(), 37);
    }

    #[test]
    fn a_state_that_has_never_been_drawn_reports_no_room() {
        // The first frame is drawn before any measurement exists, so every
        // motion against this must be inert rather than wrong.
        let geometry = Geometry::default();

        for panel in Panel::ALL {
            assert_eq!(geometry.height(panel), 0);
        }
        assert_eq!(geometry.ddl_lines(), 0);
    }

    #[test]
    fn a_geometry_carries_the_selected_ddl_rows_span() {
        let span = Span { first: 7, lines: 4 };

        assert_eq!(
            Geometry::new([1, 2, 3, 4], 40, 5)
                .with_ddl_selection(span)
                .ddl_selection(),
            span
        );
        assert_eq!(
            Geometry::default().ddl_selection(),
            Span::default(),
            "a frame that drew no selection reports none"
        );
    }
}
