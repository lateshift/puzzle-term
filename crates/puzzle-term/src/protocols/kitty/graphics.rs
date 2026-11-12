//! Kitty graphics wire handlers; decoded storage remains in images (D-58).
//! Reference: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>.
/// Ordered graphics actions and protocol replies.
mod actions;
/// Strict chunk decoding, before shared storage admission.
mod base64;
/// Parsed bounded metadata, re-exported through the existing images API.
pub mod control;
/// Wire deletion selectors operating on the shared image store.
mod deletion;
/// Raw colour identifiers and exact placeholder diacritic inheritance.
pub(crate) mod placeholders;
/// Protocol source crops, cell dimensions and cursor side effects.
mod placement;
