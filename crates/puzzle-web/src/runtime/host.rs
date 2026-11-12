//! DOM input drives the same typed Session policy as programmatic input.
//!
//! Overlay regions claim pointer samples before the browser host submits selection input.
//! Both native and web use FrameBuilder's shared platform-neutral region router.
//! The web runs both owners on one thread with the same sealed visible BufferView.
//! An offscreen row consequently never becomes readable from a provider callback.
//! Hover transitions apply the winning region cursor to the canvas immediately.
//! An unclaimed sample restores the ordinary text cursor instead of stale hover.
//! Surface leave cancels a queued move before delivering the explicit Leave event.
//! This prevents the next rAF from re-entering an already abandoned region.
//! A handled press owns subsequent drag/release samples through registry capture.
//! The canvas can therefore finish a drag outside its original hit rectangle.
//! Consumed buttons never enter local selection or the application mouse encoder.
//! Input routing itself does not require a new frame or another font measurement.
//! A changed tooltip requests a dirty provider build through the shared request state.
//! The subsequent turn schedules that build with the normal display pacing rule.
//! Runtime disposal invalidates the weak owner before another callback can run.
//! Missing owners consume no terminal input and need no retained pointer state.
//! Programmatic input still follows its existing typed Session command policy.
//!
//! Weak ownership lets disposal remove all input resources without a cycle.
//! Geometry is read from the current context, never a stale startup snapshot.
//!
//! DOM input has two possible owners for a printable press: a shortcut
//! interceptor or the browser's textarea. The mapper records that distinction
//! before the Session decides whether a binding consumes the press. Native
//! text is allowed only after the same policies as encoded input have run.
//!
//! Paste is different from ordinary native text. A clipboard read requested
//! by the matched binding releases the browser's native paste event, whose
//! clipboardData supplies the actual value. No asynchronous clipboard read
//! is started speculatively before the browser grants user activation.
//!
//! IME composition crosses the bus as Preedit. Only its separate commit
//! becomes Text and reaches the application. This host therefore never
//! invents characters from keydown while the input method owns composition.
//!
//! Pointer geometry uses device pixels because applications may request
//! SGR pixel reports. CSS coordinates are multiplied by DPR once in the DOM
//! adapter; glyph and cursor origins already use the physical cell geometry.
//! The padding origin is included when positioning the candidate textarea.
//!
//! The weak host does not prolong a terminal's lifetime. A queued observer
//! callback after disposal can simply find no owner; it cannot resurrect
//! listeners or issue another frame against released GPU resources.
//!
//! Media and resize callbacks describe external facts. Appearance uses the
//! core's mode-2031 path, while geometry waits for the next refresh callback.
//! None of these event handlers acquires a surface image or paints pixels.
//! The application's active screen matters to input encoding. Kitty flags
//! belong to its per-screen stack, while mouse reporting samples DEC modes.
//! Caching either at mount time would produce incorrect reports after a
//! program enters the alternate screen or changes tracking during a drag.
//!
//! A focus callback is not proof that the application enabled focus reports.
//! The core decides whether mode 1004 permits bytes; the browser still changes
//! local cursor focus geometry even when that mode is off.
//!
//! A native printable candidate must not send its encoded bytes and later
//! send beforeinput text as well. pump_native_key marks only that candidate;
//! older pending sequence keys still flush through the normal encoder FIFO.
//! This prevents identical leader presses from being lost or duplicated.
//!
//! System appearance and user appearance are distinct facts. The host reports
//! light or dark, but the core keeps a forced theme choice and its cursor
//! precedence. It also emits the exact mode-2031 bytes after a resolved change.
//!
//! The candidate anchor follows the cursor's cell top-left, not its font ink.
//! A ligature may cover several cells without moving the IME popup origin.
//! Fallback glyph widths therefore cannot leak into pointer hit testing.
//! MIME clipboard work reaches this host through the same neutral effect type.
//! Its default unsupported implementation reports that outcome explicitly.
//! Browser MIME clipboard and paste-grant generation belong to the wasm phase.
//! The native token generator is not compiled into this host or its dependencies.
//! Existing user paste handling still runs inside the browser gesture callback.
//! No asynchronous MIME completion is converted into ordinary pasted text.
//! Text clipboard writes retain their current permission and activation semantics.
//! The web runtime stays single-threaded even though native completion uses messages.
//! Core API adaptation must preserve skipped-presentation glyph uploads.
//! Unsupported platform services must not silently report success to terminal clients.
//! The existing app event queue remains the only callback delivery seam.
//! No native clipboard object is shared through the platform-neutral Session.
use super::*;
use crate::dom::{
    DomHost, Geometry, KeyDisposition,
    keyboard::{KeyPolicy, MappedKey},
};
use puzzle_term::{Origin, messages::Command, theme::Appearance, vt::input::MouseModes};
use std::rc::Weak;

/// Input listeners do not keep a disposed terminal alive.
pub(super) struct Host(pub(super) Weak<RefCell<Runtime>>);
/// DOM ownership decisions precede native text; no input callback draws.
impl DomHost for Host {
    /// Samples modes afresh because an application can switch screens.
    fn key_policy(&self) -> KeyPolicy {
        self.0.upgrade().map_or(
            KeyPolicy {
                platform: crate::dom::platform(),
                option_as_alt: Default::default(),
                kitty_flags: 0,
                locked: false,
            },
            |o| {
                let runtime = o.borrow();
                KeyPolicy {
                    platform: crate::dom::platform(),
                    option_as_alt: runtime.settings.option_as_alt(),
                    kitty_flags: puzzle_term::vt::input::InputModes::from_terminal(
                        runtime.session.terminal(),
                    )
                    .kitty_flags,
                    locked: false,
                }
            },
        )
    }
    /// The ordinary binding interceptor runs before the textarea owns text.
    fn key(&mut self, mapped: MappedKey) -> KeyDisposition {
        let Some(owner) = self.0.upgrade() else {
            return KeyDisposition::Consumed;
        };
        // A policy may claim printable text; ownership is decided before native insertion.
        let native = {
            let mut runtime = owner.borrow_mut();
            let message = if mapped.native_text {
                puzzle_term::SessionMessage::NativeKey(mapped.event, Origin::User)
            } else {
                puzzle_term::SessionMessage::Command(Command::Key(mapped.event), Origin::User)
            };
            if let Err(error) = runtime.apply(message) {
                tracing::warn!(event = "web.input_failed", %error);
            }
            mapped.native_text && runtime.results.native_text
        };
        // Paste releases the browser gesture instead of prompting for clipboard permission.
        let clipboard = owner.borrow_mut().take_clipboard_effect();
        // Flush bytes immediately; scheduling affects only presentation of the echo.
        turn(&owner);
        match clipboard {
            // Browser MIME support waits for the wasm phase; its service rejects requests.
            Some(puzzle_term::platform::ClipboardEffect::Mime(_)) => {}
            Some(puzzle_term::platform::ClipboardEffect::Read) => return KeyDisposition::Paste,
            Some(puzzle_term::platform::ClipboardEffect::Write(text)) => {
                return KeyDisposition::Copy(text);
            }
            Some(puzzle_term::platform::ClipboardEffect::WriteHtml { html, text }) => {
                return KeyDisposition::CopyHtml { html, text };
            }
            None => {}
        }
        if native {
            KeyDisposition::Text
        } else {
            KeyDisposition::Consumed
        }
    }
    /// Committed text and composition cross typed bus policy exactly once.
    fn command(&mut self, command: Command) {
        if let Some(owner) = self.0.upgrade() {
            // Focus facts follow the core report-mode decision and are delivered once.
            let focus = match &command {
                Command::Focus(focus) => Some(*focus),
                _ => None,
            };
            {
                let mut runtime = owner.borrow_mut();
                if let Err(error) =
                    runtime.apply(puzzle_term::SessionMessage::Command(command, Origin::User))
                {
                    tracing::warn!(event = "web.input_failed", %error);
                }
            }
            turn(&owner);
            if let Some(focus) = focus {
                events::emit(
                    &owner,
                    if focus { "focus" } else { "blur" },
                    JsValue::UNDEFINED,
                );
            }
        }
    }
    /// Shared region routing precedes terminal gestures and applies the CSS cursor.
    fn pointer(&mut self, input: puzzle_term::pointer::PointerInput) -> bool {
        let Some(owner) = self.0.upgrade() else {
            return false;
        };
        let handled = {
            let mut runtime = owner.borrow_mut();
            // Session alone routes overlays and encodes an unclaimed sample.
            if let Err(error) = runtime.apply(puzzle_term::SessionMessage::Command(
                Command::Pointer(input),
                Origin::User,
            )) {
                tracing::warn!(event = "web.input_failed", %error);
            }
            runtime.results.pointer_handled
        };
        turn(&owner);
        handled
    }
    /// Ends a canvas hover through the same neutral router without sending PTY input.
    fn pointer_left(&mut self) {
        if let Some(owner) = self.0.upgrade() {
            {
                let mut runtime = owner.borrow_mut();
                if let Err(error) = runtime.apply(puzzle_term::SessionMessage::OverlayPointer(
                    puzzle_term::overlay::provider::PointerEvent {
                        kind: puzzle_term::overlay::provider::PointerKind::Leave,
                        point: [0.0; 2],
                        button: None,
                        modifiers: 0,
                    },
                )) {
                    tracing::warn!(event = "web.input_failed", %error);
                }
            }
            turn(&owner);
        }
    }
    /// Sub-row application deltas are consumed because their fraction is retained.
    /// Empty history and local boundary gestures pass through to page scrolling.
    /// An exited application cannot consume a gesture merely because old modes remain.
    /// This query encodes no bytes and never requests a GPU frame.
    /// The neutral Session shares this predicate with its actual wheel handler.
    fn wheel_consumable(
        &self,
        delta: puzzle_term::viewport::ScrollInput,
        pointer: puzzle_term::vt::input::MouseEvent,
    ) -> bool {
        self.0
            .upgrade()
            .is_some_and(|o| o.borrow().session.wheel_consumable(delta, pointer))
    }
    /// Media accessibility or the explicit setting suppresses kinetic continuation.
    fn motion_reduced(&self) -> bool {
        self.0.upgrade().is_none_or(|o| {
            let r = o.borrow();
            r.reduced_motion || !r.settings.scrolling_smooth
        })
    }
    /// All numbers are device pixels until the DOM adapter divides by DPR.
    fn geometry(&self) -> Geometry {
        self.0.upgrade().map_or(
            Geometry {
                dpr: 1.0,
                padding: [16.0; 2],
                cell: [8.0, 16.0],
                grid: [80, 24],
                cursor: [16.0; 2],
            },
            |o| {
                let r = o.borrow();
                // Hit-test bounds describe occupied cells; a leftover strip is outside the grid.
                let size = r.session.buffer().size();
                let m = r.session.cell_metrics();
                let cell = m.map_or([8.0, 16.0], |m| {
                    [f64::from(m.width_px), f64::from(m.height_px)]
                });
                let cursor = r.session.terminal().cursor;
                // Hit testing and IME anchoring use the same physical inset as drawing.
                let padding = (r.settings.render_padding * r.dpr as f32).round() as f64;
                Geometry {
                    dpr: r.dpr,
                    padding: [padding; 2],
                    cell,
                    grid: [size.0 as u32, size.1 as u32],
                    cursor: [
                        padding + cursor.column as f64 * cell[0],
                        padding + cursor.row as f64 * cell[1],
                    ],
                }
            },
        )
    }
    /// The VT's encoding/tracking mode decides pointer ownership.
    fn mouse_modes(&self) -> MouseModes {
        // The active-screen tracking mode may change between adjacent pointer events.
        self.0.upgrade().map_or_else(MouseModes::default, |o| {
            puzzle_term::vt::input::MouseModes::from_terminal(o.borrow().session.terminal())
        })
    }
    /// DOM ownership follows the same current Session policy as native hosts.
    fn shift_selects(&self) -> bool {
        self.0
            .upgrade()
            .is_none_or(|o| o.borrow().session.mouse_shift_selects())
    }
    /// Parsing continues while hidden, but rendering and blink timers stop.
    fn visibility(&mut self, visible: bool) {
        if let Some(o) = self.0.upgrade() {
            {
                let mut r = o.borrow_mut();
                // Presentation permission cannot change ownership of incoming output.
                r.visible = visible;
                // Hidden pages cancel both already scheduled lanes; simply
                // rejecting new requests would leave an old blink timer alive.
                if !visible {
                    if let Some(id) = r.raf.take() {
                        let _ = window().and_then(|w| w.cancel_animation_frame(id));
                    }
                    if let Some(id) = r.timer_id.take()
                        && let Ok(w) = window()
                    {
                        w.clear_timeout_with_handle(id);
                    }
                } else {
                    r.damage = true;
                    let size = r.pixels;
                    if let Err(error) = r.apply(puzzle_term::SessionMessage::SurfaceSize(size)) {
                        tracing::warn!(event = "web.resize_failed", %error);
                    }
                }
            }
            // Showing a hidden page schedules current state rather than replaying old paints.
            turn(&o);
        }
    }
    /// OS appearance reaches the existing mode-2031 notification path.
    fn appearance(&mut self, appearance: Appearance) {
        if let Some(owner) = self.0.upgrade() {
            {
                let mut r = owner.borrow_mut();
                // A browser preference is a trusted platform fact, never API origin.
                if let Err(error) = r.apply(puzzle_term::SessionMessage::Command(
                    Command::SetSystemAppearance(appearance),
                    Origin::Platform,
                )) {
                    tracing::warn!(event = "web.appearance_failed", %error);
                }
            }
            turn(&owner);
        }
    }
    /// Reduced motion shares the core setting used by native fades.
    fn reduced_motion(&mut self, enabled: bool) {
        if let Some(o) = self.0.upgrade() {
            // User layers survive toggling the media preference.
            let mut r = o.borrow_mut();
            r.reduced_motion = enabled;
            if let Err(error) = r.apply(puzzle_term::SessionMessage::ReducedMotion(enabled)) {
                tracing::warn!(event = "web.motion_failed", %error);
            }
            settings::apply(&mut r);
        }
    }
    /// CSS system colors are sampled only when the platform preference changes.
    fn forced_colors(&mut self, enabled: bool) {
        if let Some(owner) = self.0.upgrade() {
            if let Err(error) = super::media::forced_colors(&owner, enabled) {
                events::emit(&owner, "error", error);
            }
            turn(&owner);
        }
    }
    /// ResizeObserver facts coalesce at rAF; they never present here.
    fn resize(&mut self, pixels: [u32; 2], dpr: f64) {
        if let Some(o) = self.0.upgrade() {
            let mut r = o.borrow_mut();
            r.resize = Some([pixels[0].clamp(1, 8192), pixels[1].clamp(1, 8192)]);
            // A new scale selects another ppem context without changing other terminals.
            let changed = r.dpr != dpr;
            r.dpr = dpr;
            drop(r);
            if changed {
                settings::select_text(&o);
            }
            // Showing a hidden page schedules current state rather than replaying old paints.
            turn(&o);
        }
    }
}

/// Foreign callbacks run after the event's synchronous Session borrow ends.
pub(super) fn turn(owner: &Owner) {
    loop {
        // End the borrow before window.open can synchronously re-enter page code.
        let uri = owner.borrow_mut().take_open_link();
        let Some(uri) = uri else {
            break;
        };
        if events::allow_link(owner, &uri)
            && let Ok(window) = window()
        {
            let _ =
                window.open_with_url_and_target_and_features(&uri, "_blank", "noopener,noreferrer");
        }
    }
    settings::actions(owner);
    if let Err(error) = output::flush(owner) {
        tracing::warn!(event = "web.transport_failed", ?error);
    }
    if let Err(error) = draw::schedule(owner) {
        tracing::warn!(event = "web.frame_schedule_failed", ?error);
    }
}
