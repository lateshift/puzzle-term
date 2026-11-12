//! Winit-shaped pointer state in device pixels, before terminal encoding.
//!
//! Overlay motion keeps physical precision independently of protocol cell deduplication.
//! Discarding a handled sample also clears pending application motion for that refresh.
//!
//! Motion retains only the latest position until the display turn (P12).
//! Buttons and wheel reports stay immediate. Live callbacks retain view-local
//! backing pixels until the embedding seam converts them to logical points.
//! Only Puzzle's presented geometry removes padding and the producer gutter.
//! Click counts use an injected millisecond time, keeping tests deterministic.
//! Tracking decides which transitions leave the terminal; encoding decides
//! whether a position is a cell or device pixel. A 1016 application can see
//! movement inside one cell that classic SGR deliberately suppresses.
//! The pending slot is discarded on button transitions, preventing a delayed
//! pre-release motion from being sent after its release report.
//! Held buttons are retained in press order; releasing one leaves another
//! button available for the next drag report rather than fabricating hover.
//! Vertical wheel units now reach Session intact for local or application routing.
//! Coordinate and click-count data also support the local selection state.
//! Fractional wheel deltas accumulate across callbacks. Rounding each callback
//! would lose smooth-trackpad gestures that never individually reach one tick.
//! Horizontal and vertical wheel directions use xterm's distinct button codes.
//! A drag outside the window clamps to the last visible cell/pixel, because
//! terminal report coordinates cannot name a negative or nonexistent cell.
//! Clamping uses geometry from the current text context, not a fixed 8×16 grid.
//! The adapter retains no VT borrow between callbacks, so parser mode changes
//! remain visible at flush and at the final Session command handler.
use puzzle_term::vt::input::{Modifiers, MouseEvent, MouseKind, MouseModes, MouseTracking};

#[cfg(test)]
use puzzle_term::vt::input::MouseEncoding;

/// Native pixel and row deltas remain distinct until Session dispatch.
mod wheel;

/// Converts native buttons to xterm's numbering, independent of enum order.
pub(crate) fn button_code(button: winit::event::MouseButton) -> u16 {
    use winit::event::MouseButton;
    // Back/Forward and vendor buttons use xterm's extended 128+ range.
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Back => 128,
        MouseButton::Forward => 129,
        MouseButton::Other(number) => 128u16.saturating_add(number),
    }
}

/// Cell geometry copied from the same context that draws the live grid.
#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    /// Integer device-cell width and height, never logical pixels.
    pub cell: [u32; 2],
    /// Visible columns and rows used to clamp an outside drag.
    pub grid: [u32; 2],
}

/// Native pointer state; no winit object survives the callback boundary.
#[derive(Default)]
pub(crate) struct Adapter {
    /// Last physical position, relative to the window surface.
    position: [f64; 2],
    /// Press order identifies the button used in drag reports.
    held: Vec<u16>,
    /// Latest eligible motion, replacing earlier callbacks this frame.
    pending: Option<MouseEvent>,
    /// Last transmitted cell/pixel and encoding for duplicate suppression.
    #[cfg(test)]
    reported: Option<(MouseEncoding, u32, u32)>,
    /// Local coalescing compares raw signed position, including outside distance.
    #[cfg(test)]
    local_reported: Option<[f64; 2]>,
    /// Last click identity and time for double/triple selection in M8.
    click: Option<(u16, u32, u32, u64)>,
    /// Consecutive presses at the same cell, capped at three.
    clicks: u8,
    /// Fractional wheel ticks retained until a whole direction is available.
    wheel: [f64; 2],
}

/// Maps coordinates once, while Session owns the final protocol policy.
impl Adapter {
    /// Replaces a pending move; returns whether a display turn is needed.
    pub(crate) fn moved(
        &mut self,
        position: [f64; 2],
        geometry: Geometry,
        mods: u8,
        modes: MouseModes,
    ) -> bool {
        // The latest sample replaces the entire pending position, so a fast
        // cursor path cannot enqueue hundreds of reports ahead of a key.
        self.position = position;
        let event = self.event(
            MouseKind::Move,
            self.held.last().copied().unwrap_or(3),
            geometry,
            mods,
        );
        // Normal tracking deliberately ignores motion. ButtonMotion needs
        // a held button; AnyMotion is the only hover-reporting mode.
        let eligible = modes.tracking == MouseTracking::AnyMotion
            || (modes.tracking == MouseTracking::ButtonMotion && event.button_held);
        // Local drag and semantic hover also consume one sample at the display turn.
        self.pending = Some(event);
        eligible
            || self.held.contains(&0)
            || modes.tracking == MouseTracking::None
            || mods & Modifiers::SHIFT != 0
    }

    /// Encodes at most one changed position in this display turn.
    #[cfg(test)]
    pub(crate) fn flush(&mut self, modes: MouseModes) -> Option<MouseEvent> {
        let event = self.pending.take()?;
        // Recheck modes: output may disable tracking after the callback.
        if modes.tracking != MouseTracking::AnyMotion
            && !(modes.tracking == MouseTracking::ButtonMotion && event.button_held)
        {
            return None;
        }
        let (x, y) = if modes.encoding == MouseEncoding::SgrPixels {
            (event.pixel_x, event.pixel_y)
        } else {
            (event.column, event.row)
        };
        // Classic reports cannot express subcell motion; comparing pixels
        // in those modes would send redundant reports with identical bytes.
        let identity = (modes.encoding, x, y);
        if self.reported == Some(identity) {
            return None;
        }
        // Save the encoding as well: changing from cells to pixels must
        // allow the same physical position to be reported in its new unit.
        self.reported = Some(identity);
        Some(event)
    }

    /// Flushes a signed sample for shared local selection and live tracking routing.
    #[cfg(test)]
    pub(crate) fn flush_pointer(
        &mut self,
        geometry: Geometry,
        modes: MouseModes,
        shift_selects: bool,
    ) -> Option<puzzle_term::pointer::PointerInput> {
        // Peek before taking: application routing uses flush’s encoding identity.
        let event = self.pending?;
        // Shift selection is live policy, not inferred from a tracking mode alone.
        let local = modes.tracking == MouseTracking::None
            || (shift_selects && event.modifiers.0 & Modifiers::SHIFT != 0);
        if !local {
            // Normal/X10 cannot report motion, but still need to cancel a local
            // left drag when Shift is released. Session's encoder remains inert.
            if event.button == 0
                && event.button_held
                && matches!(modes.tracking, MouseTracking::Normal | MouseTracking::X10)
            {
                self.pending = None;
                return Some(self.pointer_input(event, geometry));
            }
            // Protocol duplicate suppression still compares cells or 1016 pixels.
            return self
                .flush(modes)
                .map(|event| self.pointer_input(event, geometry));
        }
        // Local position identity includes subcell and outside distance.
        // Equal clamped cells can still demand different edge-scroll speeds.
        self.pending = None;
        if self.local_reported == Some(self.position) {
            return None;
        }
        // Store the raw sample only after it is accepted for this display turn.
        self.local_reported = Some(self.position);
        Some(self.pointer_input(event, geometry))
    }

    /// Moves one physical refresh sample; live protocol policy belongs to Session.
    pub(crate) fn take_motion(&mut self) -> Option<MouseEvent> {
        // Move the sample exactly once. Duplicate suppression belongs to the
        // Session, where parser-driven mode changes are ordered with input.
        self.pending.take()
    }
    /// A consumed region move must never subsequently become a selection or report.
    pub(crate) fn discard_motion(&mut self) {
        // A handled overlay event cannot leak into the later protocol flush.
        self.pending = None;
    }
    /// Converts backing pixels to the embedding API's view-local logical points.
    pub(crate) fn surface_pointer(
        &self,
        event: MouseEvent,
        scale: f64,
    ) -> puzzle_embed::SurfacePointer {
        puzzle_embed::SurfacePointer {
            // Do not subtract the legacy headless-test inset here. SurfacePointer
            // is view-local: the owner removes the actual presented origin once,
            // including configured padding and any producer gutter, before
            // scaling that grid-relative point into core device pixels.
            position: self.position.map(|n| n / scale),
            event,
        }
    }

    /// Legacy direct-to-core fixtures use a fixed sixteen-device-pixel grid origin.
    /// Live hosts instead use surface_pointer and the presented embedding geometry.
    #[cfg(test)]
    pub(crate) fn pointer_input(
        &self,
        event: MouseEvent,
        geometry: Geometry,
    ) -> puzzle_term::pointer::PointerInput {
        puzzle_term::pointer::PointerInput {
            event,
            // Signed coordinates preserve distance beyond either grid boundary.
            // Report coordinates above were independently clamped for xterm.
            position: self.position.map(|n| n - 16.0),
            cell: geometry.cell,
        }
    }

    /// Returns an immediate press/release with selection-ready click count.
    pub(crate) fn button(
        &mut self,
        button: u16,
        pressed: bool,
        geometry: Geometry,
        mods: u8,
        now_ms: u64,
    ) -> MouseEvent {
        // A button transition changes drag identity immediately; an older
        // pending motion must not cross that press/release boundary.
        self.pending = None;
        #[cfg(test)]
        {
            self.reported = None;
            self.local_reported = None;
        }
        if pressed {
            // Retain press order for multi-button drags without duplicate holds.
            if !self.held.contains(&button) {
                self.held.push(button);
            }
        } else {
            // Releasing the newest button restores an older held drag button.
            self.held.retain(|held| *held != button);
        }
        let mut event = self.event(
            if pressed {
                MouseKind::Press
            } else {
                MouseKind::Release
            },
            button,
            geometry,
            mods,
        );
        if pressed {
            // A half-second, same-cell repeat forms a selection click chain.
            // After triple click the next press starts a fresh chain.
            let repeated = self.click.is_some_and(|(b, x, y, t)| {
                b == button
                    && x == event.column
                    && y == event.row
                    && now_ms.saturating_sub(t) <= 500
            });
            self.clicks = if repeated { self.clicks % 3 + 1 } else { 1 };
            self.click = Some((button, event.column, event.row, now_ms));
        }
        event.click_count = self.clicks;
        event
    }

    /// Accumulates line ticks; pixel deltas are converted by the host metrics.
    pub(crate) fn wheel(
        &mut self,
        delta: [f64; 2],
        geometry: Geometry,
        mods: u8,
    ) -> Vec<MouseEvent> {
        let mut events = Vec::new();
        for (axis, amount) in delta.into_iter().enumerate() {
            // Nonfinite device input has no direction. Bound a single callback
            // to avoid excessive report work after a broken input driver.
            if !amount.is_finite() {
                continue;
            }
            self.wheel[axis] = (self.wheel[axis] + amount).clamp(-64.0, 64.0);
            // Retain the fractional remainder for the next trackpad callback;
            // xterm uses a complete button report for each whole wheel tick.
            while self.wheel[axis].abs() >= 1.0 {
                let positive = self.wheel[axis] > 0.0;
                let button = if axis == 0 {
                    if positive { 67 } else { 66 }
                } else if positive {
                    64
                } else {
                    65
                };
                events.push(self.event(MouseKind::Wheel, button, geometry, mods));
                self.wheel[axis] -= if positive { 1.0 } else { -1.0 };
            }
        }
        events
    }

    /// Removes the physical inset and clamps drags to the visible text area.
    fn event(&self, kind: MouseKind, button: u16, geometry: Geometry, mods: u8) -> MouseEvent {
        // Subtract padding before converting to integer device coordinates.
        // Saturation also handles negative/outside values without panicking.
        let pixel = self.position.map(|v| (v - 16.0).max(0.0) as u32);
        MouseEvent {
            kind,
            button,
            // Divide in device space: applying DPR a second time here would
            // make a Retina click select a cell at half its visible position.
            column: (pixel[0] / geometry.cell[0].max(1)).min(geometry.grid[0].saturating_sub(1)),
            row: (pixel[1] / geometry.cell[1].max(1)).min(geometry.grid[1].saturating_sub(1)),
            // Pixel reporting is relative to the text area, with the same
            // outside-drag bound as cell reporting, not the whole surface.
            pixel_x: pixel[0].min(
                geometry.grid[0]
                    .saturating_mul(geometry.cell[0])
                    .saturating_sub(1),
            ),
            pixel_y: pixel[1].min(
                geometry.grid[1]
                    .saturating_mul(geometry.cell[1])
                    .saturating_sub(1),
            ),
            modifiers: Modifiers(mods),
            button_held: !self.held.is_empty(),
            click_count: self.clicks,
        }
    }
}

#[cfg(test)]
/// Byte tables and per-frame coalescing are independent of a native window.
mod tests;
