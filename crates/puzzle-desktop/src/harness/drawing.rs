//! Headless scenarios drive the engine’s sealed-publication drawing helper.
//!
//! Script clocks and expectations remain in the harness; retained frame slots,
//! row caches and presentation acknowledgements use the public embedding path.
pub(crate) use puzzle_embed::diagnostics::drawing::*;
