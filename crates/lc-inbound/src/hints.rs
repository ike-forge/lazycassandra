//! The hint bar: what the focused panel responds to, along the bottom row.

use lc_domain::style::Token;
use lc_domain::view::{Hint, HintBar, HintGroup};
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
/// the way out (design §6).
pub(crate) fn line(bar: &HintBar, width: u16, theme: Theme) -> Line<'static> {
    let panel = panel_segments(&bar.panel);
    let global: Vec<Segment> = bar.global.iter().map(Segment::hint).collect();
    let (kept, from) = fit(&panel, &global, usize::from(width));

    Line::from(spans(&panel[..kept], &global[from..], theme))
}

enum Segment {
    Hint {
        keys: String,
        action: String,
    },
    /// A panel with no bindings, saying so. Carries no key to draw.
    Sentence(String),
}

impl Segment {
    fn hint(hint: &Hint) -> Self {
        Self::Hint {
            keys: hint.keys.clone(),
            action: hint.action.to_owned(),
        }
    }

    /// Columns, on the assumption every label is single-width — true of the
    /// ASCII names and of the single-codepoint glyphs `name` produces (`⏎`,
    /// `↑`, `↓`, `←`, `→`), each one column wide.
    fn width(&self) -> usize {
        match self {
            Self::Hint { keys, action } => keys.chars().count() + 1 + action.chars().count(),
            Self::Sentence(text) => text.chars().count(),
        }
    }

    fn spans(&self, theme: Theme) -> Vec<Span<'static>> {
        match self {
            Self::Hint { keys, action } => vec![
                Span::styled(keys.clone(), theme.style(Token::HintKey)),
                Span::styled(format!(" {action}"), theme.style(Token::HintAction)),
            ],
            Self::Sentence(text) => {
                vec![Span::styled(text.clone(), theme.style(Token::HintAction))]
            }
        }
    }
}

fn panel_segments(group: &HintGroup) -> Vec<Segment> {
    match group {
        HintGroup::Keys(hints) => hints.iter().map(Segment::hint).collect(),
        HintGroup::Idle(text) => vec![Segment::Sentence((*text).to_owned())],
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
    use lc_domain::state::Panel;
    use lc_domain::{State, view_model};

    use super::*;

    fn bar_of(focus: Panel) -> HintBar {
        let state = State {
            focus,
            ..State::default()
        };
        view_model(&state).hints
    }

    fn drawn(focus: Panel, width: u16) -> String {
        line(&bar_of(focus), width, Theme::TerminalDefault)
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn a_wide_frame_shows_every_hint() {
        assert_eq!(
            drawn(Panel::Tables, 80),
            " j/k move  l/h fold  ⏎ open │ tab panel  q quit"
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

    #[test]
    fn the_widest_bar_still_fits_the_narrowest_legal_frame() {
        assert_eq!(drawn(Panel::Ddl, 40), " j/k scroll │ tab panel  q quit");
    }

    #[test]
    fn a_panel_with_no_bindings_shows_its_sentence_and_the_global_tail() {
        assert_eq!(
            drawn(Panel::Queries, 80),
            " nothing here yet │ tab panel  q quit"
        );
    }

    #[test]
    fn a_global_tail_longer_than_two_entries_still_drops_down_to_the_last_one() {
        let bar = HintBar {
            panel: HintGroup::Keys(vec![]),
            global: vec![
                Hint {
                    keys: "j/k".to_owned(),
                    action: "scroll",
                },
                Hint {
                    keys: "tab".to_owned(),
                    action: "panel",
                },
                Hint {
                    keys: "q".to_owned(),
                    action: "cancel",
                },
            ],
        };

        assert_eq!(
            line(&bar, 15, Theme::TerminalDefault)
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            " q cancel"
        );
    }

    #[test]
    fn the_key_and_its_action_are_drawn_differently() {
        let line = line(&bar_of(Panel::Tables), 80, Theme::TerminalDefault);
        let keys = line
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "j/k")
            .expect("the first hint's key");

        assert_eq!(keys.style, Theme::TerminalDefault.style(Token::HintKey));
    }
}
