//! Safe GPU initialization waits are provided by the embedding platform.
//!
//! The harness uses this executor only for its native diagnostics; window
//! input never calls it while a frame is being built.
pub(crate) use puzzle_embed::diagnostics::block_on::*;
