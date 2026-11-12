//! Native image decoding belongs to the embedding font pipeline (D-37).
//!
//! The diagnostic emoji command uses the same bounded decoder as production.
//! Its installed-font readback fixture remains with the harness command.
pub(crate) use puzzle_embed::diagnostics::bitmap::*;
/// End-to-end CLI witness, independent of pure decoder contract tests.
#[cfg(test)]
mod tests;
