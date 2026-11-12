//! Find UI belongs to each surface, independent of the shared literal-search engine.
//! Switching UI closes query capture, never cancels a host's retained search results.
//!
//! Native UI ownership cannot reuse the browser keybinding flag: that flag also
//! controls URL policy and platform input handling. This choice is independent,
//! survives keybinding replacement and changes only how find intent is presented.
//! An already-open search field must release its keyboard/IME capture when a
//! host replaces it. Closing that field dirties only the overlay state; tracked
//! matches and the current viewport remain available to the host's find box.
//! Changing UI ownership never scans text, borrows fonts or calls the platform.
//! Only an explicit selection request copies source text, on this same owner.
use super::Session;
use crate::search::SearchUi;

impl Session {
    /// Copy only on explicit request, on the owner, without a clipboard transaction.
    pub(super) fn selected_text_requested(&mut self, request: u64) {
        let text = self.selection_text();
        self.host_events
            .push_back(super::host_events::HostEvent::SelectedText { request, text });
    }

    /// Hand query editing to the host without changing URL/security platform policy.
    pub fn set_search_ui(&mut self, ui: SearchUi) {
        self.search_ui = ui;
        let query = &mut self.model_overlays.query;
        if ui == SearchUi::Host
            && query.kind == super::model_overlays::query::Kind::Search
            && query.open
        {
            // A replacement host must not leave an invisible input-capturing field.
            query.open = false;
            query.preedit.clear();
            query.dirty = true;
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }
}
