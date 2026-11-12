//! Repeated host appearance changes through the production Session (§14.7).
//!
//! The static gallery installs a Theme directly into a FrameBuilder. This
//! probe instead submits each appearance as a platform-origin bus command,
//! then samples Session's injected clock and reads actual GPU pixels. It
//! catches a stale cursor token in the transition, row policy or renderer.
//! Blink is disabled so an intentional off phase cannot impersonate a
//! missing cursor. Every cell starts blank, making inverse color equal to
//! the theme foreground without glyph anti-aliasing at the sample point.
//!
//! This probe differs from the static theme gallery. The gallery installs
//! a theme directly in its FrameBuilder. The live window submits a typed
//! platform-origin appearance command, then Session chooses a pair member,
//! resets OSC overrides, starts a transition, and samples it at a frame
//! time. Direct installation could pass even if the bus route or fade
//! retained a stale cursor color. This probe follows the live route.
//!
//! Each case owns a fresh Session, TextContext and FrameRenderer. A new
//! Session prevents a preceding light case from influencing a dark case.
//! A new text context prevents its glyph atlas from borrowing the last
//! case's prepared handles. Within a case, the renderer remains alive
//! across all four switches, because that is how the actual window keeps
//! GPU resources. A test that recreated it after every switch would erase
//! the cache history implicated by the user's second-switch report.
//!
//! The matrix covers both initial OS appearances, instant and 150-ms
//! changes, and the configured `theme` and `inverse` cursor policies. An
//! initial light fact is submitted before the first frame, so its first
//! image is a light endpoint rather than a manufactured startup fade.
//! Each later host fact arrives 500 ms after its predecessor, after the
//! longest requested fade is complete. That makes the expected source
//! color of every midpoint precise, including the second dark return.
//!
//! Time is injected into Session. A 150-ms case is read at its 75-ms
//! midpoint and its exact 150-ms endpoint. A zero-duration case reads
//! the endpoint immediately. The expected midpoint applies the public
//! OKLab color mixer to source-backed tokens, independent of Session's
//! own cached `rendered` field. Exact endpoints are compared byte for byte
//! because solid rectangles and clear pixels have no antialiasing error.
//!
//! The four-cell grid is intentionally blank. The block cursor begins at
//! physical (16,16); (17,17) lies inside its filled rectangle and away
//! from a softened boundary. With no source glyph there, `inverse` uses
//! the cell's resolved foreground as the block color. `theme` uses the
//! authored cursor token. Pixel (0,0) is outside the grid in padding and
//! must use the concurrently interpolated background/clear token.
//!
//! Checking both pixels distinguishes a stale cursor field from a whole
//! frame stuck on an old palette. It also rules out a cursor rectangle
//! that is present but has the same bytes as the background. The program
//! writes a numbered PNG only after both exact readback checks pass, so a
//! failed sample cannot be mistaken for an accepted review reference.
//!
//! Cursor blinking is disabled for the color matrix. The default blink
//! has a legitimate off phase that leaves no cursor rectangle to sample.
//! Session's separate visibility regression drives that off phase, then
//! tests appearance, settings, focus and the idle cutoff. Together the
//! two checks distinguish a color-cache fault from missing geometry.

use super::Scenario;
use puzzle_render::{FrameRenderer, RgbaImage};
use puzzle_term::{
    Origin, Session, SessionConfig,
    frame_builder::context::TextContext,
    messages::Command,
    platform::{PtySize, Services, Transport, Waker},
    settings::{BlinkMode, Settings},
    text::{FontData, font_set::FontSet},
    theme::{Appearance, Theme, color},
};
use std::time::{Duration, Instant};

/// Headless transport leaves the bus and appearance path intact.
struct Sink;

impl Transport for Sink {
    /// Mode-2031 reports have no child in this GPU color probe.
    fn write(&mut self, _bytes: &[u8]) -> Result<(), String> {
        // A successful sink keeps the bus and OSC notification path active.
        // An error here would test transport recovery instead of color.
        Ok(())
    }
    /// Fixed cell geometry needs no PTY resize.
    fn resize(&mut self, _size: PtySize) -> Result<(), String> {
        // Session still computes device cells; only the absent child is skipped.
        Ok(())
    }
    /// No child exists to close after the scenario.
    fn close(&mut self) -> Result<(), String> {
        // Completion cannot leave an operating-system PTY behind in this case.
        Ok(())
    }
}

/// The scenario drives every requested display turn by injected time.
struct ManualWaker;

impl Waker for ManualWaker {
    /// A frame request is fulfilled by the next explicit sample.
    fn request_frame(&mut self) {
        // Injected scenario times replace nondeterministic display callbacks.
    }
    /// A finite fade is sampled at its midpoint and endpoint below.
    fn wake_at(&mut self, _instant: Instant) {
        // Every relevant deadline is sampled explicitly in the case loop.
    }
}

/// One session and retained GPU atlas for an isolated appearance sequence.
struct Probe {
    /// Typed commands and the fade clock live in the production Session.
    session: Session,
    /// Host-owned copies/caches persist across every appearance sample.
    drawing: super::drawing::Drawing,
    /// The platform owns font bytes and shares their prepared glyph atlas.
    context: TextContext,
    /// GPU textures persist across all frames of one case.
    gpu: FrameRenderer,
}

impl Probe {
    /// Starts with an exact system appearance before any frame is shown.
    fn new(
        state: &Scenario,
        start: Appearance,
        duration: u32,
        inverse: bool,
        now: Instant,
    ) -> Result<Self, String> {
        // Font bytes stay with the native harness. The neutral Session
        // receives only measured geometry and prepared atlas handles.
        let bytes = std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf")
            .map_err(|error| error.to_string())?;
        let fonts = FontSet::new(FontData::new(bytes, 0), None, None)?;
        let context = TextContext::new(fonts, 14.0, 1.0)?;
        // The typed bus still requires platform origin even though the
        // child transport has been replaced by an in-memory sink.
        let services = Services::new(Box::new(Sink), Box::new(ManualWaker));
        // Four columns keep readback cheap while leaving the first cursor
        // cell and a distinct padding pixel available to assertions.
        let config = SessionConfig {
            columns: 4,
            rows: 1,
            ..SessionConfig::default()
        };
        let mut session = Session::new(config, services);
        session.set_text_context(&context);
        // Blink is handled by the separate visibility table. This matrix
        // isolates cursor-color decisions at exact transition times.
        let settings = Settings {
            theme_transition_ms: duration,
            cursor_blink: BlinkMode::Off,
            cursor_color: if inverse { "inverse" } else { "theme" }.into(),
            ..Settings::default()
        };
        if start == Appearance::Light {
            // An initial host fact precedes the first visible frame, so
            // startup has an exact endpoint instead of a dark flash.
            // Submitting it after settings would manufacture a fade from
            // the neutral Session's provisional dark appearance.
            switch(&mut session, start, now);
        }
        session.apply_settings(&settings, now);
        // The cell metrics determine the backing extent. A hard-coded
        // 8×16 grid would sample the wrong pixel at a different font size.
        let metrics = context.metrics();
        session.set_surface_size([4 * metrics.width_px + 32, metrics.height_px + 32]);
        let gpu = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
        // This retained renderer sees every upload across the four switches.
        Ok(Self {
            session,
            drawing: Default::default(),
            context,
            gpu,
        })
    }

    /// Checks one exact theme endpoint against two GPU pixels.
    fn sample(
        &mut self,
        state: &mut Scenario,
        at: Instant,
        theme: &Theme,
        inverse: bool,
        label: &str,
    ) -> Result<(), String> {
        // The top-left cell contains no ink: inverse fill is its foreground.
        // Theme mode instead uses the explicit semantic cursor token.
        let cursor = if inverse {
            theme.foreground
        } else {
            theme.cursor
        };
        self.sample_colors(state, at, (cursor, theme.background), label)
    }

    /// Reads the filled cursor and padding from an actual attachment.
    fn sample_colors(
        &mut self,
        state: &mut Scenario,
        at: Instant,
        expected: ([u8; 3], [u8; 3]),
        label: &str,
    ) -> Result<(), String> {
        // Session samples its transition at this injected instant before
        // the same RenderFrame type used by the live surface is built.
        let frame = self
            .drawing
            .draw(&mut self.session, at, &mut self.context)?
            .ok_or("appearance frame absent")?;
        let (image, _) = self
            .gpu
            .render_to_image(&state.renderer, frame)
            .map_err(|error| error.to_string())?;
        // Both probes inspect GPU bytes, never only CPU instance colors.
        expect_pixel(&image, 17, 17, expected.0, label)?;
        expect_pixel(&image, 0, 0, expected.1, label)?;
        if expected.0 == expected.1 {
            // An opaque block can still disappear if its token equals paper.
            return Err(format!("{label}: cursor token equals background"));
        }
        // Keep only successful samples as screenshot evidence for review.
        state.image = Some(image);
        state.preview_ready = true;
        state.screenshot(label)
    }
}

/// Renders both starting appearances, durations and cursor color choices.
pub(super) fn run(state: &mut Scenario) -> Result<(), String> {
    // Isolating each case prevents prior GPU uploads or Session state from
    // concealing a second-switch cache error in another combination.
    for start in [Appearance::Dark, Appearance::Light] {
        for duration in [0, 150] {
            for inverse in [false, true] {
                run_case(state, start, duration, inverse)?;
            }
        }
    }
    Ok(())
}

/// Creates the same typed appearance and cursor settings used by the window.
fn run_case(
    state: &mut Scenario,
    start: Appearance,
    duration: u32,
    inverse: bool,
) -> Result<(), String> {
    let now = Instant::now();
    let mut probe = Probe::new(state, start, duration, inverse, now)?;
    let initial = Theme::builtin(name(start)).ok_or("missing starting theme")?;
    let label = format!("start-{}-{duration}-{}", name(start), mode(inverse));
    probe.sample(state, now, &initial, inverse, &label)?;
    // A completed switch precedes the next one. Both directions recur,
    // catching state that is overwritten only on the second OS callback.
    let mut from = initial;
    for (index, next) in sequence(start).into_iter().enumerate() {
        let changed = now + Duration::from_millis(500 * (index as u64 + 1));
        switch(&mut probe.session, next, changed);
        let to = Theme::builtin(name(next)).ok_or("missing destination theme")?;
        if duration > 0 {
            let middle = changed + Duration::from_millis(u64::from(duration / 2));
            let expected = mixed(&from, &to, inverse, 0.5);
            let label = format!(
                "{}-{duration}-{}-{}-mid",
                name(start),
                mode(inverse),
                index + 1
            );
            probe.sample_colors(state, middle, expected, &label)?;
        }
        let end = changed + Duration::from_millis(u64::from(duration));
        let label = format!(
            "{}-{duration}-{}-{}-end",
            name(start),
            mode(inverse),
            index + 1
        );
        probe.sample(state, end, &to, inverse, &label)?;
        from = to;
    }
    Ok(())
}

/// Sends a real platform-origin command rather than directly setting colors.
fn switch(session: &mut Session, appearance: Appearance, at: Instant) {
    session.submit(Command::SetSystemAppearance(appearance), Origin::Platform);
    session.pump(at);
}

/// Implements the scenario's exact `expect-pixel` rule for one RGB sample.
fn expect_pixel(
    image: &RgbaImage,
    x: u32,
    y: u32,
    rgb: [u8; 3],
    label: &str,
) -> Result<(), String> {
    let offset = ((y * image.width + x) * 4) as usize;
    let actual = image
        .pixels
        .get(offset..offset + 3)
        .ok_or("pixel outside image")?;
    if actual == rgb {
        Ok(())
    } else {
        Err(format!(
            "{label}: expect-pixel {x} {y} #{:02x}{:02x}{:02x}, got #{:02x}{:02x}{:02x}",
            rgb[0], rgb[1], rgb[2], actual[0], actual[1], actual[2]
        ))
    }
}

/// Computes the expected midpoint independently from Session's rendered theme.
fn mixed(from: &Theme, to: &Theme, inverse: bool, amount: f64) -> ([u8; 3], [u8; 3]) {
    let cursor = if inverse {
        color::mix(from.foreground, to.foreground, amount)
    } else {
        color::mix(from.cursor, to.cursor, amount)
    };
    (cursor, color::mix(from.background, to.background, amount))
}

/// Names the source-backed pair member selected by `appearance`.
fn name(appearance: Appearance) -> &'static str {
    match appearance {
        Appearance::Dark => "puzzle-dark",
        Appearance::Light => "puzzle-light",
    }
}

/// Keeps screenshot labels distinct for two setting policies.
fn mode(inverse: bool) -> &'static str {
    if inverse { "inverse" } else { "theme" }
}

/// Repeats both directions, including the user's light-to-dark failure path.
fn sequence(start: Appearance) -> [Appearance; 4] {
    match start {
        Appearance::Dark => [
            Appearance::Light,
            Appearance::Dark,
            Appearance::Light,
            Appearance::Dark,
        ],
        Appearance::Light => [
            Appearance::Dark,
            Appearance::Light,
            Appearance::Dark,
            Appearance::Light,
        ],
    }
}
