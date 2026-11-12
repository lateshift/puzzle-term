//! Native settings loading and snapshots are owned by the embedding layer.
//!
//! The harness forwards its CLI options to the same validated native adapter
//! used by embedded applications; file reload preserves the last good values.
pub(crate) use puzzle_embed::DesktopSettings;
/// JSON replay conversion is diagnostic, outside the native host settings API.
pub(crate) mod json {
    pub(crate) use puzzle_embed::diagnostics::settings::json::*;
}
