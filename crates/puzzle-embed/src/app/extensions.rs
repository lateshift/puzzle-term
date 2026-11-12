//! Standard process policy has no extra lifecycle state.
/// Select the same ordinary lifecycle used by a feature-disabled working build.
#[path = "extensions_base.rs"]
mod implementation;
// Preserve the parent module's interface without exposing lifecycle types to hosts.
pub(super) use implementation::State;
