//! Row-local Slug texture uploads from the GPU-neutral frame contract.
//!
//! The CPU packer promises each dirty span stays within one physical row.
//! Checking that boundary again here prevents a malformed frame from
//! accidentally writing into another glyph's curve or band data.
//! Texture growth preserves earlier rows and replaces the sampled view;
//! the caller must rebuild its bind group before submitting this frame.
//!
//! Curves and bands are independent planes with different bytes per texel.
//! The Upload enum keeps those component types distinct at compile time,
//! while this adapter shares the bounds and queue-write procedure.
//! A frame may upload new glyph data before drawing instances that point
//! to it. Queue ordering guarantees the writes are visible to that draw.
//! An empty span is a harmless no-op: WebGPU has no zero-width copy.
//! Full pack uploads and device recovery use the other, complete-mirror
//! paths in the parent module; this method handles incremental changes.
use super::*;
use puzzle_frame::Upload;

impl SlugTextures {
    /// Applies one frame-contract texel span and reports a view replacement.
    ///
    /// A grown plane needs a new Slug bind group before the same frame draws.
    pub fn apply_upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        upload: &Upload,
    ) -> Result<bool, RenderError> {
        match upload {
            Upload::Curves { origin, texels } => {
                // The source data is f32 control points; converting it
                // through another format would change Slug coverage.
                let grew = self.curves.height <= origin[1];
                // The last row index plus one is a checked capacity;
                // wraparound must not make an invalid frame appear small.
                let rows = origin[1]
                    .checked_add(1)
                    .ok_or_else(|| RenderError::Upload("curve row overflow".into()))?;
                self.curves.ensure_rows(device, queue, rows)?;
                write_one(queue, &self.curves, *origin, texels)?;
                Ok(grew)
            }
            Upload::Bands { origin, texels } => {
                // Band indices must remain exact integers, so they use
                // the separate unsigned texture rather than curve floats.
                let grew = self.bands.height <= origin[1];
                let rows = origin[1]
                    .checked_add(1)
                    .ok_or_else(|| RenderError::Upload("band row overflow".into()))?;
                self.bands.ensure_rows(device, queue, rows)?;
                write_one(queue, &self.bands, *origin, texels)?;
                Ok(grew)
            }
            // Atlas and paint uploads belong to different GPU stores.
            _ => Ok(false),
        }
    }
}

/// Writes one row-local span from a frame without copying its Pod texels.
fn write_one<T: bytemuck::Pod>(
    queue: &wgpu::Queue,
    plane: &Plane,
    origin: [u32; 2],
    texels: &[T],
) -> Result<(), RenderError> {
    // A source slice larger than u32 cannot be described to WebGPU;
    // reject it before calculating destination coordinates.
    let width =
        u32::try_from(texels.len()).map_err(|error| RenderError::Upload(error.to_string()))?;
    if width == 0 {
        // The empty frame span changes neither texture nor binding.
        return Ok(());
    }
    // Packed curve lists and band lists may differ in semantics, but
    // both must stop before the physical 4096-texel row boundary.
    if origin[0]
        .checked_add(width)
        .is_none_or(|end| end > TEXTURE_WIDTH as u32)
    {
        return Err(RenderError::Upload(
            "Slug upload crosses a texture row".into(),
        ));
    }
    // Queue writes accept one tightly packed row. The CPU packer already
    // obeys C1/B1, and this boundary check rejects a malformed frame.
    // Source bytes come directly from Pod texels. Copying into a second
    // vector here would allocate once per dirty glyph and violate P1.
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &plane.texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * plane.bytes_per_texel),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    Ok(())
}
