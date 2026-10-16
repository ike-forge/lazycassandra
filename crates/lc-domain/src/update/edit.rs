//! Editing a cell in place.

use crate::contents::{ContentsState, Subject};
use crate::edit::Field;
use crate::key;
use crate::key::RowKey;
use crate::model::{CqlValue, Key};
use crate::overlay::{Message, Overlay};
use crate::state::State;
use crate::style::Severity;

use super::{Effect, edit_line, raise, refuse_read_only, uncommitted};

/// Every refusal is spoken: a silent key teaches that the value is unreachable.
pub(super) fn start_edit(state: &mut State) -> Vec<Effect> {
    if !state.posture().permits_mutation() {
        refuse_read_only(state, "editing a value");
        return Vec::new();
    }

    match editable(state) {
        Ok(field) => {
            if let Some(Overlay::Inspect(inspector)) = &mut state.overlay {
                inspector.start_edit(field);
            }
        }
        // A modal would take the half-filled form with it.
        Err(refusal) if matches!(&state.overlay, Some(Overlay::Inspect(inspector)) if inspector.is_inserting()) =>
        {
            state.notify(Severity::Warning, refusal);
        }
        Err(refusal) => raise(
            state,
            Message {
                title: "Not editable".to_owned(),
                message: refusal,
                severity: Severity::Warning,
            },
        ),
    }

    Vec::new()
}

#[must_use]
pub(crate) fn editable_here(state: &State) -> bool {
    state.posture().permits_mutation() && editable(state).is_ok()
}

#[must_use]
pub(crate) fn has_uncommitted(state: &State) -> bool {
    uncommitted(state).0 > 0
}

fn editable(state: &State) -> Result<Field, String> {
    let Some(Overlay::Inspect(inspector)) = &state.overlay else {
        return Err(String::new());
    };
    // A typed statement's columns belong to no one table.
    let Subject::Table(table) = inspector.subject() else {
        return Err(
            "Only a table's own rows can be edited. This grid came from a typed statement."
                .to_owned(),
        );
    };
    let Some(described) = state
        .schema()
        .and_then(|schema| schema.keyspaces.get(&table.keyspace))
        .and_then(|keyspace| keyspace.tables.get(&table.table))
    else {
        return Err(format!(
            "The loaded schema no longer has {}.",
            table.qualified()
        ));
    };

    let Some((_, column, name)) = inspector.at_field() else {
        return Err("The cursor is not on a column of this table.".to_owned());
    };
    let Some(declared) = described.column(name) else {
        return Err(format!("{name} is not a column of {}.", table.qualified()));
    };

    let inserting = inspector.is_inserting();

    if !inserting && key::columns(described).iter().any(|(key, _)| *key == name) {
        return Err(format!(
            "{name} is part of the primary key. A key column cannot be updated."
        ));
    }
    if !crate::edit::editable(&declared.cql_type) {
        return Err(format!(
            "A column of type {} cannot be edited here.",
            declared.cql_type
        ));
    }
    if !inserting && row_key(state, described).is_none() {
        return Err("This row cannot be addressed: its primary key is not in the page.".to_owned());
    }

    let value = inspector
        .value_at_cursor()
        .map_or_else(String::new, |value| {
            match value {
                // A null starts empty: `null` is not what the cell holds.
                CqlValue::Null => String::new(),
                other => crate::cell::text(other),
            }
        });

    Ok(Field::opened(column, name, &declared.cql_type, value))
}

fn row_key(state: &State, described: &crate::model::Table) -> Option<RowKey> {
    let Some(Overlay::Inspect(inspector)) = &state.overlay else {
        return None;
    };
    let ContentsState::Loaded { page, .. } = state.contents.state() else {
        return None;
    };
    let key = key::columns(described);

    if key.is_empty() {
        return None;
    }

    // As the cluster sent it: a `WHERE` from an edited row would address a row
    // the cluster lacks.
    RowKey::of(&key, &page.page().columns, page.row(inspector.row())?)
}

pub(super) fn field_key(state: &mut State, key: Key) -> Vec<Effect> {
    let accepted = {
        let Some(Overlay::Inspect(inspector)) = &mut state.overlay else {
            return Vec::new();
        };

        match key {
            Key::Escape => {
                inspector.cancel_edit();
                return Vec::new();
            }
            Key::Enter if inspector.is_inserting() => {
                inspector.accept_new();
                return Vec::new();
            }
            Key::Enter => inspector.accept_edit(),
            _ => {
                if let Some(field) = inspector.field_mut() {
                    let before = field.line().text().to_owned();
                    edit_line(field.line_mut(), key);
                    if field.line().text() != before {
                        field.touch();
                    }
                }
                return Vec::new();
            }
        }
    };

    // Text that does not parse keeps the field open.
    let Some((at, column, edited)) = accepted else {
        return Vec::new();
    };

    let row = match &state.overlay {
        Some(Overlay::Inspect(inspector)) => inspector.row(),
        _ => return Vec::new(),
    };
    if let Some(page) = state.contents.loaded_mut() {
        page.set_edit(row, column, edited);

        let dirty = page.is_edited(row, column);
        if dirty {
            state.notify(
                Severity::Info,
                "edit staged, close the inspector and ctrl-s to commit",
            );
        } else if let Some(Overlay::Inspect(inspector)) = &mut state.overlay {
            inspector.unmark(at);
        }
    }

    Vec::new()
}
