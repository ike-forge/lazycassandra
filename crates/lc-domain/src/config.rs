//! The merged, validated configuration.
//!
//! Total by construction: `Option` lives in `lc-outbound`'s wire structs and
//! stops there, so nothing downstream has to ask whether a setting was stated
//! (docs/superpowers/specs/2026-08-14-configuration-design.md §7).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::posture::Posture;
use crate::style::Severity;
use crate::theme::{Role, ThemeChoice, ThemeName};

/// A value a config file stated that no constructor would accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{value} is not a valid {what}: {expected}")]
pub struct OutOfRange {
    pub what: &'static str,
    pub value: String,
    pub expected: &'static str,
}

macro_rules! bounded {
    ($name:ident, $inner:ty, $what:literal, $lo:expr, $hi:expr, $default:expr) => {
        #[doc = concat!("A validated `", $what, "`, between ", stringify!($lo), " and ", stringify!($hi), ".")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name($inner);

        impl $name {
            /// # Errors
            /// When `value` falls outside the range.
            pub fn try_new(value: $inner) -> Result<Self, OutOfRange> {
                if ($lo..=$hi).contains(&value) {
                    Ok(Self(value))
                } else {
                    Err(OutOfRange {
                        what: $what,
                        value: value.to_string(),
                        expected: concat!(stringify!($lo), "..=", stringify!($hi)),
                    })
                }
            }

            #[must_use]
            pub const fn get(self) -> $inner {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self($default)
            }
        }
    };
}

bounded!(MaxColumnWidth, u16, "max_column_width", 1, 512, 24);
bounded!(RowLimit, u32, "row_limit", 1, 1_000_000, 500);
bounded!(PageSize, u32, "page_size", 1, 100_000, 5_000);
bounded!(Millis, u32, "milliseconds", 1, 600_000, 5_000);

/// The three shares `lc-inbound`'s layout is built from.
///
/// One type rather than three newtypes: a partial set of shares is
/// meaningless — there is no useful value for a sidebar share alone with no
/// `ddl` or `queries` to go with it — so the unit type makes a partial set
/// unconstructable and the fallback atomic. Each share is still validated
/// independently, against the literal `5..=95`, not against the floors
/// `layout.rs` declares — that is follow-up work (design §7, §11.8); this
/// type does not yet make the three shares jointly valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRatios {
    sidebar: u16,
    ddl: u16,
    queries: u16,
}

impl PanelRatios {
    /// # Errors
    /// When any share falls outside 5..=95.
    pub fn try_new(sidebar: u16, ddl: u16, queries: u16) -> Result<Self, OutOfRange> {
        for (what, value) in [
            ("layout.sidebar", sidebar),
            ("layout.ddl", ddl),
            ("layout.queries", queries),
        ] {
            if !(5..=95).contains(&value) {
                return Err(OutOfRange {
                    what: "panel share",
                    value: format!("{what} = {value}"),
                    expected: "5..=95",
                });
            }
        }

        Ok(Self {
            sidebar,
            ddl,
            queries,
        })
    }

    #[must_use]
    pub const fn sidebar(self) -> u16 {
        self.sidebar
    }

    #[must_use]
    pub const fn ddl(self) -> u16 {
        self.ddl
    }

    #[must_use]
    pub const fn queries(self) -> u16 {
        self.queries
    }
}

impl Default for PanelRatios {
    fn default() -> Self {
        Self {
            sidebar: 25,
            ddl: 60,
            queries: 10,
        }
    }
}

/// A connection's name, which is also the key §5's merge unions on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionName(Arc<str>);

impl ConnectionName {
    /// # Errors
    /// When `name` is blank or holds whitespace — it is a merge key and an
    /// identifier the user types, not a description.
    pub fn try_new(name: &str) -> Result<Self, OutOfRange> {
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(OutOfRange {
                what: "connection name",
                value: name.to_owned(),
                expected: "non-empty, no whitespace",
            });
        }

        Ok(Self(Arc::from(name)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A colour as a config file states it. Not `ratatui::Color`: `lc-outbound`
/// parses the hex and never sees a terminal (design §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    /// Parses `#rrggbb`.
    ///
    /// # Errors
    /// When the string is not seven characters beginning with `#`, or holds a
    /// non-hex digit.
    pub fn parse(hex: &str) -> Result<Self, OutOfRange> {
        let out_of_range = || OutOfRange {
            what: "colour",
            value: hex.to_owned(),
            expected: "#rrggbb",
        };
        let digits = hex.strip_prefix('#').ok_or_else(out_of_range)?;

        if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(out_of_range());
        }

        let byte = |range: std::ops::Range<usize>| {
            digits
                .get(range)
                .and_then(|part| u8::from_str_radix(part, 16).ok())
                .ok_or_else(out_of_range)
        };

        Ok(Self {
            red: byte(0..2)?,
            green: byte(2..4)?,
            blue: byte(4..6)?,
        })
    }
}

/// What is painted behind everything.
///
/// `Terminal` is not a colour — it means paint nothing, which is the only
/// mechanism that lets a translucent or blurred terminal show through
/// (design §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Background {
    #[default]
    Theme,
    Terminal,
    Fixed(Rgb),
}

/// Whether Nerd Font glyphs are drawn. An enum rather than a `bool`, per
/// conventions.md §1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Icons {
    #[default]
    On,
    Off,
}

/// Whether pointer events are captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mouse {
    #[default]
    On,
    Off,
}

/// The consistency level the driver requests for every statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Consistency {
    Any,
    One,
    Two,
    Three,
    Quorum,
    All,
    #[default]
    LocalQuorum,
    EachQuorum,
    LocalOne,
}

/// The wire compression the driver negotiates with the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    None,
    #[default]
    Lz4,
    Snappy,
}

/// Where a credential comes from.
///
/// `Command` was removed rather than renamed: a committed project file naming
/// a shell command means cloning a repository and running the tool executes a
/// stranger's command (design §2.2). Nothing here executes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    Literal(String),
    Env(String),
    File(PathBuf),
}

/// Which nodes the driver opens connections to.
///
/// `Cluster` is what a driver does by default: dial a contact point, read
/// `system.peers`, then pool to every node it names. Those names are `inet`
/// columns — raw addresses, never hostnames — so a cluster reached through a
/// tunnel reports addresses that only resolve inside the cluster, and every
/// peer beyond the tunnelled one is permanently down.
///
/// `ContactPoints` is the escape hatch: use exactly the hosts stated here and
/// ignore the topology. It costs token-aware routing, which a browsing tool
/// does not spend, and it buys a working session over a single
/// `kubectl port-forward`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Discovery {
    #[default]
    Cluster,
    ContactPoints,
}

impl Discovery {
    /// How a config file spells this, and the one spelling — the same rule
    /// [`Posture::label`] follows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cluster => "cluster",
            Self::ContactPoints => "contact-points",
        }
    }
}

/// What a cluster running `PasswordAuthenticator` is told.
///
/// One type rather than two `Option` fields on [`ConnectionConfig`]: a
/// username with no password authenticates nothing and a password with no
/// username names nobody, so the pair is stated together or not at all
/// (conventions.md §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: Credential,
}

/// One cluster to connect to, as a config file names it under `[[connections]]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub name: ConnectionName,
    pub hosts: Vec<String>,
    pub posture: Posture,
    pub discovery: Discovery,
    pub datacenter: Option<String>,
    pub credentials: Option<Credentials>,
}

/// How the Contents grid renders and pages what it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridConfig {
    pub max_column_width: MaxColumnWidth,
    pub row_limit: RowLimit,
}

/// Driver-level settings applied to every connection.
///
/// Global rather than per-connection: nothing in the design so far calls for
/// one cluster to page or time out differently from another, so there is
/// only one of these per [`Config`] rather than one per
/// [`ConnectionConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DriverConfig {
    pub page_size: PageSize,
    pub request_timeout: Millis,
    pub connect_timeout: Millis,
    pub consistency: Consistency,
    pub compression: Compression,
}

/// Where a grid export is written.
///
/// Both fields are `Option` rather than defaulted: there is no export
/// feature yet to give a bare `path` or `prefix` meaning, so an unstated
/// value stays absent instead of being guessed at.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportConfig {
    pub path: Option<PathBuf>,
    pub prefix: Option<String>,
}

/// A palette a config file supplies.
///
/// `extends` is a [`BuiltIn`] rather than a [`ThemeChoice`], so a custom theme
/// extending another custom theme — and the cycle that permits — is
/// unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTheme {
    pub name: ThemeName,
    pub extends: crate::theme::BuiltIn,
    pub colors: BTreeMap<Role, Rgb>,
}

/// The merged, validated result of layering every config source.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub theme: ThemeChoice,
    pub background: Background,
    pub icons: Icons,
    pub mouse: Mouse,
    pub layout: PanelRatios,
    pub grid: GridConfig,
    pub driver: DriverConfig,
    pub export: ExportConfig,
    pub themes: Vec<CustomTheme>,
    /// Ordered as design §5 defines: global order first, then local-only
    /// entries, with a name in both keeping its global position. Left to the
    /// merge it would be incidental, and every test asserting on it a coin
    /// flip.
    pub connections: Vec<ConnectionConfig>,
    pub default_connection: Option<ConnectionName>,
}

/// The file a layer was read from, quoted by every diagnostic it produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin(Arc<str>);

impl Origin {
    #[must_use]
    pub fn new(path: &str) -> Self {
        Self(Arc::from(path))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A position in a config file, one-based, as a user's editor counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

/// Something a config file said that was dropped, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub origin: Origin,
    /// Absent for an unknown key: the `serde` flatten that captures one keeps
    /// the name and not the position (design §6).
    pub span: Option<Span>,
    pub severity: Severity,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_is_the_built_in_layer() {
        let config = Config::default();

        assert_eq!(config.grid.max_column_width.get(), 24);
        assert_eq!(config.layout.sidebar(), 25);
        assert_eq!(config.layout.ddl(), 60);
        assert_eq!(config.layout.queries(), 10);
        assert_eq!(config.icons, Icons::On);
        assert_eq!(config.mouse, Mouse::On);
        assert_eq!(config.background, Background::Theme);
        assert!(config.connections.is_empty());
        assert!(config.default_connection.is_none());
    }

    #[test]
    fn a_column_width_of_zero_is_not_constructible() {
        assert!(MaxColumnWidth::try_new(0).is_err());
        assert!(MaxColumnWidth::try_new(1).is_ok());
        assert!(MaxColumnWidth::try_new(512).is_ok());
        assert!(MaxColumnWidth::try_new(513).is_err());
    }

    #[test]
    fn each_panel_ratio_is_validated_against_the_literal_bound_not_layout_floors() {
        assert!(PanelRatios::try_new(25, 60, 10).is_ok());
        assert!(
            PanelRatios::try_new(4, 60, 10).is_err(),
            "5..=95 is the literal bound this type checks, not the floor layout.rs declares"
        );
        assert!(PanelRatios::try_new(96, 60, 10).is_err());
    }

    #[test]
    fn rgb_parses_a_valid_hex_triple() {
        let rgb = Rgb::parse("#1a2b3c").expect("well-formed hex parses");

        assert_eq!((rgb.red, rgb.green, rgb.blue), (0x1a, 0x2b, 0x3c));
    }

    #[test]
    fn rgb_rejects_a_string_the_wrong_length() {
        assert!(Rgb::parse("#1a2b3").is_err());
        assert!(Rgb::parse("#1a2b3cd").is_err());
    }

    #[test]
    fn rgb_rejects_a_string_missing_the_hash() {
        assert!(Rgb::parse("1a2b3c").is_err());
    }

    #[test]
    fn rgb_rejects_a_non_hex_digit() {
        assert!(Rgb::parse("#1a2b3g").is_err());
    }

    #[test]
    fn rgb_rejects_a_leading_plus_a_radix_parse_would_otherwise_accept() {
        assert!(
            Rgb::parse("#+1+2+3").is_err(),
            "u8::from_str_radix accepts a leading '+', which is not a hex digit"
        );
    }

    #[test]
    fn a_connection_name_may_not_be_blank_or_hold_whitespace() {
        assert!(ConnectionName::try_new("prod").is_ok());
        assert!(ConnectionName::try_new("").is_err());
        assert!(ConnectionName::try_new("  ").is_err());
        assert!(ConnectionName::try_new("my prod").is_err());
    }

    #[test]
    fn a_connection_that_states_no_discovery_uses_the_whole_cluster() {
        assert_eq!(Discovery::default(), Discovery::Cluster);
    }

    #[test]
    fn discovery_spells_itself_the_way_a_config_file_does() {
        assert_eq!(Discovery::Cluster.label(), "cluster");
        assert_eq!(Discovery::ContactPoints.label(), "contact-points");
    }

    #[test]
    fn a_connection_that_states_no_posture_is_read_only() {
        assert_eq!(Posture::default(), Posture::ReadOnly);
    }

    #[test]
    fn a_diagnostic_quotes_the_file_it_came_from() {
        let diagnostic = Diagnostic {
            origin: Origin::new("./.lazycassandra.toml"),
            span: Some(Span {
                line: 14,
                column: 11,
            }),
            severity: Severity::Danger,
            message: "connection `dev`: `posture` unknown value \"read\" — entry dropped"
                .to_owned(),
        };

        assert_eq!(diagnostic.origin.as_str(), "./.lazycassandra.toml");
    }
}
