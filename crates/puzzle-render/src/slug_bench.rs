//! Native D-8 curve-texture precision cost probe (design §9.4).
//!
//! The same prepared glyph, band texture, shader, target, instance count and
//! adapter are used for both formats. Only the curve plane changes from
//! Rgba32Float to the reference's Rgba16Float. Texture conversion, pipeline
//! compilation and instance upload occur before timing. A warmup submission
//! absorbs first-use compilation and texture residency. Timed samples wait
//! for GPU completion; their wall time includes command encoding and queue
//! overhead, so the result is reported as queue-completion cost rather than
//! a hardware timestamp query (which needs an optional device feature).
//!
//! The batch deliberately overdraws one small glyph many times. It is a
//! texture-format comparison, not a frame-time benchmark for a normal
//! terminal screen; the absolute milliseconds must not be compared with
//! the full-screen §15 budget. Overdraw keeps every fragment accessing
//! the same packed band and curve locations, minimizing geometry changes
//! between formats. The GPU still executes the normal Slug pixel shader.
//!
//! Each sample submits a fresh clear-and-draw pass and waits for its own
//! submission index. The clear prevents a previous sample's accumulated
//! alpha from changing work in the next one. A median of seven warmed
//! submissions resists an occasional OS scheduling interruption. Because
//! wall timing includes command encoding and queue overhead, the measured
//! format delta is conservative: unrelated overhead pulls the ratio toward
//! one. A hardware timestamp query would need an optional device feature,
//! contrary to §10.1's default-feature device policy in this milestone.
use crate::{RenderError, Renderer, SlugPipeline, SlugTextures};
use puzzle_frame::slug::SlugInstance;
use puzzle_slug::SlugAtlas;
use std::time::Instant;

/// A batch large enough to make shader texture reads visible in wall timing.
const INSTANCES: usize = 16_384;
/// Repeats after a warmup; medians suppress one-off scheduling noise.
const SAMPLES: usize = 7;

/// D-8 measurement methods run only in the native diagnostic build.
impl Renderer {
    /// Returns median milliseconds for f32 and f16 curves on one glyph batch.
    pub fn benchmark_slug_formats(
        &self,
        atlas: &mut SlugAtlas,
        instance: SlugInstance,
    ) -> Result<(f64, f64), RenderError> {
        // Both runs use the same CPU mirror. Full uploads make a drained
        // dirty list irrelevant to the second precision variant.
        let full = self.benchmark_format(atlas, instance, false)?;
        // Order is fixed for reproducibility; repeated CLI runs reveal
        // thermal or scheduler drift in the ratio if it matters.
        let half = self.benchmark_format(atlas, instance, true)?;
        Ok((full, half))
    }

    /// Prepares one texture format, then times repeated identical draws.
    fn benchmark_format(
        &self,
        atlas: &mut SlugAtlas,
        instance: SlugInstance,
        half: bool,
    ) -> Result<f64, RenderError> {
        // The two stores have identical band formats and addresses. The
        // curve plane alone switches between sixteen and eight bytes/texel.
        let mut textures = if half {
            SlugTextures::new_half(&self.device)
        } else {
            SlugTextures::new(&self.device)
        };
        textures.sync_full(&self.device, &self.queue, atlas)?;
        // Pipeline construction and GPU upload are outside the stopwatch.
        // Only the later draw submissions are compared.
        let (curves, bands) = textures.views();
        let mut pipeline =
            SlugPipeline::new(&self.device, wgpu::TextureFormat::Rgba8Unorm, curves, bands);
        pipeline.set_viewport(&self.queue, 48, 48);
        // Copies of one B glyph give both texture formats the same band
        // accesses and fragment count while retaining one indexed draw.
        let instances = vec![instance; INSTANCES];
        pipeline.upload(&self.device, &self.queue, &instances);
        // One target allocation is reused across samples of this format;
        // the render pass clears it before each draw.
        let target = self.benchmark_target();
        self.timed_slug_draw(&pipeline, &target, INSTANCES as u32)?;
        // The warmup touches shader code and texture pages before numbers
        // are collected, reducing pipeline or residency startup effects.
        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            samples.push(self.timed_slug_draw(&pipeline, &target, INSTANCES as u32)?);
        }
        samples.sort_by(f64::total_cmp);
        // A median avoids treating a single interrupted GPU submission as
        // a precision penalty, while keeping raw repetitions bounded.
        Ok(samples[SAMPLES / 2])
    }

    /// Uses a tiny target so repeated glyph overdraw emphasizes curve loads.
    fn benchmark_target(&self) -> wgpu::Texture {
        // No CPU readback is timed. Waiting for the submission fence is
        // sufficient to ensure this render attachment was actually used.
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Slug D-8 glyph target"),
            size: wgpu::Extent3d {
                width: 48,
                height: 48,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    /// Waits for one indexed draw submission and returns elapsed milliseconds.
    fn timed_slug_draw(
        &self,
        pipeline: &SlugPipeline,
        target: &wgpu::Texture,
        count: u32,
    ) -> Result<f64, RenderError> {
        // The clock starts after resource setup but before encoder creation;
        // both formats pay identical CPU command-recording overhead.
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let started = Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Slug D-8 timed draw"),
            });
        {
            // Coverage blending and the opaque clear are identical to the
            // production pipeline, keeping the fragment workload realistic.
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Slug D-8 glyph batch"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pipeline.draw(&mut pass, count);
        }
        // The returned submission index names this pass specifically;
        // waiting only for earlier work would understate GPU execution.
        let submission = self.queue.submit(Some(encoder.finish()));
        // Waiting for this specific submission makes elapsed wall time
        // include the GPU work rather than only CPU command recording.
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|error| RenderError::Readback(error.to_string()))?;
        Ok(started.elapsed().as_secs_f64() * 1000.0)
    }
}
