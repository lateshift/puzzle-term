//! Deterministic COLR layer/gradient fixtures, excluded from shipped wasm.
//!
//! The platform reads the D-37 font bytes. This scene retains paint records
//! beside atlas handles so the native oracle can evaluate the actual brushes.
//!
//! The scene has no terminal parser or font discovery. Its platform caller
//! supplies the exact licensed font bytes and a glyph ID from the fixture.
//! A root color graph emits consecutive fills; their innermost clips become
//! quads and enclosing clips become extra Slug handles inside the paint row.
//! Paths and brushes remain in em coordinates until one common crop transform
//! is selected from the union of all leaf bounds. A different fit per layer
//! would shift the graph's registration and hide errors in brush transforms.
//!
//! CPU brush records are retained independently of their encoded rows. That
//! lets the oracle evaluate semantic geometry rather than decoding its own
//! GPU input as the expected answer. The shader receives only encoded texels.
//! ColorScene uses the ordinary render-frame contract: a single text batch,
//! all curve/band dirty spans, and one fixed paint row for each fill.
//! An 80×80 logical-pixel crop leaves an eight-pixel margin around a 64-pixel
//! fitted specimen; DPR doubles both geometry and target dimensions together.
//! Clear pixels and antialiased outlines therefore remain inspectable at both
//! scales, without any browser or system-font differences in the references.
//! A restore request reconstructs uploads solely from retained CPU mirrors.
//! It never opens the font or walks the paint graph again, which separates
//! device-loss recovery from new glyph preparation.
//! The platform owns image encoding and GPU adapter selection; this module
//! contains only font, geometry and plain frame data (R9).
use puzzle_frame::{Batch, PipelineKind, RenderFrame, SlugInstance, Upload, ZSlot};
use puzzle_slug::{ShapeHandle, ShapeKey, SlugAtlas, atlas::FillRule, paint::Paint};
use puzzle_text::{FontData, face::FontFace};
/// One color glyph with per-layer brush evidence and a complete frame.
pub struct ColorScene {
    /// Source-of-truth curve/band planes for GPU upload and the oracle.
    pub atlas: SlugAtlas,
    /// Actual brush records, including root-em enclosing clip handles.
    pub paints: Vec<Paint>,
    /// Innermost clip handles parallel to frame Slug instances.
    pub handles: Vec<ShapeHandle>,
    /// Ordered instances and retained initial uploads.
    pub frame: RenderFrame,
}
impl ColorScene {
    /// Builds one fixture glyph at DPR 1 or 2 inside an 80-logical-pixel crop.
    pub fn new(data: FontData, glyph: u32, dpr: u32) -> Result<Self, String> {
        // Parsing happens once for this cold fixture; no system fallback is enabled.
        let face = FontFace::new(data, 400)?;
        let color = face
            .color_layers(glyph, 0xffffffff)?
            .ok_or("fixture glyph has no COLR graph")?;
        // Keep semantic brushes beside the exact geometry handles they clip.
        let mut scene = Self {
            atlas: SlugAtlas::default(),
            paints: Vec::new(),
            handles: Vec::new(),
            frame: RenderFrame {
                target_size: [80 * dpr; 2],
                clear: [0.0, 0.0, 0.0, 1.0],
                ..RenderFrame::default()
            },
        };
        // Monotonic local IDs distinguish every transformed clip outline.
        let mut id = 0u64;
        // Painter order is source-over draw order; never sort fills by brush type.
        for mut layer in color.layers {
            id += 1;
            let handle = scene
                .atlas
                .prepare(ShapeKey(id), &layer.path, FillRule::NonZero, 0)
                .map_err(|e| e.to_string())?;
            // Outer clips are independent frozen root-em paths, not leaf glyph IDs.
            for path in layer.clips {
                id += 1;
                layer.paint.clips.push(
                    scene
                        .atlas
                        .prepare(ShapeKey(id), &path, FillRule::NonZero, 0)
                        .map_err(|e| e.to_string())?,
                );
            }
            scene.handles.push(handle);
            scene.paints.push(layer.paint);
        }
        // One common fit is computed only after every leaf bound is available.
        scene.instances(dpr)?;
        scene.uploads()?;
        Ok(scene)
    }
    /// Recreates uploads from retained CPU data, without reopening the font.
    pub fn restore_uploads(&mut self) -> Result<(), String> {
        // Simulate an empty recreated device while retaining all CPU owners.
        self.frame.uploads.clear();
        // Restoration includes reserved rows as well as occupied geometry.
        for (y, row) in self.atlas.curve_texels().chunks(4096).enumerate() {
            self.frame.uploads.push(Upload::Curves {
                origin: [0, y as u32],
                texels: row.to_vec(),
            });
        }
        // Band references remain valid because the curve mirror keeps its addresses.
        for (y, row) in self.atlas.band_texels().chunks(4096).enumerate() {
            self.frame.uploads.push(Upload::Bands {
                origin: [0, y as u32],
                texels: row.to_vec(),
            });
        }
        // Paint row addresses are unchanged by device recreation.
        // Row zero stays reserved for the existing paint-binding probe.
        for (i, paint) in self.paints.iter().enumerate() {
            self.frame.uploads.push(Upload::Paint {
                first: ((i + 1) * 256) as u32,
                colors: paint.texels()?.to_vec(),
            });
        }
        Ok(())
    }

    /// Fits one union box to the crop while keeping every layer registered.
    fn instances(&mut self, dpr: u32) -> Result<(), String> {
        // Union min/max uses the font's y-up em coordinates before the screen flip.
        let mut bbox = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for h in &self.handles {
            for i in 0..2 {
                bbox[i] = bbox[i].min(h.bbox[i]);
                bbox[i + 2] = bbox[i + 2].max(h.bbox[i + 2]);
            }
        }
        // Uniform scale preserves circle/outline proportions in the test crop.
        let extent = (bbox[2] - bbox[0]).max(bbox[3] - bbox[1]);
        if !extent.is_finite() || extent <= 0.0 {
            return Err("COLR fixture has no bounded ink".into());
        }
        // Convert the logical fitted extent to device pixels exactly once.
        let scale = 64.0 * dpr as f32 / extent;
        // All layers reuse the same m/t, retaining original color graph registration.
        for (index, handle) in self.handles.iter().enumerate() {
            self.frame.slugs.push(SlugInstance {
                m: [scale, 0.0, 0.0, -scale],
                t: [
                    8.0 * dpr as f32 - bbox[0] * scale,
                    8.0 * dpr as f32 + bbox[3] * scale,
                ],
                bbox: handle.bbox,
                glyph_loc: puzzle_slug::encoding::encode_glyph_loc(handle.glyph_loc),
                band_info: handle.band_info,
                color: 0xffffffff,
                paint: ((index + 1) * 256 + 1) as u32,
            });
        }
        // One instanced Slug batch exercises the production consecutive-layer path.
        self.frame.batches.push(Batch {
            slot: ZSlot::Text,
            pipeline: PipelineKind::Slug,
            instances: 0..self.handles.len() as u32,
            scissor: None,
        });
        Ok(())
    }
    /// Queues complete occupied atlas rows plus actual encoded paint records.
    fn uploads(&mut self) -> Result<(), String> {
        // Dirty spans are row-local: copying only their occupied intervals preserves
        // the allocator's exact packed curve and band addresses.
        let (curves, bands) = self.atlas.take_dirty();
        for span in curves {
            let first = usize::from(span.row) * 4096 + usize::from(span.start);
            let end = usize::from(span.row) * 4096 + usize::from(span.end);
            self.frame.uploads.push(Upload::Curves {
                origin: [u32::from(span.start), u32::from(span.row)],
                texels: self.atlas.curve_texels()[first..end].to_vec(),
            });
        }
        for span in bands {
            let first = usize::from(span.row) * 4096 + usize::from(span.start);
            let end = usize::from(span.row) * 4096 + usize::from(span.end);
            self.frame.uploads.push(Upload::Bands {
                origin: [u32::from(span.start), u32::from(span.row)],
                texels: self.atlas.band_texels()[first..end].to_vec(),
            });
        }
        // Each gradient record fits wholly within its 256-texel paint row.
        for (index, paint) in self.paints.iter().enumerate() {
            self.frame.uploads.push(Upload::Paint {
                first: ((index + 1) * 256) as u32,
                colors: paint.texels()?.to_vec(),
            });
        }
        Ok(())
    }
}
