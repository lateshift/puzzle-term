//! Stable registration identity shared by independent model and view owners.
//! IDs increase monotonically so old presented regions cannot address a new model.
/// Shared logical placement and scissor picking, also used by presented tables.
pub(crate) mod hit;
/// Stable provider identity; removed slots are never silently reused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProviderId(pub u64);
