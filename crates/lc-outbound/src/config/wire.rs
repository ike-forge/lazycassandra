//! One config file, exactly as it was written.
//!
//! Every field is `Option`, which is what makes *unstated* distinguishable
//! from *stated as the default* — without it a local `icons = true` cannot be
//! told from silence, and the layer beneath wins a key the user set
//! (design §5).
//!
//! Fields whose validation can fail carry a [`Located`], so the diagnostic
//! that rejects one can quote a file and a line. Fields that cannot fail do
//! not pay for a position they would never use.

use std::collections::BTreeMap;

use lc_domain::config::{Origin, Span};
use serde::{Deserialize, Deserializer};
use toml::Spanned;

/// A value and where it was written.
///
/// `toml::Spanned` gives a byte range, and turning one into a line and column
/// needs the source text — which `validate` does not have, and could not use
/// anyway: after the merge a field may have come from either file. So the
/// position is resolved in `parse`, while the source and the origin are both
/// in hand, and travels with the value from then on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Located<T> {
    pub(crate) value: T,
    byte: usize,
    pub(crate) at: Option<(Origin, Span)>,
}

impl<T> Located<T> {
    /// Fills in the position. Called once, by `Layer::locate`.
    fn locate(&mut self, source: &str, origin: &Origin) {
        self.at = Some((origin.clone(), super::line_col(source, self.byte)));
    }

    pub(crate) fn span(&self) -> Option<Span> {
        self.at.as_ref().map(|(_, span)| *span)
    }

    pub(crate) fn origin(&self) -> Option<&Origin> {
        self.at.as_ref().map(|(origin, _)| origin)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Located<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let spanned = Spanned::<T>::deserialize(deserializer)?;

        Ok(Self {
            byte: spanned.span().start,
            value: spanned.into_inner(),
            at: None,
        })
    }
}

/// Keys no struct here declares, captured so they can be reported rather than
/// silently dropped. `deny_unknown_fields` would raise an error we cannot
/// downgrade, and a typo should not cost a user their file (design §6).
pub(crate) type Unknown = BTreeMap<String, toml::Value>;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Layer {
    pub(crate) theme: Option<Located<String>>,
    pub(crate) background: Option<Located<String>>,
    pub(crate) icons: Option<bool>,
    pub(crate) mouse: Option<bool>,
    pub(crate) default_connection: Option<Located<String>>,
    pub(crate) layout: Option<LayoutLayer>,
    pub(crate) grid: Option<GridLayer>,
    pub(crate) driver: Option<DriverLayer>,
    pub(crate) export: Option<ExportLayer>,
    #[serde(default)]
    pub(crate) connections: Vec<ConnectionLayer>,
    #[serde(default)]
    pub(crate) themes: Vec<ThemeLayer>,
    #[serde(flatten)]
    pub(crate) unknown: Unknown,
}

fn at<T>(located: Option<&mut Located<T>>, source: &str, origin: &Origin) {
    if let Some(located) = located {
        located.locate(source, origin);
    }
}

impl Layer {
    /// Resolves every position, once, while the source and the origin are
    /// still in hand. After this the layer can be merged with another file's
    /// and each value still knows which file it came from.
    pub(crate) fn locate(&mut self, source: &str, origin: &Origin) {
        at(self.theme.as_mut(), source, origin);
        at(self.background.as_mut(), source, origin);
        at(self.default_connection.as_mut(), source, origin);

        if let Some(layout) = self.layout.as_mut() {
            at(layout.sidebar.as_mut(), source, origin);
            at(layout.ddl.as_mut(), source, origin);
            at(layout.queries.as_mut(), source, origin);
        }
        if let Some(grid) = self.grid.as_mut() {
            at(grid.max_column_width.as_mut(), source, origin);
            at(grid.row_limit.as_mut(), source, origin);
        }
        if let Some(driver) = self.driver.as_mut() {
            at(driver.page_size.as_mut(), source, origin);
            at(driver.request_timeout_ms.as_mut(), source, origin);
            at(driver.connect_timeout_ms.as_mut(), source, origin);
            at(driver.consistency.as_mut(), source, origin);
            at(driver.compression.as_mut(), source, origin);
        }
        for connection in &mut self.connections {
            connection.name.locate(source, origin);
            at(connection.posture.as_mut(), source, origin);
            at(connection.discovery.as_mut(), source, origin);
        }
        for theme in &mut self.themes {
            theme.name.locate(source, origin);
            at(theme.extends.as_mut(), source, origin);

            for colour in theme.colors.values_mut() {
                colour.locate(source, origin);
            }
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct LayoutLayer {
    pub(crate) sidebar: Option<Located<u16>>,
    pub(crate) ddl: Option<Located<u16>>,
    pub(crate) queries: Option<Located<u16>>,
    #[serde(flatten)]
    pub(crate) unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct GridLayer {
    pub(crate) max_column_width: Option<Located<u16>>,
    pub(crate) row_limit: Option<Located<u32>>,
    #[serde(flatten)]
    pub(crate) unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DriverLayer {
    pub(crate) page_size: Option<Located<u32>>,
    pub(crate) request_timeout_ms: Option<Located<u32>>,
    pub(crate) connect_timeout_ms: Option<Located<u32>>,
    pub(crate) consistency: Option<Located<String>>,
    pub(crate) compression: Option<Located<String>>,
    #[serde(flatten)]
    pub(crate) unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ExportLayer {
    pub(crate) path: Option<String>,
    pub(crate) prefix: Option<String>,
    #[serde(flatten)]
    pub(crate) unknown: Unknown,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ConnectionLayer {
    pub(crate) name: Located<String>,
    pub(crate) hosts: Option<Vec<String>>,
    pub(crate) posture: Option<Located<String>>,
    pub(crate) discovery: Option<Located<String>>,
    pub(crate) datacenter: Option<String>,
    pub(crate) username: Option<String>,
    pub(crate) password: Option<CredentialLayer>,
    #[serde(flatten)]
    pub(crate) unknown: Unknown,
}

/// `password = "hunter2"` and `password = { env = "DEV_PW" }` are one key.
/// The alternative — a `password_env` sibling per source — multiplies keys and
/// lets a user state two at once (design §2.2).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum CredentialLayer {
    Literal(String),
    Reference(CredentialSource),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CredentialSource {
    Env(String),
    File(String),
}

#[derive(Debug, Deserialize)]
pub(crate) struct ThemeLayer {
    pub(crate) name: Located<String>,
    pub(crate) extends: Option<Located<String>>,
    #[serde(default)]
    pub(crate) colors: BTreeMap<String, Located<String>>,
    #[serde(flatten)]
    pub(crate) unknown: Unknown,
}
