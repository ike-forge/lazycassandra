//! Whole sessions that end in a file
//! (docs/superpowers/specs/2026-08-14-export-design.md §9).

use lc_domain::clock::Stamp;
use lc_domain::config::Config;
use lc_domain::export::{Confirm, Format, Stop};
use lc_domain::model::{ConnId, CqlValue, DbError, Key, KeyPress, Page, PagingState, Row};
use lc_domain::posture::Posture;
use lc_domain::state::{Geometry, Message, Overlay};
use lc_domain::style::Severity;
use lc_domain::update::ROW_LIMIT;
use lc_domain::{Effect, Msg, Source, State, init, update};
use lc_outbound::fake::{FakeCluster, FakeWriter};

mod support;
use support::{press, run_with_writer, tabled_schema_of};

/// `count` rows, `ROW_LIMIT` of them exactly, so the panel's read is
/// incomplete and the form's fetched-rows scope is reachable at all
/// (design §3): a complete read makes the two scopes write the same file
/// and disables the choice between them.
fn full_page(count: usize) -> Page {
    Page {
        columns: vec!["id".to_owned(), "email".to_owned()],
        rows: (0..count)
            .map(|i| Row {
                values: vec![
                    CqlValue::Uuid(format!("id-{i}")),
                    CqlValue::Text(format!("user{i}@example.com")),
                ],
            })
            .collect(),
        next: None,
    }
}

fn page(rows: &[(&str, &str)]) -> Page {
    Page {
        columns: vec!["id".to_owned(), "email".to_owned()],
        rows: rows
            .iter()
            .map(|(id, email)| Row {
                values: vec![
                    CqlValue::Uuid((*id).to_owned()),
                    CqlValue::Text((*email).to_owned()),
                ],
            })
            .collect(),
        next: None,
    }
}

/// Three pages, one row each, walked by `execute_paged` in order.
fn three_pages() -> Vec<Page> {
    vec![
        Page {
            next: Some(PagingState(vec![1])),
            ..page(&[("a1", "ada@example.com")])
        },
        Page {
            next: Some(PagingState(vec![2])),
            ..page(&[("a2", "bob@example.com")])
        },
        Page {
            next: None,
            ..page(&[("a3", "cleo@example.com")])
        },
    ]
}

/// Connect, describe, unfold `shop`, step onto `orders`, and read it — the
/// state every export scenario opens its form from.
async fn on_a_read_table(cluster: &FakeCluster) -> State {
    let conn = ConnId::next();
    let (mut state, effects) = init(conn, Config::default(), Vec::new(), Stamp::default());
    run_with_writer(cluster, None, &mut state, effects).await;

    let effects = press(&mut state, Key::Char('l'));
    run_with_writer(cluster, None, &mut state, effects).await;
    let effects = press(&mut state, Key::Char('l'));
    run_with_writer(cluster, None, &mut state, effects).await;
    let effects = press(&mut state, Key::Enter);
    run_with_writer(cluster, None, &mut state, effects).await;

    // A frame with room for the form: `ctrl-e` is refused where nothing would
    // draw one (design §2).
    update(
        &mut state,
        Msg::Measured(Geometry::new([8, 14, 20, 1], 37, 14).with_export_fits(true)),
    );

    state
}

fn ctrl_e(state: &mut State) {
    update(state, Msg::Input(KeyPress::ctrl(Key::Char('e'))));
}

fn run_export_form(state: &mut State, format: Format) -> Vec<Effect> {
    ctrl_e(state);
    press(state, Key::Tab); // Scope -> Format

    if format == Format::Json {
        press(state, Key::Char(' ')); // toggle Csv -> Json
    }

    press(state, Key::Tab); // Format -> Path
    press(state, Key::Tab); // Path -> Export
    press(state, Key::Char(' ')) // run
}

fn expect_export(effects: &[Effect]) -> &Effect {
    let [effect @ Effect::Export { .. }] = effects else {
        panic!("expected exactly one Effect::Export, got {effects:?}");
    };
    effect
}

fn expect_finished(state: &State) -> &Message {
    match &state.overlay {
        Some(Overlay::Message(message)) => message,
        other => panic!("expected the finish message on screen, got {other:?}"),
    }
}

#[tokio::test]
async fn a_whole_table_export_writes_a_header_and_every_page() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(page(&[("seed", "seed@example.com")]))
    .paging(three_pages());
    let writer = FakeWriter::default();
    let mut state = on_a_read_table(&cluster).await;

    let effects = run_export_form(&mut state, Format::Csv);
    let export = expect_export(&effects).clone();
    assert!(matches!(
        export,
        Effect::Export {
            source: Source::Whole(_),
            overwrite: false,
            ..
        }
    ));

    run_with_writer(&cluster, Some(&writer), &mut state, effects).await;

    assert_eq!(
        writer.written(),
        "id,email\na1,ada@example.com\na2,bob@example.com\na3,cleo@example.com\n"
    );
    let finished = expect_finished(&state);
    assert_eq!(finished.severity, Severity::Info, "Ok(3), not a failure");
    assert!(
        finished.message.contains("wrote 3 rows"),
        "the finish message carried Ok(3): {finished:?}"
    );
}

#[tokio::test]
async fn a_fetched_export_asks_the_cluster_nothing() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(full_page(ROW_LIMIT));
    let writer = FakeWriter::default();
    let mut state = on_a_read_table(&cluster).await;
    let read = cluster.executed();

    ctrl_e(&mut state);
    press(&mut state, Key::Char(' ')); // toggle Scope: Whole -> Fetched
    press(&mut state, Key::Tab);
    press(&mut state, Key::Tab);
    press(&mut state, Key::Tab);
    let effects = press(&mut state, Key::Char(' '));

    let export = expect_export(&effects).clone();
    assert!(matches!(
        export,
        Effect::Export {
            source: Source::Fetched { .. },
            ..
        }
    ));

    run_with_writer(&cluster, Some(&writer), &mut state, effects).await;

    assert_eq!(
        cluster.executed(),
        read,
        "the export asked the cluster nothing beyond the panel's original read"
    );

    let written = writer.written();
    let mut lines = written.lines();
    assert_eq!(lines.next(), Some("id,email"));
    assert_eq!(
        lines.clone().count(),
        ROW_LIMIT,
        "the file holds exactly the panel's rows, one line each"
    );
    assert_eq!(lines.next(), Some("id-0,user0@example.com"));
    assert_eq!(
        lines.last(),
        Some(format!(
            "id-{last},user{last}@example.com",
            last = ROW_LIMIT - 1
        ))
        .as_deref()
    );
}

#[tokio::test]
async fn a_json_export_is_a_document() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(page(&[("seed", "seed@example.com")]))
    .paging(three_pages());
    let writer = FakeWriter::default();
    let mut state = on_a_read_table(&cluster).await;

    let effects = run_export_form(&mut state, Format::Json);
    run_with_writer(&cluster, Some(&writer), &mut state, effects).await;

    let written = writer.written();
    assert!(written.starts_with("[\n"), "{written}");
    assert!(written.ends_with("\n]\n"), "{written}");
    assert_eq!(
        written.matches(",\n").count(),
        2,
        "three objects need exactly two separators between them: {written}"
    );
}

#[tokio::test]
async fn an_existing_destination_asks_before_it_clobbers() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(page(&[("seed", "seed@example.com")]));
    let writer = FakeWriter::refusing_because_it_exists();
    let mut state = on_a_read_table(&cluster).await;

    let effects = run_export_form(&mut state, Format::Csv);
    run_with_writer(&cluster, Some(&writer), &mut state, effects).await;

    let Some(Overlay::Export(form)) = &state.overlay else {
        panic!("the form is back up asking to overwrite");
    };
    assert_eq!(form.confirm(), Some(Confirm::Overwrite));
    assert_eq!(form.stop(), Stop::Export);

    let effects = press(&mut state, Key::Char(' '));
    let [Effect::Export { overwrite, .. }] = effects.as_slice() else {
        panic!("pressing space re-emits the export");
    };
    assert!(*overwrite);
}

/// The last thing an export does is flush, which is where a full disk shows
/// up — and where the file on disk is likeliest to be a truncated one. The
/// user got `Export failed` and kept the truncation (final review I4).
#[tokio::test]
async fn a_finish_that_fails_leaves_no_partial_file_either() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(page(&[("seed", "seed@example.com")]))
    .paging(three_pages());
    let writer = FakeWriter::failing_to_finish();
    let mut state = on_a_read_table(&cluster).await;

    let effects = run_export_form(&mut state, Format::Json);
    run_with_writer(&cluster, Some(&writer), &mut state, effects).await;

    assert!(writer.aborted(), "the partial file goes with the failure");
    assert_eq!(writer.written(), "", "abort took the truncated document");
    let finished = expect_finished(&state);
    assert_eq!(finished.severity, Severity::Danger);
    assert!(
        finished.message.contains("no space left on device"),
        "the flush's own failure reaches the user: {finished:?}"
    );
}

#[tokio::test]
async fn a_cluster_that_fails_mid_export_leaves_no_partial_file() {
    let cluster = FakeCluster::new(
        tabled_schema_of(&[("shop", &["orders"])]),
        Posture::ReadOnly,
    )
    .returning(page(&[("seed", "seed@example.com")]))
    .paging(three_pages())
    .failing_page(1, DbError::Unreachable("connection reset".to_owned()));
    let writer = FakeWriter::default();
    let mut state = on_a_read_table(&cluster).await;

    let effects = run_export_form(&mut state, Format::Csv);
    run_with_writer(&cluster, Some(&writer), &mut state, effects).await;

    assert!(writer.aborted(), "the sink was aborted");
    assert!(
        !writer.finished(),
        "a run that aborts must never also finish"
    );
    let finished = expect_finished(&state);
    assert_eq!(
        finished.severity,
        Severity::Danger,
        "the finish message carried an Err"
    );
    assert!(
        finished.message.contains("connection reset"),
        "the cluster's own failure reaches the user: {finished:?}"
    );
}
