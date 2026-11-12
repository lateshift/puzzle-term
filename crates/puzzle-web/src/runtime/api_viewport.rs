//! Viewport mutations are thin API-origin commands over the shared Rust model.
//! IDs are exact JavaScript integers; missing retained lines are harmless no-ops.
//!
//! Public IDs describe retained document rows, not positions in the current
//! grid. Their values can outlive retention, so validation proves numeric
//! identity while Session decides whether the line still exists at dispatch.
//! Navigation to an evicted line never substitutes another row by index.
//!
//! Relative counts use down-positive document direction, while scroll_px
//! measures distance above the bottom. The viewport owns that sign conversion
//! and the active device-cell height; JS never rounds counts to screen pixels.
//! Page navigation uses rows−1 to keep one line visible for orientation.
//! Very large finite counts clamp at the document boundary instead of wrapping.
//!
//! Centre and top alignment are explicit Rust targets, allowing recordings,
//! native callers and browser callers to replay the identical operation.
//! Prompt navigation uses the retained shell marker model, not a JS scan.
//! A signed count of zero is valid and leaves the document position unchanged.
//!
//! Commands are submitted with API origin and drained before this method
//! returns. Policy therefore sees programmatic selection and navigation just
//! as it sees user commands, without a direct viewport mutation bypass.
//! Host callbacks run after the mutable borrow ends and may safely dispose
//! or issue another request. GPU presentation remains owned by the next rAF.
//! No API call paints immediately: the host schedules the existing rAF lane.
use super::{api::PuzzleTerminal, *};
use puzzle_term::{Origin, messages::Command, viewport::ScrollTarget, vt::tracked::LineId};

/// Runs coarse public mutations through interception, releasing state before callbacks.
pub(super) fn command(owner: &Owner, command: Command) -> Result<(), JsValue> {
    {
        let mut r = owner.borrow_mut();
        r.ensure_live()?;
        r.session.submit(command, Origin::Api);
        r.session.pump(Instant::now());
    }
    host::turn(owner);
    Ok(())
}
/// Rejects implicit coercion, nonfinite distances and IDs that JS cannot name exactly.
pub(super) fn number(v: &JsValue, integer: bool, maximum: f64) -> Result<f64, JsValue> {
    v.as_f64()
        .filter(|n| n.is_finite() && (!integer || n.fract() == 0.0) && n.abs() <= maximum)
        .ok_or_else(|| value::error("InvalidArgument", "invalid numeric argument"))
}
/// Line and zone identities share the public nonnegative exact-integer domain.
pub(super) fn id(v: &JsValue) -> Result<u64, JsValue> {
    let n = number(v, true, 9_007_199_254_740_991.0)?;
    if n < 0.0 {
        return Err(value::error(
            "InvalidArgument",
            "identity must be nonnegative",
        ));
    }
    Ok(n as u64)
}
/// All six viewport entry points preserve the core's clamping and following policy.
#[wasm_bindgen]
impl PuzzleTerminal {
    /// Moves by signed rows; negative reveals older retained output.
    /// Fractional row counts remain meaningful in the physical-pixel viewport.
    #[wasm_bindgen(js_name = scrollLines)]
    pub fn scroll_lines(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] n: JsValue,
    ) -> Result<(), JsValue> {
        command(&self.owner, Command::Scroll(number(&n, false, f64::MAX)?))
    }
    /// Moves by pages of rows−1, preserving one row of visual context.
    #[wasm_bindgen(js_name = scrollPages)]
    pub fn scroll_pages(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] n: JsValue,
    ) -> Result<(), JsValue> {
        let pages = number(&n, false, f64::MAX)?;
        let rows = self.rows()?.saturating_sub(1);
        command(
            &self.owner,
            Command::Scroll((pages * rows as f64).clamp(-f64::MAX, f64::MAX)),
        )
    }
    /// Shows the oldest retained history without adding blank document rows.
    #[wasm_bindgen(js_name = scrollToTop)]
    pub fn scroll_to_top(&self) -> Result<(), JsValue> {
        command(&self.owner, Command::ScrollTo(ScrollTarget::Top))
    }
    /// Resumes following live output and clears the model's new-output state.
    #[wasm_bindgen(js_name = scrollToBottom)]
    pub fn scroll_to_bottom(&self) -> Result<(), JsValue> {
        command(&self.owner, Command::ScrollTo(ScrollTarget::Bottom))
    }
    /// Centres a retained identity by default; top alignment clamps at the tail.
    /// Evicted identities leave the existing viewport unchanged.
    #[wasm_bindgen(js_name = scrollToLine)]
    pub fn scroll_to_line(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] line: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "'top' | 'center'")] align: Option<String>,
    ) -> Result<(), JsValue> {
        let line = LineId(id(&line)?);
        let target = match align.as_deref().unwrap_or("center") {
            "center" => ScrollTarget::Line(line),
            "top" => ScrollTarget::LineTop(line),
            _ => return Err(value::error("InvalidArgument", "unknown line alignment")),
        };
        command(&self.owner, Command::ScrollTo(target))
    }
    /// Jumps to earlier/later OSC 133 prompts in document order.
    /// Delta must be an exact signed i32, matching keyboard prompt navigation.
    #[wasm_bindgen(js_name = scrollToPrompt)]
    pub fn scroll_to_prompt(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] delta: JsValue,
    ) -> Result<(), JsValue> {
        let n = number(&delta, true, -f64::from(i32::MIN))?;
        if n > f64::from(i32::MAX) {
            return Err(value::error("InvalidArgument", "prompt delta exceeds i32"));
        }
        command(
            &self.owner,
            Command::ScrollTo(ScrollTarget::Prompt(n as i32)),
        )
    }
}
