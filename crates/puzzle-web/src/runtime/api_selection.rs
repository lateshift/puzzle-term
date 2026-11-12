//! Selection queries and mutations use Session's tracked range and extraction.
//! Facades retain their terminal owner, but never a borrow across foreign code.
//! Explicit terminal disposal remains authoritative even if a page keeps a facade.
//! Inclusive endpoints include whole cells; block mode selects physical row slices.
//!
//! The range getter returns expanded cells, rather than the raw press intent.
//! Word and logical-line expansion can move both endpoints across soft wraps.
//! Native gestures and these API requests use the same separators and registry.
//! The text getter alone allocates copied content; paint uses the small range.
//! Wide spacer cells contribute no duplicate character to the copied string.
//! Block copying retains one physical-row slice and newline per selected row.
//!
//! Public positions contain lineId and col. A LineId is a document identity,
//! not a visible row number. Scrolling cannot alter which text it identifies.
//! Reflow remaps the registered endpoints; rewrite or eviction clears them.
//! JS numbers are checked before narrowing to integer types, so negative or
//! fractional positions never become plausible unsigned grid coordinates.
//! A retained identity outside the current viewport is still a valid selection.
//! An identity gone by dispatch leaves the old selection unchanged.
//!
//! Semantic selection resolves the stable OSC command ID in Rust, then submits
//! the same CommandOutput request as native bindings. This keeps selection
//! policy independently interceptable and preserves its final hard newline.
//! Unknown command IDs never fall back to the most recent unrelated command.
//! The M10 buffer facade will expose the existing Rust-assigned zone identities.
//!
//! Mutation completes before callbacks run, with no RefCell borrow across JS.
//! A selectionChange subscriber can query text, change selection or dispose.
//! The getter and event encoder share one Position/Range shape, preventing
//! the convenience event from drifting away from programmatic selection.
//! Clearing returns undefined rather than an invented zero-length range.

//!
//! The optional block flag supplies the default mode only when no mode was
//! passed. An explicit word or line mode therefore retains its documented
//! expansion even if the range also carries a block flag from earlier state.
//! Malformed booleans do not coerce through JavaScript truthiness.
//! Columns are checked against the largest allowed grid axis before dispatch;
//! actual retained-row width validation remains with the Rust selection owner.
//! Unknown modes produce an argument error instead of accidentally selecting.
//! Reading selection text uses the current trimTrailing preference, so a live
//! settings change affects the next copy without rewriting any selected row.
//! HTML, VT and plain copy remain native key actions rather than facade methods.
//! A facade keeps no independent selected-string cache that could become stale.
//! Keeping a facade alive retains its owner, while explicit dispose releases
//! the owner's DOM, GPU and transport resources and invalidates future calls.
use super::{
    api::PuzzleTerminal,
    api_viewport::{command, id},
    *,
};
use puzzle_term::{
    messages::Command,
    selection::{SelectionCommand, SelectionRange},
    vt::{
        selection::{SelectPoint, SelectionKind},
        tracked::LineId,
    },
};

/// Owned browser facade over one Session's current selection.
#[wasm_bindgen]
pub struct PuzzleSelection {
    /// Terminal state remains per instance and disposal invalidates this facade.
    owner: Owner,
}
/// The documented selection shape is exposed without the deferred buffer API.
#[wasm_bindgen]
impl PuzzleTerminal {
    /// Returns a facade sharing this terminal's one active tracked selection.
    #[wasm_bindgen(getter)]
    pub fn selection(&self) -> Result<PuzzleSelection, JsValue> {
        self.owner.borrow().ensure_live()?;
        Ok(PuzzleSelection {
            owner: self.owner.clone(),
        })
    }
}
/// Queries are direct; mutations pass through Select policy with Origin::Api.
#[wasm_bindgen]
impl PuzzleSelection {
    /// Returns undefined after clearing, otherwise inclusive line/column endpoints.
    #[wasm_bindgen(getter, unchecked_return_type = "Range | undefined")]
    pub fn range(&self) -> Result<JsValue, JsValue> {
        let r = self.owner.borrow();
        r.ensure_live()?;
        range_value(r.session.selection_range())
    }
    /// Extracts whole Unicode clusters, joining soft wraps and respecting trim settings.
    #[wasm_bindgen(getter)]
    pub fn text(&self) -> Result<String, JsValue> {
        let r = self.owner.borrow();
        r.ensure_live()?;
        Ok(r.session.selection_text())
    }
    /// Selects an inclusive range with optional word, logical-line or semantic expansion.
    /// Invalid IDs or columns are rejected before entering core policy.
    pub fn select(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Range")] range: JsValue,
        #[wasm_bindgen(unchecked_optional_param_type = "SelectMode")] mode: Option<String>,
    ) -> Result<(), JsValue> {
        let block = value::get(&range, "block")?;
        let block = if block.is_undefined() {
            false
        } else {
            block
                .as_bool()
                .ok_or_else(|| value::error("InvalidArgument", "block must be boolean"))?
        };
        let kind = match mode
            .as_deref()
            .unwrap_or(if block { "block" } else { "simple" })
        {
            "simple" => SelectionKind::Simple,
            "word" => SelectionKind::Word,
            "line" => SelectionKind::Line,
            "block" => SelectionKind::Block,
            "semantic" => SelectionKind::Semantic,
            _ => return Err(value::error("InvalidArgument", "unknown selection mode")),
        };
        command(
            &self.owner,
            Command::Select(SelectionCommand::Range {
                kind,
                start: point(&value::get(&range, "start")?)?,
                end: point(&value::get(&range, "end")?)?,
            }),
        )
    }
    /// Selects all retained content on the active screen, including history.
    #[wasm_bindgen(js_name = selectAll)]
    pub fn select_all(&self) -> Result<(), JsValue> {
        command(&self.owner, Command::Select(SelectionCommand::All))
    }
    /// Selects the complete logical line containing a retained physical row.
    #[wasm_bindgen(js_name = selectLine)]
    pub fn select_line(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] line: JsValue,
    ) -> Result<(), JsValue> {
        let at = SelectPoint {
            line: LineId(id(&line)?),
            column: 0,
        };
        command(
            &self.owner,
            Command::Select(SelectionCommand::Range {
                kind: SelectionKind::Line,
                start: at,
                end: at,
            }),
        )
    }
    /// Selects a completed OSC 133 command by its stable Rust-assigned identity.
    /// Unknown/evicted commands are no-ops; identities are never recycled by reset.
    #[wasm_bindgen(js_name = selectZoneOutput)]
    pub fn select_zone_output(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] zone: JsValue,
    ) -> Result<(), JsValue> {
        let id = id(&zone)?;
        let at = {
            let r = self.owner.borrow();
            r.ensure_live()?;
            r.session.zone_output_point(id)
        };
        if let Some(at) = at {
            command(
                &self.owner,
                Command::Select(SelectionCommand::CommandOutput(Some(at))),
            )?;
        }
        Ok(())
    }
    /// Releases tracked endpoints and removes selection fill without rewriting text.
    pub fn clear(&self) -> Result<(), JsValue> {
        command(&self.owner, Command::Select(SelectionCommand::Clear))
    }
}
/// Plain endpoint validation prevents JS integer truncation and negative wrapping.
fn point(v: &JsValue) -> Result<SelectPoint, JsValue> {
    let column = id(&value::get(v, "col")?)?;
    if column > 4095 {
        return Err(value::error(
            "InvalidArgument",
            "column exceeds grid limits",
        ));
    }
    Ok(SelectPoint {
        line: LineId(id(&value::get(v, "lineId")?)?),
        column: column as usize,
    })
}
/// One range encoding is shared by the getter and the selectionChange fact.
pub(super) fn range_value(range: Option<SelectionRange>) -> Result<JsValue, JsValue> {
    let Some(r) = range else {
        return Ok(JsValue::UNDEFINED);
    };
    let object = js_sys::Object::new();
    for (name, p) in [("start", r.start), ("end", r.end)] {
        let point = js_sys::Object::new();
        js_sys::Reflect::set(&point, &"lineId".into(), &(p.line.0 as f64).into())?;
        js_sys::Reflect::set(&point, &"col".into(), &(p.column as f64).into())?;
        js_sys::Reflect::set(&object, &name.into(), &point)?;
    }
    js_sys::Reflect::set(&object, &"block".into(), &r.block.into())?;
    Ok(object.into())
}
