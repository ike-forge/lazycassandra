#![cfg(feature = "integration")]

use std::path::{Path, PathBuf};

use lc_outbound::config::write_example_config;

const EXAMPLE: &str = include_str!("../src/config/example.toml");

/// A directory under the target dir, so a run leaves nothing in the
/// user's tree and two runs cannot collide on a name.
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

fn config_in(xdg: &Path) -> PathBuf {
    xdg.join("lazycassandra").join("config.toml")
}

#[test]
fn a_first_run_writes_the_example_and_the_directory_holding_it() {
    let xdg = scratch("example-fresh");

    write_example_config(Some(&xdg), None);

    assert_eq!(
        std::fs::read_to_string(config_in(&xdg)).expect("the example was written"),
        EXAMPLE
    );
}

#[test]
fn an_existing_config_is_never_overwritten() {
    let xdg = scratch("example-existing");
    let path = config_in(&xdg);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the config directory");
    std::fs::write(&path, "theme = \"ash-dark\"\n").expect("a config to protect");

    write_example_config(Some(&xdg), None);

    assert_eq!(
        std::fs::read_to_string(&path).expect("it is still there"),
        "theme = \"ash-dark\"\n",
        "the user's config is the one thing this feature must never touch"
    );
}

#[test]
fn a_write_that_cannot_happen_is_silent() {
    let xdg = scratch("example-blocked");
    let path = config_in(&xdg);
    std::fs::create_dir_all(&path).expect("a directory sitting where the file belongs");

    write_example_config(Some(&xdg), None);

    assert!(
        path.is_dir(),
        "a failed write leaves what was there alone and does not panic"
    );
}

#[test]
fn no_config_directory_to_resolve_means_no_write() {
    write_example_config(None, None);
}

#[test]
fn a_symlink_pre_created_at_the_path_is_not_followed() {
    let xdg = scratch("example-symlink");
    let path = config_in(&xdg);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the config directory");
    let target = xdg.join("elsewhere.toml");
    std::os::unix::fs::symlink(&target, &path).expect("a symlink to pre-create");

    write_example_config(Some(&xdg), None);

    assert!(
        !target.exists(),
        "create_new must fail on a path that already exists as a symlink, without ever \
         following it to create or write the target"
    );
    assert!(
        path.symlink_metadata()
            .expect("the symlink is still there")
            .file_type()
            .is_symlink(),
        "a blocked write must leave the pre-created symlink exactly as it was"
    );
}
