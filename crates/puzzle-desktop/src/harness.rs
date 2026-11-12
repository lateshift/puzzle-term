//! Deterministic headless scenario runner for GPU and terminal evidence (§13.1).
//!
//! Scenario input is line oriented. Blank lines and whole-line `#` comments
//! are ignored; every other line starts with one command name. M0 recognizes
//! sizing, an offscreen screenshot, an empty-grid dump, and a pixel assertion.
//! Later milestones can extend the command dispatcher without changing the
//! renderer and output directory used by current scenarios.
//!
//! A screenshot is a real GPU clear followed by texture readback. `expect-pixel`
//! checks that returned byte array, rather than predicting the color from a
//! Rust constant. This catches surface format and transfer mistakes that a
//! CPU-only assertion would miss. The PNG is retained as visual review evidence.
//!
//! Outputs use numbered names to preserve scenario order. The source filename
//! selects a subdirectory under `target/harness`, while labels identify a frame
//! within it. A later command can assert the most recent screenshot.
//!
//! The harness owns default settings and never reads the user's desktop
//! configuration. A smoke run therefore yields the same pixel expectations
//! on machines with different personal themes.
//!
//! M3 scene selection replaces only the image producer. The same GPU device,
//! PNG path and readback assertions remain active for shape and font scenes,
//! which makes the early clear-only smoke scenario a regression too.
//!
//! M4 text commands select a third image producer after shaping and
//! backend-neutral placement. Selecting a new producer clears the pending
//! previous fixture, so old geometry cannot satisfy a new text assertion.
//!
//! Captured M5.5b PTY fixtures now pass through M6's retained frame builder
//! and the same three-pipeline executor as a live surface. Readback serves
//! PNG evidence only; it no longer recolors application pixels on the CPU.
/// Headless hosts consume sealed publications with independently owned caches.
pub(crate) mod drawing;
use crate::{block_on::block_on, png};
use puzzle_render::{FrameRenderer, Renderer, RgbaImage};
use puzzle_term::frame_builder::context::TextContext;
use puzzle_term::session::cursor_settings::CursorSettings;
use puzzle_term::slug_scene::{SlugScene, SlugSceneKind};
use puzzle_term::{frame::RenderFrame, frame_builder::FrameBuilder};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};
use tracing_subscriber::prelude::*;
/// The headless host transfers its whole scenario to the process actor.
mod actor;
/// Screenshot assertions use the scene's actual pixel or cell geometry.
mod assertions;
// Optional scene families are whole-file extensions of the ordinary runner.
include!("harness/optional_items.rs");
/// Explicit typed gestures cover drag negotiation independently of the window toolkit.
mod dnd;
/// GPU readback, independently built cold frames and test-only damage faults.
mod drawn;
/// Failed commands retain immutable pixels and transcripts across later retries.
mod failures;
/// Continuous truecolor output checks with a bounded fake PTY.
mod flood;
/// D-31 assertions about physical columns and sparse grapheme text.
mod grapheme;
/// Kitty image bytes, cell anchors and exact raster pixels at both DPRs.
mod images;
/// Cell-range ink assertions shared by M3 and later visual scenarios.
mod ink;
/// Recorded PTY fixtures rendered through the retained session frame contract.
mod preview;
/// Native action log replay with actual parse and snapshot offsets.
mod replay_actions;
/// Fractional scroll readback uses the real Session and typed wheel path.
mod scroll;
/// Search commands drive the real bounded Session model and match fills.
mod search;
/// Tracked selection colors and ligature clipping through actual GPU pixels.
mod selection;
/// Pixel equality and atlas-generation probes for two shared sessions.
mod shared_context;
/// Live PTY regression scenario for nvim keyboard negotiation.
mod shell;
/// M4 quoted text commands and text-scene preparation through puzzle-text.
mod text;
/// Deterministic theme selection and all built-in gallery readbacks.
mod theme;
/// Live Session appearance cycles rendered through GPU readback.
mod theme_cycle;
/// D-29 threaded resize and flood measurements use the live Session seam.
mod thread_storm;
pub(crate) use actor::run;
/// Executes the supported M0 scenario commands and writes evidence files.
///
/// The source path names a `.pzs` file. Parsing errors include line numbers so
/// a failing acceptance run can point to the precise scenario instruction.
fn run_inline(scenario: Option<&str>) -> Result<(), String> {
    let scenario = scenario.ok_or("usage: puzzle-desktop harness <scenario.pzs>")?;
    let source = fs::read_to_string(scenario).map_err(|error| error.to_string())?;
    let stem = Path::new(scenario)
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or("invalid scenario filename")?;
    // Keep all generated artifacts under the build tree, not alongside source.
    let output = PathBuf::from("target/harness").join(stem);
    fs::create_dir_all(&output).map_err(|error| error.to_string())?;
    // Send harness diagnostics to a reviewable file beside the image.
    // The file is created before rendering so initialization failures leave evidence.
    let log = fs::File::create(output.join("log.txt")).map_err(|error| error.to_string())?;
    // Keep headless logs out of stdout, which xtask uses for command results.
    let file = tracing_subscriber::fmt::layer()
        .with_writer(Mutex::new(log))
        .with_filter(crate::logging::targets()?);
    tracing_subscriber::registry().with(file).init();
    // Name the script in the log before requesting the adapter, which may fail.
    tracing::info!(scenario, "headless scenario started");
    // Measure after setup so reported time describes scenario execution.
    let started = Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let renderer = block_on(Renderer::new(&instance, None)).map_err(|error| error.to_string())?;
    // The initial cell size is intentionally fixed until font metrics exist.
    let mut state = Scenario::new(renderer, output);
    for (line_no, raw) in source.lines().enumerate() {
        // Preserve source line numbers even when comments are skipped.
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Err(error) = state.command(line) {
            let error = format!("{scenario}:{}: {error}", line_no + 1);
            // Preserve even recognizer-only failures whose warm/cold images agree.
            return match failures::capture(&state, &error, &source) {
                Ok(path) => Err(format!("{error}; retained failure: {}", path.display())),
                Err(capture) => Err(format!("{error}; failure capture: {capture}")),
            };
        }
    }
    // M0 records total scenario time; later milestones add per-phase timings.
    fs::write(
        state.output.join("timings.txt"),
        format!("total_ms={}\n", started.elapsed().as_millis()),
    )
    .map_err(|error| error.to_string())?;
    // A completion event distinguishes a fully asserted scenario from a crash.
    tracing::info!(scenario, "headless scenario completed");
    Ok(())
}
/// Mutable state shared by commands in one scenario file.
///
/// Grid dimensions and the latest image evolve as commands execute in order.
/// The GPU device remains constant for the whole run so readback is comparable
/// across screenshots in a scenario.
struct Scenario {
    /// Additional scene state is absent until a script explicitly requests it.
    extra_scene: ExtraScene,
    /// Shared renderer whose output is asserted by pixel checks.
    renderer: Renderer,
    /// M0 grid width in cells.
    cols: u32,
    /// M0 grid height in cells.
    rows: u32,
    /// Most recent offscreen image, retained for assertions.
    image: Option<RgbaImage>,
    /// Named M3 geometry fixture selected for the next screenshot.
    slug_scene: Option<SlugSceneKind>,
    /// The next screenshot's M4 grid or free text request.
    text_request: Option<text::TextRequest>,
    /// Byte-fed VT state for deterministic recorded-session screenshots.
    terminal: Option<puzzle_term::vt::Terminal>,
    /// Synthetic drag owner exists only when a scenario invokes drag-start.
    drag_scene: Option<dnd::Scene>,
    /// Optional Session-backed search fixture retains fonts and GPU uploads.
    search_scene: Option<search::SearchScene>,
    /// A rendered terminal image remains valid until more bytes arrive.
    preview_ready: bool,
    /// Neutral row cache retained across terminal screenshots.
    frame_builder: Option<FrameBuilder>,
    /// Shared shaping and atlas resources for this scenario's frame builds.
    text_context: Option<TextContext>,
    /// Harness cursor preferences persist across successive terminal frames.
    cursor_settings: CursorSettings,
    /// Explicit harness theme selection, absent in legacy M6.3 scenes.
    theme_selection: Option<theme::ThemeSelection>,
    /// GPU pipelines and atlas textures retained across screenshots.
    frame_renderer: Option<FrameRenderer>,
    /// Reusable instance arrays filled by the production frame builder.
    terminal_frame: RenderFrame,
    /// Pre-edit row revisions used only to inject a skipped-damage test fault.
    old_row_versions: Vec<u64>,
    /// Scenario device-pixel ratio, set explicitly by a `dpr` command.
    dpr: f32,
    /// Extra backing pixels left after all complete terminal cells.
    surface_extra: [u32; 2],
    /// Optional live child for scenarios that exercise PTY input bytes.
    shell: Option<shell::ShellRun>,
    /// Number of real PTY bytes mirrored into the drawn shell witness.
    shell_render_offset: usize,
    /// Pixel-inferred rows from the frame immediately before the last render.
    prior_drawn_rows: Option<Vec<String>>,
    /// Recognition is reused by several assertions on the same GPU image.
    drawn_cache: Option<drawn::recognize::Readback>,
    /// Output directory for review evidence.
    output: PathBuf,
    /// Sequence number used in evidence filenames.
    number: usize,
}
/// Interprets supported M0 commands against one persistent GPU device.
impl Scenario {
    /// Starts one script with no carried-over scene or terminal state.
    fn new(renderer: Renderer, output: PathBuf) -> Self {
        Self {
            extra_scene: Default::default(),
            renderer,
            cols: 80,
            rows: 24,
            image: None,
            slug_scene: None,
            text_request: None,
            terminal: None,
            search_scene: None,
            drag_scene: None,
            preview_ready: false,
            frame_builder: None,
            text_context: None,
            cursor_settings: CursorSettings::default(),
            theme_selection: None,
            frame_renderer: None,
            terminal_frame: RenderFrame::default(),
            old_row_versions: Vec::new(),
            dpr: 1.0,
            surface_extra: [0, 0],
            shell: None,
            shell_render_offset: 0,
            prior_drawn_rows: None,
            drawn_cache: None,
            output,
            number: 0,
        }
    }
    /// Applies one M0 command or rejects an unsupported spelling.
    ///
    /// Unknown names fail fast. Accepting and ignoring them would make a later
    /// milestone's scenario appear green under an older harness binary.
    fn command(&mut self, line: &str) -> Result<(), String> {
        if let Some(result) = self.model_command(line) {
            return result;
        }
        let mut parts = line.split_whitespace();
        match parts.next().ok_or("empty command")? {
            "dpr" => self.set_dpr(parts.next()),
            "render-text" | "render-grid" => self.select_text(line),
            "slug-scene" => self.select_slug(parts.next()),
            "terminal" => preview::terminal(self, parts.next()),
            "resize-terminal" => preview::resize(self, parts.next()),
            "cursor-settings" => preview::cursor_settings(self, line),
            "appearance" => theme::appearance(self, parts.next()),
            "set" => theme::set(self, line),
            "theme-gallery" => theme::gallery(self),
            "image-scenes" => images::run(self),
            "image-replay" => images::replay::run(self, parts.next()),
            "appearance-cycle" => theme_cycle::run(self),
            "surface-extra" => preview::set_surface_extra(self, parts.next()),
            "shell" => shell::open(self, line),
            "key" if self.shell.is_some() => shell::key(self, line),
            "input" if self.shell.is_some() => shell::input(self, line),
            "expect-pty-key" => shell::expect_pty_key(self, line),
            "expect-paced-no-marker" => shell::expect_paced_no_marker(self, line),
            "expect-paced-repeat-no-marker" => shell::expect_paced_repeat_no_marker(self, line),
            "wait-idle" => shell::wait_idle(self, line),
            "render-shell" => shell::render(self),
            "frames" if self.shell.is_some() => shell::frames(self, parts.next()),
            "flood" => flood::run(&self.renderer, &self.output),
            "resize-storm" => flood::run_storm(&self.renderer, &self.output),
            "thread-storm" => thread_storm::run(&self.renderer, &self.output),
            // Real PTY captures must pass through the same bounded parser
            // as later transports, preserving modes and alternate screens.
            "replay" => preview::replay(self, parts.next()),
            "replay-actions" => replay_actions::run(self, line),
            "feed" => preview::feed(self, line),
            "render-terminal" => preview::render(self),
            "fractional-scroll" => scroll::run(self),
            "selection-scenes" => selection::run(self),
            "osc-selection-scenes" => selection::osc_colours(self),
            "shared-context" => shared_context::run(self),
            // Grid assertions run before rasterization, so a white glyph
            // cannot conceal incorrect cursor or parser state.
            "expect-text" => preview::expect_text(self, line),
            "expect-column" => grapheme::expect_column(self, line),
            "expect-cell" => grapheme::expect_cell(self, line),
            "expect-grapheme-count" => grapheme::expect_grapheme_count(self, line),
            "expect-screen" => preview::expect_screen(self, parts.next()),
            "size" => self.set_size(parts.next()),
            "screenshot" => self.screenshot_named(parts.next()),
            "dump" => preview::dump(self, parts.next()),
            "expect-pixel" => self.expect_pixel(&mut parts),
            "expect-ink" => self.expect_ink(&mut parts),
            "expect-ink-px" => self.expect_ink_pixels(&mut parts),
            "expect-cells-identical" => self.expect_cells_identical(&mut parts),
            // A typo must fail rather than silently skip an assertion.
            other => Err(format!("unsupported M0 command: {other}")),
        }
    }
    /// Validates a screenshot label before selecting its evidence filename.
    fn screenshot_named(&mut self, label: Option<&str>) -> Result<(), String> {
        self.screenshot(label.ok_or("screenshot needs a label")?)
    }
    /// Changes the active image producer to a shaped text fixture.
    fn select_text(&mut self, line: &str) -> Result<(), String> {
        // The text parser keeps quoted spaces and escapes intact; selecting
        // it discards the prior Slug fixture before a later screenshot.
        self.text_request = Some(text::parse(line, self.dpr)?);
        self.slug_scene = None;
        self.preview_ready = false;
        self.drawn_cache = None;
        Ok(())
    }

    /// Changes the active image producer to a standalone Slug fixture.
    fn select_slug(&mut self, raw: Option<&str>) -> Result<(), String> {
        // A screenshot prepares a fresh atlas, exposing first-use upload
        // errors that a retained text scene could otherwise hide.
        let name = raw.ok_or("slug-scene needs a name")?;
        self.slug_scene = Some(SlugSceneKind::parse(name).ok_or("unknown Slug scene")?);
        self.text_request = None;
        self.preview_ready = false;
        self.drawn_cache = None;
        Ok(())
    }
    /// Sets physical scale for every subsequent text or terminal rendering.
    fn set_dpr(&mut self, raw: Option<&str>) -> Result<(), String> {
        // Fractional DPRs exercise D-27's 1.5 handoff; the upper bound
        // prevents an accidental huge offscreen target.
        let value: f32 = raw
            .ok_or("dpr needs a number")?
            .parse()
            .map_err(|_| "invalid DPR")?;
        if !value.is_finite() || value <= 0.0 || value > 4.0 {
            return Err("DPR must be within (0,4]".into());
        }
        self.dpr = value;
        if let Some(context) = self.text_context.as_mut() {
            context.reconfigure(14.0, value)?;
            if let Some(builder) = self.frame_builder.as_mut() {
                // A monitor change invalidates this session's cached rows,
                // while the context retains outlines keyed by ppem.
                builder.set_context(context);
            }
        }
        Ok(())
    }
    /// Sets M0 placeholder dimensions without creating a VT model.
    fn set_size(&mut self, raw: Option<&str>) -> Result<(), String> {
        // `size` remains the original clear-only command; M5 previews use
        // `terminal` to create an actual grid with its own bounded history.
        let value = raw.ok_or("size needs COLSxROWS")?;
        let (cols, rows) = value.split_once('x').ok_or("size needs COLSxROWS")?;
        self.cols = cols.parse().map_err(|_| "invalid column count")?;
        self.rows = rows.parse().map_err(|_| "invalid row count")?;
        if self.cols == 0 || self.rows == 0 {
            return Err("size must be nonzero".into());
        }
        Ok(())
    }
    /// Selects text, Slug fixture or the original clear-only image source.
    fn screenshot(&mut self, label: &str) -> Result<(), String> {
        if self.preview_ready {
            // Reuse exactly the image produced by `render-terminal`;
            // falling through would silently replace it with an M0 clear.
            let path = self.path(label, "png");
            let image = self
                .image
                .as_ref()
                .ok_or("terminal preview image missing")?;
            return png::write(&path, image);
        }
        // A fresh producer changes attachment pixels. Keep recognition
        // only when re-saving the unchanged terminal preview above.
        self.drawn_cache = None;
        // The pending producer determines the image. All three paths feed
        // the same readback and later pixel/ink assertion commands.
        if let Some(request) = self.text_request.as_ref() {
            let image = text::render(&self.renderer, request)?;
            let path = self.path(label, "png");
            png::write(&path, &image)?;
            self.image = Some(image);
            return Ok(());
        }
        if let Some(kind) = self.slug_scene {
            return self.screenshot_slug(label, kind);
        }
        // Guard both integer overflow and wgpu's default texture limit.
        let width = self
            .cols
            .checked_mul(8)
            .filter(|value| *value <= 8192)
            .ok_or("scenario width exceeds the M0 GPU limit")?;
        let height = self
            .rows
            .checked_mul(16)
            .filter(|value| *value <= 8192)
            .ok_or("scenario height exceeds the M0 GPU limit")?;
        let image = self
            .renderer
            .render_to_image(width, height)
            .map_err(|error| error.to_string())?;
        // Keep GPU readback bytes for later `expect-pixel` and `expect-ink`.
        let path = self.path(label, "png");
        png::write(&path, &image)?;
        self.image = Some(image);
        Ok(())
    }
    /// Renders one named shape with the production Slug path and keeps pixels.
    fn screenshot_slug(&mut self, label: &str, kind: SlugSceneKind) -> Result<(), String> {
        // Only the platform reads asset files. Synthetic fixtures ignore the
        // optional face, while glyph fixtures require these exact bytes.
        let font = crate::font_data::bundled_font()?;
        let mut scene = SlugScene::new(kind, Some(&font))?;
        let image = self
            .renderer
            .render_slug_to_image(
                &mut scene.atlas,
                &scene.instances,
                scene.width,
                scene.height,
            )
            .map_err(|error| error.to_string())?;
        let path = self.path(label, "png");
        png::write(&path, &image)?;
        self.image = Some(image);
        Ok(())
    }
    /// Returns the next numbered evidence path.
    ///
    /// Numbering is shared by screenshots and grid dumps, matching the order
    /// in the scenario instead of maintaining one counter per artifact kind.
    fn path(&mut self, label: &str, extension: &str) -> PathBuf {
        self.number += 1;
        self.output
            .join(format!("{:03}-{label}.{extension}", self.number))
    }
}
