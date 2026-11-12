//! JavaScript arguments are validated before constructing neutral input (R8).
//! Optional fields distinguish absent values from malformed supplied values;
//! silently defaulting a bad modifier would send a different command to a shell.
use super::value;
use wasm_bindgen::JsValue;

/// An absent optional string is not the same as an explicitly invalid type.
/// Null is permitted only where the public contract denotes an unknown value.
/// The caller still decides whether the particular string is mandatory.
pub(super) fn optional_string(object: &JsValue, name: &str) -> Result<Option<String>, JsValue> {
    let field = value::get(object, name)?;
    // Null represents an unknown exit signal; undefined represents an omitted option.
    if field.is_undefined() || field.is_null() {
        return Ok(None);
    }
    field
        .as_string()
        .map(Some)
        .ok_or_else(|| value::error("InvalidArgument", &format!("{name} must be a string")))
}

/// A wrong modifier type cannot quietly turn a modified key into plain text.
/// Native DOM modifier flags are boolean too, so programmatic input follows
/// the same identity without JavaScript truthiness changing a chord.
fn modifier(object: &JsValue, name: &str) -> Result<bool, JsValue> {
    let field = value::get(object, name)?;
    // Only omitted modifiers use the default; null and numeric coercions are rejected.
    if field.is_undefined() {
        return Ok(false);
    }
    field
        .as_bool()
        .ok_or_else(|| value::error("InvalidArgument", "modifiers must be booleans"))
}

/// Creates the DOM-shaped identity used by the existing layout/kitty mapper.
pub(super) fn key_event(spec: &JsValue) -> Result<web_sys::KeyboardEvent, JsValue> {
    // Creating an event is only an identity conversion; it is never dispatched into DOM.
    let init = web_sys::KeyboardEventInit::new();
    // W3C key identity is mandatory; physical identity is optional for synthetic input.
    let key = optional_string(spec, "key")?
        .filter(|s| !s.is_empty())
        .ok_or_else(|| value::error("InvalidArgument", "key must be a W3C key string"))?;
    // Produced text stays distinct from the physical code under non-US layouts.
    init.set_key(&key);
    if let Some(code) = optional_string(spec, "code")? {
        init.set_code(&code);
    }
    // Every modifier is independently checked so mixed chords remain representable.
    init.set_ctrl_key(modifier(spec, "ctrl")?);
    init.set_alt_key(modifier(spec, "alt")?);
    init.set_shift_key(modifier(spec, "shift")?);
    init.set_meta_key(modifier(spec, "meta")?);
    // Releases and repeats remain typed key events; the current kitty flags decide reporting.
    let kind = optional_string(spec, "type")?.unwrap_or_else(|| "press".into());
    // Repeat identity matters only when the application requested kitty event types.
    init.set_repeat(kind == "repeat");
    let event = match kind.as_str() {
        "press" | "repeat" => "keydown",
        "release" => "keyup",
        _ => return Err(value::error("InvalidArgument", "unknown key event type")),
    };
    // Browser construction can fail; the exception returns to the calling API.
    web_sys::KeyboardEvent::new_with_keyboard_event_init_dict(event, &init)
}

/// Callback transport methods are checked before replacing a working transport.
/// Validate optional callbacks now, rather than discovering a bad resize method
/// after a remote shell and local grid have already diverged.
pub(super) fn transport(object: &JsValue) -> Result<(), JsValue> {
    for name in ["write", "resize", "close"] {
        let method = value::get(object, name)?;
        // write is required; resize and close can be absent, never arbitrary values.
        if name != "write" && method.is_undefined() {
            continue;
        }
        // No callable coercion is attempted on objects or string method names.
        if !method.is_function() {
            return Err(value::error(
                "InvalidArgument",
                "transport methods must be functions",
            ));
        }
    }
    Ok(())
}

/// Unknown exit status stays unknown; invalid supplied codes are rejected.
/// A valid unknown status must not become code zero (success), while a bad
/// numeric value must not become a plausible truncated process status.
pub(super) fn exit_code(object: &JsValue) -> Result<Option<i32>, JsValue> {
    // Property access can throw; propagate that exception without changing lifecycle state.
    // Unknown and zero status carry different exit-policy meaning in the core.
    let code = value::get(object, "code")?;
    if code.is_undefined() || code.is_null() {
        return Ok(None);
    }
    // The wire/core code is i32, so JS fractional and out-of-range numbers cannot truncate.
    // i32 bounds exactly match both the wire codec and the core lifecycle command.
    code.as_f64()
        .filter(|n| {
            n.is_finite()
                && n.fract() == 0.0
                && *n >= f64::from(i32::MIN)
                && *n <= f64::from(i32::MAX)
        })
        .map(|n| Some(n as i32))
        .ok_or_else(|| value::error("InvalidArgument", "exit code must be an i32 or null"))
}

/// Fixed-grid creation requires both dimensions before any DOM or GPU work.
pub(super) fn grid(options: &JsValue) -> Result<Option<[u16; 2]>, JsValue> {
    if options.is_undefined() || options.is_null() {
        return Ok(None);
    }
    // Getter exceptions must reject creation before any partial DOM mount.
    let cols = value::get(options, "cols")?;
    // A missing partner is rejected by the same numeric validator, not guessed.
    let rows = value::get(options, "rows")?;
    // One axis alone cannot define a fixed grid; both omitted means container fitting.
    if cols.is_undefined() && rows.is_undefined() {
        return Ok(None);
    }
    let cols = axis(&cols)?;
    let rows = axis(&rows)?;
    // Physical dimensions come from the canvas after its separate CSS fit.
    let size = puzzle_term::bridge::Size {
        cols,
        rows,
        width_px: 0,
        height_px: 0,
    };
    // The protocol's total-cell cap also bounds the local terminal allocation.
    if !size.valid() {
        return Err(value::error("InvalidArgument", "fixed grid exceeds limits"));
    }
    // This validated pair opts out of automatic observer-driven cell fitting.
    Ok(Some([size.cols, size.rows]))
}

/// Checks JS's number domain before wasm integer conversion can truncate or wrap.
/// A value like 65537 must be rejected rather than becoming one column after
/// conversion to u16. Creation and resize share this check, then separately
/// validate the product of the axes before any screen allocation or reflow.
pub(super) fn axis(v: &JsValue) -> Result<u16, JsValue> {
    // Fractional, negative and nonfinite dimensions cannot name terminal cells.
    v.as_f64()
        .filter(|n| n.is_finite() && n.fract() == 0.0 && *n >= 1.0 && *n <= 4096.0)
        .map(|n| n as u16)
        .ok_or_else(|| {
            value::error(
                "InvalidArgument",
                "grid axes must be integers from 1 to 4096",
            )
        })
}
