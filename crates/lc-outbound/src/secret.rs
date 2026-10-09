//! The `SecretResolver` port.
//!
//! Resolution happens here rather than in the effect runtime so a plaintext
//! password never crosses into `lc-domain`: an [`Effect`] carries the
//! [`Credential`] — a name, a path, or a literal the config file already
//! held — and the value it stands for exists only inside this crate, between
//! this call and `SessionBuilder::user`
//! (docs/superpowers/specs/2026-08-31-connection-picker-design.md §8).
//!
//! [`Effect`]: lc_domain::update::Effect

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lc_domain::config::{Credential, Credentials, Tls, TlsSettings, Trust, Verification};
use lc_domain::ports::{ConfigError, SecretResolver};

/// Reads a credential from the environment or the filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct Secrets;

impl Secrets {
    /// The username and password a connection authenticates with.
    ///
    /// # Errors
    /// When a stated credential does not resolve, or a `cqlshrc` names no
    /// username, or the password it points at cannot be read.
    pub async fn credentials(
        &self,
        credentials: &Credentials,
    ) -> Result<(String, String), ConfigError> {
        match credentials {
            Credentials::Stated { username, password } => {
                Ok((username.clone(), self.resolve(password).await?))
            }
            Credentials::Cqlshrc(path) => cqlshrc(&expand_home(path)).await,
        }
    }
}

impl Secrets {
    /// The TLS settings a `cqlshrc` states.
    ///
    /// # Errors
    /// When the file cannot be read.
    pub async fn cqlshrc_tls(&self, path: &Path) -> Result<Tls, ConfigError> {
        Ok(tls_from(&read(&expand_home(path)).await?))
    }
}

/// cqlsh turns SSL on with `[connection] ssl = true`; `[ssl]` only configures it.
///
/// `validate = true` (cqlsh's default) maps to `ChainOnly`: cqlsh checks the
/// chain and not the hostname, so a setup that works there keeps working here.
fn tls_from(text: &str) -> Tls {
    let flag = |section, key| value(text, section, key).map(|v| v.eq_ignore_ascii_case("true"));

    if flag("connection", "ssl") != Some(true) {
        return Tls::Off;
    }

    let identity = match (
        value(text, "ssl", "usercert"),
        value(text, "ssl", "userkey"),
    ) {
        (Some(cert), Some(key)) => Some(lc_domain::config::ClientIdentity {
            cert: PathBuf::from(cert),
            key: PathBuf::from(key),
        }),
        _ => None,
    };

    Tls::On(TlsSettings {
        trust: value(text, "ssl", "certfile")
            .map_or(Trust::System, |path| Trust::CaFile(PathBuf::from(path))),
        identity,
        verification: if flag("ssl", "validate") == Some(false) {
            Verification::Off
        } else {
            Verification::ChainOnly
        },
    })
}

/// `~` against `$HOME`, and nothing else.
///
/// Not a shell expansion: `~user` is left alone rather than guessed at, and a
/// path that does not begin with `~` is never rewritten.
pub(crate) fn expand_home(path: &Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path.to_owned();
    };

    std::env::var_os("HOME").map_or_else(|| path.to_owned(), |home| PathBuf::from(home).join(rest))
}

/// The username and password a cqlsh configuration holds.
///
/// Cassandra 4.x splits them: the username sits in the `cqlshrc` under
/// `[auth_provider]`, and the password in a separate mode-600 file under a
/// section named for the provider class. Older cqlsh put both under
/// `[authentication]`, which is read as a fallback.
async fn cqlshrc(path: &Path) -> Result<(String, String), ConfigError> {
    let text = read(path).await?;

    let username = value(&text, "auth_provider", "username")
        .or_else(|| value(&text, "authentication", "username"))
        .ok_or_else(|| {
            ConfigError::Unresolvable(format!(
                "{}: no username under [auth_provider] or [authentication]",
                path.display()
            ))
        })?;

    if let Some(password) = value(&text, "authentication", "password") {
        return Ok((username, password));
    }

    let (credentials, text) = credentials(&text).await?;

    let password = value(&text, "PlainTextAuthProvider", "password")
        .or_else(|| value(&text, "plain_text_auth", "password"))
        .ok_or_else(|| {
            ConfigError::Unresolvable(format!(
                "{}: no password under [PlainTextAuthProvider]",
                credentials.display()
            ))
        })?;

    Ok((username, password))
}

/// The credentials file a `cqlshrc` points at.
///
/// cqlsh's own rule, and nothing more: the stated path when there is one, and
/// `~/.cassandra/credentials` when there is not. A `cqlshrc` naming a path
/// this host does not have — one written for cqlsh running inside a
/// container, say — is a broken configuration to fix where it is written,
/// not something to guess around here.
async fn credentials(cqlshrc: &str) -> Result<(PathBuf, String), ConfigError> {
    let path = value(cqlshrc, "authentication", "credentials")
        .map_or_else(default_credentials, |stated| {
            expand_home(Path::new(&stated))
        });
    let text = read(&path).await?;

    Ok((path, text))
}

fn default_credentials() -> PathBuf {
    expand_home(Path::new("~/.cassandra/credentials"))
}

async fn read(path: &Path) -> Result<String, ConfigError> {
    tokio::fs::read_to_string(path)
        .await
        .map_err(|error| ConfigError::Unresolvable(format!("{}: {error}", path.display())))
}

/// One key from one section of a `configparser` file.
///
/// Hand-rolled rather than a dependency: cqlsh writes the format Python's
/// `configparser` reads, and the part of it a credential lives in is sections,
/// `key = value`, and comments. Inline comments are not stripped — Python does
/// not strip them either unless asked, and a `#` is legal in a password.
fn value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;

    for line in text.lines() {
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            inside = name.trim() == section;
            continue;
        }

        if inside
            && let Some((stated, value)) = line.split_once('=')
            && stated.trim() == key
        {
            return Some(value.trim().to_owned());
        }
    }

    None
}

#[async_trait]
impl SecretResolver for Secrets {
    async fn resolve(&self, credential: &Credential) -> Result<String, ConfigError> {
        match credential {
            Credential::Literal(value) => Ok(value.clone()),
            Credential::Env(name) => std::env::var(name).map_err(|_| {
                ConfigError::Unresolvable(format!("${name} is not set in the environment"))
            }),
            Credential::File(path) => {
                let value = tokio::fs::read_to_string(path).await.map_err(|error| {
                    ConfigError::Unresolvable(format!("{}: {error}", path.display()))
                })?;

                // Only the line ending, not `trim`: a password may legitimately
                // begin or end with a space, and every way of writing a secret
                // to a file — `echo`, an editor, a secrets mount — appends a
                // newline the cluster was never meant to receive.
                Ok(value
                    .strip_suffix('\n')
                    .map_or(value.as_str(), |line| {
                        line.strip_suffix('\r').unwrap_or(line)
                    })
                    .to_owned())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Env` and `File` read the world, so they are exercised in the
    /// integration tier (`tests/secret.rs`) rather than here — the fast tiers
    /// touch no environment and no filesystem (architecture.md §10).
    const CQLSHRC: &str = "\
[auth_provider]
module = cassandra.auth
classname = PlainTextAuthProvider
username = cassandra-superuser

[authentication]
credentials = /cassandra-conf/credentials
";

    #[test]
    fn a_cqlshrc_without_ssl_enabled_is_plaintext_whatever_its_ssl_section_says() {
        assert_eq!(tls_from(CQLSHRC), Tls::Off);
        assert_eq!(tls_from("[ssl]\ncertfile = /ca.pem\n"), Tls::Off);
        assert_eq!(
            tls_from("[connection]\nssl = false\n[ssl]\ncertfile = /ca.pem\n"),
            Tls::Off
        );
    }

    #[test]
    fn an_enabled_cqlshrc_takes_its_ca_identity_and_validation() {
        let text = "[connection]\nssl = true\n[ssl]\ncertfile = /ca.pem\n\
                    usercert = /c.pem\nuserkey = /k.pem\nvalidate = false\n";

        assert_eq!(
            tls_from(text),
            Tls::On(TlsSettings {
                trust: Trust::CaFile(PathBuf::from("/ca.pem")),
                identity: Some(lc_domain::config::ClientIdentity {
                    cert: PathBuf::from("/c.pem"),
                    key: PathBuf::from("/k.pem"),
                }),
                verification: Verification::Off,
            })
        );
    }

    #[test]
    fn an_enabled_cqlshrc_with_no_ssl_section_trusts_the_system_and_checks_the_chain() {
        assert_eq!(
            tls_from("[connection]\nssl = True\n"),
            Tls::On(TlsSettings {
                verification: Verification::ChainOnly,
                ..TlsSettings::default()
            })
        );
    }

    #[test]
    fn a_key_is_read_from_its_own_section() {
        assert_eq!(
            value(CQLSHRC, "auth_provider", "username"),
            Some("cassandra-superuser".to_owned())
        );
        assert_eq!(
            value(CQLSHRC, "authentication", "credentials"),
            Some("/cassandra-conf/credentials".to_owned())
        );
    }

    #[test]
    fn a_key_present_in_another_section_is_not_found() {
        assert_eq!(value(CQLSHRC, "authentication", "username"), None);
        assert_eq!(value(CQLSHRC, "auth_provider", "credentials"), None);
    }

    #[test]
    fn a_comment_is_not_a_key_and_a_hash_inside_a_value_is_not_a_comment() {
        let text = "[authentication]\n# username = commented\npassword = hunter#2\n";

        assert_eq!(value(text, "authentication", "username"), None);
        assert_eq!(
            value(text, "authentication", "password"),
            Some("hunter#2".to_owned()),
            "configparser keeps an inline hash, and it is legal in a password"
        );
    }

    #[test]
    fn a_value_holding_an_equals_sign_keeps_everything_after_the_first_one() {
        let text = "[PlainTextAuthProvider]\npassword = a=b=c\n";

        assert_eq!(
            value(text, "PlainTextAuthProvider", "password"),
            Some("a=b=c".to_owned())
        );
    }

    #[test]
    fn a_path_without_a_tilde_is_left_alone() {
        assert_eq!(
            expand_home(Path::new("/cassandra-conf/credentials")),
            PathBuf::from("/cassandra-conf/credentials")
        );
    }

    #[test]
    fn a_tilde_naming_another_user_is_not_guessed_at() {
        assert_eq!(
            expand_home(Path::new("~someone/.cassandra/cqlshrc")),
            PathBuf::from("~someone/.cassandra/cqlshrc"),
            "only a bare `~` is $HOME; a shell would resolve the rest and this is not a shell"
        );
    }

    #[tokio::test]
    async fn a_literal_resolves_to_itself() {
        let resolved = Secrets
            .resolve(&Credential::Literal("hunter2".to_owned()))
            .await;

        assert_eq!(resolved, Ok("hunter2".to_owned()));
    }

    #[tokio::test]
    async fn an_unset_variable_names_itself_rather_than_resolving_to_empty() {
        let resolved = Secrets
            .resolve(&Credential::Env(
                "LC_A_VARIABLE_NO_ENVIRONMENT_SETS".to_owned(),
            ))
            .await;

        assert!(matches!(resolved, Err(ConfigError::Unresolvable(_))));
    }
}
