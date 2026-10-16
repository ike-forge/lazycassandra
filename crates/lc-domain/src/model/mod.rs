//! The Cassandra vocabulary in our terms; driver types stop at the adapter
//! (architecture.md §5).

pub mod class_option;
pub mod error;
pub mod ids;
pub mod input;
pub mod page;
pub mod schema;
pub mod statement;
pub mod value;

pub use class_option::{ClassKind, ClassOption, Setting};
pub use error::DbError;
pub use ids::{ConnId, JobId, QueryId};
pub use input::{Key, KeyPress, Modifiers};
pub use page::{Page, PagingState, Row};
pub use schema::{
    ClusteringColumn, ClusteringOrder, CollectionPart, Column, Index, IndexKind, IndexTarget,
    Keyspace, KeyspaceKind, PrimaryKey, Replication, Schema, Table, TableOptions,
};
pub use statement::{Statement, StatementKind, UpdatedRow};
pub use value::CqlValue;
