//! The embedding engine installs the native Slug bitmap producer (D-47).
//!
//! Harness measurements exercise that producer through the public engine seam;
//! they do not own a separate glyph preparation or upload implementation.
pub(crate) use puzzle_embed::diagnostics::grid_rasterizer::*;
/// Developer cost measurements keep their command-line entry point in the host.
pub(crate) mod bench;
