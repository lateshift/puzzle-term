//! GPU store growth and incremental upload probe (design §9.8).
//!
//! The first shape is uploaded while both textures have 64 rows. More
//! distinct shapes then cross that capacity, causing new textures and a GPU
//! copy of the original contents. The incremental dirty spans upload only
//! new rows. Readback checks one old and one late texel from each plane.
//! This catches a growth path that draws freshly added glyphs but loses
//! earlier ones, as well as a path that copies old data but drops new spans.
//!
//! The test is native-only because texture-to-buffer mapping is synchronous
//! in this diagnostic. Browser device loss and recovery follow later; the
//! CPU atlas mirrors remain the source of truth on both platforms.
//!
//! A mere capacity assertion would miss a copy submitted in the wrong
//! order or a bind group still pointing at the old view. This probe reads
//! actual GPU bytes after the second sync, including the earliest texel
//! that existed before growth and the final texel added after growth.
//! Curve and band stores grow independently; both are forced across their
//! initial 64-row threshold before either result is accepted.
use super::{SlugTextures, TEXTURE_WIDTH};
use crate::{RenderError, Renderer};
use puzzle_slug::{ShapeKey, SlugAtlas, atlas::FillRule, synthetic::SyntheticShape};

/// Exercises both texture planes through a real 64-to-128-row GPU copy.
impl Renderer {
    /// Returns grown curve and band row capacities after texel validation.
    pub fn probe_slug_store_growth(&self) -> Result<(u32, u32), RenderError> {
        // Initial insertion gives each store a nonzero texel that must be
        // preserved when `ensure_rows` copies the old GPU allocation.
        let mut atlas = SlugAtlas::default();
        let rectangle = SyntheticShape::Rectangle.path();
        atlas
            .prepare(ShapeKey(1), &rectangle, FillRule::NonZero, 0)
            .map_err(|error| RenderError::Upload(error.to_string()))?;
        let first_curve = atlas.curve_texels()[0];
        let first_band = atlas.band_texels()[0];
        // The first incremental sync creates the before-growth GPU state.
        // Full reupload would bypass the copy path being tested.
        let mut textures = SlugTextures::new(&self.device);
        textures.sync(&self.device, &self.queue, &mut atlas)?;
        // Use the same representative curved outline repeatedly with unique
        // keys. Coordinates stay small while packed row addresses grow.
        fill_both_planes(&mut atlas)?;
        let curve_index = atlas.curve_texels().len() - 1;
        let band_index = atlas.band_texels().len() - 1;
        // The last occupied positions are guaranteed to be beyond row 64
        // for both planes; they test new-span upload at far addresses.
        let last_curve = atlas.curve_texels()[curve_index];
        let last_band = atlas.band_texels()[band_index];
        textures.sync(&self.device, &self.queue, &mut atlas)?;
        // A successful API call alone says nothing about the actual new
        // capacities, which must exceed the old 64-row texture dimensions.
        if textures.curves.height <= 64 || textures.bands.height <= 64 {
            return Err(RenderError::Upload(
                "Slug plane did not grow beyond 64 rows".into(),
            ));
        }
        // Bit-exact comparisons avoid a float equality ambiguity: these
        // are copied texels, not interpolated coverage samples.
        self.check_texel(&textures.curves, 0, bytemuck::bytes_of(&first_curve))?;
        self.check_texel(&textures.bands, 0, bytemuck::bytes_of(&first_band))?;
        self.check_texel(
            &textures.curves,
            curve_index,
            bytemuck::bytes_of(&last_curve),
        )?;
        self.check_texel(&textures.bands, band_index, bytemuck::bytes_of(&last_band))?;
        // Report both row counts so a reviewer can see independent growth
        // rather than inferring it from a generic PASS line.
        Ok((textures.curves.height, textures.bands.height))
    }

    /// Copies one atlas texel through a 256-byte-aligned readback buffer.
    fn check_texel(
        &self,
        plane: &super::Plane,
        index: usize,
        expected: &[u8],
    ) -> Result<(), RenderError> {
        // The fixed atlas width maps a linear CPU mirror position to the
        // same physical x/y coordinate used by shader textureLoad.
        let x = (index % TEXTURE_WIDTH) as u32;
        let y = (index / TEXTURE_WIDTH) as u32;
        // Copying a one-texel slice still needs a mappable buffer with the
        // 256-byte row alignment required by WebGPU transfer commands.
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Slug growth texel readback"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Slug growth readback encoder"),
            });
        // The source texture is the post-growth allocation. Reading the
        // old allocation would falsely pass the preservation assertion.
        // One-texel copies still need a 256-byte destination stride; the
        // source location uses the atlas's unchanged 4096-wide row mapping.
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &plane.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        // map_pixels waits for transfer completion and strips padding;
        // comparison can then use exact raw texel bytes.
        let actual = self.map_pixels(&buffer, plane.bytes_per_texel, 256, 1)?;
        if actual != expected {
            return Err(RenderError::Upload(format!(
                "Slug texel {index} changed across growth"
            )));
        }
        Ok(())
    }
}

/// Fills both planes past their initial capacity without changing geometry.
fn fill_both_planes(atlas: &mut SlugAtlas) -> Result<(), RenderError> {
    // One path is reused to avoid changing curve values while growing the
    // atlas. Unique keys ensure storage is allocated for every insertion.
    let path = SyntheticShape::TypicalGlyph.path();
    for key in 2..80_000_u64 {
        // The atlas stores each key separately, so both physical cursors
        // advance even though the outline shape is deliberately identical.
        atlas
            .prepare(ShapeKey(key), &path, FillRule::NonZero, 0)
            .map_err(|error| RenderError::Upload(error.to_string()))?;
        if atlas.curve_texels().len() > 64 * TEXTURE_WIDTH
            && atlas.band_texels().len() > 64 * TEXTURE_WIDTH
        {
            // Returning only after both cursors cross their boundaries
            // proves that each independently allocated plane must grow.
            return Ok(());
        }
    }
    Err(RenderError::Upload(
        "growth fixture did not fill both planes".into(),
    ))
}
