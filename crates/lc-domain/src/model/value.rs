use std::collections::BTreeMap;

/// Only the shapes the grid renders differently; the long tail of CQL types
/// arrives as [`CqlValue::Text`] in the driver's rendering.
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
