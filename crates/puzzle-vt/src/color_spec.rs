//! Shared X11 colour-spec decoder for xterm OSC 4/5/10–19 and kitty OSC 21.
//! Reference: https://sw.kovidgoyal.net/kitty/color-stack/#color-value-encoding
//! `rgb:` scales short channels; `#` supplies their most significant bits.
//! The grid renderer uses sRGB bytes, so inputs are quantized once, at admission.
//! Names use the static X.Org table and need neither allocation nor initialization.

/// Generated sorted named-colour lookup, independent of any wire family.
mod names;

/// Parses an opaque X11 colour without allocating, rejecting malformed channels.
pub fn parse(bytes: &[u8]) -> Option<[u8; 3]> {
    // Invalid UTF-8 is not a colour name and cannot reach string slicing.
    let text = std::str::from_utf8(bytes).ok()?;
    if let Some(hex) = text.strip_prefix('#') {
        // XParseColor accepts equal-width red/green/blue groups only.
        let width = hex.len() / 3;
        // Validate ASCII before slicing untrusted UTF-8 at byte boundaries (R8).
        if hex.len() % 3 != 0
            || !(1..=4).contains(&width)
            || !hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return None;
        }
        return Some(std::array::from_fn(|i| {
            // All chunks were validated above; a failed conversion stays harmless.
            let value = u16::from_str_radix(&hex[i * width..(i + 1) * width], 16).unwrap_or(0);
            ((u32::from(value) << (16 - width * 4)) >> 8) as u8
        }));
    }
    // The prefixes are wire keywords; only named colours are case-insensitive.
    if let Some(body) = text.strip_prefix("rgb:") {
        return triplet(body, hex_channel);
    }
    if let Some(body) = text.strip_prefix("rgbi:") {
        return triplet(body, intensity);
    }
    // Names compare normalized iterators against immutable sorted records.
    names::lookup(bytes)
}

/// Requires exactly three slash-separated channels, preserving independent widths.
fn triplet(body: &str, channel: fn(&str) -> Option<u8>) -> Option<[u8; 3]> {
    // Pull exactly three fields: a fourth slash is malformed, not ignored.
    let mut parts = body.split('/');
    let rgb = [
        channel(parts.next()?)?,
        channel(parts.next()?)?,
        channel(parts.next()?)?,
    ];
    parts.next().is_none().then_some(rgb)
}

/// Scales one through four hex digits, rather than treating them as high bits.
fn hex_channel(hex: &str) -> Option<u8> {
    if !(1..=4).contains(&hex.len()) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // At most 16 input bits keep the scaled product below u32 overflow.
    let value = u32::from_str_radix(hex, 16).ok()?;
    // The denominator depends on this channel, not its neighbours' widths.
    let maximum = (1_u32 << (hex.len() * 4)) - 1;
    Some(((value * 255 + maximum / 2) / maximum) as u8)
}

/// Parses finite X11 intensity, clipping the documented out-of-range values.
fn intensity(text: &str) -> Option<u8> {
    // Rust accepts NaN/inf as floats; the wire grammar permits only decimal forms.
    if text.is_empty()
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
    {
        return None;
    }
    // Reject overflow to infinity before clamping or converting to a byte.
    let value: f64 = text.parse().ok()?;
    // Quantize only once, so every OSC family agrees on half-channel rounding.
    value
        .is_finite()
        .then(|| (value.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
/// Every approved source entry and numeric grammar boundary are executable contracts.
mod tests;
