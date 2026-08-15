//! Splitting a drawn string at the matches inside it.
//!
//! The ranges are computed against the string about to be drawn, not carried
//! from the domain: the DDL panel wraps and truncates here, so an offset
//! measured against the source would name the wrong byte by the time it is
//! painted. A match a wrap broke in half is therefore painted on neither half
//! (search highlight design §3.1).

use lc_domain::search::Search;
use lc_domain::style::Token;
use ratatui::style::Style;
use ratatui::text::Span as TextSpan;

use crate::theme::Theme;

/// `text` as spans, with the matched runs patched with [`Token::Match`] over
/// `base`.
///
/// `Token::Match` is foreground-only, so patching composes with whatever the
/// segment already wore — the idiom `body.rs` uses for a matched null.
pub(crate) fn spans(
    text: &str,
    search: Option<&Search>,
    base: Style,
    theme: Theme,
) -> Vec<TextSpan<'static>> {
    let Some(search) = search else {
        return vec![TextSpan::styled(text.to_owned(), base)];
    };

    let ranges = search.ranges(text);
    if ranges.is_empty() {
        return vec![TextSpan::styled(text.to_owned(), base)];
    }

    let matched = base.patch(theme.style(Token::Match));
    let mut spans = Vec::with_capacity(ranges.len() * 2 + 1);
    let mut at = 0;

    for range in ranges {
        if at < range.start {
            spans.push(TextSpan::styled(text[at..range.start].to_owned(), base));
        }
        spans.push(TextSpan::styled(text[range.clone()].to_owned(), matched));
        at = range.end;
    }

    if at < text.len() {
        spans.push(TextSpan::styled(text[at..].to_owned(), base));
    }

    spans
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::*;

    #[test]
    fn an_unmatched_string_is_one_span() {
        let spans = spans("orders", None, Style::default(), Theme::default());

        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "orders");
    }

    #[test]
    fn a_match_is_split_out_and_keeps_the_base_style() {
        let search = Search::new("der").expect("non-empty");
        let base = Style::default().add_modifier(Modifier::DIM);
        let theme = Theme::default();

        let spans = spans("orders", Some(&search), base, theme);

        let texts: Vec<&str> = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(texts, vec!["or", "der", "s"]);
        assert!(
            spans[1].style.fg == theme.style(Token::Match).fg,
            "the match wears Token::Match"
        );
        assert!(
            spans[1].style.add_modifier.contains(Modifier::DIM),
            "and keeps what the base already carried"
        );
    }

    #[test]
    fn a_match_at_both_ends_leaves_no_empty_span() {
        let search = Search::new("o").expect("non-empty");

        let spans = spans("oro", Some(&search), Style::default(), Theme::default());

        assert!(spans.iter().all(|span| !span.content.is_empty()));
    }
}
