#![cfg(feature = "integration")]

use std::path::PathBuf;

use lc_domain::ports::{ExportError, Writer};
use lc_outbound::file::FileWriter;

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

#[tokio::test]
async fn writing_a_file_puts_every_chunk_in_it_in_order() {
    let path = scratch("write").join("out.csv");
    let mut sink = FileWriter
        .open(&path, false)
        .await
        .expect("a fresh path opens");

    sink.write("a,b\n").await.expect("a chunk is written");
    sink.write("1,2\n").await.expect("a chunk is written");
    sink.finish().await.expect("the file closes");

    assert_eq!(
        std::fs::read_to_string(&path).expect("it exists"),
        "a,b\n1,2\n"
    );
}

#[tokio::test]
async fn an_existing_file_is_refused_unless_overwriting_was_asked_for() {
    let path = scratch("exists").join("out.csv");
    std::fs::write(&path, "old").expect("a file to collide with");

    assert_eq!(
        FileWriter.open(&path, false).await.err(),
        Some(ExportError::Exists)
    );

    let mut sink = FileWriter
        .open(&path, true)
        .await
        .expect("overwriting opens");
    sink.write("new").await.expect("a chunk is written");
    sink.finish().await.expect("the file closes");

    assert_eq!(std::fs::read_to_string(&path).expect("it exists"), "new");
}

#[tokio::test]
async fn aborting_removes_the_partial_file() {
    let path = scratch("abort").join("out.json");
    let mut sink = FileWriter
        .open(&path, false)
        .await
        .expect("a fresh path opens");
    sink.write("[\n{").await.expect("a chunk is written");

    sink.abort().await;

    assert!(
        !path.exists(),
        "a truncated array is not a document (design §4)"
    );
}

#[tokio::test]
async fn a_path_in_a_directory_that_does_not_exist_is_an_io_failure_not_a_panic() {
    let path = scratch("missing").join("nope").join("out.csv");

    assert!(matches!(
        FileWriter.open(&path, false).await,
        Err(ExportError::Io(_))
    ));
}
