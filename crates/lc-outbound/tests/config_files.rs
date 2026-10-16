#![cfg(feature = "integration")]

use std::path::{Path, PathBuf};

use lc_domain::ports::ConfigSource;
use lc_outbound::config::FileConfigSource;

/// Under the target dir, so runs leave nothing behind and cannot collide.
#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ignored = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path
}

#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the parent directory");
    }
    std::fs::write(path, contents).expect("a file to load");
}

#[tokio::test]
async fn a_file_written_after_the_source_was_built_is_still_read() {
    let xdg = scratch("files-late-global");
    let cwd = scratch("files-late-cwd");
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    write(
        &xdg.join("lazycassandra").join("config.toml"),
        "theme = \"ash-dark\"\n",
    );
    let (config, diagnostics) = source.load().await;

    assert_eq!(config.theme.slug(), "ash-dark");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[tokio::test]
async fn a_project_file_written_after_the_source_was_built_still_wins() {
    let xdg = scratch("files-late-local-xdg");
    let cwd = scratch("files-late-local-cwd");
    write(
        &xdg.join("lazycassandra").join("config.toml"),
        "theme = \"ash-dark\"\n",
    );
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    write(&cwd.join(".lazycassandra.toml"), "theme = \"quill-dark\"\n");
    let (config, _) = source.load().await;

    assert_eq!(
        config.theme.slug(),
        "quill-dark",
        "the project file wins whole (configuration design §3)"
    );
}

#[tokio::test]
async fn a_named_file_wins_over_the_project_and_user_files() {
    let xdg = scratch("files-named-xdg");
    let cwd = scratch("files-named-cwd");
    write(
        &xdg.join("lazycassandra").join("config.toml"),
        "theme = \"ash-dark\"\nicons = false\n",
    );
    write(&cwd.join(".lazycassandra.toml"), "theme = \"quill-dark\"\n");
    let named = cwd.join("elsewhere.toml");
    write(&named, "theme = \"beacon-dark\"\n");

    let (config, diagnostics) = FileConfigSource::new(&cwd, Some(&xdg), None)
        .named(&named)
        .load()
        .await;

    assert_eq!(config.theme.slug(), "beacon-dark");
    assert_eq!(
        config.icons,
        lc_domain::config::Icons::Off,
        "and the keys it leaves unstated still layer up"
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[tokio::test]
async fn a_named_file_that_is_not_there_is_a_diagnostic() {
    let cwd = scratch("files-named-missing");

    let (_, diagnostics) = FileConfigSource::new(&cwd, None, None)
        .named(&cwd.join("gone.toml"))
        .load()
        .await;

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0].message.contains("could not be read"),
        "{diagnostics:?}"
    );
}
