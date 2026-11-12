//! Public grid queries distinguish cell counts from physical canvas dimensions.
use super::{api::PuzzleTerminal, *};

/// Geometry operations share Session reflow and the mounted container's fit path.
#[wasm_bindgen]
impl PuzzleTerminal {
    /// Columns count VT cells, not glyphs: a wide cluster occupies two cells.
    /// The value can change after a host fit and is reported to the backend
    /// through the same resize operation used by the local grid.
    /// Current columns reflect the fitted or explicitly sized VT grid.
    #[wasm_bindgen(getter)]
    pub fn cols(&self) -> Result<u32, JsValue> {
        let r = self.owner.borrow();
        r.ensure_live()?;
        Ok(r.session.buffer().size().0 as u32)
    }
    /// Rows count visible grid lines, excluding retained scrollback.
    /// A callback transport receives this count along with the column count.
    /// Fonts can change ink without changing a cluster's occupied cells.
    /// Current rows reflect the fitted or explicitly sized VT grid.
    #[wasm_bindgen(getter)]
    pub fn rows(&self) -> Result<u32, JsValue> {
        let r = self.owner.borrow();
        r.ensure_live()?;
        Ok(r.session.buffer().size().1 as u32)
    }
    /// CSS controls the container; fit converts its dimensions to device
    /// pixels once. Whole cells fit inside that surface and any leftover
    /// partial strip remains the active theme's default background.
    /// Reflows to the host's current CSS geometry and monitor DPR.
    pub fn fit(&self) -> Result<(), JsValue> {
        self.owner.borrow_mut().fixed_grid = None;
        draw::fit(&self.owner)
    }
    /// Axis and total-cell limits are validated before allocating a grid.
    /// Resize reflows logical lines using the same core implementation as
    /// desktop; this operation does not create another terminal screen.
    /// Sets an explicit grid, checking all allocation limits before resize.
    pub fn resize(
        &self,
        #[wasm_bindgen(unchecked_param_type = "number")] cols: JsValue,
        #[wasm_bindgen(unchecked_param_type = "number")] rows: JsValue,
    ) -> Result<(), JsValue> {
        // Reuse axis and total-cell bounds before a grid allocation or reflow occurs.
        // Receiving raw JS values lets validation run before wasm's integer coercion.
        let cols = arguments::axis(&cols)?;
        let rows = arguments::axis(&rows)?;
        let size = puzzle_term::bridge::Size {
            cols,
            rows,
            width_px: 0,
            height_px: 0,
        };
        if !size.valid() {
            return Err(value::error("InvalidArgument", "invalid grid size"));
        }
        {
            let mut r = self.owner.borrow_mut();
            r.ensure_live()?;
            r.fixed_grid = Some([cols, rows]);
            let pixels = r.pixels;
            r.session
                .resize(cols as usize, rows as usize, pixels)
                .map_err(|e| value::error("InvalidArgument", &e))?;
        }
        host::turn(&self.owner);
        Ok(())
    }
}
