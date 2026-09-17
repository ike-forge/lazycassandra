//! The merged layer into the total type the rest of the application reads.
//!
//! The smallest addressable unit is dropped: a named entry that will not
//! validate loses the entry, a scalar that will not validate loses the key and
//! falls through to the default (design §6).

use std::collections::BTreeMap;
use std::path::PathBuf;

use lc_domain::config::{
    Background, Compression, Config, ConnectionConfig, ConnectionName, Consistency, Credential,
    Credentials, CustomTheme, Diagnostic, Discovery, DriverConfig, ExportConfig, GridConfig, Icons,
    MaxColumnWidth, Millis, Mouse, Origin, OutOfRange, PageRows, PageSize, PanelRatios, Rgb, Span,
};
use lc_domain::posture::Posture;
use lc_domain::style::Severity;
use lc_domain::theme::{BuiltIn, Ours, Role, ThemeChoice, ThemeName};

use super::wire::{
    ConnectionLayer, CredentialLayer, CredentialSource, DriverLayer, ExportLayer, GridLayer, Layer,
    LayoutLayer, Located, ThemeLayer,
};

/// Collects diagnostics so a caller never has to thread a `Vec` by hand.
///
/// `fallback` is for the few diagnostics that name no located value; every
/// other one takes its origin from the value that failed, which is what keeps
/// a merged config pointing at the file the key was actually written in.
struct Report {
    fallback: Origin,
    diagnostics: Vec<Diagnostic>,
}

impl Report {
    fn at<V>(&self, located: &Located<V>) -> (Origin, Option<Span>) {
        (
            located
                .origin()
                .cloned()
                .unwrap_or_else(|| self.fallback.clone()),
            located.span(),
        )
    }

    fn error<V>(&mut self, located: &Located<V>, message: String) {
        let (origin, span) = self.at(located);

        self.diagnostics.push(Diagnostic {
            origin,
            span,
            severity: Severity::Danger,
            message,
        });
    }

    /// For the few diagnostics that name no located value.
    fn error_unlocated(&mut self, message: String) {
        self.diagnostics.push(Diagnostic {
            origin: self.fallback.clone(),
            span: None,
            severity: Severity::Danger,
            message,
        });
    }

    fn warn<V>(&mut self, located: &Located<V>, message: String) {
        let (origin, span) = self.at(located);

        self.diagnostics.push(Diagnostic {
            origin,
            span,
            severity: Severity::Warning,
            message,
        });
    }

    /// The value if it validated, and a diagnostic naming where it was
    /// written if it did not.
    fn keep<T, V>(
        &mut self,
        result: Result<T, OutOfRange>,
        at: &Located<V>,
        key: &str,
    ) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                let (origin, span) = self.at(at);

                self.diagnostics.push(Diagnostic {
                    origin,
                    span,
                    severity: Severity::Danger,
                    message: format!("{key}: {error}"),
                });
                None
            }
        }
    }
}

pub(crate) fn validate(merged: Layer) -> (Config, Vec<Diagnostic>) {
    let mut report = Report {
        fallback: Origin::new("<config>"),
        diagnostics: Vec::new(),
    };
    let themes = themes(merged.themes, &mut report);
    let connections = connections(merged.connections, &mut report);
    let config = Config {
        // Resolved against `themes` rather than within a layer: `theme =
        // "midnight"` is valid if the *other* file defines it (design §6).
        theme: theme(merged.theme, &themes, &mut report),
        background: background(merged.background, &mut report),
        icons: merged.icons.map_or_else(Default::default, bool_to_icons),
        mouse: merged.mouse.map_or_else(Default::default, bool_to_mouse),
        layout: layout(merged.layout, &mut report),
        grid: grid(merged.grid, &mut report),
        driver: driver(merged.driver, &mut report),
        export: export(merged.export),
        default_connection: default_connection(
            merged.default_connection,
            &connections,
            &mut report,
        ),
        themes,
        connections,
    };

    (config, report.diagnostics)
}

fn grid(layer: Option<GridLayer>, report: &mut Report) -> GridConfig {
    let Some(layer) = layer else {
        return GridConfig::default();
    };
    let width = layer.max_column_width.and_then(|value| {
        report.keep(
            MaxColumnWidth::try_new(value.value),
            &value,
            "grid.max_column_width",
        )
    });
    let rows = layer
        .page_rows
        .and_then(|value| report.keep(PageRows::try_new(value.value), &value, "grid.page_rows"));

    GridConfig {
        max_column_width: width.unwrap_or_default(),
        page_rows: rows.unwrap_or_default(),
    }
}

/// One [`PanelRatios`] rather than three independent shares: a partial set
/// cannot be constructed, so a failure falls back to
/// [`PanelRatios::default`] as a unit rather than share by share.
fn layout(layer: Option<LayoutLayer>, report: &mut Report) -> PanelRatios {
    let Some(layer) = layer else {
        return PanelRatios::default();
    };
    let default = PanelRatios::default();
    let sidebar = layer
        .sidebar
        .as_ref()
        .map_or(default.sidebar(), |located| located.value);
    let ddl = layer
        .ddl
        .as_ref()
        .map_or(default.ddl(), |located| located.value);
    let queries = layer
        .queries
        .as_ref()
        .map_or(default.queries(), |located| located.value);

    if let Ok(ratios) = PanelRatios::try_new(sidebar, ddl, queries) {
        return ratios;
    }

    // Each share is re-checked in isolation, against the other two at their
    // defaults, so the diagnostic blames the share that is actually out of
    // range rather than merely the first one the layer stated.
    let failing = [
        (
            layer.sidebar.as_ref(),
            PanelRatios::try_new(sidebar, default.ddl(), default.queries()),
        ),
        (
            layer.ddl.as_ref(),
            PanelRatios::try_new(default.sidebar(), ddl, default.queries()),
        ),
        (
            layer.queries.as_ref(),
            PanelRatios::try_new(default.sidebar(), default.ddl(), queries),
        ),
    ]
    .into_iter()
    .find_map(|(located, isolated)| located.zip(isolated.err()));

    match failing {
        Some((located, error)) => report.error(located, format!("layout: {error}")),
        // Unreachable while every share is checked only against the literal
        // `5..=95` (design §7, §11.8): no combination fails jointly without
        // one of the three failing in isolation first. The day joint
        // validation lands, this is exactly the case a silent fallback would
        // be worst in, so it is reported now rather than left to be found
        // then.
        None => report.error_unlocated(
            "layout: shares are individually in range but not jointly valid — using default"
                .to_owned(),
        ),
    }

    PanelRatios::default()
}

fn driver(layer: Option<DriverLayer>, report: &mut Report) -> DriverConfig {
    let Some(layer) = layer else {
        return DriverConfig::default();
    };
    let page_size = layer
        .page_size
        .and_then(|value| report.keep(PageSize::try_new(value.value), &value, "driver.page_size"))
        .unwrap_or_default();
    let request_timeout = layer
        .request_timeout_ms
        .and_then(|value| {
            report.keep(
                Millis::try_new(value.value),
                &value,
                "driver.request_timeout_ms",
            )
        })
        .unwrap_or_default();
    let connect_timeout = layer
        .connect_timeout_ms
        .and_then(|value| {
            report.keep(
                Millis::try_new(value.value),
                &value,
                "driver.connect_timeout_ms",
            )
        })
        .unwrap_or_default();
    let consistency = layer
        .consistency
        .and_then(|value| {
            let parsed = match value.value.as_str() {
                "any" => Ok(Consistency::Any),
                "one" => Ok(Consistency::One),
                "two" => Ok(Consistency::Two),
                "three" => Ok(Consistency::Three),
                "quorum" => Ok(Consistency::Quorum),
                "all" => Ok(Consistency::All),
                "local-quorum" => Ok(Consistency::LocalQuorum),
                "each-quorum" => Ok(Consistency::EachQuorum),
                "local-one" => Ok(Consistency::LocalOne),
                other => Err(OutOfRange {
                    what: "driver.consistency",
                    value: other.to_owned(),
                    expected: "any|one|two|three|quorum|all|local-quorum|each-quorum|local-one",
                }),
            };
            report.keep(parsed, &value, "driver.consistency")
        })
        .unwrap_or_default();
    let compression = layer
        .compression
        .and_then(|value| {
            let parsed = match value.value.as_str() {
                "none" => Ok(Compression::None),
                "lz4" => Ok(Compression::Lz4),
                "snappy" => Ok(Compression::Snappy),
                other => Err(OutOfRange {
                    what: "driver.compression",
                    value: other.to_owned(),
                    expected: "none|lz4|snappy",
                }),
            };
            report.keep(parsed, &value, "driver.compression")
        })
        .unwrap_or_default();

    DriverConfig {
        page_size,
        request_timeout,
        connect_timeout,
        consistency,
        compression,
    }
}

fn export(layer: Option<ExportLayer>) -> ExportConfig {
    let Some(layer) = layer else {
        return ExportConfig::default();
    };

    ExportConfig {
        path: layer.path.map(PathBuf::from),
        prefix: layer.prefix,
    }
}

fn background(layer: Option<Located<String>>, report: &mut Report) -> Background {
    let Some(located) = layer else {
        return Background::default();
    };

    match located.value.as_str() {
        "theme" => Background::Theme,
        "terminal" => Background::Terminal,
        _ => match Rgb::parse(&located.value) {
            Ok(rgb) => Background::Fixed(rgb),
            Err(error) => {
                report.error(&located, format!("background: {error}"));
                Background::Theme
            }
        },
    }
}

fn themes(layers: Vec<ThemeLayer>, report: &mut Report) -> Vec<CustomTheme> {
    let mut themes = Vec::new();

    for layer in layers {
        let name = match ThemeName::try_new(&layer.name.value) {
            Ok(name) => name,
            Err(error) => {
                report.error(&layer.name, format!("theme.name: {error}"));
                continue;
            }
        };

        if ThemeChoice::builtin_from_slug(name.as_str()).is_some() {
            report.error(
                &layer.name,
                format!("theme `{}` claims a built-in slug", name.as_str()),
            );
            continue;
        }

        let extends = match layer.extends {
            None => BuiltIn::Ours(Ours::DefaultDark),
            Some(located) => {
                let Some(built_in) = ThemeChoice::builtin_from_slug(&located.value) else {
                    report.error(
                        &located,
                        format!(
                            "theme `{}`: extends unknown theme `{}`",
                            name.as_str(),
                            located.value
                        ),
                    );
                    continue;
                };
                built_in
            }
        };

        let mut colors = BTreeMap::new();
        for (key, located) in layer.colors {
            match Role::from_key(&key) {
                Some(role) => match Rgb::parse(&located.value) {
                    Ok(rgb) => {
                        colors.insert(role, rgb);
                    }
                    Err(error) => {
                        report.error(
                            &located,
                            format!("theme `{}`.{key}: {error}", name.as_str()),
                        );
                    }
                },
                None => report.warn(
                    &located,
                    format!("theme `{}`: unknown colour role `{key}`", name.as_str()),
                ),
            }
        }

        themes.push(CustomTheme {
            name,
            extends,
            colors,
        });
    }

    themes
}

fn connections(layers: Vec<ConnectionLayer>, report: &mut Report) -> Vec<ConnectionConfig> {
    let mut connections = Vec::new();

    for layer in layers {
        let name = match ConnectionName::try_new(&layer.name.value) {
            Ok(name) => name,
            Err(error) => {
                report.error(&layer.name, format!("connection.name: {error}"));
                continue;
            }
        };

        let Some(hosts) = layer.hosts.filter(|hosts| !hosts.is_empty()) else {
            report.error(
                &layer.name,
                format!(
                    "connection `{}`: hosts required and non-empty",
                    name.as_str()
                ),
            );
            continue;
        };

        let posture = match layer.posture {
            None => Posture::default(),
            Some(located) => match located.value.as_str() {
                "read-only" => Posture::ReadOnly,
                "writable" => Posture::Writable,
                "protected" => Posture::Protected,
                other => {
                    report.error(
                        &located,
                        format!("connection `{}`: posture `{other}` unknown", name.as_str()),
                    );
                    continue;
                }
            },
        };

        let discovery = match layer.discovery {
            None => Discovery::default(),
            Some(located) => match located.value.as_str() {
                "cluster" => Discovery::Cluster,
                "contact-points" => Discovery::ContactPoints,
                other => {
                    report.error(
                        &located,
                        format!(
                            "connection `{}`: discovery `{other}` unknown",
                            name.as_str()
                        ),
                    );
                    continue;
                }
            },
        };

        if discovery == Discovery::ContactPoints && hosts.len() != 1 {
            report.error(
                &layer.name,
                format!(
                    "connection `{}`: discovery `contact-points` reaches exactly one host, {} stated",
                    name.as_str(),
                    hosts.len()
                ),
            );
            continue;
        }

        let password = layer.password.map(|credential| match credential {
            CredentialLayer::Literal(value) => Credential::Literal(value),
            CredentialLayer::Reference(CredentialSource::Env(name)) => Credential::Env(name),
            CredentialLayer::Reference(CredentialSource::File(path)) => {
                Credential::File(PathBuf::from(path))
            }
        });

        let credentials = match credentials(layer.cqlshrc, layer.username, password) {
            Ok(credentials) => credentials,
            Err(reason) => {
                report.error(
                    &layer.name,
                    format!("connection `{}`: {reason}", name.as_str()),
                );
                continue;
            }
        };

        connections.push(ConnectionConfig {
            name,
            hosts,
            posture,
            discovery,
            datacenter: layer.datacenter,
            credentials,
        });
    }

    connections
}

/// The pair a connection authenticates with, or why it has none the cluster
/// would accept.
///
/// Separate from [`connections`] so each source of a credential is refused in
/// one place: `cqlshrc` supplies both halves, so anything stated beside it is
/// a second answer to a question already answered rather than an override.
fn credentials(
    cqlshrc: Option<String>,
    username: Option<String>,
    password: Option<Credential>,
) -> Result<Option<Credentials>, &'static str> {
    match (cqlshrc, username, password) {
        (None, None, None) => Ok(None),
        (None, Some(username), Some(password)) => {
            Ok(Some(Credentials::Stated { username, password }))
        }
        (Some(path), None, None) => Ok(Some(Credentials::Cqlshrc(PathBuf::from(path)))),
        (Some(_), _, _) => {
            Err("`cqlshrc` supplies the username and the password, so neither is stated beside it")
        }
        (None, Some(_), None) => {
            Err("username stated without a password — a cluster is sent both or neither")
        }
        (None, None, Some(_)) => {
            Err("password stated without a username — a cluster is sent both or neither")
        }
    }
}

fn theme(
    layer: Option<Located<String>>,
    themes: &[CustomTheme],
    report: &mut Report,
) -> ThemeChoice {
    let Some(located) = layer else {
        return ThemeChoice::default();
    };

    if let Some(built_in) = ThemeChoice::builtin_from_slug(&located.value) {
        return built_in.into();
    }

    if let Some(custom) = themes
        .iter()
        .find(|theme| theme.name.as_str() == located.value)
    {
        return ThemeChoice::Custom(custom.name.clone());
    }

    report.error(
        &located,
        format!("theme `{}` is not defined", located.value),
    );
    ThemeChoice::default()
}

fn default_connection(
    layer: Option<Located<String>>,
    connections: &[ConnectionConfig],
    report: &mut Report,
) -> Option<ConnectionName> {
    let located = layer?;

    if let Some(connection) = connections
        .iter()
        .find(|c| c.name.as_str() == located.value)
    {
        return Some(connection.name.clone());
    }

    report.error(
        &located,
        format!("default_connection `{}` is not defined", located.value),
    );
    None
}

fn bool_to_icons(value: bool) -> Icons {
    if value { Icons::On } else { Icons::Off }
}

fn bool_to_mouse(value: bool) -> Mouse {
    if value { Mouse::On } else { Mouse::Off }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{merge::merge, parse};

    fn config_of(sources: &[&str]) -> (Config, Vec<Diagnostic>) {
        let origin = Origin::new("test.toml");
        let layers = sources
            .iter()
            .filter_map(|source| parse(source, &origin).0)
            .collect();
        let (merged, mut diagnostics) = merge(layers);
        let (config, more) = validate(merged);
        diagnostics.extend(more);

        (config, diagnostics)
    }

    #[test]
    fn a_connection_with_an_unknown_posture_is_dropped_and_the_rest_of_the_layer_stands() {
        let (config, diagnostics) = config_of(&["theme = \"ash-dark\"\n\
             [[connections]]\nname = \"dev\"\nhosts = [\"a:9042\"]\nposture = \"read\"\n\
             [[connections]]\nname = \"prod\"\nhosts = [\"b:9042\"]\n"]);

        assert_eq!(
            config.theme,
            ThemeChoice::Ours(Ours::AshDark),
            "the layer still applies"
        );
        assert_eq!(config.connections.len(), 1);
        assert_eq!(config.connections[0].name.as_str(), "prod");
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("dev") && d.message.contains("posture"))
        );
    }

    #[test]
    fn discovery_is_read_per_connection_and_defaults_to_cluster() {
        let (config, diagnostics) =
            config_of(
                &["[[connections]]\nname = \"direct\"\nhosts = [\"a:9042\"]\n\
             [[connections]]\nname = \"tunnelled\"\nhosts = [\"127.0.0.1:9042\"]\n\
             discovery = \"contact-points\"\n"],
            );

        assert!(diagnostics.is_empty());
        assert_eq!(config.connections[0].discovery, Discovery::Cluster);
        assert_eq!(config.connections[1].discovery, Discovery::ContactPoints);
    }

    #[test]
    fn a_connection_with_an_unknown_discovery_is_dropped_rather_than_defaulted() {
        let (config, diagnostics) = config_of(&["[[connections]]\nname = \"dev\"\n\
             hosts = [\"a:9042\"]\ndiscovery = \"single\"\n"]);

        assert!(
            config.connections.is_empty(),
            "silently pooling to every node in a cluster the user meant to reach through one \
             tunnel is the failure this key exists to prevent"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("dev") && d.message.contains("discovery"))
        );
    }

    #[test]
    fn a_key_out_of_range_falls_through_to_the_default_and_reports() {
        let (config, diagnostics) = config_of(&["[grid]\nmax_column_width = 0\n"]);

        assert_eq!(config.grid.max_column_width, MaxColumnWidth::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Danger);
    }

    #[test]
    fn a_stated_page_size_for_the_grid_is_kept() {
        let (config, diagnostics) = config_of(&["[grid]\npage_rows = 50\n"]);

        assert_eq!(config.grid.page_rows.get(), 50);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_page_size_for_the_grid_out_of_range_falls_back_to_the_default() {
        let (config, diagnostics) = config_of(&["[grid]\npage_rows = 0\n"]);

        assert_eq!(config.grid.page_rows.get(), 200);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("grid.page_rows"));
    }

    #[test]
    fn a_stated_column_width_is_kept() {
        let (config, diagnostics) = config_of(&["[grid]\nmax_column_width = 40\n"]);

        assert_eq!(config.grid.max_column_width.get(), 40);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_layout_failure_blames_the_share_that_is_out_of_range_not_the_first_one_stated() {
        let (config, diagnostics) = config_of(&["[layout]\nsidebar = 50\nddl = 3\n"]);

        assert_eq!(config.layout, PanelRatios::default());
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].message.contains("ddl"),
            "must name the failing share, not sidebar: {diagnostics:?}"
        );
        assert_eq!(
            diagnostics[0].span.map(|span| span.line),
            Some(3),
            "must point at ddl's line, not sidebar's: {diagnostics:?}"
        );
    }

    #[test]
    fn a_theme_named_in_one_file_and_defined_in_the_other_resolves() {
        let (config, diagnostics) = config_of(&[
            "theme = \"midnight\"\n",
            "[[themes]]\nname = \"midnight\"\nextends = \"ash-dark\"\n[themes.colors]\nbase = \"#0b0d12\"\n",
        ]);

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            config.theme,
            ThemeChoice::Custom(ThemeName::try_new("midnight").expect("valid")),
            "a cross-reference is resolved after the merge, not within a layer"
        );
        assert_eq!(
            config.themes[0].colors[&Role::Base],
            Rgb {
                red: 0x0b,
                green: 0x0d,
                blue: 0x12
            }
        );
    }

    #[test]
    fn a_theme_that_names_nothing_falls_back_to_the_default_and_reports() {
        let (config, diagnostics) = config_of(&["theme = \"nowhere\"\n"]);

        assert_eq!(config.theme, ThemeChoice::default());
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("nowhere"));
    }

    #[test]
    fn a_default_connection_naming_nothing_falls_back_to_none_and_reports() {
        let (config, diagnostics) = config_of(&["default_connection = \"nowhere\"\n"]);

        assert!(config.default_connection.is_none());
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn a_custom_theme_claiming_a_built_in_slug_is_dropped() {
        let (config, diagnostics) = config_of(&["[[themes]]\nname = \"default-dark\"\n"]);

        assert!(config.themes.is_empty());
        assert!(diagnostics[0].message.contains("default-dark"));
    }

    #[test]
    fn a_background_is_a_word_or_a_hex() {
        assert_eq!(
            config_of(&["background = \"terminal\"\n"]).0.background,
            Background::Terminal
        );
        assert_eq!(
            config_of(&["background = \"theme\"\n"]).0.background,
            Background::Theme
        );
        assert_eq!(
            config_of(&["background = \"#0b0d12\"\n"]).0.background,
            Background::Fixed(Rgb {
                red: 0x0b,
                green: 0x0d,
                blue: 0x12
            })
        );
        let (config, diagnostics) = config_of(&["background = \"puce\"\n"]);
        assert_eq!(config.background, Background::Theme);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.starts_with("background: ")),
            "the diagnostic must name the key it is about, not only the value: {diagnostics:?}"
        );
    }

    #[test]
    fn every_accepted_consistency_string_round_trips() {
        let table = [
            ("any", Consistency::Any),
            ("one", Consistency::One),
            ("two", Consistency::Two),
            ("three", Consistency::Three),
            ("quorum", Consistency::Quorum),
            ("all", Consistency::All),
            ("local-quorum", Consistency::LocalQuorum),
            ("each-quorum", Consistency::EachQuorum),
            ("local-one", Consistency::LocalOne),
        ];

        for (word, expected) in table {
            let (config, diagnostics) =
                config_of(&[format!("[driver]\nconsistency = \"{word}\"\n").as_str()]);

            assert_eq!(
                config.driver.consistency, expected,
                "`{word}` must parse to {expected:?}"
            );
            assert!(diagnostics.is_empty(), "{word}: {diagnostics:?}");
        }
    }

    #[test]
    fn every_accepted_compression_string_round_trips() {
        let table = [
            ("none", Compression::None),
            ("lz4", Compression::Lz4),
            ("snappy", Compression::Snappy),
        ];

        for (word, expected) in table {
            let (config, diagnostics) =
                config_of(&[format!("[driver]\ncompression = \"{word}\"\n").as_str()]);

            assert_eq!(
                config.driver.compression, expected,
                "`{word}` must parse to {expected:?}"
            );
            assert!(diagnostics.is_empty(), "{word}: {diagnostics:?}");
        }
    }

    #[test]
    fn an_unknown_consistency_word_falls_back_and_is_reported() {
        let (config, diagnostics) = config_of(&["[driver]\nconsistency = \"stale\"\n"]);

        assert_eq!(config.driver.consistency, Consistency::default());
        assert!(diagnostics[0].message.contains("driver.consistency"));
    }

    #[test]
    fn an_unknown_compression_word_falls_back_and_is_reported() {
        let (config, diagnostics) = config_of(&["[driver]\ncompression = \"zstd\"\n"]);

        assert_eq!(config.driver.compression, Compression::default());
        assert!(diagnostics[0].message.contains("driver.compression"));
    }

    #[test]
    fn an_unknown_colour_role_is_a_warning_and_the_theme_survives() {
        let (config, diagnostics) = config_of(&[
            "[[themes]]\nname = \"midnight\"\n[themes.colors]\nbaes = \"#0b0d12\"\nfocus = \"#7aa2f7\"\n",
        ]);

        assert_eq!(config.themes.len(), 1);
        assert_eq!(config.themes[0].colors.len(), 1);
        let unknown_role = diagnostics
            .iter()
            .find(|d| d.message.contains("baes"))
            .expect("the unknown role is reported");
        assert_eq!(unknown_role.severity, Severity::Warning);
    }

    #[test]
    fn an_unknown_role_is_a_warning_but_a_malformed_hex_on_a_real_role_is_an_error_and_only_that_theme_survives_with_what_was_valid()
     {
        let (config, diagnostics) = config_of(&[
            "[[themes]]\nname = \"midnight\"\n[themes.colors]\nbaes = \"#0b0d12\"\nfocus = \"not-a-colour\"\nbase = \"#7aa2f7\"\n",
        ]);

        assert_eq!(config.themes.len(), 1, "the theme survives both drops");
        assert_eq!(
            config.themes[0].colors.len(),
            1,
            "only the valid colour is kept"
        );

        let unknown_role = diagnostics
            .iter()
            .find(|d| d.message.contains("baes"))
            .expect("the unknown role is reported");
        assert_eq!(unknown_role.severity, Severity::Warning);

        let malformed_hex = diagnostics
            .iter()
            .find(|d| d.message.contains("focus"))
            .expect("the malformed hex is reported");
        assert_eq!(malformed_hex.severity, Severity::Danger);
    }

    #[test]
    fn a_password_is_read_as_a_literal_or_as_a_reference() {
        let (config, _) = config_of(&["[[connections]]\nname = \"a\"\nhosts = [\"h:9042\"]\n\
             username = \"u\"\npassword = \"hunter2\"\n\
             [[connections]]\nname = \"b\"\nhosts = [\"h:9042\"]\n\
             username = \"u\"\npassword = { env = \"DEV_PW\" }\n"]);

        assert_eq!(
            config.connections[0].credentials,
            Some(Credentials::Stated {
                username: "u".to_owned(),
                password: Credential::Literal("hunter2".to_owned()),
            })
        );
        assert_eq!(
            config.connections[1].credentials,
            Some(Credentials::Stated {
                username: "u".to_owned(),
                password: Credential::Env("DEV_PW".to_owned()),
            })
        );
    }

    #[test]
    fn a_tunnelled_connection_stating_more_than_one_host_is_dropped() {
        let (config, diagnostics) = config_of(&["[[connections]]\nname = \"two\"\n\
             hosts = [\"127.0.0.1:9043\", \"127.0.0.1:9044\"]\ndiscovery = \"contact-points\"\n"]);

        assert!(
            config.connections.is_empty(),
            "every peer folds onto one endpoint, so a second host names a tunnel nothing reaches"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("two") && d.message.contains("one host"))
        );
    }

    #[test]
    fn a_connection_stating_a_username_without_a_password_is_dropped() {
        let (config, diagnostics) = config_of(&[
            "[[connections]]\nname = \"half\"\nhosts = [\"h:9042\"]\nusername = \"u\"\n",
        ]);

        assert!(
            config.connections.is_empty(),
            "a connection that would reach the cluster as an anonymous session is not what the \
             file asked for"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("half") && d.message.contains("username"))
        );
    }

    #[test]
    fn a_connection_stating_a_password_without_a_username_is_dropped() {
        let (config, diagnostics) = config_of(&[
            "[[connections]]\nname = \"half\"\nhosts = [\"h:9042\"]\npassword = \"hunter2\"\n",
        ]);

        assert!(config.connections.is_empty());
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("half") && d.message.contains("password"))
        );
    }

    #[test]
    fn a_cqlshrc_is_taken_as_the_whole_pair() {
        let (config, diagnostics) = config_of(&["[[connections]]\nname = \"prod\"\n\
             hosts = [\"h:9042\"]\ncqlshrc = \"~/.cassandra/prod/cqlshrc\"\n"]);

        assert!(diagnostics.is_empty());
        assert_eq!(
            config.connections[0].credentials,
            Some(Credentials::Cqlshrc(PathBuf::from(
                "~/.cassandra/prod/cqlshrc"
            ))),
            "the path is kept as written; `~` is expanded where the file is read"
        );
    }

    #[test]
    fn a_cqlshrc_beside_a_stated_username_is_dropped_rather_than_one_winning() {
        let (config, diagnostics) = config_of(&["[[connections]]\nname = \"prod\"\n\
             hosts = [\"h:9042\"]\ncqlshrc = \"rc\"\nusername = \"u\"\npassword = \"p\"\n"]);

        assert!(
            config.connections.is_empty(),
            "a silent precedence rule between two sources of one password is worth less than \
             being told the file states both"
        );
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("prod") && d.message.contains("cqlshrc"))
        );
    }

    #[test]
    fn a_connection_stating_neither_needs_no_credentials() {
        let (config, diagnostics) =
            config_of(&["[[connections]]\nname = \"open\"\nhosts = [\"h:9042\"]\n"]);

        assert!(diagnostics.is_empty());
        assert_eq!(config.connections[0].credentials, None);
    }
}
