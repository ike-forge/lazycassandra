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

use async_trait::async_trait;
use lc_domain::config::Credential;
use lc_domain::ports::{ConfigError, SecretResolver};

/// Reads a credential from the environment or the filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct Secrets;

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
