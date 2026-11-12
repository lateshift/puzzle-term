//! Small color math used by theme derivation and contrast reports (§14.6).
//!
//! Mixing encoded sRGB bytes bends hue and lightness. UI colors are mixed
//! in OKLab so their midpoint looks perceptually centred. WCAG contrast
//! uses linear-light relative luminance, independently of text compositing.
//!
//! The transfer-function breakpoints are the sRGB specification's exact
//! piecewise boundaries. All conversion arithmetic uses f64 because the
//! OKLab matrices amplify small channel errors near very dark colors.
//! The output is clipped only when converted back to display bytes:
//! intermediate OKLab points can leave sRGB gamut even when endpoints do
//! not. No clipping occurs before a mix, which would bend its hue path.

/// Decodes an sRGB byte through the exact transfer function.
fn decode(value: u8) -> f64 {
    let encoded = f64::from(value) / 255.0;
    // Low encoded values are linear; the higher branch follows a power
    // curve. Treating the byte directly as light would over-darken blends.
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// Encodes a linear-light channel, clipping out-of-gamut OKLab results.
fn encode(value: f64) -> u8 {
    // OKLab can represent colors outside the device gamut. Clipping at
    // this final boundary keeps WebGPU instance bytes valid and finite.
    let linear = value.clamp(0.0, 1.0);
    let encoded = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    // Byte rounding is explicit so generated UI colors are stable across
    // native and wasm builds using the same floating-point operations.
    (encoded * 255.0).round() as u8
}

/// Converts encoded RGB to the perceptual OKLab axes.
fn to_oklab(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(decode);
    // Ottosson's published linear-sRGB to LMS matrix is kept at its
    // stated precision; rounding coefficients shifts dark UI tints.
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    // The cube roots linearize perceived brightness; these following
    // axes separate lightness from the two opponent color directions.
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// Returns encoded RGB after the inverse OKLab and sRGB transforms.
fn from_oklab(lab: [f64; 3]) -> [u8; 3] {
    let [lightness, a, b] = lab;
    // Inverse opponent axes return LMS cube roots. Cubing before the
    // inverse matrix restores linear-light RGB channel energy.
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    [
        encode(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        encode(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        encode(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    ]
}

/// Mixes two colors at `amount` along straight OKLab coordinates.
#[must_use]
pub fn mix(left: [u8; 3], right: [u8; 3], amount: f64) -> [u8; 3] {
    let a = to_oklab(left);
    let b = to_oklab(right);
    let amount = amount.clamp(0.0, 1.0);
    // A clamped fraction also protects a custom theme from extrapolating
    // wildly outside gamut if a future UI calls this with user input.
    let mut result = [0.0; 3];
    for axis in 0..3 {
        result[axis] = a[axis] * (1.0 - amount) + b[axis] * amount;
    }
    from_oklab(result)
}

/// Computes WCAG 2 relative luminance from linear-light RGB channels.
#[must_use]
pub fn luminance(rgb: [u8; 3]) -> f64 {
    let [r, g, b] = rgb.map(decode);
    // WCAG's green-heavy weights are defined for linear channels. Applying
    // them to gamma-encoded bytes would overstate dark-color contrast.
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Gives the brighter-to-darker WCAG contrast ratio in 1..=21.
#[must_use]
pub fn contrast(left: [u8; 3], right: [u8; 3]) -> f64 {
    let a = luminance(left);
    let b = luminance(right);
    // Adding 0.05 avoids division by zero for black and gives 21:1 for
    // black against white, independent of argument order.
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Chooses source ink with better contrast over a derived selection fill.
#[must_use]
pub fn higher_contrast(background: [u8; 3], first: [u8; 3], second: [u8; 3]) -> [u8; 3] {
    // Some official schemes omit selection ink. Reuse a source color,
    // choosing whichever side of the palette reads on the derived fill.
    if contrast(first, background) >= contrast(second, background) {
        first
    } else {
        second
    }
}
