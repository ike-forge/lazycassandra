//! Reading, layering and validating the config files.
//!
//! Everything here but `discover` is pure, so the merge rules are tested in
//! the fast tier over inline strings
//! (docs/superpowers/specs/2026-08-14-configuration-design.md §7).

mod merge;
mod wire;

use lc_domain::config::{Diagnostic, Origin, Span};
use lc_domain::style::Severity;

pub(crate) use wire::Layer;

/// One config file's values, and everything wrong with it.
///
/// `None` means a syntax error: the file yielded no values at all, so there is
/// nothing to drop selectively and the layer contributes nothing (design §6).
pub(crate) fn parse(source: &str, origin: &Origin) -> (Option<Layer>, Vec<Diagnostic>) {
    let mut layer: Layer = match toml::from_str(source) {
        Ok(layer) => layer,
        Err(error) => {
            let span = error.span().map(|range| line_col(source, range.start));

            return (
                None,
                vec![Diagnostic {
                    origin: origin.clone(),
                    span,
                    severity: Severity::Danger,
                    message: format!("{} — file ignored", error.message()),
                }],
            );
        }
    };
    // Before anything else sees it: after the merge a value may have come
    // from either file, and only here is it known which.
    layer.locate(source, origin);
    let diagnostics = unknown_keys(&layer, origin);

    (Some(layer), diagnostics)
}

/// A byte offset as an editor counts it: both one-based, columns in
/// characters rather than bytes, so a diagnostic on a line holding non-ASCII
/// points where the user's cursor would land.
fn line_col(source: &str, offset: usize) -> Span {
    let consumed = source.get(..offset).unwrap_or(source);
    let line = consumed.matches('\n').count() + 1;
    let column = consumed
        .rsplit_once('\n')
        .map_or(consumed, |(_, last)| last)
        .chars()
        .count()
        + 1;

    Span { line, column }
}

fn unknown_keys(layer: &Layer, origin: &Origin) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut report = |table: &str, keys: &wire::Unknown| {
        for key in keys.keys() {
            diagnostics.push(Diagnostic {
                origin: origin.clone(),
                // `serde`'s flatten keeps the name and not the position.
                span: None,
                severity: Severity::Warning,
                message: format!("unknown key `{table}{key}` — ignored"),
            });
        }
    };

    report("", &layer.unknown);
    if let Some(layout) = &layer.layout {
        report("layout.", &layout.unknown);
    }
    if let Some(grid) = &layer.grid {
        report("grid.", &grid.unknown);
    }
    if let Some(driver) = &layer.driver {
        report("driver.", &driver.unknown);
    }
    if let Some(export) = &layer.export {
        report("export.", &export.unknown);
    }
    for connection in &layer.connections {
        report("connections.", &connection.unknown);
    }
    for theme in &layer.themes {
        report("themes.", &theme.unknown);
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Origin {
        Origin::new("./.lazycassandra.toml")
    }

    #[test]
    fn a_syntax_error_costs_the_whole_layer_and_reports_where() {
        let (layer, diagnostics) = parse("theme = \n", &origin());

        assert!(
            layer.is_none(),
            "a file that does not parse has no values to keep"
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Danger);
        assert_eq!(diagnostics[0].span.map(|span| span.line), Some(1));
    }

    #[test]
    fn an_unknown_key_is_a_warning_and_costs_nothing_else() {
        let (layer, diagnostics) = parse("panel_ratios = 3\ntheme = \"ash-dark\"\n", &origin());

        let layer = layer.expect("an unknown key does not stop the file parsing");
        assert_eq!(
            layer.theme.as_ref().map(|t| t.value.as_str()),
            Some("ash-dark")
        );
        assert_eq!(
            layer.theme.and_then(|t| t.span()),
            Some(Span { line: 2, column: 9 }),
            "a value carries where it was written, resolved while the source is in hand"
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(diagnostics[0].message.contains("panel_ratios"));
    }

    #[test]
    fn an_unstated_key_is_distinguishable_from_one_stated_as_its_default() {
        let (stated, _) = parse("icons = true\n", &origin());
        let (silent, _) = parse("theme = \"ash-dark\"\n", &origin());

        assert_eq!(stated.expect("parses").icons, Some(true));
        assert_eq!(
            silent.expect("parses").icons,
            None,
            "silence must not win a key the layer beneath stated"
        );
    }

    #[test]
    fn a_line_and_column_are_counted_the_way_an_editor_counts_them() {
        assert_eq!(line_col("abc\ndef", 0), Span { line: 1, column: 1 });
        assert_eq!(line_col("abc\ndef", 4), Span { line: 2, column: 1 });
        assert_eq!(line_col("abc\ndef", 6), Span { line: 2, column: 3 });
    }
}
