//! The Table DDL panel's projection: what the Tables cursor is on, described.
//!
//! A table leads with its physical shape and follows with the properties
//! `system_schema.tables` reports; a keyspace shows its replication. Nothing
//! selected shows nothing — the connection's state is the Tables panel's job
//! to narrate, and saying it twice in one sidebar is noise
//! (doc/specs/2026-08-09-table-metadata-design.md §1).

use crate::model::{ClusteringOrder, Keyspace, Replication, Table};
use crate::state::State;
use crate::style::Token;
use crate::tree::Node;

use super::PanelBody;

/// A heading and the groups beneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    /// `shop.orders_by_customer`, or a keyspace name.
    pub heading: String,
    pub sections: Vec<Section>,
}

impl Details {
    /// Every row the panel describes, in the order it draws them — which is
    /// the order the cursor walks. A heading is not a row: it labels the group
    /// beneath it, and there is nothing a cursor on one could mean
    /// (doc/specs/2026-08-10-ddl-selection-design.md §3).
    pub fn rows(&self) -> impl Iterator<Item = &DetailRow> {
        self.sections.iter().flat_map(|section| section.rows.iter())
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        self.sections.iter().map(|section| section.rows.len()).sum()
    }
}

/// One labelled group of rows.
///
/// Grouping is structural rather than a blank-line marker in a flat list, so
/// the vertical rhythm cannot be forgotten or emitted twice, and a group with
/// nothing in it — a table with no clustering columns — disappears by being
/// empty rather than by a projection remembering to skip it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// `Partition Key`, `Clustering Key`, `Options`, `Replication`.
    pub heading: String,
    /// What the group is, which decides both the blank line above it and
    /// whether its rows read as key structure.
    pub group: Group,
    /// Styles the heading and every row beneath it.
    pub token: Option<Token>,
    pub rows: Vec<DetailRow>,
}

/// Which half of the panel a section belongs to.
///
/// The panel reads as two groups — what the table *is*, then what it is
/// configured to do — and the blank line between them follows from this rather
/// than from a projection remembering to emit one in the right place
/// (doc/specs/2026-08-09-visual-polish-design.md §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// The physical shape: which columns decide placement and ordering.
    Shape,
    /// What the schema reports about it: table options, replication.
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailRow {
    /// A column: name and CQL type on one line, the type pushed right.
    Column {
        name: String,
        /// `None` for a partition column, which has no ordering of its own.
        order: Option<ClusteringOrder>,
        cql_type: String,
    },
    /// A property: name on its own line, value indented beneath it.
    Property { name: String, value: String },
}

pub(super) fn body(state: &State) -> PanelBody {
    match of(state) {
        Some(details) => scrolled(state, details),
        None => PanelBody::Blank,
    }
}

/// What the Tables cursor is on, described — or nothing, when it is on
/// nothing.
pub(super) fn of(state: &State) -> Option<Details> {
    let schema = state.schema()?;

    Some(match state.tables.selected(schema)? {
        Node::Table { keyspace, table } => of_table(keyspace, table),
        Node::Keyspace { keyspace, .. } => of_keyspace(keyspace),
    })
}

fn scrolled(state: &State, details: Details) -> PanelBody {
    PanelBody::Details {
        details,
        selected: state.ddl.cursor.index(),
        offset: state.ddl.scroll.offset(),
    }
}

fn of_table(keyspace: &Keyspace, table: &Table) -> Details {
    let partition = Section {
        heading: "Partition Key".to_owned(),
        group: Group::Shape,
        token: Some(Token::PartitionKey),
        rows: table
            .primary_key
            .partition
            .iter()
            .map(|column| DetailRow::Column {
                name: column.name.clone(),
                order: None,
                cql_type: column.cql_type.clone(),
            })
            .collect(),
    };

    let clustering = Section {
        heading: "Clustering Key".to_owned(),
        group: Group::Shape,
        token: Some(Token::ClusteringKey),
        rows: table
            .primary_key
            .clustering
            .iter()
            .map(|clustering| DetailRow::Column {
                name: clustering.column.name.clone(),
                order: Some(clustering.order),
                cql_type: clustering.column.cql_type.clone(),
            })
            .collect(),
    };

    let options = Section {
        heading: "Options".to_owned(),
        group: Group::Settings,
        token: None,
        rows: table
            .options
            .in_display_order()
            .map(|(name, value)| DetailRow::Property {
                name: name.to_owned(),
                value: value.to_owned(),
            })
            .collect(),
    };

    Details {
        heading: format!("{}.{}", keyspace.name, table.name),
        sections: populated([partition, clustering, options]),
    }
}

fn of_keyspace(keyspace: &Keyspace) -> Details {
    let replication = Section {
        heading: "Replication".to_owned(),
        group: Group::Settings,
        token: None,
        rows: replication_rows(&keyspace.replication),
    };

    Details {
        heading: keyspace.name.clone(),
        sections: populated([replication]),
    }
}

/// Named as `system_schema.keyspaces.replication` names them, so what the panel
/// shows is what `DESCRIBE` and every Cassandra document call the same thing.
fn replication_rows(replication: &Replication) -> Vec<DetailRow> {
    let property = |name: &str, value: String| DetailRow::Property {
        name: name.to_owned(),
        value,
    };

    match replication {
        Replication::SimpleStrategy { factor } => vec![
            property("class", "SimpleStrategy".to_owned()),
            property("replication_factor", factor.to_string()),
        ],
        Replication::NetworkTopologyStrategy { factors } => {
            std::iter::once(property("class", "NetworkTopologyStrategy".to_owned()))
                .chain(
                    factors
                        .iter()
                        .map(|(datacenter, factor)| property(datacenter, factor.to_string())),
                )
                .collect()
        }
        Replication::Other { class } => vec![property("class", class.clone())],
    }
}

fn populated<const N: usize>(sections: [Section; N]) -> Vec<Section> {
    sections
        .into_iter()
        .filter(|section| !section.rows.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::model::{
        ClusteringColumn, Column, ConnId, DbError, PrimaryKey, Replication, Schema, TableOptions,
    };
    use crate::state::{Connection, ConnectionState, SchemaState, State};
    use crate::tree::tests_support::{options, schema_of};

    use super::super::tests_support::{connected, panel_body, state_with, tabled};
    use super::*;

    fn ddl_body(state: &State) -> PanelBody {
        panel_body(state, 1)
    }

    fn details(state: &State) -> Details {
        let PanelBody::Details { details, .. } = ddl_body(state) else {
            panic!("a selected row has something to describe");
        };
        details
    }

    fn headings(details: &Details) -> Vec<&str> {
        details
            .sections
            .iter()
            .map(|section| section.heading.as_str())
            .collect()
    }

    fn section<'a>(details: &'a Details, heading: &str) -> &'a Section {
        details
            .sections
            .iter()
            .find(|section| section.heading == heading)
            .unwrap_or_else(|| panic!("{heading} is missing"))
    }

    fn column(name: &str, cql_type: &str) -> Column {
        Column {
            name: name.to_owned(),
            cql_type: cql_type.to_owned(),
        }
    }

    /// `shop.orders_by_customer`, with the fixture's own shape.
    fn orders() -> Table {
        Table {
            name: "orders_by_customer".to_owned(),
            primary_key: PrimaryKey {
                partition: vec![column("customer_id", "uuid")],
                clustering: vec![
                    ClusteringColumn {
                        column: column("order_date", "date"),
                        order: ClusteringOrder::Descending,
                    },
                    ClusteringColumn {
                        column: column("order_id", "timeuuid"),
                        order: ClusteringOrder::Ascending,
                    },
                ],
            },
            static_columns: Vec::new(),
            regular_columns: vec![column("status", "text")],
            options: options(&[
                ("gc_grace_seconds", "864000"),
                ("comment", "Order history."),
                ("bloom_filter_fp_chance", "0.01"),
            ]),
        }
    }

    /// A one-keyspace schema holding `table`, with the keyspace unfolded and
    /// the cursor on the table.
    fn browsing(table: Table) -> State {
        let keyspace = Keyspace {
            name: "shop".to_owned(),
            replication: Replication::SimpleStrategy { factor: 1 },
            tables: BTreeMap::from([(table.name.clone(), table)]),
        };
        let schema = Schema {
            keyspaces: BTreeMap::from([(keyspace.name.clone(), keyspace)]),
        };

        let mut state = connected(schema.clone());
        state.tables.open(&schema);
        state.tables.open(&schema);
        state
    }

    #[test]
    fn a_selected_table_is_named_by_its_keyspace_and_its_own_name() {
        assert_eq!(
            details(&browsing(orders())).heading,
            "shop.orders_by_customer"
        );
    }

    #[test]
    fn a_tables_physical_shape_is_described_before_its_settings() {
        let details = details(&browsing(orders()));

        assert_eq!(
            headings(&details),
            ["Partition Key", "Clustering Key", "Options"],
            "vision.md §2: the shape is what the panel exists to keep visible"
        );
    }

    #[test]
    fn key_columns_carry_their_type_and_the_clustering_ones_their_direction() {
        let details = details(&browsing(orders()));

        assert_eq!(
            section(&details, "Partition Key").rows,
            [DetailRow::Column {
                name: "customer_id".to_owned(),
                order: None,
                cql_type: "uuid".to_owned(),
            }]
        );
        assert_eq!(
            section(&details, "Clustering Key").rows,
            [
                DetailRow::Column {
                    name: "order_date".to_owned(),
                    order: Some(ClusteringOrder::Descending),
                    cql_type: "date".to_owned(),
                },
                DetailRow::Column {
                    name: "order_id".to_owned(),
                    order: Some(ClusteringOrder::Ascending),
                    cql_type: "timeuuid".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_tables_shape_and_its_settings_are_marked_as_separate_groups() {
        let table = details(&browsing(orders()));
        let groups: Vec<Group> = table.sections.iter().map(|section| section.group).collect();

        assert_eq!(groups, [Group::Shape, Group::Shape, Group::Settings]);
        assert_eq!(
            section(&details(&tabled(&[("shop", &["orders"])])), "Replication").group,
            Group::Settings,
            "a keyspace has settings and no shape of its own"
        );
    }

    #[test]
    fn the_key_sections_are_the_ones_that_spend_the_key_tokens() {
        let details = details(&browsing(orders()));

        assert_eq!(
            section(&details, "Partition Key").token,
            Some(Token::PartitionKey)
        );
        assert_eq!(
            section(&details, "Clustering Key").token,
            Some(Token::ClusteringKey)
        );
        assert_eq!(
            section(&details, "Options").token,
            None,
            "an option is not part of the key structure and must not read as one"
        );
    }

    #[test]
    fn the_compact_options_are_listed_before_the_ones_that_wrap() {
        let details = details(&browsing(orders()));

        assert_eq!(
            section(&details, "Options").rows,
            [
                DetailRow::Property {
                    name: "gc_grace_seconds".to_owned(),
                    value: "864000".to_owned(),
                },
                DetailRow::Property {
                    name: "bloom_filter_fp_chance".to_owned(),
                    value: "0.01".to_owned(),
                },
                DetailRow::Property {
                    name: "comment".to_owned(),
                    value: "Order history.".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_table_with_no_clustering_columns_has_no_clustering_section() {
        let mut table = orders();
        table.primary_key.clustering = Vec::new();

        assert_eq!(
            headings(&details(&browsing(table))),
            ["Partition Key", "Options"],
            "an empty group is worse than none: it claims there is something to see"
        );
    }

    #[test]
    fn a_table_whose_options_row_was_missing_still_shows_its_shape() {
        let mut table = orders();
        table.options = TableOptions::default();

        assert_eq!(
            headings(&details(&browsing(table))),
            ["Partition Key", "Clustering Key"],
            "options carry no invariant, so their absence costs the panel nothing else"
        );
    }

    #[test]
    fn a_selected_keyspace_reports_its_replication_in_cassandras_own_vocabulary() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.connection = ConnectionState::Connected(Connection {
            id: ConnId::next(),
            posture: crate::posture::Posture::ReadOnly,
            schema: SchemaState::Loaded(Schema {
                keyspaces: BTreeMap::from([(
                    "shop".to_owned(),
                    Keyspace {
                        name: "shop".to_owned(),
                        replication: Replication::NetworkTopologyStrategy {
                            factors: BTreeMap::from([
                                ("datacenter1".to_owned(), 3),
                                ("datacenter2".to_owned(), 2),
                            ]),
                        },
                        tables: BTreeMap::new(),
                    },
                )]),
            }),
        });

        let details = details(&state);

        assert_eq!(details.heading, "shop");
        assert_eq!(
            section(&details, "Replication").rows,
            [
                DetailRow::Property {
                    name: "class".to_owned(),
                    value: "NetworkTopologyStrategy".to_owned(),
                },
                DetailRow::Property {
                    name: "datacenter1".to_owned(),
                    value: "3".to_owned(),
                },
                DetailRow::Property {
                    name: "datacenter2".to_owned(),
                    value: "2".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_simple_strategy_keyspace_reports_its_one_factor() {
        let details = details(&tabled(&[("shop", &["orders"])]));

        assert_eq!(
            section(&details, "Replication").rows,
            [
                DetailRow::Property {
                    name: "class".to_owned(),
                    value: "SimpleStrategy".to_owned(),
                },
                DetailRow::Property {
                    name: "replication_factor".to_owned(),
                    value: "1".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn the_projection_carries_the_panels_own_offset() {
        let mut state = browsing(orders());
        state.ddl.scroll.to_end(9, 3);

        let PanelBody::Details { offset, .. } = ddl_body(&state) else {
            panic!("a selected table has something to describe");
        };

        assert_eq!(offset, 6);
    }

    #[test]
    fn the_rows_the_cursor_walks_are_the_described_ones_and_no_headings() {
        let details = details(&browsing(orders()));

        assert_eq!(
            details.row_count(),
            6,
            "one partition column, two clustering, three options — and no heading"
        );
        assert_eq!(
            details.rows().nth(1),
            Some(&DetailRow::Column {
                name: "order_date".to_owned(),
                order: Some(ClusteringOrder::Descending),
                cql_type: "date".to_owned(),
            }),
            "down from the last partition column lands on the first clustering one"
        );
    }

    #[test]
    fn the_projection_carries_the_panels_own_selection() {
        let mut state = browsing(orders());
        state.ddl.cursor.go_to(3, 6);

        let PanelBody::Details { selected, .. } = ddl_body(&state) else {
            panic!("a selected table has something to describe");
        };

        assert_eq!(selected, 3);
    }

    #[test]
    fn a_panel_with_nothing_selected_stays_blank_rather_than_repeating_the_tables_panel() {
        for state in [
            State::default(),
            state_with(ConnectionState::Connecting(ConnId::next())),
            state_with(ConnectionState::Failed(DbError::Timeout)),
            state_with(ConnectionState::Connected(Connection {
                id: ConnId::next(),
                posture: crate::posture::Posture::ReadOnly,
                schema: SchemaState::Loading,
            })),
            connected(schema_of(&[])),
        ] {
            assert_eq!(ddl_body(&state), PanelBody::Blank);
        }
    }
}
