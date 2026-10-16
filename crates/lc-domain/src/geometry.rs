//! What the last frame measured: the renderer's one report back into the domain
//! (architecture.md §3).

use crate::panel::Panel;
use crate::viewport::Span;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Geometry {
    heights: [usize; 4],
    ddl_lines: usize,
    ddl_selection: Span,
    log_lines: usize,
    log_selection: Span,
    command_rows: usize,
    /// A function of the terminal size alone, so geometry never depends on the
    /// offsets it bounds.
    export_fits: bool,
    /// Apart from `export_fits`: the blocks are different sizes.
    drop_fits: bool,
    /// Gutter included. Terminal size alone, which keeps the measurement loop
    /// open.
    grid_width: usize,
    theme_rows: usize,
    /// Its own: the block is wider than the theme picker's.
    connection_rows: usize,
    /// A ceiling, not a refusal threshold: the block scrolls.
    history_rows: usize,
    health_rows: usize,
    /// Sized from the terminal alone, so neither depends on the offset they
    /// bound.
    inspector_rows: usize,
    inspector_lines: usize,
    inspector_selection: Span,
    /// Sized from the terminal alone, so neither depends on the offset they
    /// bound.
    commit_rows: usize,
    commit_lines: usize,
}

impl Geometry {
    /// Each in the unit the panel scrolls in, net of chrome such as the grid's
    /// header row.
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

    #[must_use]
    pub const fn height(self, panel: Panel) -> usize {
        self.heights[match panel {
            Panel::Tables => 0,
            Panel::Ddl => 1,
            Panel::Contents => 2,
            Panel::Queries => 3,
        }]
    }

    /// The one length the domain cannot count: wrapping needs a width.
    #[must_use]
    pub const fn ddl_lines(self) -> usize {
        self.ddl_lines
    }

    #[must_use]
    pub const fn grid_width(self) -> usize {
        self.grid_width
    }

    /// Zero means no palette fits, which makes `?` inert.
    #[must_use]
    pub const fn command_rows(self) -> usize {
        self.command_rows
    }

    /// The threshold is the renderer's own, so the domain never spells the
    /// form's size.
    #[must_use]
    pub const fn export_fits(self) -> bool {
        self.export_fits
    }

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
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

    /// Zero makes `ctrl-s` inert and closes an open confirmation.
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

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
    #[must_use]
    pub const fn with_drop_fits(self, drop_fits: bool) -> Self {
        Self { drop_fits, ..self }
    }

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
    #[must_use]
    pub const fn with_ddl_selection(self, ddl_selection: Span) -> Self {
        Self {
            ddl_selection,
            ..self
        }
    }

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
    #[must_use]
    pub const fn with_grid_width(self, grid_width: usize) -> Self {
        Self { grid_width, ..self }
    }

    /// Zero means it does not fit, which makes `ctrl-t` inert.
    #[must_use]
    pub const fn theme_rows(self) -> usize {
        self.theme_rows
    }

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
    #[must_use]
    pub const fn with_theme_rows(self, theme_rows: usize) -> Self {
        Self { theme_rows, ..self }
    }

    /// Zero means it does not fit. An empty list is the opener's exception:
    /// guidance is sized as a message.
    #[must_use]
    pub const fn connection_rows(self) -> usize {
        self.connection_rows
    }

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
    #[must_use]
    pub const fn with_connection_rows(self, connection_rows: usize) -> Self {
        Self {
            connection_rows,
            ..self
        }
    }

    /// Zero means not even its borders fit.
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

    /// Zero makes `H` inert.
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

    /// Zero makes `Enter` over the grid inert.
    #[must_use]
    pub const fn inspector_rows(self) -> usize {
        self.inspector_rows
    }

    #[must_use]
    pub const fn inspector_lines(self) -> usize {
        self.inspector_lines
    }

    #[must_use]
    pub const fn inspector_selection(self) -> Span {
        self.inspector_selection
    }

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
    #[must_use]
    pub const fn with_inspector(self, rows: usize, lines: usize, selection: Span) -> Self {
        Self {
            inspector_rows: rows,
            inspector_lines: lines,
            inspector_selection: selection,
            ..self
        }
    }

    /// A function of the wrapping, so only the renderer can produce it.
    #[must_use]
    pub const fn ddl_selection(self) -> Span {
        self.ddl_selection
    }

    /// Wrapping needs a width, so only the renderer can count it.
    #[must_use]
    pub const fn log_lines(self) -> usize {
        self.log_lines
    }

    #[must_use]
    pub const fn log_selection(self) -> Span {
        self.log_selection
    }

    /// Split from [`Self::new`]: most callers are tests with nothing to say
    /// here.
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
        // The first frame is drawn before any measurement, so motion against
        // this must be inert.
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
