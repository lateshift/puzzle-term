//! Harness artifacts use the engine diagnostic PNG writer (D-14).
//!
//! A single stored-DEFLATE implementation preserves exact readback bytes for
//! font failures and scenario screenshots without another image dependency.
pub(crate) use puzzle_embed::diagnostics::png::*;
