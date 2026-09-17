//! Reading, layering and validating the config files.
//!
//! Everything here but `discover` is pure, so the merge rules are tested in
//! the fast tier over inline strings
//! (docs/superpowers/specs/2026-08-14-configuration-design.md §7).

mod discover;
mod example;
mod merge;
mod save;
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
                message: renamed(table, key).map_or_else(
                    || format!("unknown key `{table}{key}` — ignored"),
                    |now| format!("`{table}{key}` is now `{now}` — ignored"),
                ),
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

/// The key a retired one was renamed to. `grid.row_limit` was a `LIMIT` and
/// `grid.page_rows` is a page size (grid paging design §1).
fn renamed(table: &str, key: &str) -> Option<&'static str> {
    (table == "grid." && key == "row_limit").then_some("grid.page_rows")
}

/// An environment variable, `None` if unset, empty, or not an absolute path.
///
/// The XDG spec treats an empty value as unset; without this, `XDG_CONFIG_HOME=""`
/// or `HOME=""` would resolve `global_path` against the current directory
/// instead. The spec also requires a non-absolute value to be ignored — for a
/// read that only meant a surprising place to look, but for
/// `write_example_config_from_env` it would mean writing
/// `./lazycassandra/config.toml` into whatever directory the process happened
/// to start in, exactly the untracked-file-in-a-repository outcome design §2
/// rules out for the project file.
fn non_empty_env(name: &str) -> Option<PathBuf> {
    non_empty(std::env::var_os(name))
}

fn non_empty(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// Where the global config file goes, whether or not it is there.
///
/// Exposed for the connection picker, which names it when a config defines no
/// connections — the guidance has to point somewhere, and `lc-domain` cannot
/// read an environment variable (connection picker design §3). `None` when
/// neither `$XDG_CONFIG_HOME` nor `$HOME` is set.
#[must_use]
pub fn global_path() -> Option<PathBuf> {
    let xdg = non_empty_env("XDG_CONFIG_HOME");
    let home = non_empty_env("HOME");

    discover::global_path(xdg.as_deref(), home.as_deref())
}

/// The files, layered (design §3).
///
/// Holds where to look rather than what was found: the save may create the
/// user-level file, and both operations must see the disk as it is when they
/// run (theme persistence design §3).
#[derive(Debug, Clone)]
pub struct FileConfigSource {
    cwd: PathBuf,
    global: Option<PathBuf>,
    named: Option<PathBuf>,
    opening: Option<String>,
}

impl FileConfigSource {
    #[must_use]
    pub fn new(cwd: &Path, xdg: Option<&Path>, home: Option<&Path>) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            global: discover::global_path(xdg, home),
            named: None,
            opening: None,
        }
    }

    /// The file `--config` named, layered above both discovered ones.
    ///
    /// Above rather than instead of: a user pointing at a file with two keys
    /// in it is stating those two keys, not asking for the defaults
    /// everywhere else (design §5).
    #[must_use]
    pub fn named(self, path: &Path) -> Self {
        Self {
            named: Some(path.to_path_buf()),
            ..self
        }
    }

    /// The connection `--connection` named, in place of whatever
    /// `default_connection` says.
    ///
    /// An override of that key rather than a fourth kind of thing: what opens
    /// a cluster at startup, and what the picker opens seated on, both already
    /// read it (connection picker design §3).
    #[must_use]
    pub fn opening(self, name: &str) -> Self {
        Self {
            opening: Some(name.to_owned()),
            ..self
        }
    }

    #[must_use]
    pub fn from_env() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let xdg = non_empty_env("XDG_CONFIG_HOME");
        let home = non_empty_env("HOME");

        Self::new(&cwd, xdg.as_deref(), home.as_deref())
    }

    /// Every file that applies, nearest last, skipping whichever is not there.
    ///
    /// The named file is not skipped when it is absent: the read then fails
    /// and says so, because a `--config` naming nothing is a typo, and
    /// falling through to the discovered files silently would answer it with
    /// somebody else's settings.
    fn paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Some(global) = self.global.as_ref().filter(|path| path.is_file()) {
            paths.push(global.clone());
        }
        if let Some(local) = discover::first_existing(&discover::local_candidates(&self.cwd)) {
            paths.push(local.clone());
        }
        if let Some(named) = &self.named {
            paths.push(named.clone());
        }

        paths
    }

    /// The file whose `theme` is in force: the highest layer stating one,
    /// else the user-level file — created if it is not there.
    ///
    /// Writing a file that a higher layer overrides would save a value that
    /// loses on the next launch, which is the bug this exists to remove
    /// (theme persistence design §3).
    fn save_target(&self) -> Result<PathBuf, lc_domain::ports::ConfigWriteError> {
        let stated = self.paths().into_iter().rev().find(|path| {
            std::fs::read_to_string(path).is_ok_and(|source| save::states_theme(&source))
        });

        stated
            .or_else(|| self.global.clone())
            .or_else(|| self.named.clone())
            .ok_or(lc_domain::ports::ConfigWriteError::NoPath)
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

/// Points `default_connection` at `name`, or at nothing when the config holds
/// no such connection.
///
/// Nothing rather than the file's own default: a mistyped `--connection` must
/// not open a different cluster than the one asked for, and the picker asking
/// is the outcome a user can see and correct.
fn open_instead(config: &mut Config, name: &str) -> Vec<Diagnostic> {
    let named = config
        .connections
        .iter()
        .find(|connection| connection.name.as_str() == name)
        .map(|connection| connection.name.clone());

    config.default_connection = named;
    if config.default_connection.is_some() {
        return Vec::new();
    }

    vec![Diagnostic {
        origin: Origin::new("--connection"),
        span: None,
        severity: Severity::Danger,
        message: format!("no connection named `{name}` — the picker opens instead"),
    }]
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
        let layers: Vec<Layer> = self
            .paths()
            .iter()
            .filter_map(|path| Self::read(path, &mut diagnostics))
            .collect();
        let (merged, more) = merge::merge(layers);
        diagnostics.extend(more);
        let (mut config, more) = validate::validate(merged);
        diagnostics.extend(more);
        if let Some(name) = &self.opening {
            diagnostics.extend(open_instead(&mut config, name));
        }

        (config, diagnostics)
    }
}

#[async_trait::async_trait]
impl lc_domain::ports::ConfigSink for FileConfigSource {
    async fn save_theme(
        &self,
        theme: &lc_domain::theme::ThemeChoice,
    ) -> Result<(), lc_domain::ports::ConfigWriteError> {
        let target = self.save_target()?;
        let source = match std::fs::read_to_string(&target) {
            Ok(source) => source,
            // Nothing there yet is not a failure: the first run's example
            // write can have been blocked, and the key still has to land.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(lc_domain::ports::ConfigWriteError::Io(format!(
                    "{}: {error}",
                    target.display()
                )));
            }
        };
        let edited = save::set_theme(&source, &theme.slug()).map_err(|error| match error {
            lc_domain::ports::ConfigWriteError::Unparsable(_, message) => {
                lc_domain::ports::ConfigWriteError::Unparsable(
                    target.display().to_string(),
                    message,
                )
            }
            other => other,
        })?;

        save::write(&target, &edited)
    }
}

#[cfg(test)]
mod tests {
    use lc_domain::config::ConnectionName;

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
    fn a_retired_row_limit_names_the_key_that_replaced_it() {
        let (_, diagnostics) = parse("[grid]\nrow_limit = 500\n", &origin());

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert!(
            diagnostics[0].message.contains("grid.page_rows"),
            "{}",
            diagnostics[0].message
        );
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

    fn two_connections() -> Config {
        let (layer, _) = parse(
            "default_connection = \"local\"\n\n[[connections]]\nname = \"local\"\nhosts = \
             [\"127.0.0.1:9042\"]\n\n[[connections]]\nname = \"prod\"\nhosts = \
             [\"10.0.0.1:9042\"]\n",
            &origin(),
        );
        let (merged, _) = merge::merge(vec![layer.expect("it parses")]);
        let (config, _) = validate::validate(merged);

        config
    }

    #[test]
    fn a_named_connection_replaces_the_files_default() {
        let mut config = two_connections();

        let diagnostics = open_instead(&mut config, "prod");

        assert_eq!(
            config
                .default_connection
                .as_ref()
                .map(ConnectionName::as_str),
            Some("prod")
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn a_name_no_connection_carries_opens_nothing_at_all() {
        let mut config = two_connections();

        let diagnostics = open_instead(&mut config, "prd");

        assert_eq!(
            config.default_connection, None,
            "falling back to the file's own default would open a cluster the \
             user did not name"
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Danger);
        assert!(diagnostics[0].message.contains("`prd`"), "{diagnostics:?}");
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

    #[test]
    fn a_relative_environment_variable_is_treated_as_unset() {
        assert_eq!(
            non_empty(Some(std::ffi::OsString::from("relative/path"))),
            None,
            "the XDG spec requires a non-absolute value to be ignored — otherwise a relative \
             XDG_CONFIG_HOME turns write_example_config_from_env into a write into whatever \
             directory the process started in"
        );
        assert_eq!(
            non_empty(Some(std::ffi::OsString::from("."))),
            None,
            "a relative path is still relative when it names the current directory itself"
        );
    }
}

/// The repository's own `.lazycassandra.toml`, checked here rather than only
/// by hand: it is what points a checkout at the docker fixture, and a typo in
/// it would surface as an empty connection picker rather than as an error
/// anyone would connect to this file.
#[cfg(test)]
mod repo_config {
    use lc_domain::config::Origin;
    use lc_domain::posture::Posture;
    use lc_domain::style::Severity;

    use super::{merge, parse, validate};

    #[test]
    fn the_repository_config_parses_validates_and_names_its_default() {
        let source = include_str!("../../../../.lazycassandra.toml");
        let (layer, diagnostics) = parse(source, &Origin::new(".lazycassandra.toml"));

        assert!(
            diagnostics.is_empty(),
            "the committed config has no unknown keys: {diagnostics:?}"
        );

        let (merged, more) = merge::merge(vec![layer.expect("it parses")]);
        assert!(more.is_empty(), "{more:?}");

        let (config, more) = validate::validate(merged);
        assert!(
            !more.iter().any(|d| d.severity == Severity::Danger),
            "the committed config validates: {more:?}"
        );

        let default = config
            .default_connection
            .as_ref()
            .expect("it names a default, so a checkout connects without the picker");
        let connection = config
            .connections
            .iter()
            .find(|c| c.name == *default)
            .expect("and the name it gives resolves to an entry");

        assert_eq!(connection.hosts, ["127.0.0.1:9042"]);
        assert_eq!(
            connection.posture,
            Posture::Writable,
            "the fixture is a scratch cluster, and says so deliberately"
        );
        assert_eq!(connection.datacenter.as_deref(), Some("datacenter1"));
    }
}
