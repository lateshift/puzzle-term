//! Finite real-host keys and scrolls while the other App surface floods.
//!
//! Warming and the two-second measured interval match the established scroll
//! witness. No input comes from a render callback: all Surface calls happen on
//! this winit owner. Child read receipts and writer clocks measure different
//! endpoints and are retained separately. Native interruptions abort the run.
//! A key deadline cannot be postponed by a render callback or GPU acquisition.
use super::*;
use puzzle_embed::{RawInput, RawKey};
use puzzle_term::{
    viewport::ScrollInput,
    vt::input::{Modifiers, MouseEvent, MouseKind},
};

impl Probe {
    /// Advances the finite fixture; product scheduler decisions are unchanged.
    pub(super) fn advance(&mut self, now: Instant) -> Result<Option<Instant>, String> {
        // Resumed may be queued after an initial about_to_wait callback.
        // Do not manufacture a warm-up origin before native surface creation.
        let Some(created) = self.created else {
            return Ok(Some(now));
        };
        // Warm-up is fixed fixture setup, not permission to discard a slow sample.
        // Readiness is checked once at the boundary, and a failure aborts explicitly.
        if self.started.is_none() {
            let due = created + Duration::from_secs(2);
            if now < due {
                return Ok(Some(due));
            }
            self.arm(now)?;
        }
        // All frames and both child streams share this exact two-second denominator.
        // The later receipt guard cannot contribute extra frames to the cadence.
        let end = self.started.ok_or("missing measurement origin")? + Duration::from_secs(2);
        // A generous child-receipt drain is a functional guard, not a speed budget.
        if now >= end + Duration::from_millis(200) {
            return Ok(None);
        }
        // Inputs cease at the interval edge, but queued child journals can drain.
        // This delay never relaxes the individually measured 50-ms child bound.
        if now >= end {
            return Ok(Some(end + Duration::from_millis(200)));
        }
        if self.next_key.is_some_and(|due| now >= due) {
            // SystemTime has the same epoch in Rust and the owned Python child.
            // Instant separately governs input deadlines and presentation gaps.
            self.keys.push(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_nanos(),
            );
            // The raw callback exercises owner-side translation and real writer metrics.
            // Probe callbacks use the same view-local surface API as native hosts.
            // No grid origin is subtracted here; the Session resolves its presented inset.
            self.windows[1]
                .session
                .input(RawInput::Key(RawKey::character("x", Some("x"))))
                .map_err(|e| e.to_string())?;
            // Schedule from actual delivery rather than burst-catching missed timers.
            // Every actual key is counted, even if the OS delivered it late.
            self.next_key = Some(now + Duration::from_millis(100));
        }
        if self.next_scroll.is_some_and(|due| now >= due) {
            // Shift explicitly asks Session for local scrolling instead of mouse reports.
            // The child has real history, so the tween cannot reach its top in two seconds.
            let pointer = MouseEvent {
                kind: MouseKind::Wheel,
                button: 64,
                column: 0,
                row: 0,
                pixel_x: 0,
                pixel_y: 0,
                modifiers: Modifiers(Modifiers::SHIFT),
                button_held: false,
                click_count: 0,
            };
            // Probe callbacks use the same view-local surface API as native hosts.
            // No grid origin is subtracted here; the Session resolves its presented inset.
            self.windows[1]
                .session
                .wheel(
                    ScrollInput::Rows(-16.0),
                    puzzle_embed::SurfacePointer {
                        position: [0.0, 0.0],
                        event: pointer,
                    },
                )
                .map_err(|e| e.to_string())?;
            // This is the established 80-ms retarget cadence of the M8 scroll witness.
            // It keeps the normal 120-ms animation active without overriding its clock.
            self.next_scroll = Some(now + Duration::from_millis(80));
        }
        // Return only the next authored input deadline; App owns all frame wakeups.
        // An idle control window creates no render-side timer of its own.
        Ok([self.next_key, self.next_scroll, Some(end)]
            .into_iter()
            .flatten()
            .min())
    }
    /// Validate actual monitor observations and readiness before counting a frame.
    fn arm(&mut self, now: Instant) -> Result<(), String> {
        for (index, window) in self.windows.iter().enumerate() {
            // Check where AppKit actually put each window, not the requested monitor.
            // A mistaken display makes the whole cohort invalid before measurement.
            let info = host_surface::info(&window.window);
            if info.millihertz != 60_000
                || host_surface::visibility(&window.window)
                    .monitor_extent
                    .is_none_or(|size| size.width != 5120)
            {
                return Err(format!(
                    "window {index} is not on the LG 5K 60-Hz display: {info:?}"
                ));
            }
            // Children publish readiness only after raw mode and scrollback output.
            // The no-flood control retains the same second child and geometry.
            let role = if index == 1 {
                "sibling"
            } else if self.flood {
                "flood"
            } else {
                "idle"
            };
            if !self.out.join(format!("{role}.ready")).is_file() {
                return Err(format!("owned {role} child did not become ready"));
            }
            // Reserve before input starts so observation does not allocate per frame.
            // The actor and host only share this clock journal, never a Session.
            let mut sample = self.samples[index]
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            sample.presents = Vec::with_capacity(512);
            sample.start = Some(now);
        }
        // Arm keys and notches together; their subsequent schedules remain independent.
        // The first real submission must satisfy the same leading-edge allowance.
        self.started = Some(now);
        self.next_key = Some(now);
        self.next_scroll = Some(now);
        Ok(())
    }
}
