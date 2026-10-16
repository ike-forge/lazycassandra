//! Greedy word wrap on whitespace, breaking a word only when it cannot fit
//! alone (`compaction`'s class name is 60 characters). Styled per character, so
//! highlighted statements wrap without losing tokens.

use lc_domain::search::Search;
use lc_domain::style::Segment;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// Every line opens with `indent` blanks, which callers overwrite with a label
/// or timestamp.
pub(crate) fn wrap(
    chars: &[(char, Style)],
    width: usize,
    indent: usize,
) -> Vec<Vec<(char, Style)>> {
    let room = width.saturating_sub(indent);
    if room == 0 {
        return Vec::new();
    }
    let prefix = vec![(' ', Style::default()); indent];

    let mut lines = Vec::new();
    let mut current: Vec<(char, Style)> = Vec::new();

    for word in words(chars)
        .into_iter()
        .flat_map(|word| broken(&word, room))
    {
        if current.is_empty() {
            current = word;
        } else if current.len() + 1 + word.len() <= room {
            current.push((' ', Style::default()));
            current.extend(word);
        } else {
            lines.push(prefixed(&prefix, current));
            current = word;
        }
    }

    if !current.is_empty() {
        lines.push(prefixed(&prefix, current));
    }
    lines
}

/// For uniformly styled text.
pub(crate) fn plain(text: &str, width: usize, indent: usize) -> Vec<String> {
    let chars: Vec<(char, Style)> = text
        .chars()
        .map(|character| (character, Style::default()))
        .collect();

    wrap(&chars, width, indent)
        .into_iter()
        .map(|line| line.into_iter().map(|(character, _)| character).collect())
        .collect()
}

pub(crate) fn chars_of(segments: &[Segment], theme: Theme) -> Vec<(char, Style)> {
    segments
        .iter()
        .flat_map(|segment| {
            let style = segment
                .token
                .map_or_else(Style::default, |token| theme.style(token));
            segment
                .text
                .chars()
                .map(move |character| (character, style))
        })
        .collect()
}

/// Split at style changes and at `search` matches within each run.
pub(crate) fn line(
    chars: Vec<(char, Style)>,
    search: Option<&Search>,
    theme: Theme,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style: Option<Style> = None;

    for (character, style) in chars {
        if run_style != Some(style) {
            if let Some(previous) = run_style.take() {
                spans.extend(crate::highlight::spans(&run, search, previous, theme));
                run.clear();
            }
            run_style = Some(style);
        }
        run.push(character);
    }
    if let Some(style) = run_style {
        spans.extend(crate::highlight::spans(&run, search, style, theme));
    }

    Line::from(spans)
}

fn prefixed(prefix: &[(char, Style)], line: Vec<(char, Style)>) -> Vec<(char, Style)> {
    prefix.iter().copied().chain(line).collect()
}

fn words(chars: &[(char, Style)]) -> Vec<Vec<(char, Style)>> {
    let mut words = Vec::new();
    let mut current = Vec::new();

    for &pair in chars {
        if pair.0.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else {
            current.push(pair);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn broken(word: &[(char, Style)], room: usize) -> Vec<Vec<(char, Style)>> {
    word.chunks(room).map(<[_]>::to_vec).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_with_no_break_in_it_is_broken_anyway() {
        assert_eq!(
            plain(
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
    fn a_word_that_fits_shares_a_line_with_the_next() {
        assert_eq!(plain("one two three", 8, 0), ["one two", "three"]);
    }

    #[test]
    fn every_line_including_the_first_opens_with_the_indent() {
        assert_eq!(plain("hi", 10, 2), ["  hi"]);
    }

    #[test]
    fn no_room_for_even_the_indent_wraps_to_nothing() {
        assert_eq!(plain("hi", 1, 2), Vec::<String>::new());
    }

    #[test]
    fn styled_chars_keep_their_style_across_the_wrap() {
        let red = Style::default().fg(ratatui::style::Color::Red);
        let chars = [('h', red), ('i', red)];

        let lines = wrap(&chars, 10, 0);

        assert_eq!(lines, [[('h', red), ('i', red)]]);
    }
}
