#![cfg(feature = "integration")]

use std::path::PathBuf;

use lc_domain::config::{ConnectionName, HistoryLimit};
use lc_domain::ports::Store;
use lc_outbound::history::FileStore;

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

#[allow(clippy::expect_used, reason = "see scratch")]
fn name(name: &str) -> ConnectionName {
    ConnectionName::try_new(name).expect("a valid name")
}

#[allow(clippy::expect_used, reason = "see scratch")]
fn limit(rows: u32) -> HistoryLimit {
    HistoryLimit::try_new(rows).expect("in range")
}

fn entries(of: &[&str]) -> Vec<String> {
    of.iter().map(|entry| (*entry).to_owned()).collect()
}

#[tokio::test]
async fn what_one_run_saves_the_next_loads() {
    let path = scratch("history-roundtrip").join("state/history.toml");
    let store = FileStore::at(Some(path.clone()));

    store
        .save(&name("dev"), &entries(&["SELECT 2;", "SELECT\n1;"]))
        .await
        .expect("the first save creates the directory and the file");

    let (histories, diagnostics) = FileStore::at(Some(path)).load(limit(100)).await;

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(histories.of(&name("dev")), ["SELECT 2;", "SELECT\n1;"]);
}

#[tokio::test]
async fn saving_one_connection_leaves_the_others_alone() {
    let path = scratch("history-others").join("history.toml");
    let store = FileStore::at(Some(path));

    store
        .save(&name("dev"), &entries(&["SELECT 1;"]))
        .await
        .expect("saved");
    store
        .save(&name("prod"), &entries(&["SELECT 2;"]))
        .await
        .expect("saved");
    store
        .save(&name("dev"), &entries(&["SELECT 3;"]))
        .await
        .expect("saved");

    let (histories, _) = store.load(limit(100)).await;

    assert_eq!(histories.of(&name("dev")), ["SELECT 3;"]);
    assert_eq!(histories.of(&name("prod")), ["SELECT 2;"]);
}

#[tokio::test]
async fn a_lower_limit_trims_on_load() {
    let path = scratch("history-limit").join("history.toml");
    let store = FileStore::at(Some(path));
    store
        .save(
            &name("dev"),
            &entries(&["SELECT 3;", "SELECT 2;", "SELECT 1;"]),
        )
        .await
        .expect("saved");

    let (histories, _) = store.load(limit(2)).await;

    assert_eq!(histories.of(&name("dev")), ["SELECT 3;", "SELECT 2;"]);
}

#[tokio::test]
async fn no_file_yet_is_an_empty_history_and_no_complaint() {
    let path = scratch("history-absent").join("history.toml");

    let (histories, diagnostics) = FileStore::at(Some(path)).load(limit(100)).await;

    assert!(histories.of(&name("dev")).is_empty());
    assert!(diagnostics.is_empty());
}

#[tokio::test]
async fn a_file_that_is_not_toml_is_reported_and_replaced_by_the_next_save() {
    let path = scratch("history-corrupt").join("history.toml");
    std::fs::write(&path, "[connections.dev\nentries = ").expect("a corrupt file");
    let store = FileStore::at(Some(path.clone()));

    let (histories, diagnostics) = store.load(limit(100)).await;

    assert!(histories.of(&name("dev")).is_empty());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("replaced"));

    store
        .save(&name("dev"), &entries(&["SELECT 1;"]))
        .await
        .expect("a corrupt file does not block a save");
    let (histories, diagnostics) = store.load(limit(100)).await;

    assert!(diagnostics.is_empty());
    assert_eq!(histories.of(&name("dev")), ["SELECT 1;"]);
}

#[tokio::test]
async fn with_nowhere_to_write_a_save_fails_and_a_load_is_empty() {
    let store = FileStore::at(None);

    assert!(
        store
            .save(&name("dev"), &entries(&["SELECT 1;"]))
            .await
            .is_err()
    );
    assert!(store.load(limit(100)).await.1.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn the_file_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;

    let directory = scratch("history-mode").join("state");
    let path = directory.join("history.toml");

    FileStore::at(Some(path.clone()))
        .save(
            &name("dev"),
            &entries(&["CREATE ROLE x WITH PASSWORD = 'p';"]),
        )
        .await
        .expect("saved");

    let mode = |path: &PathBuf| {
        std::fs::metadata(path)
            .expect("exists")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&directory), 0o700);
}
