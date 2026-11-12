//! The toolkit host transfers surface ownership to the embedding engine.
//! Authored readback and workload experiments remain development-only extensions.
/// Surface creation installs the engine's actual device recovery monitor.
pub(crate) use puzzle_embed::diagnostics::window::live::*;
/// Finite alphabet-cell ink experiment, never a product frame path.
#[cfg(feature = "lock-metrics")]
mod glyph_bisect;
/// Explicit unpaced counter experiment, retained only by the harness.
#[cfg(feature = "lock-metrics")]
mod present_probe;
/// Authored Unicode workload readback observes the retained engine image.
#[cfg(feature = "lock-metrics")]
mod unicode_pressure;
/// Measurement methods do not belong to the host-facing surface API.
#[cfg(feature = "lock-metrics")]
pub(super) use {
    glyph_bisect::GlyphBisect, present_probe::PresentProbe, unicode_pressure::UnicodePressure,
};
