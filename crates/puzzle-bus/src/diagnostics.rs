//! Stable diagnostic names emitted by bus infrastructure (design §5.6).
//!
//! Tests and integrations should match the `event` field, not prose. The
//! terminal composition catalogue in `puzzle-term` lists meanings for users.

/// Cascade work exceeded the sixteen-round bound and was dropped.
pub const CASCADE_LIMIT: &str = "bus.cascade_limit";
