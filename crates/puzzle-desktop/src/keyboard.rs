//! Host input translation uses the embedding engine’s adapter.
//!
//! The window still captures native callbacks; mode-dependent encoding and
//! recorded raw-input translation now have one implementation for every host.
pub(crate) use puzzle_embed::diagnostics::keyboard::*;
