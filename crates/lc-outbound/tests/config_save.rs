#![cfg(feature = "integration")]

use std::path::{Path, PathBuf};

use lc_domain::ports::{ConfigSink, ConfigSource, ConfigWriteError};
use lc_domain::theme::{Ours, ThemeChoice};
use lc_outbound::config::FileConfigSource;

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
    std::fs::write(path, contents).expect("a file to edit");
}

#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("the file is still there")
}

fn global(xdg: &Path) -> PathBuf {
    xdg.join("lazycassandra").join("config.toml")
}

fn quill() -> ThemeChoice {
    ThemeChoice::Ours(Ours::QuillDark)
}

#[tokio::test]
async fn a_saved_theme_is_the_theme_the_next_load_reads() {
    let xdg = scratch("save-round-trip-xdg");
    let cwd = scratch("save-round-trip-cwd");
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    source.save_theme(&quill()).await.expect("a writable home");
    let (config, diagnostics) = source.load().await;

    assert_eq!(config.theme, quill(), "the whole point of the feature");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[tokio::test]
async fn the_project_file_is_the_one_rewritten_when_it_states_the_theme() {
    let xdg = scratch("save-local-xdg");
    let cwd = scratch("save-local-cwd");
    write(&global(&xdg), "theme = \"ash-dark\"\n");
    write(
        &cwd.join(".lazycassandra.toml"),
        "theme = \"beacon-dark\"\n",
    );
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    source
        .save_theme(&quill())
        .await
        .expect("a writable project file");

    assert_eq!(
        read(&cwd.join(".lazycassandra.toml")),
        "theme = \"quill-dark\"\n",
        "writing the global file would save a value that loses on the next launch"
    );
    assert_eq!(
        read(&global(&xdg)),
        "theme = \"ash-dark\"\n",
        "and the file that does not win is not touched"
    );
}

#[tokio::test]
async fn a_project_file_that_states_no_theme_leaves_the_global_file_the_target() {
    let xdg = scratch("save-local-silent-xdg");
    let cwd = scratch("save-local-silent-cwd");
    write(&global(&xdg), "theme = \"ash-dark\"\n");
    write(&cwd.join(".lazycassandra.toml"), "icons = false\n");
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    source.save_theme(&quill()).await.expect("a writable home");

    assert_eq!(read(&global(&xdg)), "theme = \"quill-dark\"\n");
    assert_eq!(read(&cwd.join(".lazycassandra.toml")), "icons = false\n");
}

#[tokio::test]
async fn a_file_that_does_not_parse_is_left_exactly_as_it_was() {
    let xdg = scratch("save-unparsable");
    let cwd = scratch("save-unparsable-cwd");
    write(&global(&xdg), "theme = \n");
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    let error = source
        .save_theme(&quill())
        .await
        .expect_err("it does not parse");

    assert!(
        matches!(error, ConfigWriteError::Unparsable(..)),
        "{error:?}"
    );
    assert_eq!(read(&global(&xdg)), "theme = \n", "not one byte of it");
    assert!(
        error.to_string().contains("config.toml"),
        "the message names the file: {error}"
    );
}

#[tokio::test]
async fn nowhere_to_write_is_an_error_and_not_a_panic() {
    let cwd = scratch("save-nowhere");
    let source = FileConfigSource::new(&cwd, None, None);

    assert_eq!(
        source.save_theme(&quill()).await,
        Err(ConfigWriteError::NoPath)
    );
}

/// The write must fail as a write, not as a half-finished one. The temporary
/// file is created beside the target, so a directory that cannot be written is
/// where this is felt (design §4).
#[tokio::test]
async fn a_target_that_cannot_be_written_fails_and_damages_nothing() {
    use std::os::unix::fs::PermissionsExt;

    let xdg = scratch("save-readonly-xdg");
    let cwd = scratch("save-readonly-cwd");
    let path = global(&xdg);
    write(&path, "theme = \"ash-dark\"\n");
    let directory = path.parent().expect("a parent").to_path_buf();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500))
        .expect("a read-only config directory");
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    let error = source
        .save_theme(&quill())
        .await
        .expect_err("nothing can be written here");

    assert!(matches!(error, ConfigWriteError::Io(_)), "{error:?}");
    assert_eq!(
        read(&path),
        "theme = \"ash-dark\"\n",
        "the file it could not replace is intact"
    );
    assert!(
        std::fs::read_dir(&directory)
            .expect("the directory")
            .flatten()
            .all(|entry| entry.file_name() == "config.toml"),
        "and no temporary file is left behind"
    );

    // Restored so the next run's `scratch` can clear the directory.
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
        .expect("the directory back");
}

/// A `config.toml` symlinked into a dotfiles repository is the ordinary case
/// for anyone who keeps one. Renaming over the link would replace it with a
/// regular file and quietly detach the user's config from their repository
/// (design §4).
#[tokio::test]
async fn a_symlinked_config_keeps_its_symlink() {
    let xdg = scratch("save-symlink-xdg");
    let cwd = scratch("save-symlink-cwd");
    let dotfiles = scratch("save-symlink-dotfiles");
    let real = dotfiles.join("config.toml");
    write(&real, "theme = \"ash-dark\"\n");
    let link = global(&xdg);
    std::fs::create_dir_all(link.parent().expect("a parent")).expect("the config directory");
    std::os::unix::fs::symlink(&real, &link).expect("a symlink");
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    source
        .save_theme(&quill())
        .await
        .expect("a writable target");

    assert!(
        std::fs::symlink_metadata(&link)
            .expect("the link")
            .file_type()
            .is_symlink(),
        "the link must survive the write"
    );
    assert_eq!(read(&real), "theme = \"quill-dark\"\n");
}

/// A config file may hold a `password` literal (configuration design §2.2), so
/// a save must not widen it from 0600 to whatever the umask says.
#[tokio::test]
async fn the_files_permissions_are_the_ones_it_had() {
    use std::os::unix::fs::PermissionsExt;

    let xdg = scratch("save-mode-xdg");
    let cwd = scratch("save-mode-cwd");
    let path = global(&xdg);
    write(&path, "theme = \"ash-dark\"\n");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .expect("a private config");
    let source = FileConfigSource::new(&cwd, Some(&xdg), None);

    source
        .save_theme(&quill())
        .await
        .expect("a writable target");

    assert_eq!(
        std::fs::metadata(&path)
            .expect("the file")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
