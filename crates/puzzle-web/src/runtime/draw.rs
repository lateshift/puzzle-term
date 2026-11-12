//! Presentation is confined to rAF and obeys the shared D-33 settle rule.
//!
//! Surface dimensions are device pixels; the DOM container owns CSS size.
//! The texture owner is shared with its CPU text context so uploads drained
//! by one terminal are already available to every other terminal's canvas.
//!
//! A rendering request is damage, not permission to snapshot immediately.
//! The core SettleWindow remembers the start of an isolated output burst.
//! At a refresh tick this host asks that same rule whether the short update
//! has settled. Continuous output still presents once per browser refresh.
//!
//! The browser supplies refresh callbacks; no timer estimates a separate
//! desktop-style phase. Successive visible rAF ticks give a bounded refresh
//! period for the core's definition of output silence. Long idle gaps do
//! not become a refresh-period sample or delay the next isolated update.
//!
//! The snapshot and frame build happen in the callback, after all output
//! received before that callback has been parsed within its feed budget.
//! The frame remains GPU-neutral until the shared painter applies uploads
//! and executes its batches. Input never needs to wait for this lane.
//!
//! Cursor blink and synchronized-output deadlines use a timeout only when
//! the core asks for one. A quiescent terminal registers no recurring rAF.
//! Visibility suppresses rendering while budgeted output parsing continues,
//! so returning to the page shows current state instead of replaying frames.
//!
//! Resize has two clocks: the local surface follows CSS geometry at refresh,
//! while the remote PTY receives the final size after the trailing delay.
//! Grid counts and backing dimensions come from the same physical metrics;
//! a partial cell strip is cleared with the default theme background.
//!
//! The text owner pairs CPU and GPU atlases. A second canvas never receives
//! an isolated painter with only that Session's latest dirty glyph spans.
//! This preserves already uploaded glyph addresses across several terminals.
//! The parser may consume a queued suffix when publish_frame is called.
//! The host compares its pending byte count before and after that call, then
//! returns credit for the newly parsed prefix through the provenance ledger.
//! A frame callback therefore cannot hide bytes from ACK or promise accounting.
//!
//! A core deadline represents a particular blink or sync timeout, not a
//! recurring display poll. The host arms the earliest such deadline together
//! with any trailing PTY resize; once consumed it is removed before scheduling.
//!
//! Surface acquisition can fail even though the Session and text remain valid.
//! That error belongs to the renderer/host boundary, not to parser state.
//! No output byte is discarded merely because its presentation failed.
//!
//! Row instances are assembled using the fitted device-pixel dimensions.
//! The surface clear comes from the frame's resolved theme background, so
//! padding follows light/dark appearance without a second CSS fill path.
//!
//! Changing a canvas backing dimension does not change its CSS layout.
//! The mounted DOM container is the observer target specifically to prevent
//! an intrinsic canvas resize from recursively growing the page geometry.
//!
//! Cold glyph admission shares one absolute allowance with completion publication.
//! Reusable font-call readiness is established by background work before inline use.
//! The first Canvas family draw remains off-frame because its cost is unknown.
//! Canvas calls exceeding their prediction revoke that policy's inline readiness.
//! Ordinary frame cost samples exclude measured source and publication work.
//! Ready ownership left after publication creates an explicit next-frame request.
//! A background job that has not completed cannot make the canvas poll for ink.
//! All atlas clearing precedes collection of this frame's retained row addresses.
//! Time-to-ink samples are committed only after successful surface submission.
//! Text contexts shared by several canvases retain one common publication owner.

use super::*;
use wasm_bindgen::{JsCast, closure::Closure};
/// Measured refresh slack admits completion work and records ordinary frame cost.
mod completion;
/// GPU submission retains prepared ink independently of Session publication.
mod submission;

/// Installs one reusable callback; cancellation drops it with the terminal.
pub(super) fn install(owner: &Owner) -> Result<(), JsValue> {
    let weak = Rc::downgrade(owner);
    // Only a weak reference crosses the browser callback boundary; no ownership cycle.
    let callback = Closure::wrap(Box::new(move |_: f64| {
        if let Some(owner) = weak.upgrade() {
            // Consume the outstanding request before a turn schedules its successor.
            owner.borrow_mut().raf = None;
            if let Err(error) = present(&owner) {
                tracing::warn!(event = "web.render_failed", ?error);
                events::emit(&owner, "error", error);
            }
        }
    }) as Box<dyn FnMut(f64)>);
    owner.borrow_mut().draw = Some(callback);
    let weak = Rc::downgrade(owner);
    let timer = Closure::wrap(Box::new(move || {
        if let Some(owner) = weak.upgrade() {
            {
                let mut r = owner.borrow_mut();
                // Expired IDs cannot be canceled later as if they still represented a wake.
                r.timer_id = None;
                let now = Instant::now();
                if r.deadline.is_some_and(|at| at <= now) {
                    r.deadline = None;
                    r.damage = true;
                }
                if r.pty_due.is_some_and(|at| at <= now) {
                    // A single trailing notification uses the last coalesced observer size.
                    r.pty_due = None;
                    let size = r.pixels;
                    if let Err(error) = r.session.resize_pty(size) {
                        tracing::warn!(event="web.resize_failed", %error);
                    }
                }
            }
            host::turn(&owner);
        }
    }) as Box<dyn FnMut()>);
    owner.borrow_mut().timer = Some(timer);
    Ok(())
}

/// Scheduling does not acquire a surface or snapshot terminal rows.
pub(super) fn schedule(owner: &Owner) -> Result<(), JsValue> {
    // A disposed or hidden owner registers neither paint work nor blink deadlines.
    let mut runtime = owner.borrow_mut();
    if runtime.disposed || !runtime.visible || runtime.draw.is_none() {
        return Ok(());
    }
    let requests = runtime.take_wake_requests();
    // Coalesce core requests without taking a snapshot before paint.
    runtime.damage |= requests.frame;
    if let Some(at) = requests.deadline {
        runtime.deadline = Some(runtime.deadline.map_or(at, |old| old.min(at)));
    }
    // Blink and delayed PTY size share the earliest required wake, not a poll.
    let deadline = runtime.deadline.into_iter().chain(runtime.pty_due).min();
    if let Some(id) = runtime.timer_id.take() {
        window()?.clear_timeout_with_handle(id);
    }
    if let (Some(at), Some(callback)) = (deadline, runtime.timer.as_ref()) {
        let delay = at
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(i32::MAX as u128) as i32;
        runtime.timer_id = Some(
            window()?.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                delay.max(1),
            )?,
        );
    }
    // Multiple parser slices share one paint request; no idle rAF loop is installed.
    if runtime.raf.is_none() && (runtime.damage || runtime.resize.is_some()) {
        let callback = runtime
            .draw
            .as_ref()
            .ok_or_else(|| value::error("Disposed", "draw callback absent"))?;
        runtime.raf = Some(window()?.request_animation_frame(callback.as_ref().unchecked_ref())?);
    }
    Ok(())
}

/// Fit uses actual container pixels; one leftover fraction remains background.
/// The requested device limit bounds both axes before surface configuration.
/// Device limits can be lower than an adapter advertises, so reading the
/// adapter alone could still request a texture the renderer cannot create.
/// Scheduling only retains the size here; the next rAF performs the resize.
pub(crate) fn fit(owner: &Owner) -> Result<(), JsValue> {
    let mut runtime = owner.borrow_mut();
    runtime.ensure_live()?;
    let parent = runtime
        .parent
        .as_ref()
        .ok_or_else(|| value::error("NotRendered", "no host element"))?;
    // Surface dimensions must fit the requested device, whose limit may differ
    // from the WebGPU default on another adapter or future browser.
    let text = runtime
        .text
        .as_ref()
        .ok_or_else(|| value::error("NotRendered", "text missing"))?;
    let renderer = text.gpu.renderer.borrow();
    let limit = f64::from(
        renderer
            .as_ref()
            .ok_or_else(|| value::error("WebGpuUnavailable", "device missing"))?
            .device()
            .limits()
            .max_texture_dimension_2d,
    );
    drop(renderer);
    // Round once in device pixels; CSS dimensions never resample a rendered bitmap.
    let rect = parent.get_bounding_client_rect();
    runtime.resize = Some([
        (rect.width() * runtime.dpr).round().clamp(1.0, limit) as u32,
        (rect.height() * runtime.dpr).round().clamp(1.0, limit) as u32,
    ]);
    drop(runtime);
    schedule(owner)
}

/// The only production draw path executes at the browser's paint deadline.
pub(super) fn present(owner: &Owner) -> Result<(), JsValue> {
    // A disposed or hidden owner registers neither paint work nor blink deadlines.
    let mut runtime = owner.borrow_mut();
    if runtime.disposed || !runtime.visible {
        return Ok(());
    }
    // Idle gaps do not describe the display cadence; only refresh-sized gaps do.
    let now = Instant::now();
    if let Some(last) = runtime.tick.replace(now) {
        let gap = now.saturating_duration_since(last);
        if gap >= std::time::Duration::from_millis(4) && gap <= std::time::Duration::from_millis(35)
        {
            runtime.period = gap;
        }
    }
    if runtime.resize.is_none()
        && runtime.settle.earliest().is_some()
        && !runtime.settle.may_present(
            runtime.started.elapsed(),
            runtime.session.scroll_animation_frame(now),
        )
    {
        drop(runtime);
        // Retain damage until a later refresh satisfies the shared settle deadline.
        return schedule(owner);
    }
    // Resource initialization can precede DOM mounting; a missing context cannot paint.
    let Some(text) = runtime.text.clone() else {
        return Ok(());
    };
    let renderer = text.gpu.renderer.borrow();
    let renderer = renderer
        .as_ref()
        .ok_or_else(|| value::error("WebGpuUnavailable", "device missing"))?;
    if let Some(size) = runtime.resize.take() {
        resize(&mut runtime, &text, renderer, size)?;
    }
    // publish_frame may drain a pending suffix; account for it below
    // using the same provenance as a transport feed before returning credit.
    let before = runtime.parsed.accepted - runtime.parsed.consumed;
    let completions_ready = paint(&mut runtime, &text, renderer)?;
    let now = runtime.started.elapsed();
    // Only a successfully submitted image ends the current settle burst.
    runtime.settle.presented(now);
    ready(&runtime)?;
    runtime.damage = false;
    if completions_ready {
        // Ready owned ink requests another paint even when the application is idle.
        // Queued but incomplete jobs never start a refresh polling loop.
        runtime.session.submit(
            puzzle_term::messages::Command::GlyphRasterized,
            puzzle_term::Origin::Platform,
        );
        runtime.session.pump(Instant::now());
    }
    let pending = runtime.session.pending_bytes() as u64;
    let acks = runtime
        .parsed
        .advance(before.saturating_sub(pending) as usize)
        .map_err(|e| value::error("TransportError", e))?;
    // Foreign callbacks can reenter, so no Session borrow spans their flush.
    drop(runtime);
    socket::ack(owner, acks)?;
    output::flush(owner)?;
    output::complete(owner);
    schedule(owner)
}

/// A readiness marker follows submission rather than capability discovery.
fn ready(runtime: &Runtime) -> Result<(), JsValue> {
    // Readiness means an actual submitted frame, never just successful device creation.
    if let Some(dom) = runtime.dom.as_ref()
        && dom.canvas().get_attribute("data-puzzle-state").as_deref() != Some("ready")
    {
        dom.canvas().set_attribute("data-puzzle-state", "ready")?;
        tracing::info!(
            event = "startup.first_frame",
            "puzzle-web: first frame presented"
        );
    }
    Ok(())
}

/// Grid, backing pixels and application resize originate from the same geometry.
fn resize(
    runtime: &mut Runtime,
    text: &resources::Text,
    renderer: &puzzle_render::Renderer,
    size: [u32; 2],
) -> Result<(), JsValue> {
    let surface = runtime
        .surface
        .as_ref()
        .ok_or_else(|| value::error("NotRendered", "surface missing"))?;
    if let Some(dom) = &runtime.dom {
        dom.canvas().set_width(size[0]);
        dom.canvas().set_height(size[1]);
    }
    renderer
        .configure(surface, size[0], size[1])
        .map_err(|e| value::error("WebGpuUnavailable", &e.to_string()))?;
    // The canvas backing and VT size derive from exactly these physical pixels.
    runtime.pixels = size;
    runtime.pty_due = Some(Instant::now() + std::time::Duration::from_millis(50));
    // Font geometry is shared, while each Session keeps its independent row count.
    let context = text.context.borrow();
    let m = context
        .as_ref()
        .ok_or_else(|| value::error("FontLoadFailed", "metrics missing"))?
        .metrics();
    let padding = (runtime.settings.render_padding * runtime.dpr as f32).round() as u32;
    runtime.session.set_padding(padding);
    let grid = runtime.fixed_grid.unwrap_or([
        (size[0].saturating_sub(2 * padding) / m.width_px).max(2) as u16,
        (size[1].saturating_sub(2 * padding) / m.height_px).max(1) as u16,
    ]);
    // Local reflow follows each refresh; SIGWINCH waits for the trailing
    // deadline above, so dragging cannot flood the remote program.
    runtime
        .session
        .resize_grid(grid[0] as usize, grid[1] as usize, size)
        .map_err(|e| value::error("InvalidArgument", &e))
}

/// Uploads and paints only after the shared settle rule admits this rAF tick.
fn paint(
    runtime: &mut Runtime,
    text: &resources::Text,
    renderer: &puzzle_render::Renderer,
) -> Result<bool, JsValue> {
    let mut guard = text.context.borrow_mut();
    let context = guard
        .as_mut()
        .ok_or_else(|| value::error("FontLoadFailed", "context missing"))?;
    let allowance = completion::budget(runtime);
    context.set_completion_budget(allowance);
    let started = Instant::now();
    let now = Instant::now();
    if runtime
        .build_frame(now, context)
        .map_err(|e| value::error("WebGpuUnavailable", &e))?
        .is_some()
    {
        submission::submit(runtime, text, renderer, context.glyph_budget_bytes())?;
        #[cfg(test)]
        // Capture the actual submitted canvas within its drawing-buffer lifetime.
        // This observer never creates a frame, snapshot or damage of its own.
        if let Some(dom) = runtime.dom.as_ref() {
            runtime.measurements.capture_canvas(dom.canvas())?;
        }
        #[cfg(any(test, feature = "measurements"))]
        // Count successful submissions, not refresh callbacks or attempted draws.
        runtime.measurements.presented(runtime.started.elapsed());
        #[cfg(any(test, feature = "measurements"))]
        completion::observe(runtime, context, started, allowance);
        completion::record(
            runtime,
            started
                .elapsed()
                .saturating_sub(context.completion_time())
                .saturating_sub(context.source_preparation_time()),
        );
    }
    Ok(context.source_completions_ready())
}
