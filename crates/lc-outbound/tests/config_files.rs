#![cfg(feature = "integration")]

use std::path::{Path, PathBuf};

use lc_domain::ports::ConfigSource;
use lc_outbound::config::FileConfigSource;

/// A directory under the target dir, so a run leaves nothing in the user's
/// tree and two runs cannot collide on a name.
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

/// The save writes a file that may not have existed when the source was built,
/// and the next load must see it. A source that snapshots existence in its
/// constructor reads a stale disk (theme persistence design §3).
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

/// And the project file, discovered by the same walk, has the same property.
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
