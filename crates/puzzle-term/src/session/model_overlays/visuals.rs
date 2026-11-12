//! Session-owned composition, suggestions and status sources (design §11.6).
//!
//! Suggestions are bounded UI data. Only explicit acceptance submits Text through
//! the normal bus policies; drawing cannot write bytes or mutate terminal cells.
//!
//! The installation path runs once per visual host. Each registration transfers
//! a view to the frame builder and keeps the model with Session. Static models
//! need no private payload because the rotating snapshot already owns these sources.
//! Bell is different: its model owns finite endpoints and requests the final wake.
//! Neither progress nor cursor annotations introduces a polling timer.
//!
//! Composition originates in accepted Preedit commands, while ghost text originates
//! in the embedding API. Keeping those sources separate lets clearing composition
//! reveal a suggestion without accepting it. A commit clears only the preedit state.
//! Snapshots reuse their own String allocations; an older sealed publication cannot
//! observe a subsequent edit or read a live Session from the render thread.
//! Font size comes from the effective frame policy rather than a second UI setting.
//! DPR conversion belongs to the view's geometry, not this logical source capture.
//!
//! Explicit acceptance moves the suggestion into the normal command queue. That
//! operation may allocate like other user input, but is absent from frame hot paths.
//! Its Api origin remains inspectable by policy hooks, and vetoes still consume the
//! local suggestion: a denied write must not leave stale accepted suggestion ink.
use super::*;
use puzzle_overlay::{
    builtins::{
        inline::InlineView,
        status::{Bell, BellView, ProgressView},
    },
    model::StaticModel,
};
impl Session {
    /// Changes visual bell policy without suppressing the Bell fact or its marker.
    pub fn set_visual_bell(&mut self, enabled: bool) {
        if self.model_overlays.visuals.bell_visual == enabled {
            return;
        }
        // Removal is itself a visual change, even without subsequent application output.
        self.model_overlays.visuals.bell_visual = enabled;
        self.overlay_changed(EventKinds::STATUS);
        self.needs_frame = true;
        self.services.waker.request_frame();
    }
    /// Registers each painter through the same public model/view API hosts use.
    pub(super) fn install_status_overlays(&mut self) {
        // Query edges hide cursor annotations while composition belongs to a field.
        self.overlays_mut().add(
            Box::new(StaticModel::new(
                "cursor-annotations",
                EventKinds(
                    EventKinds::INPUT.0
                        | EventKinds::GEOMETRY.0
                        | EventKinds::THEME.0
                        | EventKinds::QUERY.0,
                ),
            )),
            Box::new(InlineView),
        );
        // The stateful bell owns its interval independently of grid damage.
        self.overlays_mut()
            .add(Box::new(Bell::default()), Box::new(BellView));
        // Progress changes only on OSC status, geometry or theme edges.
        self.overlays_mut().add(
            Box::new(StaticModel::new(
                "progress",
                EventKinds(EventKinds::STATUS.0 | EventKinds::GEOMETRY.0 | EventKinds::THEME.0),
            )),
            Box::new(ProgressView),
        );
    }
    /// Captures only current UI sources; no cell or history scan is involved.
    pub(super) fn copy_visuals(&mut self, out: &mut Models) {
        // Compare this slot's composition before replacing its reusable source buffers.
        if out.visuals.preedit != self.preedit.text
            || out.visuals.preedit_cursor != self.preedit.cursor
        {
            self.overlay_changed(EventKinds::INPUT);
        }
        out.hud = self.model_overlays.hud.value;
        out.visuals.copy_from(&self.model_overlays.visuals);
        out.visuals.preedit.clone_from(&self.preedit.text);
        out.visuals.preedit_cursor = self.preedit.cursor;
        out.visuals.font_size = self
            .frame_policy
            .as_ref()
            .map_or(14.0, |policy| policy.font_size);
    }
    /// Replaces a single-line suggestion; None clears it without writing to the PTY.
    pub fn set_ghost_text(&mut self, text: Option<&str>) {
        let value = &mut self.model_overlays.visuals.ghost;
        value.clear();
        if let Some(text) = text {
            // Same bounded UI-string policy as query fields; do not split UTF-8 scalars.
            for ch in text.chars().filter(|ch| !ch.is_control()) {
                if value.len() + ch.len_utf8() > 4096 {
                    break;
                }
                value.push(ch);
            }
        }
        self.overlay_changed(EventKinds::INPUT);
        self.needs_frame = true;
        self.services.waker.request_frame();
    }
    /// Explicit acceptance is policy-controlled committed input, never renderer work.
    pub fn accept_ghost_text(&mut self, now: Instant) {
        let text = std::mem::take(&mut self.model_overlays.visuals.ghost);
        if text.is_empty() {
            return;
        }
        self.overlay_changed(EventKinds::INPUT);
        // Accepted text need not move a solid cursor until the child echoes it.
        // Remove the suggestion immediately, even if that echo never arrives.
        self.needs_frame = true;
        self.services.waker.request_frame();
        self.submit(
            crate::messages::Command::Text(text),
            puzzle_bus::Origin::Api,
        );
        self.pump(now);
    }
}
