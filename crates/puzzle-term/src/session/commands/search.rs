//! Accepted literal queries and match navigation without application bytes.
//!
//! Commands schedule a refresh, never scan history in an input callback. Match
//! navigation changes the model's current index and centres its retained line.
//! SearchRequest is UI intent; a typed fact lets any host show its find interface.
//! Native reopening resumes the retained field through the same lazy search path.
use super::Handler;
use crate::{
    messages::{Command, Event},
    search::SearchCommand,
    viewport::ScrollTarget,
};
use puzzle_bus::hooks::HookCtx;
/// Search requests are accepted by the ordinary terminal command handler.
impl Handler<'_> {
    /// Replaces or clears the lazy model after the normal interceptor chain.
    pub(super) fn search_command(
        &mut self,
        command: &SearchCommand,
        ctx: &mut HookCtx<'_, Command, Event>,
    ) {
        // Query ownership stays with Session; hosts receive only progress and intent.
        match command {
            SearchCommand::Find { query, options } => {
                self.search.find(self.terminal, query, *options)
            }
            // Clear cancels work before any later frame can copy stale fills.
            SearchCommand::Clear => self.search.clear(self.terminal),
            SearchCommand::Request => {
                // D-48 gives native hosts the same find intent as embedding pages.
                // The default native bar remains usable until a host replaces it.
                ctx.publish(Event::SearchRequested);
                if self.host_search {
                    // This native contract is distinct from the legacy public bus fact.
                    // No query overlay opens or captures subsequent terminal input.
                    self.host_events
                        .push_back(crate::HostEvent::SearchRequested);
                } else if !self.web_search {
                    // Switching from help must not turn its filter into a terminal query.
                    if self.query.kind != super::super::model_overlays::query::Kind::Search {
                        self.query.text.clear();
                        self.query.preedit.clear();
                        self.query.cursor = 0;
                    }
                    self.query.kind = super::super::model_overlays::query::Kind::Search;
                    self.query.open = true;
                    self.query.dirty = true;
                    // Closing clears matches but retains the field; reopening restores
                    // results for that same query without requiring another keystroke.
                    self.search
                        .find(self.terminal, &self.query.text, Default::default());
                    self.selection_seen = true;
                    *self.needs_frame = true;
                    self.services.waker.request_frame();
                }
                return;
            }
        }
        // The selection_seen flag requests one source marker refresh after dispatch.
        self.selection_seen = true;
        *self.needs_frame = true;
        self.services.waker.request_frame();
    }
    /// Moves the current result, so repeated next does not depend on viewport top.
    pub(super) fn search_move(&mut self, delta: i32, ctx: &mut HookCtx<'_, Command, Event>) {
        // Gone results cannot yield a navigation target from the tracked engine.
        if let Some(line) = self.search.navigate(self.terminal, delta) {
            // Both immediate and deferred steps use the shared reveal/centre path.
            self.query.dirty = true;
            self.move_viewport(ScrollTarget::Line(line), ctx);
            // Changing current fill needs a frame even when two hits share a row.
            *self.needs_frame = true;
            self.services.waker.request_frame();
        }
    }
}
