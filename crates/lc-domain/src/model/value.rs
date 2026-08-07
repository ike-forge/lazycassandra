//! CQL values.

use std::collections::BTreeMap;

/// A value as it comes back from the cluster.
///
/// Covers the shapes the grid has to render differently; the long tail of CQL
/// types arrives as [`CqlValue::Text`] carrying the driver's own rendering
/// until a driver is chosen (architecture.md §8).
#[derive(Debug, Clone, PartialEq)]
pub enum CqlValue {
    Null,
    Boolean(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Blob(Vec<u8>),
    Uuid(String),
    Timestamp(i64),
    List(Vec<CqlValue>),
    Set(Vec<CqlValue>),
    Map(BTreeMap<String, CqlValue>),
}
