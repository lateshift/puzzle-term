//! Native C input becomes the same owner messages as the winit harness.
//!
//! Numeric enum/boolean inputs are validated before constructing Rust types.
//! UTF-8 buffers are copied during the call; no host pointer enters a channel.
//! Only Session decides query focus, keybindings, mouse capture and encoding.
//!
//! Physical identity and layout text are kept distinct. W3C codes permit the
//! native adapter's base-key and keypad rules; committed text preserves non-US
//! layouts, dead keys and IME output. Modifiers precede their key on the same
//! producer channel so a tick cannot reorder them with that key.
//! The wrapper does not encode CSI or choose which query field owns keyboard
//! focus. Session's current search/cheat-sheet mode and keybindings do that.
//!
//! A preedit caret is a UTF-8 byte boundary, unlike AppKit's UTF-16 selection.
//! Hosts convert units before calling; an interior byte is rejected rather than
//! producing an invalid substring. SIZE_MAX represents an unknown caret without
//! making a real offset compete with a negative C sentinel.
//! Preedit updates never write application bytes; only committed text does.
//!
//! Pointer position stays signed for selection outside the viewport. Terminal
//! mouse coordinates must be unsigned, so that projection alone clamps at zero.
//! View-local logical points are resolved against presented geometry by Puzzle.
//! Motion takes the coalescing path; press/release are never delayed for a frame.
//! Leave has no terminal mouse report and clears only overlay hover state.
//! Wheel samples carry a location for application mouse reporting and preserve
//! row versus precise-pixel magnitude for the viewport's existing scroll model.
//! Mouse modifiers exclude native per-side Option bookkeeping; key modifiers
//! include it because layout policy needs the side that produced committed text.
use crate::{
    abi::{self, Bytes, ResultCode},
    memory, registry,
};
use puzzle_embed::{RawInput, RawKey, RawLogical, SurfacePointer};
use puzzle_term::{
    messages::PreeditState,
    vt::input::{Modifiers, MouseEvent, MouseKind},
};

/// C booleans reject accidental sentinel values rather than silently coercing them.
pub(crate) fn boolean(value: u32) -> Result<bool, ResultCode> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ResultCode::InvalidArgument),
    }
}
/// Translates the host's layout identity without interpreting terminal modes.
fn logical(kind: u32, value: String) -> Result<RawLogical, ResultCode> {
    // Dead keys are provisional layout state; only a later commit emits text.
    // Unidentified keys retain their physical identity for configured bindings.
    match kind {
        0 if !value.is_empty() => Ok(RawLogical::Character(value)),
        1 if !value.is_empty() => Ok(RawLogical::Named(value)),
        2 => Ok(RawLogical::Dead),
        3 => Ok(RawLogical::Unidentified),
        _ => Err(ResultCode::InvalidArgument),
    }
}
/// Sends one physical/layout key through the Session-owned native adapter.
#[doc = "# Safety"]
/// Input and every embedded Bytes range must be live readable memory during this call.
///
/// Layout and physical identity remain distinct until the native core adapter runs.
/// Unknown location or modifier bits reject the whole key before either message is sent.
#[allow(unsafe_code, reason = "D-43: C native key structure and strings")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_key(id: u64, input: *const abi::Key) -> ResultCode {
    let timing = puzzle_embed::InputTiming::capture_api();
    // SAFETY: the caller's Key and its string buffers remain readable for this call.
    unsafe { key_impl(id, input, timing) }
}

/// Shares key marshalling between ABI-v1 and the optional clock-pair variant.
#[doc = "# Safety"]
/// `input` and its Bytes fields obey the public key input contract.
#[allow(unsafe_code, reason = "D-43: shared checked C key marshalling")]
pub(crate) unsafe fn key_impl(
    id: u64,
    input: *const abi::Key,
    timing: Option<puzzle_embed::InputTiming>,
) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let surface = &registry.surface(id)?.surface;
        // SAFETY: caller supplies one initialized Key; read checks pointer shape first.
        let key = unsafe { memory::read(input)? };
        if key.modifiers > 63 {
            return Err(ResultCode::InvalidArgument);
        }
        // SAFETY: each layout string is a readable call-scoped buffer copied before sending.
        let name = unsafe { memory::text(key.logical)? };
        // SAFETY: base string remains live until this synchronous copy completes.
        let base = unsafe { memory::text(key.base)? };
        // SAFETY: physical W3C code is copied, never borrowed by Session.
        let physical = unsafe { memory::text(key.physical)? };
        // SAFETY: committed text range follows the same caller-owned buffer contract.
        let text = unsafe { memory::text(key.text)? };
        let location = match key.location {
            0 => "Standard",
            1 => "Left",
            2 => "Right",
            3 => "Numpad",
            _ => return Err(ResultCode::InvalidArgument),
        };
        let event = RawKey {
            logical: logical(key.kind, name)?,
            base: if base.is_empty() {
                RawLogical::Unidentified
            } else {
                RawLogical::Character(base)
            },
            physical,
            text: (!text.is_empty()).then_some(text),
            location: location.into(),
            pressed: boolean(key.pressed)?,
            repeat: boolean(key.repeat)?,
        };
        // Modifier and key messages share one producer FIFO; no tick can interleave them.
        surface
            .input(RawInput::Modifiers(key.modifiers as u8))
            .map_err(ResultCode::from)?;
        match timing {
            Some(timing) => surface.input_timed(RawInput::Key(event), timing),
            None => surface.input(RawInput::Key(event)),
        }
        .map_err(ResultCode::from)
    })())
}

/// Updates provisional text with a UTF-8 byte caret; SIZE_MAX means unknown.
#[doc = "# Safety"]
/// Text must satisfy the borrowed Bytes contract.
///
/// The byte caret must lie on a UTF-8 boundary; SIZE_MAX means absent.
/// Composition remains provisional and cannot write to the transport.
#[allow(unsafe_code, reason = "R8/D-43: provisional text marshalling")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_preedit(id: u64, text: Bytes, cursor: usize) -> ResultCode {
    // SAFETY: the input range is copied and UTF-8 checked before it can be queued.
    let text = match unsafe { memory::text(text) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    let cursor = (cursor != usize::MAX).then_some(cursor);
    if cursor.is_some_and(|cursor| !text.is_char_boundary(cursor)) {
        return ResultCode::InvalidArgument;
    }
    registry::with_surface(id, |surface| surface.preedit(PreeditState { text, cursor }))
}

/// Copies view-local logical points; only Puzzle resolves rendered cell geometry.
fn pointer(input: abi::Pointer) -> Result<SurfacePointer, ResultCode> {
    if !input.x.is_finite()
        || !input.y.is_finite()
        || input.button > u32::from(u16::MAX)
        || input.clicks > u32::from(u8::MAX)
        || input.modifiers > 15
    {
        return Err(ResultCode::InvalidArgument);
    }
    let kind = match input.kind {
        0 => MouseKind::Move,
        1 => MouseKind::Press,
        2 => MouseKind::Release,
        _ => return Err(ResultCode::InvalidArgument),
    };
    // Cell and pixel fields are deliberately unset: the Session owner resolves them
    // with the same padded, scaled geometry as the last presented frame.
    Ok(SurfacePointer {
        event: MouseEvent {
            kind,
            button: input.button as u16,
            column: 0,
            row: 0,
            pixel_x: 0,
            pixel_y: 0,
            modifiers: Modifiers(input.modifiers as u8),
            button_held: boolean(input.held)?,
            click_count: input.clicks as u8,
        },
        position: [input.x, input.y],
    })
}
/// Routes a pointer against the last presented regions before terminal mouse policy.
#[doc = "# Safety"]
/// Input must name one initialized Pointer structure for this call.
///
/// Pointer routing starts with the last presented overlay region table.
/// Motion coalesces while presses and releases enter the owner immediately.
#[allow(unsafe_code, reason = "D-43: C pointer event copy")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_pointer(id: u64, input: *const abi::Pointer) -> ResultCode {
    // SAFETY: caller's Pointer remains initialized/readable until this copy returns.
    let input = match unsafe { memory::read(input) } {
        Ok(input) => input,
        Err(error) => return error,
    };
    if input.kind == 3 {
        return registry::with_surface(id, |surface| {
            surface.overlay_pointer(puzzle_term::overlay::provider::PointerEvent {
                kind: puzzle_term::overlay::provider::PointerKind::Leave,
                point: [0.0; 2],
                button: None,
                modifiers: 0,
            })
        });
    }
    let input = match pointer(input) {
        Ok(input) => input,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| {
        if input.event.kind == MouseKind::Move {
            surface.pointer_motion(input)
        } else {
            surface.pointer(input)
        }
    })
}
/// Scroll unit 0 is rows, 1 device pixels; OS-inertial samples preserve their magnitude.
#[doc = "# Safety"]
/// Pointer must be an initialized location sample; kind is ignored for wheel routing.
///
/// Rows and device pixels retain distinct units for application and viewport routing.
/// This adapter does not synthesize inertia or divide precise deltas into key events.
#[allow(unsafe_code, reason = "D-43: scroll's C pointer-location sample")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_scroll(
    id: u64,
    delta: f64,
    unit: u32,
    input: *const abi::Pointer,
) -> ResultCode {
    if !delta.is_finite() {
        return ResultCode::InvalidArgument;
    }
    let delta = match unit {
        0 => puzzle_term::viewport::ScrollInput::Rows(delta),
        1 => puzzle_term::viewport::ScrollInput::Pixels(delta),
        _ => return ResultCode::InvalidArgument,
    };
    // SAFETY: one initialized pointer sample is copied before the wheel message is queued.
    let input = match unsafe { memory::read(input) } {
        Ok(mut input) => {
            input.kind = 0;
            input
        }
        Err(error) => return error,
    };
    let input = match pointer(input) {
        Ok(input) => input,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| surface.wheel(delta, input))
}

/// Commits text through current Session input mode.
#[doc = "# Safety"]
/// Committed text is a live readable UTF-8 range during this call.
///
/// Committed text is input, so an open Session query field may consume it.
/// UTF-8 is validated before an owned string is transferred to the owner inbox.
#[allow(unsafe_code, reason = "D-43: call-scoped text input buffer")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_text(id: u64, text: Bytes) -> ResultCode {
    let timing = puzzle_embed::InputTiming::capture_api();
    // SAFETY: caller retains committed text memory through the owned UTF-8 copy.
    unsafe { crate::input_timing::text_impl(id, text, timing) }
}

/// Pastes through the existing sanitization and bracketed-paste policy.
#[doc = "# Safety"]
/// Paste bytes remain readable until the owned copy has completed.
///
/// The shared paste command strips unsafe controls and applies current bracket mode.
/// The wrapper adds no native clipboard policy or newline conversion of its own.
#[allow(unsafe_code, reason = "D-43: call-scoped paste input buffer")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_paste(id: u64, text: Bytes) -> ResultCode {
    // SAFETY: caller retains paste input memory until the validated owned copy completes.
    let text = match unsafe { memory::text(text) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| surface.paste(text))
}

/// Answers a clipboard read through its core async-result command.
#[doc = "# Safety"]
/// Clipboard result storage remains valid until this function returns.
///
/// This is the asynchronous clipboard result, routed as the existing paste command.
/// A clipboard callback must copy its borrowed host string before returning.
#[allow(
    unsafe_code,
    reason = "D-43: call-scoped clipboard_answer input buffer"
)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_clipboard_answer(id: u64, text: Bytes) -> ResultCode {
    // SAFETY: caller retains clipboard answer memory until the validated owned copy completes.
    let text = match unsafe { memory::text(text) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| surface.paste(text))
}

/// Sets a suggestion; empty clears it without sending application bytes.
#[doc = "# Safety"]
/// Suggestion text follows the borrowed Bytes allocation contract.
///
/// An empty value removes the suggestion; no transport bytes are emitted.
/// Nonempty text becomes Session model state drawn by the ordinary overlay view.
#[allow(unsafe_code, reason = "D-43: call-scoped ghost_text input buffer")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_ghost_text(id: u64, text: Bytes) -> ResultCode {
    // SAFETY: caller retains suggestion memory until the validated owned copy completes.
    let text = match unsafe { memory::text(text) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| {
        surface.set_ghost_text((!text.is_empty()).then_some(text))
    })
}
