//! The Table DDL panel's projection: what the Tables cursor is on, described.
//!
//! A table leads with its physical shape and follows with the properties
//! `system_schema.tables` reports; a keyspace shows its replication. Nothing
//! selected shows nothing — the connection's state is the Tables panel's job
//! to narrate, and saying it twice in one sidebar is noise
//! (docs/superpowers/specs/2026-08-09-table-metadata-design.md §1).

use crate::ddl::DdlTab;
use crate::model::{ClusteringOrder, Column, Keyspace, Replication, Table};
use crate::search::Filter;
use crate::state::State;
use crate::style::Token;
use crate::tree::Node;

use super::{PanelBody, waiting};

/// The groups the panel draws, identity first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub sections: Vec<Section>,
}

impl Details {
    /// Every row the panel describes, in the order it draws them — which is
    /// the order the cursor walks. A heading is not a row: it labels the group
    /// beneath it, and there is nothing a cursor on one could mean
    /// (docs/superpowers/specs/2026-08-10-ddl-selection-design.md §3).
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
    /// `Partition Key`, `Clustering Key`, `Options`, `Replication`. `None` for
    /// the identity group, whose two rows name themselves — a heading over
    /// `Keyspace` and `Table` could only restate them.
    pub heading: Option<Heading>,
    /// What the group is, which decides both the blank line above it and
    /// whether its rows read as key structure.
    pub group: Group,
    /// Styles the rows beneath the heading, and nothing else. `None` where the
    /// group names no schema concept: an option is not key structure and must
    /// not read as any.
    pub token: Option<Token>,
    pub rows: Vec<DetailRow>,
}

/// A group's label and the colour it is drawn in.
///
/// The token is separate from [`Section::token`] because a heading is coloured
/// by what it names while its rows are coloured by what they are: `Partition
/// Key` wears the key's own colour, and a heading naming no schema concept —
/// `Options`, `Replication`, the column count — wears [`Token::Section`],
/// which its rows must not inherit
/// (docs/superpowers/specs/2026-08-13-colour-palettes-design.md §4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub text: String,
    pub token: Token,
}

impl Heading {
    fn new(text: impl Into<String>, token: Token) -> Self {
        Self {
            text: text.into(),
            token,
        }
    }
}

/// Which part of the panel a section belongs to.
///
/// The panel reads as three groups — what the rows are about, what the table
/// *is*, then what it is configured to do — and the blank line between them
/// follows from a change of group rather than from a projection remembering to
/// emit one in the right place
/// (docs/superpowers/specs/2026-08-09-visual-polish-design.md §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Which keyspace, and which of its tables, the rest of the panel is about.
    Identity,
    /// The physical shape: which columns decide placement and ordering.
    Shape,
    /// What the schema reports about it: table options, replication.
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailRow {
    /// A key column under the heading that already says which key it is part
    /// of: name and CQL type on one line, the type pushed right.
    KeyColumn {
        name: String,
        /// `None` for a partition column, which has no ordering of its own.
        order: Option<ClusteringOrder>,
        cql_type: String,
    },
    /// A column in the Table columns tab, where nothing above it says what
    /// part of the table it plays — so the row carries its own kind, and the
    /// renderer marks it (design §5).
    Column {
        name: String,
        cql_type: String,
        kind: ColumnKind,
    },
    /// A property: name on its own line, value indented beneath it.
    Property { name: String, value: String },
    /// What the panel is describing: a fixed label and the name it carries,
    /// drawn in the colour and behind the icon that name wears in the Tables
    /// tree (docs/superpowers/specs/2026-08-12-ddl-identity-rows-design.md §2).
    Identity { kind: IdentityKind, name: String },
}

/// Which name an [`DetailRow::Identity`] row carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    Keyspace,
    Table,
}

impl IdentityKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Keyspace => "Keyspace",
            Self::Table => "Table",
        }
    }

    #[must_use]
    pub const fn token(self) -> Token {
        match self {
            Self::Keyspace => Token::Keyspace,
            Self::Table => Token::Table,
        }
    }
}

/// What a column is to its table.
///
/// The four are exhaustive over [`Table`]'s own fields, so a column cannot be
/// listed under a kind the schema does not model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    Partition,
    Clustering,
    Static,
    Regular,
}

impl ColumnKind {
    /// Whether the column is part of the primary key, which is the one thing
    /// the renderer marks with a glyph — the rest of what a kind means travels
    /// as its [`token`](Self::token)
    /// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §5).
    #[must_use]
    pub const fn is_key(self) -> bool {
        matches!(self, Self::Partition | Self::Clustering)
    }

    #[must_use]
    pub const fn token(self) -> Token {
        match self {
            Self::Partition => Token::PartitionKey,
            Self::Clustering => Token::ClusteringKey,
            Self::Static => Token::StaticColumn,
            Self::Regular => Token::RegularColumn,
        }
    }
}

pub(super) fn body(state: &State) -> PanelBody {
    match of(state) {
        Some(details) => scrolled(state, details),
        None => PanelBody::Blank,
    }
}

/// What the Tables cursor is on, described and narrowed by the panel's filter
/// — or nothing, when the cursor is on nothing.
///
/// The filter is applied here rather than in [`body`] so that
/// [`ddl_rows`](super::ddl_rows) counts the rows the panel actually draws: the
/// cursor is clamped against that count, and a count of rows nobody can see
/// would let it walk off the end of a filtered panel (design §3.3).
pub(super) fn of(state: &State) -> Option<Details> {
    let schema = state.schema()?;

    let mut details = match (state.tables.selected(schema)?, shown_tab(state)) {
        (Node::Table { keyspace, table }, DdlTab::Ddl) => of_table(keyspace, table),
        (Node::Table { keyspace, table }, DdlTab::Columns) => columns_of(keyspace, table),
        (Node::Keyspace { keyspace, .. }, _) => of_keyspace(keyspace),
    };

    if let Some(filter) = state.ddl.filter() {
        details.retain(filter);
    }

    Some(details)
}

/// The tabs the panel offers for what is selected.
///
/// A keyspace has no columns, so the strip does not draw a tab that could only
/// say so — an empty tab teaches the wrong thing about what it is
/// (docs/superpowers/specs/2026-08-12-ddl-tabs-design.md §6).
pub(super) fn tabs_for(state: &State) -> &'static [DdlTab] {
    if describes_a_table(state) {
        &DdlTab::ALL
    } else {
        &[DdlTab::Ddl]
    }
}

/// The tab the panel draws, which is the one it holds only where that tab
/// exists for what is selected.
///
/// The held tab is left alone rather than reset: stepping from a table onto
/// its keyspace and back is navigation, and it may not cost the user the tab
/// they were reading.
pub(super) fn shown_tab(state: &State) -> DdlTab {
    let tab = state.ddl.tab();

    if tabs_for(state).contains(&tab) {
        tab
    } else {
        DdlTab::Ddl
    }
}

fn describes_a_table(state: &State) -> bool {
    matches!(
        state
            .schema()
            .and_then(|schema| state.tables.selected(schema)),
        Some(Node::Table { .. })
    )
}

fn scrolled(state: &State, details: Details) -> PanelBody {
    if details.sections.is_empty() {
        // Every subject the panel describes has identity rows of its own, so a
        // filter is the only thing that can empty it.
        return state.ddl.filter().map_or(PanelBody::Blank, |filter| {
            waiting(&format!("No property matches \"{}\"", filter.as_str()))
        });
    }

    PanelBody::Details {
        details,
        selected: state.ddl.cursor.index(),
        offset: state.ddl.scroll.offset(),
    }
}

impl Details {
    /// Drops the rows that miss, then the groups that emptied.
    ///
    /// Headings are not matched: the search is over properties and values, and
    /// a heading match would resurrect a whole group whose rows all missed
    /// (design §4.2).
    fn retain(&mut self, filter: &Filter) {
        for section in &mut self.sections {
            section.rows.retain(|row| row.matches(filter));
        }

        self.sections.retain(|section| !section.rows.is_empty());
    }
}

impl DetailRow {
    fn matches(&self, filter: &Filter) -> bool {
        match self {
            Self::KeyColumn { name, cql_type, .. } | Self::Column { name, cql_type, .. } => {
                filter.matches(name) || filter.matches(cql_type)
            }
            Self::Property { name, value } => filter.matches(name) || filter.matches(value),
            // The label is not matched, for the reason a section heading is
            // not: `Keyspace` and `Table` are the same two words on every
            // table, so a filter over them narrows nothing.
            Self::Identity { name, .. } => filter.matches(name),
        }
    }
}

/// The keyspace a panel is about, and the table when one is selected.
fn identity(keyspace: &Keyspace, table: Option<&Table>) -> Section {
    let row = |kind: IdentityKind, name: &str| DetailRow::Identity {
        kind,
        name: name.to_owned(),
    };

    Section {
        heading: None,
        group: Group::Identity,
        token: None,
        rows: std::iter::once(row(IdentityKind::Keyspace, &keyspace.name))
            .chain(table.map(|table| row(IdentityKind::Table, &table.name)))
            .collect(),
    }
}

fn of_table(keyspace: &Keyspace, table: &Table) -> Details {
    let partition = Section {
        heading: Some(Heading::new("Partition Key", Token::PartitionKey)),
        group: Group::Shape,
        token: Some(Token::PartitionKey),
        rows: table
            .primary_key
            .partition
            .iter()
            .map(|column| DetailRow::KeyColumn {
                name: column.name.clone(),
                order: None,
                cql_type: column.cql_type.clone(),
            })
            .collect(),
    };

    let clustering = Section {
        heading: Some(Heading::new("Clustering Key", Token::ClusteringKey)),
        group: Group::Shape,
        token: Some(Token::ClusteringKey),
        rows: table
            .primary_key
            .clustering
            .iter()
            .map(|clustering| DetailRow::KeyColumn {
                name: clustering.column.name.clone(),
                order: Some(clustering.order),
                cql_type: clustering.column.cql_type.clone(),
            })
            .collect(),
    };

    let options = Section {
        heading: Some(Heading::new("Options", Token::Section)),
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
        sections: populated([
            identity(keyspace, Some(table)),
            partition,
            clustering,
            options,
        ]),
    }
}

/// Every column the table has, in [`Table::columns`] order, as one flat list.
///
/// Flat rather than grouped by kind: the group headings would restate the DDL
/// tab's own top half, and the point of this tab is the whole column set in one
/// scan — the kind travels with each row instead (design §5).
fn columns_of(keyspace: &Keyspace, table: &Table) -> Details {
    let row = |column: &Column, kind: ColumnKind| DetailRow::Column {
        name: column.name.clone(),
        cql_type: column.cql_type.clone(),
        kind,
    };

    let rows = table
        .primary_key
        .partition
        .iter()
        .map(|column| row(column, ColumnKind::Partition))
        .chain(
            table
                .primary_key
                .clustering
                .iter()
                .map(|clustering| row(&clustering.column, ColumnKind::Clustering)),
        )
        .chain(
            table
                .static_columns
                .iter()
                .map(|column| row(column, ColumnKind::Static)),
        )
        .chain(
            table
                .regular_columns
                .iter()
                .map(|column| row(column, ColumnKind::Regular)),
        )
        .collect();

    Details {
        sections: populated([
            identity(keyspace, Some(table)),
            Section {
                heading: Some(Heading::new(
                    format!("{} columns", table.columns().count()),
                    Token::Section,
                )),
                group: Group::Shape,
                token: None,
                rows,
            },
        ]),
    }
}

fn of_keyspace(keyspace: &Keyspace) -> Details {
    let replication = Section {
        heading: Some(Heading::new("Replication", Token::Section)),
        group: Group::Settings,
        token: None,
        rows: replication_rows(&keyspace.replication),
    };

    Details {
        sections: populated([identity(keyspace, None), replication]),
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
    use crate::search::{Filter, Filtered};
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

    /// The headings the panel draws, which the identity group has none of.
    fn headings(details: &Details) -> Vec<&str> {
        details
            .sections
            .iter()
            .filter_map(|section| {
                section
                    .heading
                    .as_ref()
                    .map(|heading| heading.text.as_str())
            })
            .collect()
    }

    /// The identity rows, as the `(label, name)` pairs they draw as.
    fn identity_rows(details: &Details) -> Vec<(&str, &str)> {
        details
            .rows()
            .filter_map(|row| match row {
                DetailRow::Identity { kind, name } => Some((kind.label(), name.as_str())),
                _ => None,
            })
            .collect()
    }

    fn section<'a>(details: &'a Details, heading: &str) -> &'a Section {
        details
            .sections
            .iter()
            .find(|section| {
                section
                    .heading
                    .as_ref()
                    .is_some_and(|own| own.text == heading)
            })
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

    /// The fixture with something in every column kind, so the Table columns
    /// tab has all four to list.
    fn every_kind() -> Table {
        Table {
            static_columns: vec![column("region", "text")],
            ..orders()
        }
    }

    fn columns_tab(state: &State) -> State {
        let mut state = state.clone();
        state.ddl.show(DdlTab::Columns);
        state
    }

    #[test]
    fn the_columns_tab_lists_every_column_with_the_part_it_plays() {
        let details = details(&columns_tab(&browsing(every_kind())));

        assert_eq!(headings(&details), ["5 columns"]);
        assert_eq!(
            details
                .rows()
                .filter(|row| !matches!(row, DetailRow::Identity { .. }))
                .cloned()
                .collect::<Vec<_>>(),
            [
                DetailRow::Column {
                    name: "customer_id".to_owned(),
                    cql_type: "uuid".to_owned(),
                    kind: ColumnKind::Partition,
                },
                DetailRow::Column {
                    name: "order_date".to_owned(),
                    cql_type: "date".to_owned(),
                    kind: ColumnKind::Clustering,
                },
                DetailRow::Column {
                    name: "order_id".to_owned(),
                    cql_type: "timeuuid".to_owned(),
                    kind: ColumnKind::Clustering,
                },
                DetailRow::Column {
                    name: "region".to_owned(),
                    cql_type: "text".to_owned(),
                    kind: ColumnKind::Static,
                },
                DetailRow::Column {
                    name: "status".to_owned(),
                    cql_type: "text".to_owned(),
                    kind: ColumnKind::Regular,
                },
            ],
            "key columns first, then static, then regular — Table::columns' order"
        );
    }

    #[test]
    fn the_columns_tab_shows_the_columns_the_ddl_tab_leaves_out() {
        let ddl = details(&browsing(every_kind()));
        let columns = details(&columns_tab(&browsing(every_kind())));
        let named = |details: &Details| -> Vec<String> {
            details
                .rows()
                .filter_map(|row| match row {
                    DetailRow::KeyColumn { name, .. } | DetailRow::Column { name, .. } => {
                        Some(name.clone())
                    }
                    DetailRow::Property { .. } | DetailRow::Identity { .. } => None,
                })
                .collect()
        };

        assert_eq!(named(&ddl), ["customer_id", "order_date", "order_id"]);
        assert_eq!(
            named(&columns),
            ["customer_id", "order_date", "order_id", "region", "status"],
            "the static and regular columns are the point of the tab"
        );
    }

    #[test]
    fn a_keyspace_is_offered_no_columns_tab_and_keeps_describing_itself() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.ddl.show(DdlTab::Columns);

        assert_eq!(
            tabs_for(&state),
            [DdlTab::Ddl],
            "a tab that could only say a keyspace has no columns is not drawn"
        );
        assert_eq!(
            shown_tab(&state),
            DdlTab::Ddl,
            "so the panel draws the one tab it does offer"
        );
        assert_eq!(
            headings(&details(&state)),
            ["Replication"],
            "the held tab is masked, not the subject"
        );
    }

    #[test]
    fn a_table_is_offered_both_tabs_and_keeps_the_one_it_was_left_on() {
        let mut state = browsing(orders());
        state.ddl.show(DdlTab::Columns);

        assert_eq!(tabs_for(&state), DdlTab::ALL);
        assert_eq!(shown_tab(&state), DdlTab::Columns);
    }

    #[test]
    fn a_filter_narrows_the_columns_tab_too() {
        let mut state = columns_tab(&browsing(every_kind()));
        state.apply_filter(Filtered::Ddl, Filter::new("timeuuid"));

        assert_eq!(
            details(&state).rows().cloned().collect::<Vec<_>>(),
            [DetailRow::Column {
                name: "order_id".to_owned(),
                cql_type: "timeuuid".to_owned(),
                kind: ColumnKind::Clustering,
            }]
        );
    }

    #[test]
    fn a_selected_table_is_named_by_its_keyspace_and_its_own_name() {
        assert_eq!(
            identity_rows(&details(&browsing(orders()))),
            [("Keyspace", "shop"), ("Table", "orders_by_customer")],
            "one row each, so the panel can label them and the tree's colours \
             say which is which"
        );
        assert_eq!(
            identity_rows(&details(&columns_tab(&browsing(orders())))),
            [("Keyspace", "shop"), ("Table", "orders_by_customer")],
            "the other tab describes the same table"
        );
    }

    #[test]
    fn a_selected_keyspace_names_only_itself() {
        assert_eq!(
            identity_rows(&details(&tabled(&[("shop", &["orders"])]))),
            [("Keyspace", "shop")],
            "there is no table selected to name"
        );
    }

    #[test]
    fn the_identity_rows_open_the_body_and_the_cursor_walks_them() {
        let details = details(&browsing(orders()));

        assert!(
            matches!(
                details.rows().next(),
                Some(DetailRow::Identity {
                    kind: IdentityKind::Keyspace,
                    ..
                })
            ),
            "identity comes before the shape it describes"
        );
        assert_eq!(
            details.row_count(),
            2 + details
                .sections
                .iter()
                .filter(|section| section.group != Group::Identity)
                .map(|section| section.rows.len())
                .sum::<usize>(),
            "they are rows like any other, so the cursor can land on them"
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
            [DetailRow::KeyColumn {
                name: "customer_id".to_owned(),
                order: None,
                cql_type: "uuid".to_owned(),
            }]
        );
        assert_eq!(
            section(&details, "Clustering Key").rows,
            [
                DetailRow::KeyColumn {
                    name: "order_date".to_owned(),
                    order: Some(ClusteringOrder::Descending),
                    cql_type: "date".to_owned(),
                },
                DetailRow::KeyColumn {
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

        assert_eq!(
            groups,
            [Group::Identity, Group::Shape, Group::Shape, Group::Settings]
        );
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
    fn a_heading_naming_no_schema_concept_is_a_section_heading() {
        let keyspace = details(&tabled(&[("shop", &["orders"])]));
        let table = details(&browsing(orders()));
        let columns = details(&columns_tab(&browsing(every_kind())));
        let token = |details: &Details, heading: &str| section(details, heading).heading.clone();

        for (details, heading) in [
            (&keyspace, "Replication"),
            (&table, "Options"),
            (&columns, "5 columns"),
        ] {
            assert_eq!(
                token(details, heading).map(|heading| heading.token),
                Some(Token::Section),
                "{heading} names a group, not a schema concept"
            );
        }

        for (heading, expected) in [
            ("Partition Key", Token::PartitionKey),
            ("Clustering Key", Token::ClusteringKey),
        ] {
            assert_eq!(
                token(&table, heading).map(|heading| heading.token),
                Some(expected),
                "{heading} keeps the colour of the key it names"
            );
        }
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

        assert_eq!(identity_rows(&details), [("Keyspace", "shop")]);
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

    fn filtered_ddl(term: &str) -> State {
        let mut state = browsing(orders());
        state.apply_filter(Filtered::Ddl, Filter::new(term));
        state
    }

    #[test]
    fn a_property_matches_on_its_name() {
        let details = details(&filtered_ddl("gc_grace"));

        assert_eq!(
            section(&details, "Options").rows,
            [DetailRow::Property {
                name: "gc_grace_seconds".to_owned(),
                value: "864000".to_owned(),
            }],
            "gc_grace_seconds survives; comment and bloom_filter_fp_chance must be gone"
        );
    }

    #[test]
    fn a_property_matches_on_its_value() {
        // "Order history" is the comment's value; the property's name does not
        // contain the term.
        let details = details(&filtered_ddl("Order history"));

        assert_eq!(
            section(&details, "Options").rows,
            [DetailRow::Property {
                name: "comment".to_owned(),
                value: "Order history.".to_owned(),
            }],
            "only the comment property carries the term, in its value"
        );
    }

    #[test]
    fn a_column_matches_on_its_name() {
        let details = details(&filtered_ddl("customer_id"));

        assert_eq!(
            headings(&details),
            ["Partition Key"],
            "Clustering Key and Options emptied out and must be absent"
        );
        assert_eq!(
            section(&details, "Partition Key").rows,
            [DetailRow::KeyColumn {
                name: "customer_id".to_owned(),
                order: None,
                cql_type: "uuid".to_owned(),
            }],
            "customer_id survives; order_date, order_id and status must be gone"
        );
    }

    #[test]
    fn a_column_matches_on_its_cql_type() {
        let details = details(&filtered_ddl("timeuuid"));

        assert_eq!(
            headings(&details),
            ["Clustering Key"],
            "Partition Key and Options carry no timeuuid and must be absent"
        );
        assert_eq!(
            section(&details, "Clustering Key").rows,
            [DetailRow::KeyColumn {
                name: "order_id".to_owned(),
                order: Some(ClusteringOrder::Ascending),
                cql_type: "timeuuid".to_owned(),
            }],
            "only order_id's type contains timeuuid; order_date must be gone"
        );
    }

    #[test]
    fn a_section_emptied_by_the_filter_disappears() {
        let details = details(&filtered_ddl("customer_id"));

        assert_eq!(
            headings(&details),
            ["Partition Key"],
            "Clustering Key and Options emptied out and must be absent, per Section's own doc"
        );
    }

    #[test]
    fn a_heading_is_not_matched() {
        // "Partition Key" is a heading; no row in that section carries the word.
        assert_eq!(
            ddl_body(&filtered_ddl("Partition")),
            PanelBody::Notice {
                text: "No property matches \"Partition\"".to_owned(),
                token: Some(Token::Muted),
            },
            "matching a heading would resurrect a group whose rows all missed"
        );
    }

    #[test]
    fn nothing_matching_says_which_term_found_nothing() {
        assert_eq!(
            ddl_body(&filtered_ddl("zzz")),
            PanelBody::Notice {
                text: "No property matches \"zzz\"".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn an_unfiltered_table_keeps_its_existing_behaviour() {
        assert_eq!(
            headings(&details(&browsing(orders()))),
            ["Partition Key", "Clustering Key", "Options"],
            "no filter applied means nothing is retained away"
        );
    }

    #[test]
    fn the_rows_the_cursor_walks_are_the_described_ones_and_no_headings() {
        let details = details(&browsing(orders()));

        assert_eq!(
            details.row_count(),
            8,
            "the keyspace and the table, one partition column, two clustering, \
             three options — and no heading"
        );
        assert_eq!(
            details.rows().nth(3),
            Some(&DetailRow::KeyColumn {
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
