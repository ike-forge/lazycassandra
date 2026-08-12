//! The hint bar: what the focused panel responds to, along the bottom row.

use lc_domain::style::Token;
use lc_domain::view::{Hint, HintBar, Liveness};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

const INDENT: &str = " ";
const GAP: &str = "  ";
const SEPARATOR: &str = " │ ";

/// The bar as it fits in `width`.
///
/// Hints are dropped whole rather than clipped, lowest value first: panel
/// hints right to left, then `tab panel`, then the first panel hint with the
/// separator. `q quit` goes last, because a user who can read one hint needs
/// the way out (design §6) — except in Queries, whose global tail has no
/// `q quit` to keep, `q` being text there rather than a binding.
pub(crate) fn line(bar: &HintBar, width: u16, theme: Theme) -> Line<'static> {
    let panel: Vec<Segment> = bar.panel.iter().map(Segment::new).collect();
    let global: Vec<Segment> = bar.global.iter().map(Segment::new).collect();
    let (kept, from) = fit(&panel, &global, usize::from(width));

    Line::from(spans(&panel[..kept], &global[from..], theme))
}

struct Segment {
    keys: String,
    action: String,
    liveness: Liveness,
}

impl Segment {
    fn new(hint: &Hint) -> Self {
        Self {
            keys: hint.keys.clone(),
            action: hint.action.to_owned(),
            liveness: hint.liveness,
        }
    }

    /// Columns, on the assumption every label is single-width — true of the
    /// ASCII names and of the single-codepoint glyphs `name` produces (`⏎`,
    /// `↑`, `↓`, `←`, `→`), each one column wide.
    fn width(&self) -> usize {
        self.keys.chars().count() + 1 + self.action.chars().count()
    }

    /// An inert hint keeps its width — `fit` drops hints whole and must not
    /// see a different bar — and gives up the bold that says the key is live:
    /// both halves go to `Muted`, so the whole segment recedes together
    /// whatever `HintAction` is later drawn as.
    fn spans(&self, theme: Theme) -> Vec<Span<'static>> {
        let (keys, action) = match self.liveness {
            Liveness::Live => (Token::HintKey, Token::HintAction),
            Liveness::Inert => (Token::Muted, Token::Muted),
        };

        vec![
            Span::styled(self.keys.clone(), theme.style(keys)),
            Span::styled(format!(" {}", self.action), theme.style(action)),
        ]
    }
}

fn fit(panel: &[Segment], global: &[Segment], width: usize) -> (usize, usize) {
    let mut kept = panel.len();
    let mut from = 0;

    while kept > 1 && composed(panel, kept, global, from) > width {
        kept -= 1;
    }
    while composed(panel, kept, global, from) > width && from + 1 < global.len() {
        from += 1;
    }
    if composed(panel, kept, global, from) > width {
        kept = 0;
    }
    if composed(panel, kept, global, from) > width {
        from = global.len();
    }

    (kept, from)
}

fn composed(panel: &[Segment], kept: usize, global: &[Segment], from: usize) -> usize {
    if kept == 0 && from >= global.len() {
        return 0;
    }

    let separator = if kept > 0 && from < global.len() {
        SEPARATOR.chars().count()
    } else {
        0
    };

    INDENT.chars().count() + joined(&panel[..kept]) + separator + joined(&global[from..])
}

fn joined(segments: &[Segment]) -> usize {
    segments.iter().map(Segment::width).sum::<usize>()
        + GAP.chars().count() * segments.len().saturating_sub(1)
}

fn spans(panel: &[Segment], global: &[Segment], theme: Theme) -> Vec<Span<'static>> {
    if panel.is_empty() && global.is_empty() {
        return Vec::new();
    }

    let mut spans = vec![Span::raw(INDENT)];
    push_joined(&mut spans, panel, theme);
    if !panel.is_empty() && !global.is_empty() {
        spans.push(Span::styled(SEPARATOR, theme.style(Token::HintAction)));
    }
    push_joined(&mut spans, global, theme);
    spans
}

fn push_joined(spans: &mut Vec<Span<'static>>, segments: &[Segment], theme: Theme) {
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw(GAP));
        }
        spans.extend(segment.spans(theme));
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::state::{Geometry, Panel};
    use lc_domain::{State, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn bar_of(focus: Panel) -> HintBar {
        let state = State {
            focus,
            ..State::default()
        };
        view_model(&state).hints
    }

    fn drawn(focus: Panel, width: u16) -> String {
        line(&bar_of(focus), width, Theme::default())
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn a_wide_frame_shows_every_hint() {
        assert_eq!(
            drawn(Panel::Tables, 80),
            " j/k move  l/h fold  ⏎ open  / search │ tab panel  q quit"
        );
    }

    #[test]
    fn hints_are_dropped_whole_and_lowest_value_first() {
        // The thresholds are the widths at which the previous bar stops
        // fitting (design §6). `?` never joins this fixture's bar — these
        // states never carry a geometry a palette would fit (design §5.1) —
        // so the thresholds are exactly what they were before `?` existed.
        assert_eq!(
            drawn(Panel::Tables, 46),
            " j/k move  l/h fold │ tab panel  q quit"
        );
        assert_eq!(drawn(Panel::Tables, 38), " j/k move │ tab panel  q quit");
        assert_eq!(drawn(Panel::Tables, 28), " j/k move │ q quit");
        assert_eq!(drawn(Panel::Tables, 17), " q quit");
        assert_eq!(drawn(Panel::Tables, 6), "");
    }

    /// The `Geometry` a real 100x24 frame produces — the same frame
    /// `render::tests::the_frame_reports_how_many_rows_a_palette_would_show`
    /// pins at `command_rows() == 14` — rather than a width/height
    /// combination no terminal actually produces (design §5.1).
    fn fitting_geometry() -> Geometry {
        let mut terminal =
            Terminal::new(TestBackend::new(100, 24)).expect("TestBackend always initialises");
        let mut geometry = Geometry::default();

        terminal
            .draw(|frame| {
                geometry =
                    crate::render::render(frame, &view_model(&State::default()), Theme::default())
                        .geometry;
            })
            .expect("TestBackend always draws");

        geometry
    }

    #[test]
    fn a_frame_with_room_for_the_palette_shows_its_hint_and_drops_it_after_tab_panel() {
        let state = State {
            geometry: fitting_geometry(),
            ..State::default()
        };
        let bar = view_model(&state).hints;
        let drawn = |width: u16| -> String {
            line(&bar, width, Theme::default())
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        };

        assert_eq!(
            drawn(80),
            " j/k move  l/h fold  ⏎ open  / search │ tab panel  ? commands  q quit"
        );
        assert_eq!(
            drawn(46),
            " j/k move │ tab panel  ? commands  q quit",
            "tab panel is still here alongside ? commands"
        );
        assert_eq!(
            drawn(40),
            " j/k move │ ? commands  q quit",
            "tab panel goes first as the frame narrows"
        );
        assert_eq!(
            drawn(28),
            " j/k move │ q quit",
            "? commands goes next; q quit is still the last one out"
        );
    }

    #[test]
    fn the_widest_bar_still_fits_the_narrowest_legal_frame() {
        assert_eq!(drawn(Panel::Ddl, 40), " j/k move │ tab panel  q quit");
    }

    #[test]
    fn the_editor_names_its_own_keys_and_drops_the_quit_it_no_longer_has() {
        assert_eq!(
            drawn(Panel::Queries, 80),
            " ctrl-r run  ctrl-e edit  ⏎ newline  esc leave │ tab panel"
        );
    }

    #[test]
    fn a_global_tail_longer_than_two_entries_still_drops_down_to_the_last_one() {
        let bar = HintBar {
            panel: vec![],
            global: vec![
                Hint {
                    keys: "j/k".to_owned(),
                    action: "scroll",
                    liveness: Liveness::Live,
                },
                Hint {
                    keys: "tab".to_owned(),
                    action: "panel",
                    liveness: Liveness::Live,
                },
                Hint {
                    keys: "q".to_owned(),
                    action: "cancel",
                    liveness: Liveness::Live,
                },
            ],
        };

        assert_eq!(
            line(&bar, 15, Theme::default())
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            " q cancel"
        );
    }

    #[test]
    fn an_inert_hint_is_greyed_rather_than_dropped() {
        let of = |liveness| {
            let bar = HintBar {
                panel: vec![Hint {
                    keys: "ctrl-r".to_owned(),
                    action: "run",
                    liveness,
                }],
                global: vec![],
            };
            line(&bar, 80, Theme::default())
        };

        let live = of(Liveness::Live);
        let inert = of(Liveness::Inert);

        assert_eq!(
            live.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            inert
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "an inert hint stays put and keeps its width, so `fit` sees the same bar"
        );
        assert_ne!(
            live.spans[1].style, inert.spans[1].style,
            "an inert key must not read as pressable"
        );
        assert_eq!(inert.spans[1].style, Theme::default().style(Token::Muted));
    }

    #[test]
    fn the_key_and_its_action_are_drawn_differently() {
        let line = line(&bar_of(Panel::Tables), 80, Theme::default());
        let keys = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "j/k")
            .expect("the first hint's key");

        assert_eq!(keys.style, Theme::default().style(Token::HintKey));
    }
}
