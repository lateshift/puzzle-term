//! Native font discovery is owned by the embedding render context (D-43).
//!
//! The host retains its developer measurements; every production lookup and
//! policy update uses the same engine-owned font collection and bounded caches.
pub(crate) use puzzle_embed::diagnostics::system_fonts::*;
/// Cold installed-outline measurements are a harness command, not a host API.
pub(crate) mod bench;
