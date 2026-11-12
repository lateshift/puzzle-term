//! ABI-v1-compatible clock-pair variants share the existing input marshalling.
//!
//! Clock pairs are host observations, never terminal capabilities. Validation
//! precedes input delivery; no C buffer or callback crosses to the Session owner.
//! Timed calls preserve asynchronous tick delivery and existing input policy.
use crate::{
    abi::{self, Bytes, ResultCode},
    memory, registry,
};
use puzzle_embed::InputTiming;

/// Sends a key with its OS event/current host clock readings, in seconds.
/// Capture precedes registry access and buffer copies, so C marshalling remains
/// API work. This adds no field to Key and keeps the original ABI-v1 layout.
/// Every borrowed string is still copied under the existing bounded memory rules.
#[doc = "# Safety"]
/// The Key and every Bytes field must remain readable through this call.
#[allow(unsafe_code, reason = "D-43: delegates to the checked key decoder")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_key_timed(
    id: u64,
    key: *const abi::Key,
    event_seconds: f64,
    clock_seconds: f64,
) -> ResultCode {
    let timing = match InputTiming::new(event_seconds, clock_seconds) {
        Ok(value) => value,
        Err(error) => return error.into(),
    };
    // SAFETY: the timed variant has the same borrowed Key contract as ABI v1.
    unsafe { crate::input::key_impl(id, key, Some(timing)) }
}

/// Commits text with its OS event/current host clock readings, in seconds.
/// Reversed/nonfinite pairs reject before any text can reach the Session.
/// The shared decoder still owns UTF-8 validation and call-scoped copying.
#[doc = "# Safety"]
/// The UTF-8 byte range remains readable until this call has copied it.
#[allow(unsafe_code, reason = "D-43: delegates to shared text marshalling")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_text_timed(
    id: u64,
    text: Bytes,
    event_seconds: f64,
    clock_seconds: f64,
) -> ResultCode {
    let timing = match InputTiming::new(event_seconds, clock_seconds) {
        Ok(value) => value,
        Err(error) => return error.into(),
    };
    // SAFETY: committed text uses the unchanged call-scoped UTF-8 buffer contract.
    unsafe { text_impl(id, text, Some(timing)) }
}

/// Decode once for timed and untimed text; no foreign buffer enters the inbox.
/// Only timestamp presence chooses a method: both use the same Session policy.
#[doc = "# Safety"]
/// `text` obeys the public Bytes input contract for this synchronous call.
#[allow(unsafe_code, reason = "D-43: shared checked text input copy")]
pub(crate) unsafe fn text_impl(id: u64, text: Bytes, timing: Option<InputTiming>) -> ResultCode {
    // SAFETY: caller retains this readable range until the owned copy completes.
    let text = match unsafe { memory::text(text) } {
        Ok(text) => text,
        Err(error) => return error,
    };
    registry::with_surface(id, |surface| match timing {
        Some(timing) => surface.text_timed(text, timing),
        None => surface.text(text),
    })
}
