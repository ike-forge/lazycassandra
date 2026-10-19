//! The hint bar: what the focused panel responds to, along the bottom row.

use lc_domain::style::Token;
use lc_domain::view::{Hint, HintBar, Liveness};
use ratatui::text::{Line, Span};

use crate::theme::Theme;

const INDENT: &str = " ";
/// Narrow: the bar is the densest row on screen.
const SEPARATOR: &str = " | ";

/// The export count goes first, then panel hints right to left, then the
/// common tail left to right; `? commands` goes last, since a user who reads
/// one hint needs the way to the rest.
pub(crate) fn line(bar: &HintBar, width: u16, theme: Theme) -> Line<'static> {
    let panel: Vec<Segment> = bar.panel.iter().map(Segment::new).collect();
    let global: Vec<Segment> = bar.global.iter().map(Segment::new).collect();
    let (kept, from, export) = fit(&panel, &global, bar.export.as_deref(), usize::from(width));

    Line::from(spans(&panel[..kept], &global[from..], export, theme))
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

    /// Assumes single-width labels, true of ASCII and of the arrow and `⏎`
    /// glyphs.
    fn width(&self) -> usize {
        self.keys.chars().count() + 1 + self.action.chars().count()
    }

    /// An inert hint keeps its width, since `fit` drops whole hints, and both
    /// halves go `Faint`.
    fn spans(&self, theme: Theme) -> Vec<Span<'static>> {
        let (keys, action) = match self.liveness {
            Liveness::Live => (Token::HintKey, Token::HintAction),
            Liveness::Inert => (Token::Faint, Token::Faint),
        };

        vec![
            Span::styled(self.keys.clone(), theme.style(keys)),
            Span::styled(format!(" {}", self.action), theme.style(action)),
        ]
    }
}

fn fit<'a>(
    panel: &[Segment],
    global: &[Segment],
    export: Option<&'a str>,
    width: usize,
) -> (usize, usize, Option<&'a str>) {
    let mut kept = panel.len();
    let mut from = 0;
    let export = if export.is_some() && composed(panel, kept, global, from, export) > width {
        None
    } else {
        export
    };

    while kept > 1 && composed(panel, kept, global, from, export) > width {
        kept -= 1;
    }
    while composed(panel, kept, global, from, export) > width && from + 1 < global.len() {
        from += 1;
    }
    if composed(panel, kept, global, from, export) > width {
        kept = 0;
    }
    if composed(panel, kept, global, from, export) > width {
        from = global.len();
    }

    (kept, from, export)
}

fn composed(
    panel: &[Segment],
    kept: usize,
    global: &[Segment],
    from: usize,
    export: Option<&str>,
) -> usize {
    let has_hints = kept > 0 || from < global.len();
    if !has_hints && export.is_none() {
        return 0;
    }

    let separator = if kept > 0 && from < global.len() {
        SEPARATOR.chars().count()
    } else {
        0
    };
    let mut total =
        INDENT.chars().count() + joined(&panel[..kept]) + separator + joined(&global[from..]);

    if let Some(text) = export {
        if has_hints {
            total += SEPARATOR.chars().count();
        }
        total += text.chars().count();
    }

    total
}

fn joined(segments: &[Segment]) -> usize {
    segments.iter().map(Segment::width).sum::<usize>()
        + SEPARATOR.chars().count() * segments.len().saturating_sub(1)
}

fn spans(
    panel: &[Segment],
    global: &[Segment],
    export: Option<&str>,
    theme: Theme,
) -> Vec<Span<'static>> {
    if panel.is_empty() && global.is_empty() && export.is_none() {
        return Vec::new();
    }

    let mut spans = vec![Span::raw(INDENT)];
    push_joined(&mut spans, panel, theme);
    if !panel.is_empty() && !global.is_empty() {
        spans.push(Span::styled(SEPARATOR, theme.style(Token::Faint)));
    }
    push_joined(&mut spans, global, theme);
    if let Some(text) = export {
        if !panel.is_empty() || !global.is_empty() {
            spans.push(Span::styled(SEPARATOR, theme.style(Token::Faint)));
        }
        spans.push(Span::styled(text.to_owned(), theme.style(Token::Muted)));
    }
    spans
}

fn push_joined(spans: &mut Vec<Span<'static>>, segments: &[Segment], theme: Theme) {
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(SEPARATOR, theme.style(Token::Faint)));
        }
        spans.extend(segment.spans(theme));
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::geometry::Geometry;
    use lc_domain::panel::Panel;
    use lc_domain::{State, view_model};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    fn bar_of(focus: Panel) -> HintBar {
        let mut state = State::default();
        state.focus_on(focus);
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
            drawn(Panel::Tables, 90),
            " ⏎ open | H health | / search | v select | y copy"
        );
    }

    #[test]
    fn hints_are_dropped_whole_and_lowest_value_first() {
        // No geometry here fits a palette, so `?` never joins the bar.
        assert_eq!(drawn(Panel::Tables, 36), " ⏎ open | v select | y copy");
        assert_eq!(drawn(Panel::Tables, 25), " ⏎ open | y copy");
        assert_eq!(drawn(Panel::Tables, 15), " y copy");
        assert_eq!(drawn(Panel::Tables, 6), "");
    }

    /// A real 100x24 frame's geometry, not an impossible size.
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
    fn a_frame_with_room_for_the_palette_shows_its_hint_and_drops_it_last() {
        let mut state = State::default();
        state.geometry = fitting_geometry();
        let bar = view_model(&state).hints;
        let drawn = |width: u16| -> String {
            line(&bar, width, Theme::default())
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        };

        assert_eq!(
            drawn(90),
            " ⏎ open | H health | / search | v select | y copy | ? commands"
        );
        assert_eq!(
            drawn(51),
            " ⏎ open | / search | v select | y copy | ? commands",
            "panel hints go first, down to the first"
        );
        assert_eq!(
            drawn(40),
            " ⏎ open | v select | y copy | ? commands",
            "the common tail goes from the left"
        );
        assert_eq!(drawn(29), " ⏎ open | y copy | ? commands");
        assert_eq!(drawn(20), " ⏎ open | ? commands");
        assert_eq!(drawn(11), " ? commands");
    }

    #[test]
    fn the_widest_bar_still_fits_the_narrowest_legal_frame() {
        assert_eq!(
            drawn(Panel::Tables, 40),
            " ⏎ open | / search | v select | y copy"
        );
    }

    #[test]
    fn a_closed_buffer_names_the_way_in_and_out() {
        assert_eq!(
            drawn(Panel::Queries, 80),
            " ⏎ edit | ctrl-r run | ctrl-e $EDITOR | esc leave"
        );
    }

    #[test]
    fn the_editor_names_its_own_keys_and_nothing_it_types_over() {
        let mut state = State::default();
        state.focus_on(Panel::Queries);
        state.queries.start_editing();

        assert_eq!(
            line(&view_model(&state).hints, 80, Theme::default())
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            " ctrl-r run | ctrl-e $EDITOR | ctrl-f format | ⏎ newline | esc done"
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
            export: None,
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
                export: None,
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
        assert_eq!(inert.spans[1].style, Theme::default().style(Token::Faint));
    }

    #[test]
    fn every_pair_of_hints_is_joined_the_same_way() {
        let bar = bar_of(Panel::Tables);
        let line = line(&bar, 200, Theme::default());
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert!(
            !text.contains('│'),
            "the old boundary glyph survives: {text}"
        );
        assert!(text.contains(" | "), "{text}");
        assert!(
            !text.contains("  "),
            "a double space survives a join: {text}"
        );
    }

    #[test]
    fn the_key_and_its_action_are_drawn_differently() {
        let line = line(&bar_of(Panel::Tables), 80, Theme::default());
        let keys = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "⏎")
            .expect("the first hint's key");

        assert_eq!(keys.style, Theme::default().style(Token::HintKey));
    }

    /// Small enough to check `composed`'s arithmetic by hand.
    fn export_bar(export: Option<String>) -> HintBar {
        HintBar {
            panel: vec![Hint {
                keys: "⏎".to_owned(),
                action: "open",
                liveness: Liveness::Live,
            }],
            global: vec![Hint {
                keys: "q".to_owned(),
                action: "quit",
                liveness: Liveness::Live,
            }],
            export,
        }
    }

    /// `INDENT` (1) + `⏎ open` (6) + `SEPARATOR` (3) + `q quit` (6).
    const HINTS_ONLY: u16 = 16;
    const EXPORT_TEXT: &str = "exporting 41203 rows…";
    /// Plus `SEPARATOR` (3) and the 21-character export text.
    const WITH_EXPORT: u16 = HINTS_ONLY + 3 + 21;

    fn text_of(bar: &HintBar, width: u16) -> String {
        line(bar, width, Theme::default())
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn a_running_exports_count_is_drawn_when_present_and_absent_when_not() {
        let with = text_of(&export_bar(Some(EXPORT_TEXT.to_owned())), 60);
        let without = text_of(&export_bar(None), 60);

        assert!(with.contains(EXPORT_TEXT));
        assert!(!without.contains(EXPORT_TEXT));
    }

    #[test]
    fn the_export_segment_is_drawn_muted() {
        let bar = export_bar(Some(EXPORT_TEXT.to_owned()));
        let line = line(&bar, 60, Theme::default());
        let export_span = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == EXPORT_TEXT)
            .expect("the export text is drawn");

        assert_eq!(export_span.style, Theme::default().style(Token::Muted));
    }

    #[test]
    fn the_export_text_is_the_first_thing_dropped_and_takes_no_hint_with_it() {
        let bar = export_bar(Some(EXPORT_TEXT.to_owned()));

        let with_room = text_of(&bar, WITH_EXPORT);
        assert!(
            with_room.contains(EXPORT_TEXT),
            "at its own width the export text fits: {with_room:?}"
        );
        assert!(with_room.contains("⏎ open") && with_room.contains("q quit"));

        let one_narrower = text_of(&bar, WITH_EXPORT - 1);
        assert!(
            !one_narrower.contains(EXPORT_TEXT),
            "one column short, the export text is what goes: {one_narrower:?}"
        );
        assert_eq!(
            one_narrower, " ⏎ open | q quit",
            "both hints survive untouched — export is dropped before either is"
        );
    }
}
