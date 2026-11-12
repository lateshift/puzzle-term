//! Command-block hover shares the same presented-input route as search and links.
//! Session-backed search commands before the M9 desktop search bar exists.
//!
//! Declaration specimens load the bundled icon fallback before frame building.
//!
//! Start/next/previous/clear use public commands and bounded frame slices. The
//! scene owns its text context and GPU cache exactly like a one-window host.
//! Commands select a persistent Session rather than the older raw-VT preview.
//! Feeding bytes never performs a search: query work belongs to explicit frame
//! turns, as it does in the two live shells. Keeping that distinction lets a
//! large scripted query demonstrate partial counts without blocking input.
//!
//! Navigation uses ScrollTo(Match), not a private result-index setter. The
//! viewport therefore centres the same tracked line the public API would.
//! Clearing releases handles and paints the ordinary cached rows again.
//!
//! The scene uses a custom theme over the bundled dark palette. Two authored
//! search colors make the readback independent of the renderer color policy.
//! Each matched cell requires paper and ink; outside pixels must equal the
//! baseline, including padding and any ordinary SGR background. This prevents
//! a stale grid or broad rectangle from becoming an accepted golden candidate.
//!
//! DPR changes create independent contexts, so hinted and unhinted outlines
//! cannot accidentally substitute for each other. Within each ratio the GPU
//! cache is retained, preserving the upload/damage behavior of a real window.
//! The baseline owns its pixels because the next render replaces Scenario.image.
//! Recording it is an explicit cold diagnostic step, not a production hot path.
//!
//! Repeated screenshots preserve uploads and row geometry, making stale state
//! visible. Existing feed escaping is shared with the older VT preview path.
//! Public overlay specimens use this persistent scene to exercise real uploads.
//! Fonts added for a specimen increment the context generation before capture.
//! The COLR test face is explicitly selected to avoid Nerd Font PUA collisions.
//! Font test assets are read from disk only for the requested review specimen.
//! Provider registration requests damage; subsequent render commands drive it.
//! Ordinary search fixtures continue using only their original primary face.
use super::{
    Scenario, preview,
    scroll::{Manual, Sink},
};
use puzzle_render::{FrameRenderer, RgbaImage};
use puzzle_term::{
    Origin, Session, SessionConfig,
    frame_builder::context::TextContext,
    messages::{Command, Value},
    platform::Services,
    search::SearchCommand,
    settings::Settings,
    text::{FontData, font_set::FontSet},
    viewport::ScrollTarget,
    vt::search::FindOptions,
};
use std::{collections::BTreeMap, time::Instant};
/// Independently authored per-cell paper/ink checks for the small reference scene.
mod pixels;
/// Persistent ownership for scripted queries through the production Session.
pub(super) struct SearchScene {
    /// Parser, tracked search and viewport all share this document.
    session: Session,
    /// Sealed row copies and drawing caches stay with this diagnostic host.
    drawing: super::drawing::Drawing,
    /// Existing bundled outlines and atlas stay warm between query steps.
    context: TextContext,
    /// GPU textures receive only real frame uploads, never a second cold upload.
    gpu: FrameRenderer,
    /// Ordinary image anchors exact outside-match pixel identity.
    baseline: Option<RgbaImage>,
    /// Injected elapsed time for finite overlay deadlines; no real sleeping.
    time_offset: std::time::Duration,
}
/// Claims only explicit search commands and feed/render while that scene is active.
pub(super) fn command(state: &mut Scenario, line: &str) -> Option<Result<(), String>> {
    // Route by the first word without interpreting quoted query spaces.
    let name = line.split_whitespace().next()?;
    // Only an explicit scene command replaces the Session and shared resources.
    if name == "search-terminal" {
        return Some(create(state));
    }
    // Selection and legacy preview commands remain with their existing owners.
    let owned = matches!(
        name,
        "search"
            | "overlay"
            | "overlay-hover-scrollbar"
            | "overlay-hover-link"
            | "overlay-hover-block"
            | "overlay-copy-block"
            | "advance-ms"
            | "search-next"
            | "search-prev"
            | "search-clear"
            | "search-baseline"
            | "search-check"
            | "expect-search"
    );
    // Feed/render may be shared spellings, but belong here only after creation.
    if !owned && !(state.search_scene.is_some() && matches!(name, "feed" | "render-terminal")) {
        return None;
    }
    Some(apply(state, line, name))
}
/// Loads approved fonts and authored theme tokens without user configuration.
fn create(state: &mut Scenario) -> Result<(), String> {
    // Bundled font bytes are already approved; never consult host system fonts.
    let bytes =
        std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf").map_err(|e| e.to_string())?;
    // This context owns metrics and a warm glyph atlas at the selected DPR.
    let context = TextContext::new(
        FontSet::new(FontData::new(bytes, 0), None, None)?,
        14.0,
        state.dpr,
    )?;
    // No PTY, timer or user configuration can introduce nondeterministic output.
    let mut session = Session::new(
        SessionConfig {
            columns: state.cols as usize,
            rows: state.rows as usize,
            ..Default::default()
        },
        Services::new(Box::new(Sink), Box::new(Manual)),
    );
    // Install the host-owned context before applying renderer policy.
    session.set_text_context(&context);
    // Harness notifications are in-window even in deterministic screenshot scenes.
    session.enable_notification_toasts();
    // Fixed paper bytes permit independent pixel assertions rather than policy lookup.
    let theme = Value::Map(BTreeMap::from([
        ("extends".into(), Value::String("puzzle-dark".into())),
        ("searchMatch".into(), Value::String("#334455".into())),
        ("searchCurrent".into(), Value::String("#775533".into())),
        // Fixed UI paper/ink gives the model specimen independent pixel assertions.
        ("scrollbarThumb".into(), Value::String("#999999ff".into())),
        ("scrollbarTrack".into(), Value::String("#202020".into())),
        ("pillBackground".into(), Value::String("#335544".into())),
        ("badgeBackground".into(), Value::String("#223344".into())),
    ]));
    // Token resolution goes through production custom-theme validation.
    session.apply_settings(
        &Settings {
            theme_dark: "search-fixture".into(),
            render_padding: 16.0,
            themes: Value::Map(BTreeMap::from([("search-fixture".into(), theme)])),
            ..Default::default()
        },
        Instant::now(),
    );
    // The renderer keeps GPU textures alive across every query/navigation step.
    state.search_scene = Some(SearchScene {
        session,
        drawing: Default::default(),
        context,
        gpu: FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm),
        baseline: None,
        time_offset: std::time::Duration::ZERO,
    });
    Ok(())
}
/// Executes one scripted action, restoring the scene even when an assertion fails.
fn apply(state: &mut Scenario, line: &str, name: &str) -> Result<(), String> {
    // Taking ownership permits rendering through Scenario without aliasing it.
    let mut scene = state
        .search_scene
        .take()
        .ok_or("search needs search-terminal")?;
    // A closure collects early returns so restoration also happens on errors.
    let result = (|| {
        // One action and its frame share an instant; no sleeping drives progress.
        let now = Instant::now() + scene.time_offset;
        if !action(&mut scene, state, line, name, now)? {
            return Ok(());
        }
        scene.session.pump(now);
        // A render command advances exactly one bounded slice, not the whole query.
        if let Some(frame) = scene
            .drawing
            .draw(&mut scene.session, now, &mut scene.context)?
        {
            // Render incremental uploads exactly once through the existing pipeline.
            let (image, _) = scene
                .gpu
                .render_to_image(&state.renderer, frame)
                .map_err(|e| e.to_string())?;
            // Hit-testing follows only ink that reached this offscreen presentation.
            scene.drawing.presented(&mut scene.session, now)?;
            // Screenshots save this verified result without rebuilding cold geometry.
            state.image = Some(image);
            state.preview_ready = true;
        }
        Ok(())
    })();
    // Preserve ownership for subsequent commands even after an assertion failure.
    state.search_scene = Some(scene);
    result
}
/// Submits one query operation or performs an explicit readback assertion.
fn action(
    scene: &mut SearchScene,
    state: &Scenario,
    line: &str,
    name: &str,
    now: Instant,
) -> Result<bool, String> {
    match name {
        // Parser mutations stay separate from the explicit display slice.
        // Public overlay fixtures share this persistent Session and text context.
        // Registration requests a fresh frame; callbacks run only after row layout.
        // Keeping the GPU owner here exercises ordinary pending-upload ownership.
        // A fixture can replace provider output without replacing its retained VT.
        "overlay" => install_overlay(scene, line)?,
        // Pointer specimens consume the same acknowledged geometry as live input.
        name if name.starts_with("overlay-") => hover_overlay(scene, name, now)?,
        // Clock-only actions leave frame consumption to the next explicit render.
        "advance-ms" => return advance_time(scene, line).map(|_| false),
        "feed" => {
            scene.session.feed(&preview::feed_bytes(line)?, now);
            return Ok(false);
        }
        // Query strings preserve spaces through the shared quoted parser.
        "search" => scene.session.submit(
            Command::Search(SearchCommand::Find {
                query: preview::quoted(line, "search")?.into(),
                options: FindOptions::default(),
            }),
            Origin::Api,
        ),
        // Directional operations exercise centring and same-row current changes.
        "search-next" => scene
            .session
            .submit(Command::ScrollTo(ScrollTarget::Match(1)), Origin::Api),
        "search-prev" => scene
            .session
            .submit(Command::ScrollTo(ScrollTarget::Match(-1)), Origin::Api),
        // The public clear command releases trackers and requests repaint.
        "search-clear" => scene
            .session
            .submit(Command::Search(SearchCommand::Clear), Origin::Api),
        // Readback replacement must not overwrite the comparison baseline.
        "search-baseline" => {
            capture_baseline(scene, state);
            return Ok(false);
        }
        // Independent cell coordinates catch correctly counted but misdrawn hits.
        "search-check" => {
            return pixels::check(scene, state.image.as_ref().ok_or("search image absent")?)
                .map(|_| false);
        }
        "expect-search" => return expect_state(scene, line).map(|_| false),
        _ => {}
    }
    // Input policy executes immediately; only frame work scans retained text.
    Ok(true)
}
/// Copies baseline pixels only on an explicit diagnostic command.
fn capture_baseline(scene: &mut SearchScene, state: &Scenario) {
    // GPU readback images are replaced after every navigation frame.
    // An owned copy keeps the ordinary text stable for outside-pixel comparisons.
    scene.baseline = state.image.as_ref().map(|image| RgbaImage {
        width: image.width,
        height: image.height,
        pixels: image.pixels.clone(),
    });
}
/// Asserts exact count/current state independently of the highlight pixels.
fn expect_state(scene: &SearchScene, line: &str) -> Result<(), String> {
    // Counts and indices are fixture expectations, not inferred from GPU output.
    let mut words = line.split_whitespace().skip(1);
    // Reject malformed expected counts instead of treating them as zero hits.
    let count: usize = words
        .next()
        .ok_or("expected match count missing")?
        .parse()
        .map_err(|_| "invalid count")?;
    // Absence before first navigation differs from the first index zero.
    let current = match words.next().ok_or("expected current missing")? {
        "none" => None,
        value => Some(
            value
                .parse::<usize>()
                .map_err(|_| "invalid current index")?,
        ),
    };
    // A four-row fixture completes in one slice; partial counts cannot pass.
    let state = scene.session.search_state();
    // Counts alone cannot prove that navigation selected the intended occurrence.
    if state.count != count || state.current != current || !state.complete {
        return Err(format!(
            "search expected {count}/{current:?}, got {state:?}"
        ));
    }
    Ok(())
}

/// Loads bundled review faces only for the public free-text specimen.
fn install_overlay(scene: &mut SearchScene, line: &str) -> Result<(), String> {
    let fixture = line
        .split_whitespace()
        .nth(1)
        .ok_or("overlay needs a scene name")?;
    if fixture == "decorations" {
        // Gutter icons exercise only the already-approved bundled symbol fallback.
        let bytes = std::fs::read("assets/fonts/SymbolsNerdFontMono-Regular.ttf")
            .map_err(|e| e.to_string())?;
        scene.context.add_fallback(FontData::new(bytes, 0))?;
        scene.session.set_text_context(&scene.context);
    }
    if fixture == "text" {
        // Test assets stay on disk and never enter a distributed terminal.
        let mut color_face = 0;
        for path in [
            "assets/fonts/SymbolsNerdFontMono-Regular.ttf",
            "assets/fonts/test/test_glyphs-glyf_colr_1.ttf",
        ] {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            color_face = scene.context.add_fallback(FontData::new(bytes, 0))?;
        }
        scene.session.set_text_context(&scene.context);
        puzzle_term::test_support::overlay_scene::install_text(&mut scene.session, color_face);
        return Ok(());
    }
    puzzle_term::test_support::overlay_scene::install(
        &mut scene.session,
        line.split_whitespace()
            .nth(1)
            .ok_or("overlay needs a scene name")?,
    )?;
    Ok(())
}

/// Advances the injected model clock without consuming a frame or sleeping the host.
fn advance_time(scene: &mut SearchScene, line: &str) -> Result<(), String> {
    // Reject malformed time arguments before changing the diagnostic clock.
    let millis: u64 = line
        .split_whitespace()
        .nth(1)
        .ok_or("advance-ms needs milliseconds")?
        .parse()
        .map_err(|_| "invalid advance milliseconds")?;
    // One accumulated offset belongs to this Session, not to its individual providers.
    // The next explicit render observes deadlines with ordinary production scheduling.
    scene.time_offset += std::time::Duration::from_millis(millis);
    Ok(())
}

/// Routes fixture hover through the public pointer path after the preceding presentation.
fn hover_overlay(scene: &mut SearchScene, name: &str, now: Instant) -> Result<(), String> {
    // Keep the fixture choice separate from input geometry and region ownership.
    if name == "overlay-copy-block" {
        return puzzle_term::test_support::overlay_scene::blocks::copy(&mut scene.session, now);
    }
    if name == "overlay-hover-scrollbar" {
        puzzle_term::test_support::overlay_scene::models::hover(&mut scene.session, now);
    } else if name == "overlay-hover-block" {
        // Command identity is carried by the current presented output-row region.
        puzzle_term::test_support::overlay_scene::blocks::hover(&mut scene.session, now);
    } else {
        // This source resolves OSC groups and URL detection against presented row versions.
        puzzle_term::test_support::overlay_scene::links::hover(&mut scene.session, now);
    }
    Ok(())
}
