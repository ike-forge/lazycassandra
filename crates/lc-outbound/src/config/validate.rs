//! The merged layer into the total type the rest of the application reads.
//!
//! The smallest addressable unit is dropped: a named entry that will not
//! validate loses the entry, a scalar that will not validate loses the key and
//! falls through to the default (design §6).

use std::collections::BTreeMap;
use std::path::PathBuf;

use lc_domain::config::{
    Background, Compression, Config, ConnectionConfig, ConnectionName, Consistency, Credential,
    CustomTheme, Diagnostic, DriverConfig, ExportConfig, GridConfig, Icons, MaxColumnWidth, Millis,
    Mouse, Origin, OutOfRange, PageSize, PanelRatios, Rgb, RowLimit, Span,
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
                self.diagnostics.push(Diagnostic {
                    origin: at
                        .origin()
                        .cloned()
                        .unwrap_or_else(|| self.fallback.clone()),
                    span: at.span(),
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
    let limit = layer
        .row_limit
        .and_then(|value| report.keep(RowLimit::try_new(value.value), &value, "grid.row_limit"));

    GridConfig {
        max_column_width: width.unwrap_or_default(),
        row_limit: limit.unwrap_or_default(),
    }
}

/// The three shares are only valid together, against the floors
/// `lc-inbound`'s layout declares — so a failure falls back to
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
    let result = PanelRatios::try_new(sidebar, ddl, queries);

    let Some(located) = layer
        .sidebar
        .as_ref()
        .or(layer.ddl.as_ref())
        .or(layer.queries.as_ref())
    else {
        return result.unwrap_or_default();
    };

    report.keep(result, located, "layout").unwrap_or_default()
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
                report.error(&located, error.to_string());
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
                report.error(&layer.name, error.to_string());
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
                report.error(&layer.name, error.to_string());
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

        let password = layer.password.map(|credential| match credential {
            CredentialLayer::Literal(value) => Credential::Literal(value),
            CredentialLayer::Reference(CredentialSource::Env(name)) => Credential::Env(name),
            CredentialLayer::Reference(CredentialSource::File(path)) => {
                Credential::File(PathBuf::from(path))
            }
        });

        connections.push(ConnectionConfig {
            name,
            hosts,
            posture,
            datacenter: layer.datacenter,
            username: layer.username,
            password,
        });
    }

    connections
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
    fn a_key_out_of_range_falls_through_to_the_default_and_reports() {
        let (config, diagnostics) = config_of(&["[grid]\nmax_column_width = 0\n"]);

        assert_eq!(config.grid.max_column_width, MaxColumnWidth::default());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Danger);
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
        assert_eq!(
            config_of(&["background = \"puce\"\n"]).0.background,
            Background::Theme
        );
    }

    #[test]
    fn an_unknown_colour_role_is_a_warning_and_the_theme_survives() {
        let (config, diagnostics) = config_of(&[
            "[[themes]]\nname = \"midnight\"\n[themes.colors]\nbaes = \"#0b0d12\"\nfocus = \"#7aa2f7\"\n",
        ]);

        assert_eq!(config.themes.len(), 1);
        assert_eq!(config.themes[0].colors.len(), 1);
        assert!(diagnostics.iter().any(|d| d.message.contains("baes")));
    }

    #[test]
    fn a_password_is_read_as_a_literal_or_as_a_reference() {
        let (config, _) = config_of(&[
            "[[connections]]\nname = \"a\"\nhosts = [\"h:9042\"]\npassword = \"hunter2\"\n\
             [[connections]]\nname = \"b\"\nhosts = [\"h:9042\"]\npassword = { env = \"DEV_PW\" }\n",
        ]);

        assert_eq!(
            config.connections[0].password,
            Some(Credential::Literal("hunter2".to_owned()))
        );
        assert_eq!(
            config.connections[1].password,
            Some(Credential::Env("DEV_PW".to_owned()))
        );
    }
}
