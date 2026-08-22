//! Reading, layering and validating the config files.
//!
//! Everything here but `discover` is pure, so the merge rules are tested in
//! the fast tier over inline strings
//! (docs/superpowers/specs/2026-08-14-configuration-design.md §7).

mod discover;
mod example;
mod merge;
mod validate;
mod wire;

use std::path::{Path, PathBuf};

use lc_domain::config::{Config, Diagnostic, Origin, Span};
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
                    // "file ignored" reads as one entry, not the whole file — a
                    // wrong-typed scalar (`icons = "yes"`) takes this path too
                    // (design §6), and per-key recovery from a type mismatch is
                    // follow-up work (§11), so today the loss really is total.
                    message: format!(
                        "{} — every setting in this file was skipped",
                        error.message()
                    ),
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

/// An environment variable, `None` if unset or empty.
///
/// The XDG spec treats an empty value as unset; without this, `XDG_CONFIG_HOME=""`
/// or `HOME=""` would resolve `global_path` against the current directory
/// instead.
fn non_empty_env(name: &str) -> Option<PathBuf> {
    non_empty(std::env::var_os(name))
}

fn non_empty(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// The two files, layered (design §3).
#[derive(Debug, Clone)]
pub struct FileConfigSource {
    global: Option<PathBuf>,
    local: Option<PathBuf>,
}

impl FileConfigSource {
    #[must_use]
    pub fn new(cwd: &Path, xdg: Option<&Path>, home: Option<&Path>) -> Self {
        Self {
            global: discover::global_path(xdg, home).filter(|path| path.is_file()),
            local: discover::first_existing(&discover::local_candidates(cwd)).cloned(),
        }
    }

    #[must_use]
    pub fn from_env() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let xdg = non_empty_env("XDG_CONFIG_HOME");
        let home = non_empty_env("HOME");

        Self::new(&cwd, xdg.as_deref(), home.as_deref())
    }

    fn read(path: &Path, diagnostics: &mut Vec<Diagnostic>) -> Option<Layer> {
        let origin = Origin::new(&path.display().to_string());

        match std::fs::read_to_string(path) {
            Ok(source) => {
                let (layer, more) = parse(&source, &origin);
                diagnostics.extend(more);
                layer
            }
            Err(error) => {
                diagnostics.push(Diagnostic {
                    origin,
                    span: None,
                    severity: Severity::Danger,
                    message: format!("could not be read: {error}"),
                });
                None
            }
        }
    }
}

/// Puts the annotated example at the user-level path when nothing is there
/// (example config design §2).
///
/// Silent throughout, success and failure alike: a file appearing where the
/// documentation says config lives is not an event, and a machine where the
/// write cannot happen — no home, a read-only one, a container — is not a
/// machine with a problem to report on every launch.
pub fn write_example_config(xdg: Option<&Path>, home: Option<&Path>) {
    if let Some(path) = discover::global_path(xdg, home) {
        example::write_if_absent(&path);
    }
}

/// [`write_example_config`] against the environment this process was given.
pub fn write_example_config_from_env() {
    let xdg = non_empty_env("XDG_CONFIG_HOME");
    let home = non_empty_env("HOME");

    write_example_config(xdg.as_deref(), home.as_deref());
}

#[async_trait::async_trait]
impl lc_domain::ports::ConfigSource for FileConfigSource {
    async fn load(&self) -> (Config, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let layers: Vec<Layer> = [self.global.as_deref(), self.local.as_deref()]
            .into_iter()
            .flatten()
            .filter_map(|path| Self::read(path, &mut diagnostics))
            .collect();
        let (merged, more) = merge::merge(layers);
        diagnostics.extend(more);
        let (config, more) = validate::validate(merged);
        diagnostics.extend(more);

        (config, diagnostics)
    }
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
        assert!(
            diagnostics[0]
                .message
                .contains("every setting in this file was skipped"),
            "a wrong-typed scalar takes this same path (design §6), and the message must not \
             read as though only one entry were dropped: {}",
            diagnostics[0].message
        );
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

    #[test]
    fn an_empty_environment_variable_is_treated_as_unset() {
        assert_eq!(
            non_empty(Some(std::ffi::OsString::new())),
            None,
            "the XDG spec treats an empty value as unset, not as the current directory"
        );
        assert_eq!(non_empty(None), None);
        assert_eq!(
            non_empty(Some(std::ffi::OsString::from("/x"))),
            Some(PathBuf::from("/x"))
        );
    }
}
