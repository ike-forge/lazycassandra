//! The [`Store`] port over a file in the user's state directory.
//!
//! `$XDG_STATE_HOME/lazycassandra/history.toml`, falling back to
//! `~/.local/state`. The state directory and not the cache: the XDG spec
//! names it for "actions history", and a cache is what cleaners are allowed
//! to empty (query history design §5).
//!
//! The statements are the user's own, and `CREATE ROLE … PASSWORD` is a
//! statement, so the file is created `0600` in a `0700` directory.
//!
//! [`Store`]: lc_domain::ports::Store

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lc_domain::config::{ConnectionName, Diagnostic, HistoryLimit, Origin};
use lc_domain::history::Histories;
use lc_domain::ports::{Store, StoreError};
use lc_domain::style::Severity;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

use crate::config::{non_empty_env, save::quote};

const HEADER: &str = "# LazyCassandra query history, newest first. Rewritten on every run.\n";

/// Where the history file goes, whether or not it is there. `None` when
/// neither `$XDG_STATE_HOME` nor `$HOME` is set.
fn path(xdg_state: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let state = xdg_state
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".local").join("state")))?;

    Some(state.join("lazycassandra").join("history.toml"))
}

#[derive(Debug, Default, Deserialize)]
struct File {
    #[serde(default)]
    connections: BTreeMap<String, Section>,
}

#[derive(Debug, Default, Deserialize)]
struct Section {
    #[serde(default)]
    entries: Vec<String>,
}

/// Every connection's entries, as the file holds them.
type Sections = BTreeMap<String, Vec<String>>;

fn parse(source: &str) -> Result<Sections, String> {
    let file: File = toml::from_str(source).map_err(|error| error.message().to_owned())?;

    Ok(file
        .connections
        .into_iter()
        .map(|(name, section)| (name, section.entries))
        .collect())
}

/// `sections` as the file is written. Hand-rolled for `save.rs`'s reason:
/// `toml`'s serialiser is off, and this is two shapes of string.
fn render(sections: &Sections) -> String {
    let mut out = String::from(HEADER);

    for (name, entries) in sections.iter().filter(|(_, entries)| !entries.is_empty()) {
        // Writing into a String cannot fail.
        let _written = write!(out, "\n[connections.{}]\nentries = [\n", quote(name));

        for entry in entries {
            let _written = writeln!(out, "  {},", quote(entry));
        }

        out.push_str("]\n");
    }

    out
}

/// Keeps the history in a file.
#[derive(Debug)]
pub struct FileStore {
    path: Option<PathBuf>,
    /// Saves are read-modify-write over one file; two in flight would each
    /// start from the file as it was.
    writes: tokio::sync::Mutex<()>,
}

impl FileStore {
    #[must_use]
    pub fn new(xdg_state: Option<&Path>, home: Option<&Path>) -> Self {
        Self::at(path(xdg_state, home))
    }

    #[must_use]
    pub fn at(path: Option<PathBuf>) -> Self {
        Self {
            path,
            writes: tokio::sync::Mutex::new(()),
        }
    }

    #[must_use]
    pub fn from_env() -> Self {
        let xdg_state = non_empty_env("XDG_STATE_HOME");
        let home = non_empty_env("HOME");

        Self::new(xdg_state.as_deref(), home.as_deref())
    }

    async fn read(path: &Path) -> Result<Option<String>, std::io::Error> {
        match tokio::fs::read_to_string(path).await {
            Ok(source) => Ok(Some(source)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn warning(path: &Path, message: String) -> Diagnostic {
    Diagnostic {
        origin: Origin::new(&path.display().to_string()),
        span: None,
        severity: Severity::Warning,
        message,
    }
}

#[async_trait]
impl Store for FileStore {
    async fn load(&self, limit: HistoryLimit) -> (Histories, Vec<Diagnostic>) {
        let mut histories = Histories::default();
        let Some(path) = &self.path else {
            return (histories, Vec::new());
        };
        let source = match Self::read(path).await {
            Ok(Some(source)) => source,
            Ok(None) => return (histories, Vec::new()),
            Err(error) => {
                let message = format!("query history not read: {error}");
                return (histories, vec![warning(path, message)]);
            }
        };
        let sections = match parse(&source) {
            Ok(sections) => sections,
            Err(message) => {
                let message =
                    format!("query history ignored, and replaced on the next run: {message}");
                return (histories, vec![warning(path, message)]);
            }
        };
        let mut diagnostics = Vec::new();

        for (name, entries) in &sections {
            match ConnectionName::try_new(name) {
                Ok(name) => histories.restore(&name, entries, limit),
                Err(error) => diagnostics.push(warning(
                    path,
                    format!("history for `{name}` ignored: {error}"),
                )),
            }
        }

        (histories, diagnostics)
    }

    async fn save(
        &self,
        connection: &ConnectionName,
        entries: &[String],
    ) -> Result<(), StoreError> {
        let Some(path) = &self.path else {
            return Err(StoreError::Unavailable(
                "neither XDG_STATE_HOME nor HOME is set".to_owned(),
            ));
        };
        let failed =
            |error: std::io::Error| StoreError::Unavailable(format!("{}: {error}", path.display()));
        let _one_at_a_time = self.writes.lock().await;

        // A file that does not parse is replaced: it holds nothing the load
        // would have shown, and `load` has already said so.
        let mut sections = Self::read(path)
            .await
            .map_err(failed)?
            .and_then(|source| parse(&source).ok())
            .unwrap_or_default();
        sections.insert(connection.as_str().to_owned(), entries.to_vec());

        write(path, &render(&sections)).await.map_err(failed)
    }
}

/// Beside the target, so the rename stays on one filesystem, and named after
/// this process, so two of them cannot collide.
async fn write(path: &Path, contents: &str) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        let mut directory = tokio::fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        directory.mode(0o700);
        directory.create(parent).await?;
    }

    let temporary = path.with_file_name(format!(".history.{}.tmp", std::process::id()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);

    let mut file = options.open(&temporary).await?;
    let written = async {
        file.write_all(contents.as_bytes()).await?;
        file.flush().await
    }
    .await;

    if let Err(error) = written {
        let _ignored = tokio::fs::remove_file(&temporary).await;
        return Err(error);
    }

    tokio::fs::rename(&temporary, path).await.inspect_err(|_| {
        let _ignored = std::fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections(of: &[(&str, &[&str])]) -> Sections {
        of.iter()
            .map(|(name, entries)| {
                (
                    (*name).to_owned(),
                    entries.iter().map(|entry| (*entry).to_owned()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn the_file_is_under_the_state_directory() {
        assert_eq!(
            path(Some(Path::new("/s")), Some(Path::new("/h"))),
            Some(PathBuf::from("/s/lazycassandra/history.toml"))
        );
    }

    #[test]
    fn without_a_state_directory_it_falls_back_to_the_home_one() {
        assert_eq!(
            path(None, Some(Path::new("/h"))),
            Some(PathBuf::from("/h/.local/state/lazycassandra/history.toml"))
        );
        assert_eq!(path(None, None), None);
    }

    #[test]
    fn what_is_rendered_parses_back() {
        let held = sections(&[
            (
                "dev",
                &["SELECT 1;", "SELECT *\nFROM \"a\\b\" WHERE x = 'é';"],
            ),
            ("prod", &["SELECT 2;"]),
        ]);

        assert_eq!(parse(&render(&held)), Ok(held));
    }

    #[test]
    fn a_connection_with_nothing_is_left_out() {
        let rendered = render(&sections(&[("dev", &[]), ("prod", &["SELECT 2;"])]));

        assert!(!rendered.contains("dev"));
        assert!(rendered.contains("prod"));
    }

    #[test]
    fn a_name_that_needs_quoting_survives() {
        let held = sections(&[("we\"ird.name", &["SELECT 1;"])]);

        assert_eq!(parse(&render(&held)), Ok(held));
    }

    #[test]
    fn a_file_that_is_not_toml_is_an_error_not_a_panic() {
        assert!(parse("[connections.dev\nentries = ").is_err());
    }

    #[test]
    fn keys_nobody_declared_are_ignored() {
        let source = "future = 1\n[connections.dev]\nentries = [\"SELECT 1;\"]\nextra = true\n";

        assert_eq!(parse(source), Ok(sections(&[("dev", &["SELECT 1;"])])));
    }
}
