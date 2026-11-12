//! M11.7 measurements run through the production native fallback/frame seam.
//!
//! A 240×70 fixture occupies every cell with complete two-cell clusters. VT
//! computes the widths before frame assembly; source fonts cannot change them.
//! Emoji and CJK exercise authored images and reproducible grid masks at both DPRs.
//! Fixtures and first-use discovery are outside the warm frame timer.
//! Setting the context invalidates every cached row, while prepared glyphs and
//! source choices stay warm. Thus this is a full redraw, not an unchanged copy.
//! The timer encloses only FrameBuilder::build, matching the existing ASCII probe.
//! All 21 samples are saved with their scale and specimen; none is discarded.
//! The allocation witness separately measures warm changed-row rebuilds (P1).
//! Each glyph is first prepared before counting so cold growth cannot be hidden
//! by an allocator result for an idle frame. Preparation count stays constant.
//! Raster memory reports lazily reserved physical atlas capacity. D-47 native
//! contexts allow eight mask/one color pages; only image-only ink has CPU mirrors.
//! These timings are measurements: §15's two-millisecond frame ceiling names
//! ASCII. M11 asks for emoji/CJK results and introduces no separate ceiling.
//! Curve preparation retains its independently enforced primary/scaling budget.
use puzzle_term::{
    frame::RenderFrame,
    frame_builder::{FrameBuilder, context::TextContext},
    vt::terminal::Terminal,
};
/// One complete warm benchmark fixture with no OS/GPU work inside the timer.
struct Fixture {
    /// Shared native text state owns prepared outlines and raster pages.
    context: TextContext,
    /// Per-terminal row caches are invalidated for each full redraw.
    builder: FrameBuilder,
    /// All grid cells contain complete two-cell clusters, with the cursor hidden.
    vt: Terminal,
    /// Buffers are reused so warmed frame construction can be allocation-free.
    frame: RenderFrame,
}
/// Builds and warms fixtures identically for timing and allocation evidence.
impl Fixture {
    /// Loads native sources explicitly; deterministic goldens remain unaffected.
    fn new(emoji: bool, dpr: f32) -> Result<Self, String> {
        let mut context = TextContext::new(crate::font_data::bundled_font_set()?, 14.0, dpr)?;
        // Opt-in native discovery uses the same chain installed by the ordinary window.
        crate::system_fonts::install(&mut context, &[])?;
        let mut builder = FrameBuilder::new(&context);
        let m = context.metrics();
        // Give the full grid enough surface space, including production padding.
        builder.set_surface_size([32 + 240 * m.width_px, 32 + 70 * m.height_px]);
        let mut vt = Terminal::new(240, 70, 0, 0);
        vt.advance(b"\x1b[?25l");
        // All specimens occupy two cells under D-31, including selectors and joined emoji.
        let clusters = if emoji {
            ["🦀", "✌️", "☺️", "🇩🇪", "👍🏽", "👨‍👩‍👧‍👦"]
        } else {
            ["漢", "字", "語", "鬱", "한", "글"]
        };
        // CUP starts each row, avoiding pending-wrap/LF side effects at column 240.
        for row in 0..70 {
            vt.advance(format!("\x1b[{};1H", row + 1).as_bytes());
            for col in 0..120 {
                vt.advance(clusters[(row + col) % clusters.len()].as_bytes());
            }
        }
        let mut frame = RenderFrame::default();
        // Warm every coverage, outline, bitmap and row scratch allocation first.
        crate::system_fonts::settle::terminal(&mut context, &mut builder, &vt, &mut frame)?;
        let mut result = Self {
            context,
            builder,
            vt,
            frame,
        };
        // A second full build warms cached-source paths before allocator measurements.
        result.redraw()?;
        Ok(result)
    }
    /// Changes the warmed native fixture to ASCII while retaining loaded fallback state.
    fn ascii(&mut self) -> Result<(), String> {
        // Rewrite all cells in place so line IDs stay with this same terminal.
        for row in 0..70 {
            self.vt.advance(format!("\x1b[{};1H", row + 1).as_bytes());
            let bytes = [b'A' + row as u8 % 26; 240];
            self.vt.advance(&bytes);
        }
        // Primary outlines are warmed after the CJK source has already initialized.
        self.redraw()
    }
    /// Invalidates every row, retaining prepared native ink and reusable arrays.
    fn redraw(&mut self) -> Result<(), String> {
        self.builder.set_context(&self.context);
        self.builder
            .build(&mut self.context, &self.vt, &mut self.frame)
    }
}
/// Records full-frame timings and actual raster pages for both native display scales.
pub(crate) fn run() -> Result<(), String> {
    let mut csv = String::from("kind,dpr,sample,frame_us\n");
    for dpr in [1.0, 2.0] {
        for kind in ["emoji", "CJK", "ASCII"] {
            let mut fixture = Fixture::new(kind == "emoji", dpr)?;
            if kind == "ASCII" {
                fixture.ascii()?;
            }
            // Count vectors separately from bitmap pages; both must remain warm.
            let prepared = fixture.context.prepared_glyphs();
            let mut samples = Vec::with_capacity(21);
            for sample in 0..21 {
                // Invalidation itself stays outside the existing frame CPU boundary.
                fixture.builder.set_context(&fixture.context);
                let start = std::time::Instant::now();
                fixture
                    .builder
                    .build(&mut fixture.context, &fixture.vt, &mut fixture.frame)?;
                let us = start.elapsed().as_secs_f64() * 1_000_000.0;
                samples.push(us);
                // Formatting and evidence allocation occur after the timer has stopped.
                csv.push_str(&format!("{kind},{dpr},{sample},{us:.6}\n"));
            }
            // Warm full redraws must not request more source parsing/preparation.
            if fixture.context.prepared_glyphs() != prepared {
                return Err("warm coverage redraw prepared additional outlines".into());
            }
            samples.sort_by(f64::total_cmp);
            // The existing ASCII ceiling still applies with native sources loaded.
            if kind == "ASCII" && (!samples[10].is_finite() || samples[10] > 2000.0) {
                return Err(format!(
                    "native ASCII frame {:.3} us exceeds 2 ms",
                    samples[10]
                ));
            }
            println!(
                "coverage_frame kind={kind} dpr={dpr} grid=240x70 median_us={:.3} raster_bytes={} slug_instances={} raster_instances={}",
                samples[10],
                fixture.context.raster_bytes(),
                fixture.frame.slugs.len(),
                fixture.frame.rasters.len()
            );
        }
    }
    // Save raw frame samples before measuring unrelated cold bitmap preparation.
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/m11-coverage-frame.csv"),
        csv,
    )
    .map_err(|e| e.to_string())?;
    crate::system_fonts::bench::bitmap::run()
}
#[cfg(test)]
/// P1 is enforced on real installed fonts, not a fake bitmap fixture.
mod tests;
