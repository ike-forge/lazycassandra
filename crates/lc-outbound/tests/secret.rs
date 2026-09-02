//! `SecretResolver` against the real environment and filesystem.
//!
//! The integration tier rather than the fast one: these read the world, and
//! `Credential::Literal` — the only variant that does not — is covered by the
//! unit tests beside the implementation (architecture.md §10).
#![cfg(feature = "integration")]

use std::path::PathBuf;

use lc_domain::config::Credential;
use lc_domain::ports::{ConfigError, SecretResolver};
use lc_outbound::secret::Secrets;

/// Under the target dir, so a run leaves nothing in the user's tree — the
/// convention `tests/file.rs` set.
#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("secret");
    std::fs::create_dir_all(&path).expect("a scratch directory");
    path.join(name)
}

#[tokio::test]
async fn a_file_credential_is_read_from_disk() {
    let path = scratch("plain");
    std::fs::write(&path, "hunter2").expect("the file is written");

    let resolved = Secrets.resolve(&Credential::File(path)).await;

    assert_eq!(resolved, Ok("hunter2".to_owned()));
}

#[tokio::test]
async fn a_file_credential_drops_the_trailing_newline_echo_left_behind() {
    let path = scratch("newline");
    std::fs::write(&path, "hunter2\n").expect("the file is written");

    let resolved = Secrets.resolve(&Credential::File(path)).await;

    assert_eq!(
        resolved,
        Ok("hunter2".to_owned()),
        "a password file written by `echo` ends in a byte the cluster was never meant to receive"
    );
}

#[tokio::test]
async fn a_file_credential_keeps_the_spaces_a_password_may_legitimately_carry() {
    let path = scratch("spaces");
    std::fs::write(&path, " hunter2 \n").expect("the file is written");

    let resolved = Secrets.resolve(&Credential::File(path)).await;

    assert_eq!(
        resolved,
        Ok(" hunter2 ".to_owned()),
        "only the line ending is stripped, not every surrounding byte"
    );
}

#[tokio::test]
async fn a_missing_file_names_the_path_it_looked_at() {
    let path = scratch("absent");
    let _ignored = std::fs::remove_file(&path);

    let resolved = Secrets.resolve(&Credential::File(path.clone())).await;

    match resolved {
        Err(ConfigError::Unresolvable(message)) => {
            assert!(message.contains(&path.display().to_string()));
        }
        other => panic!("a missing file is unresolvable, got {other:?}"),
    }
}

#[tokio::test]
async fn an_environment_credential_is_read_from_the_environment() {
    // `PATH` rather than a variable the test sets: `std::env::set_var` is
    // `unsafe` in edition 2024, which `unsafe_code = "forbid"` rules out, and
    // a test that skips itself when a variable is unset is an ignored test.
    let expected = std::env::var("PATH").expect("a process has a PATH");

    let resolved = Secrets.resolve(&Credential::Env("PATH".to_owned())).await;

    assert_eq!(resolved, Ok(expected));
}
