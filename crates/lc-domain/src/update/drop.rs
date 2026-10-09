//! Dropping a keyspace or table, or deleting rows
//! (docs/superpowers/specs/2026-08-31-drop-design.md).

use crate::contents::{ContentsState, Subject, TableRef};
use crate::drop::{DropPrompt, Target};
use crate::log::{Outcome, Trace};
use crate::model::{DbError, Key, QueryId, Row, Statement};
use crate::state::{Dropping, Message, Overlay, Panel, State};
use crate::style::Severity;
use crate::tree::Node;

use super::{
    Decision, Effect, edit_line, logged, raise, refuse_read_only, sentence, spanned,
    statement_subject, submit, submit_into_contents,
};

/// What `d` would aim at in this frame, or `None` where it would open
/// nothing.
///
/// One derivation, read by the key that opens the confirmation and by the
/// hint bar that greys it (`view::hints::liveness`), so the bar cannot
/// promise a question the key refuses (drop design §1).
///
/// It answers only what the *cursor* names. The refusals that are about the
/// session rather than about the cursor — the posture, the frame, a drop
/// already running — are [`open_drop`]'s, because the bar greys the first two
/// and shows the third as a running job.
pub(crate) fn drop_target(state: &State) -> Option<Target> {
    match state.focus() {
        Panel::Tables => tree_target(state),
        Panel::Contents => rows_target(state),
        Panel::Ddl | Panel::Queries => None,
    }
}

/// The node under the tree cursor. Not the range: `v` selects rows for `y`,
/// and "drop these four" is a different question with a different
/// confirmation (design §1).
pub(super) fn tree_target(state: &State) -> Option<Target> {
    if state.tables.selects_many() {
        return None;
    }

    match state.tables.selected(state.schema()?)? {
        Node::Keyspace { keyspace, .. } => Some(Target::Keyspace(keyspace.name.clone())),
        Node::Table { keyspace, table } => {
            Some(Target::Table(TableRef::new(&keyspace.name, &table.name)))
        }
    }
}

/// The grid's selected rows, addressed by their primary key — `None` where
/// that key is not known or not present (design §3.1).
///
/// The range, here, and not only the cursor: the tree's `v` selects rows of a
/// list and a delete of several is a different statement, while the grid's
/// `v` selects the rows a command acts on and `y` already reads it that way
/// (yank selection modes design §1). Whole rows in every selection mode — a
/// column range addresses no row, and a cell is not a thing `DELETE` removes.
pub(super) fn rows_target(state: &State) -> Option<Target> {
    let ContentsState::Loaded { subject, page, .. } = state.contents.state() else {
        return None;
    };
    // A typed statement names its table in text this program will not trust
    // for a `WHERE` clause it writes itself (design §3.1).
    let Subject::Table(table) = subject else {
        return None;
    };
    let described = state
        .schema()?
        .keyspaces
        .get(&table.keyspace)?
        .tables
        .get(&table.table)?;

    let selected = spanned(&state.contents.rows, state.contents.cursor.index());
    // `GridPage::row`, not `page.rows[index]`: the cursor and the range count
    // drawn rows, and only the page resolves one through its arrangement. A
    // sorted or filtered grid otherwise confirms one row and deletes another
    // (grid sort and filter design §4).
    let rows: Vec<&Row> = selected
        .indices()
        .map(|index| page.row(index))
        .collect::<Option<Vec<_>>>()?;

    Target::rows(table, described, &page.page().columns, rows.into_iter())
}

/// `d` opens the confirmation over whatever [`drop_target`] names.
///
/// Refused, silently, wherever the question it would ask cannot be answered:
/// a frame with no room to draw the block, no schema to name a node from, a
/// drop already in flight, and a range of more than one row — "drop these
/// four" is a different question with a different confirmation (drop design
/// §1). Those are greyed on the hint bar, so the refusal is visible before
/// the key is pressed (`view::hints::liveness`).
///
/// The posture is the one refusal that is *not* silent. It is greyed too, but
/// a key that is grey because of how the connection was opened teaches
/// nothing about which connection that is, and a user who pressed it anyway
/// is owed the reason (posture design §3).
pub(super) fn open_drop(state: &mut State) {
    if !state.posture().permits_mutation() {
        refuse_read_only(state, "drop and delete");
        return;
    }

    if state.dropping.is_some() || !state.geometry.drop_fits() {
        return;
    }

    let Some(target) = drop_target(state) else {
        return;
    };

    state.overlay = Some(Overlay::Drop(DropPrompt::opened(target, state.posture())));
    state.dirty = true;
}

/// Every key the drop confirmation claims.
///
/// The two spellings answer differently and this is the one place that says
/// so: `y` runs the yes/no question, and `⏎` runs the typed one only once the
/// name matches — a keystroke landing in a field that does not yet say what
/// it was asked to say must not drop anything (drop design §2.1).
pub(super) fn drop_key(state: &mut State, key: Key) -> Vec<Effect> {
    let decision = {
        let Some(Overlay::Drop(prompt)) = &mut state.overlay else {
            return Vec::new();
        };

        if prompt.takes_text() {
            typed_key(prompt, key)
        } else {
            yes_no_key(key)
        }
    };

    state.dirty = true;

    match decision {
        Decision::Nothing => Vec::new(),
        Decision::Close => {
            state.overlay = None;
            Vec::new()
        }
        Decision::Run => start_drop(state),
    }
}

/// The yes/no spelling. Everything it does not use is inert rather than
/// dismissing, the rule the palette already follows (command palette design
/// §7).
pub(super) const fn yes_no_key(key: Key) -> Decision {
    match key {
        Key::Char('y') => Decision::Run,
        Key::Char('n') | Key::Escape => Decision::Close,
        _ => Decision::Nothing,
    }
}

/// The typed spelling. The catch-all is last and unguarded: every printable
/// key is text here, `n` included — a keyspace named `nation` could not be
/// typed otherwise (drop design §2.1).
pub(super) fn typed_key(prompt: &mut DropPrompt, key: Key) -> Decision {
    match key {
        Key::Escape => Decision::Close,
        Key::Enter if prompt.confirmed() => Decision::Run,
        Key::Enter => Decision::Nothing,
        _ => {
            if let Some(line) = prompt.typed_mut() {
                edit_line(line, key);
            }
            Decision::Nothing
        }
    }
}

/// Turns the answered confirmation into the statement it was asking about.
///
/// `submit` is where the posture guard lives, so [`open_drop`]'s own gate is
/// convenience rather than enforcement and a posture that changed under a
/// held-open confirmation is still refused here (drop design §3).
pub(super) fn start_drop(state: &mut State) -> Vec<Effect> {
    let Some(Overlay::Drop(prompt)) = state.overlay.take() else {
        return Vec::new();
    };

    let target = prompt.target().clone();
    let stmt = prompt.statement();
    let log_subject = statement_subject(&stmt);

    match submit(state, stmt) {
        Ok((query, effects)) => {
            state.dropping = Some(Dropping::started(query, target));
            effects
        }
        Err(error) => {
            // Logged before the modal, not instead of it: the modal is gone
            // the moment it is dismissed, and the trail is where the reason
            // survives (query log design §1, design §5).
            logged(
                state,
                log_subject,
                Outcome::Rejected {
                    reason: error.to_string(),
                },
            );
            raise(
                state,
                Message {
                    title: "Not dropped".to_owned(),
                    message: error.to_string(),
                    severity: Severity::Danger,
                },
            );
            Vec::new()
        }
    }
}

/// A drop ended. `error` is `None` on the success the cluster reported.
///
/// Dropped where `query` is not the drop that is outstanding, which is every
/// ordinary read: this is called from both result arms, and the id is the
/// only thing that separates a drop's answer from a grid's
/// (architecture.md §3).
///
/// The success arm re-issues `Effect::LoadSchema`: the tree named a node that
/// no longer exists the moment the statement succeeded, and `schema_loaded`
/// clamps the cursor to what is left (drop design §4). Nothing is reloaded on
/// a failure — the schema did not change.
pub(super) fn finish_drop(
    state: &mut State,
    query: QueryId,
    error: Option<&DbError>,
) -> Vec<Effect> {
    if state.dropping.as_ref().map(Dropping::query) != Some(query) {
        return Vec::new();
    }
    let Some(dropped) = state.dropping.take() else {
        return Vec::new();
    };
    state.dirty = true;

    if let Some(error) = error {
        raise(
            state,
            Message {
                title: "Not dropped".to_owned(),
                message: error.to_string(),
                severity: Severity::Danger,
            },
        );
        return Vec::new();
    }

    raise(
        state,
        Message {
            title: match dropped.target() {
                Target::Rows { .. } => "Deleted".to_owned(),
                Target::Keyspace(_) | Target::Table(_) => "Dropped".to_owned(),
            },
            message: format!("{} is gone", dropped.target().qualified()),
            severity: Severity::Info,
        },
    );

    // What went stale is what gets re-read. A drop changed the shape of the
    // cluster and the tree is describing it; a delete changed the rows of one
    // table and the grid is showing them (design §4).
    match dropped.target() {
        Target::Rows { table, .. } => {
            let table = table.clone();
            let stmt = Statement::select_all(&table.keyspace, &table.table);

            submit_into_contents(state, Subject::Table(table), stmt)
        }
        Target::Keyspace(_) | Target::Table(_) => {
            let Some(conn) = state.conn_id() else {
                return Vec::new();
            };
            logged(
                state,
                sentence("Reading schema"),
                Outcome::Pending {
                    trace: Trace::Schema(conn),
                },
            );

            vec![Effect::LoadSchema(conn)]
        }
    }
}
