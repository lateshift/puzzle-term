//! Benchmark-only Rgba16Float encoding for D-8 (design §9.4).
//!
//! The production curve plane is Rgba32Float. The Slug reference instead
//! uses Rgba16Float, so the D-8 comparison needs a second GPU texture with
//! the same quadratic points rounded to IEEE binary16. This module only
//! runs behind the native `oracle-debug` feature. Conversion occurs before
//! timing, leaving GPU texture reads as the measured difference.
//!
//! No extra crate or unsafe cast is needed: each f32 is converted through
//! its IEEE bit layout into a u16, and bytemuck views the resulting plain
//! array bytes for queue upload. Coordinates outside binary16's finite
//! range become infinities; malformed NaNs stay NaNs instead of a point.
use super::{Plane, RenderError};
use puzzle_slug::atlas::{DirtySpan, TEXTURE_WIDTH};

/// Uploads one-row spans in the reference's half-float curve format.
impl Plane {
    /// Converts only the atlas ranges sent to this diagnostic texture.
    pub(super) fn write_half_spans(
        &self,
        queue: &wgpu::Queue,
        texels: &[[f32; 4]],
        spans: &[DirtySpan],
    ) -> Result<(), RenderError> {
        for span in spans {
            // Dirty spans are physical row ranges. Shape-relative band
            // offsets are not used to address the curve texture here.
            let start = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.start);
            let end = usize::from(span.row) * TEXTURE_WIDTH + usize::from(span.end);
            let source = texels
                .get(start..end)
                .ok_or_else(|| RenderError::Upload("half curve span exceeds CPU mirror".into()))?;
            // Each source texel has four f32 coordinates. Rounding all four
            // before upload preserves the same contour packing and C1 rule.
            let half: Vec<[u16; 4]> = source.iter().map(|texel| texel.map(f32_to_f16)).collect();
            let width = u32::from(span.end - span.start);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: u32::from(span.start),
                        y: u32::from(span.row),
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&half),
                // Queue writes accept a tight one-row source, unlike the
                // padded texture-to-buffer path used for screenshots.
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 8),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
        }
        Ok(())
    }
}

/// Rounds one f32 coordinate to IEEE binary16 for the comparison plane.
fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    // Above the half range, retain infinity or a nonzero NaN payload.
    // A zero NaN payload would be misread as infinity in the texture.
    if exponent >= 31 {
        return sign | 0x7c00 | if mantissa != 0 { 0x0200 } else { 0 };
    }
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        // Subnormals shift f32's implicit leading one into a half mantissa;
        // the low-bit term implements round-to-nearest, ties-to-even.
        let shifted = (mantissa | 0x80_0000) >> (1 - exponent);
        return sign | ((shifted + 0x0fff + ((shifted >> 13) & 1)) >> 13) as u16;
    }
    // A rounding carry may increment the exponent at a power-of-two edge.
    let rounded = mantissa + 0x0fff + ((mantissa >> 13) & 1);
    sign | (((exponent as u32) << 10) + (rounded >> 13)) as u16
}

#[cfg(test)]
mod tests;
