//! Coverage-to-sRGB operations for the D-27 comparison.
//!
//! Slug coverage is independent of display transfer functions. A–E mix
//! encoded bytes, F1 mixes linear light, and F2 modifies coverage before
//! that same linear mix. All paths preserve zero and full coverage.
use super::Blend;

/// Composites a grayscale text pixel after Slug has computed coverage.
pub(crate) fn composite(coverage: f32, background: u8, foreground: u8, blend: Blend) -> u8 {
    // Clamp numerical overshoot before exponentiation or interpolation;
    // the reference's geometric coverage remains unchanged by this step.
    let a = coverage.clamp(0.0, 1.0);
    let bg = f32::from(background) / 255.0;
    let fg = f32::from(foreground) / 255.0;
    let value = match blend {
        // Gamma blending matches the current M2 specimen and ordinary
        // non-sRGB swapchain blending, so it is the comparison baseline.
        Blend::Gamma => bg + (fg - bg) * a,
        Blend::Linear | Blend::Corrected => {
            let corrected = if matches!(blend, Blend::Corrected) {
                // These panels use neutral gray, so WCAG luminance equals
                // the decoded channel. White backgrounds strengthen dark
                // text; light text on dark is deliberately left at γ=1.
                let gamma = 1.0 + 0.8 * (decode(bg) - decode(fg)).max(0.0);
                a.powf(1.0 / gamma)
            } else {
                a
            };
            // Decode each endpoint before interpolation, then encode the
            // result; blending encoded endpoint bytes is a different row.
            encode(decode(bg) + (decode(fg) - decode(bg)) * corrected)
        }
    };
    (value * 255.0).round().clamp(0.0, 255.0) as u8
}

/// The exact sRGB electrical-to-linear transfer function.
fn decode(encoded: f32) -> f32 {
    // The linear toe prevents a steep power-law slope near black.
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// The exact inverse transfer function, applied after linear-light mixing.
fn encode(linear: f32) -> f32 {
    // The threshold is expressed in linear units, unlike decode's
    // threshold, which is expressed in encoded sRGB units.
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}
