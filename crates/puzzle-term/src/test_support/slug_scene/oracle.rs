//! CPU reference sampling for complete M3 GPU scenes (design §9.6).
//!
//! Every glyph keeps its own packed handle and affine instance. The oracle
//! samples the unchanged Slug math in the same em space as the pixel shader.
//! For the end-to-end layer it inverts each instance at exact pixel centres;
//! the math-fidelity layer receives captured GPU coordinates and derivatives
//! together with the diagnostic instance index. White premultiplied glyphs
//! accumulate over opaque black in instance order, as the render pass does.
//!
//! A line at 96 px DPR 2 has millions of background pixels. Sampling every
//! glyph at every pixel would obscure the GPU comparison with unnecessary
//! CPU work. A transformed bbox plus two pixels bounds each glyph's possible
//! coverage, including Slug's half-pixel dilation and rounding at the edge.
//!
//! The bounding rectangle is only a work limit. It never decides whether
//! a pixel is covered: the reference ray solver still samples every pixel
//! inside that conservative region. Thus a curved edge and a rotated glyph
//! obey the same reference math as the single-outline M2 oracle. The result
//! is composited in script order because a repeated outline can appear
//! multiple times at different origins in the specimen line.
//!
//! The exact-affine expectation differs slightly from GPU interpolation at
//! some triangle pixels. design §9.6 checks whether each difference follows
//! from the captured coordinate; this module exposes the exact inverse and
//! the unchanged reference sampler to that native diagnostic. The licensed
//! solver is unchanged, and production shader coordinates stay interpolated.
use super::SlugScene;
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::{atlas::ShapeHandle, reference};

/// Applies the CPU reference independently of GPU interpolation.
impl SlugScene {
    /// Produces an exact-affine RGBA8 image for end-to-end comparison.
    pub fn oracle_pixels(&self) -> Result<Vec<u8>, String> {
        if self.instances.len() != self.handles.len() {
            return Err("Slug scene handle count differs from instance count".into());
        }
        // Opaque black matches the native offscreen load operation. Alpha
        // stays 255 as premultiplied white glyphs are blended over it.
        let mut pixels = vec![0_u8; self.width as usize * self.height as usize * 4];
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        for (instance, handle) in self.instances.iter().zip(&self.handles) {
            raster_instance(self, *instance, *handle, &mut pixels)?;
        }
        Ok(pixels)
    }

    /// Evaluates one diagnostic fragment at its captured GPU inputs.
    pub fn oracle_at(
        &self,
        index: usize,
        at: [f32; 2],
        ems_per_pixel: [f32; 2],
    ) -> Result<u8, String> {
        // A diagnostic index is validated before touching the CPU atlas;
        // zero is reserved outside the quad by the GPU capture target.
        let coverage = self.oracle_coverage(index, at, ems_per_pixel)?;
        Ok((coverage * 255.0).round().clamp(0.0, 255.0) as u8)
    }

    /// Returns unquantized reference coverage for compositing many glyphs.
    pub fn oracle_coverage(
        &self,
        index: usize,
        at: [f32; 2],
        ems_per_pixel: [f32; 2],
    ) -> Result<f32, String> {
        // A separate coverage value is needed when glyph quads overlap:
        // blending quantizes only after the new alpha meets the old pixel.
        let handle = *self
            .handles
            .get(index)
            .ok_or("invalid diagnostic instance index")?;
        reference::sample(&self.atlas, handle, at, ems_per_pixel).map_err(|error| error.to_string())
    }

    /// Inverts an instance at an exact device-pixel centre.
    pub fn exact_at(&self, index: usize, x: u32, y: u32) -> Result<[f32; 2], String> {
        let instance = self
            .instances
            .get(index)
            .ok_or("invalid exact-affine instance index")?;
        let [a, b, c, d] = instance.m;
        let det = a * d - b * c;
        if !det.is_finite() || det == 0.0 {
            return Err("invalid M3 scene transform".into());
        }
        // This is the same inverse and operation order as raster_instance.
        // Pixel centres use half-integers because device pixels are areas.
        let inv = [d / det, -c / det, -b / det, a / det];
        let dx = x as f32 + 0.5 - instance.t[0];
        let dy = y as f32 + 0.5 - instance.t[1];
        Ok([inv[0] * dx + inv[1] * dy, inv[2] * dx + inv[3] * dy])
    }

    /// Converts an em-coordinate difference into device-pixel distance.
    pub fn coordinate_error_px(
        &self,
        index: usize,
        exact: [f32; 2],
        interpolated: [f32; 2],
    ) -> Result<f32, String> {
        let m = self
            .instances
            .get(index)
            .ok_or("invalid coordinate-error instance index")?
            .m;
        let delta = [interpolated[0] - exact[0], interpolated[1] - exact[1]];
        // The matrix maps em x/y to physical pixels even for rotation and
        // shear; hypot measures the actual displacement in the target.
        let px = m[0] * delta[0] + m[2] * delta[1];
        let py = m[1] * delta[0] + m[3] * delta[1];
        Ok(px.hypot(py))
    }
}

/// Samples one affine glyph only within its projected outline rectangle.
fn raster_instance(
    scene: &SlugScene,
    instance: SlugInstance,
    handle: ShapeHandle,
    pixels: &mut [u8],
) -> Result<(), String> {
    let [a, b, c, d] = instance.m;
    let det = a * d - b * c;
    if !det.is_finite() || det == 0.0 {
        return Err("invalid M3 scene transform".into());
    }
    // Inverse rows map device-pixel movement into ems. fwidth is the sum
    // of absolute screen-axis partial derivatives for each em coordinate.
    let inv = [d / det, -c / det, -b / det, a / det];
    // Each instance has its own rounded baseline translation. Inverting
    // only the shared matrix would shift small glyphs by subpixel amounts.
    let derivative = [inv[0].abs() + inv[1].abs(), inv[2].abs() + inv[3].abs()];
    let [x0, y0, x1, y1] = clipped_bounds(instance, scene.width, scene.height);
    for y in y0..y1 {
        for x in x0..x1 {
            let dx = x as f32 + 0.5 - instance.t[0];
            let dy = y as f32 + 0.5 - instance.t[1];
            let at = [inv[0] * dx + inv[1] * dy, inv[2] * dx + inv[3] * dy];
            let coverage = reference::sample(&scene.atlas, handle, at, derivative)
                .map_err(|error| error.to_string())?;
            // The GPU blends white source-over black in its 8-bit target.
            // Quantize after each instance, matching target-store order.
            let offset = ((y * scene.width + x) * 4) as usize;
            let old = f32::from(pixels[offset]) / 255.0;
            let byte = ((coverage + old * (1.0 - coverage)) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            pixels[offset..offset + 3].fill(byte);
        }
    }
    Ok(())
}

/// Bounds a transformed em bbox with room for dynamic dilation.
fn clipped_bounds(instance: SlugInstance, width: u32, height: u32) -> [u32; 4] {
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for x in [instance.bbox[0], instance.bbox[2]] {
        // A shear can move the upper-left and lower-right corners in
        // opposite directions, so all four projected points are examined.
        for y in [instance.bbox[1], instance.bbox[3]] {
            let px = instance.m[0] * x + instance.m[2] * y + instance.t[0];
            let py = instance.m[1] * x + instance.m[3] * y + instance.t[1];
            low[0] = low[0].min(px);
            low[1] = low[1].min(py);
            high[0] = high[0].max(px);
            high[1] = high[1].max(py);
        }
    }
    [
        (low[0] - 2.0).floor().max(0.0) as u32,
        (low[1] - 2.0).floor().max(0.0) as u32,
        (high[0] + 2.0).ceil().min(width as f32) as u32,
        (high[1] + 2.0).ceil().min(height as f32) as u32,
    ]
}
