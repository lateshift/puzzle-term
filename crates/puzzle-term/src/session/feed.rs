//! Budgeted output feeding preserves replies and service work in source order.
//! The parser, damage wake, reconciliation and bus pump have distinct costs.
//! An opt-in clock observation brackets these coarse stages, never each scalar;
//! ordinary sessions have no additional clock calls or allocated sample storage.
//! Offered bytes keep an earlier suffix ahead of new transport data (§6.4).
//! The six measured phases are nested inside one complete Session-owner feed turn.
//! They exclude host channel handoffs and cannot substitute for that total.
//! Service timing uses Session's existing cross-platform clock boundary; VT
//! still receives only bytes and records clock-free operation counts.
//! Empty feeds are significant: they drain retained output at child EOF and
//! pump queued lifecycle commands after the last kernel read has completed.
//! Reply transport remains inside the feed so request ordering is preserved.
//! A reply-write failure is logged before its buffer is retired, as before.
//! Timing is diagnostic; the injected budget clock still controls slice admission.
use super::{FeedOutcome, Session};
use web_time::Instant;

/// Parsing stays synchronous; the owner checks priority messages between slices.
impl Session {
    /// Parses available output in at most 64 KiB slices within the budget.
    pub fn feed(&mut self, bytes: &[u8], start: Instant) -> FeedOutcome {
        // One public call is one work quantum even if OSC yields split it many
        // times. Default sessions have an empty registry and no callbacks here.
        self.osc.begin_turn();
        let outcome = self.feed_inner(bytes, start);
        self.osc.end_turn();
        outcome
    }

    /// Shares queued parsing with feed_to_sync_end without replenishing credits.
    fn feed_inner(&mut self, bytes: &[u8], start: Instant) -> FeedOutcome {
        // Host animation facts use the caller's clock even on parser-only turns.
        self.host_time = Some(start);
        if self.exited.is_some() {
            // No later bytes can reopen a drained child session.
            return FeedOutcome {
                parsed_bytes: 0,
                pending_bytes: 0,
                slices: 0,
            };
        }
        // User-initiated optional work uses the same declared turn, even when
        // program replies are waiting for stdin. Only actual work gets a call;
        // absent or idle handlers trigger no source reconciliation here.
        if self.osc.owner_work_pending() {
            self.osc
                .sync_endpoints(&mut self.terminal, false, &mut self.bus);
            self.osc
                .continue_owner_work(&mut self.terminal, &mut self.bus);
        }
        // Appending before parsing preserves an older pending suffix ahead
        // of fresh bytes, even when transport callbacks arrive rapidly.
        let copying = self.begin_feed_phase();
        self.observe_output_activity(bytes.len());
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > 64 * 1024 * 1024 {
            tracing::warn!(
                event = "session.pending_bytes",
                bytes = self.pending.len(),
                "transport should apply backpressure"
            );
        }
        self.end_feed_phase(0, copying);
        let (parsed, slices) = self.parse_pending(start);
        // The source owner accounts for every parser entry, including publication
        // suffixes. A host counter updated only around feed would miss those bytes.
        self.parsed_total = self.parsed_total.saturating_add(parsed as u64);
        let copying = self.begin_feed_phase();
        // Shifting once per feed avoids cloning each 64 KiB parser slice.
        // The pending allocation survives later calls, including bursts.
        self.pending.drain(..parsed);
        self.end_feed_phase(0, copying);
        self.finish_feed(parsed, start);
        FeedOutcome {
            parsed_bytes: parsed,
            pending_bytes: self.pending_bytes(),
            slices,
        }
    }

    /// Consumes only through an active synchronized transaction's end (§10.4).
    /// A host with a pending frame retains `bytes[consumed..]` in its own inbox.
    /// Unlike `feed`, this bounded-slice API never accepts that unparsed suffix.
    /// The desktop still supplies at most 16 KiB; the web keeps budgeted `feed`.
    pub fn feed_to_sync_end(&mut self, bytes: &[u8], now: Instant) -> usize {
        // Draining old accepted data and consuming this borrowed slice share
        // one turn. Calling public feed here would replenish/log twice.
        self.osc.begin_turn();
        let consumed = self.feed_to_sync_end_inner(bytes, now);
        self.osc.end_turn();
        consumed
    }

    /// Keeps the borrowed suffix with the host behind any outstanding boundary.
    fn feed_to_sync_end_inner(&mut self, bytes: &[u8], now: Instant) -> usize {
        // Earlier budgeted data must stay ahead of the caller's unaccepted slice.
        // If any survives, the caller retries this slice rather than losing bytes.
        if self.feed_inner(&[], now).pending_bytes != 0
            || self.exited.is_some()
            || !self.replies.can_parse()
        {
            return 0;
        }
        self.host_time = Some(now);
        let consumed = self.advance_external(bytes);
        self.observe_output_activity(consumed);
        self.parsed_total = self.parsed_total.saturating_add(consumed as u64);
        // Replies precede publication and the unaccepted suffix in producer order.
        self.flush_parser_replies();
        self.finish_feed(consumed, now);
        consumed
    }

    /// Preserves the parser cap and ordered replies, with separate nested clocks.
    fn parse_pending(&mut self, start: Instant) -> (usize, usize) {
        let mut parsed = 0;
        let mut slices = 0;
        // Retry retained raw replies before considering another source callback.
        // An unavailable precharged buffer needs a completion, not parser polling.
        self.flush_parser_replies();
        if self.osc.waiting_reply()
            || !self.replies.can_parse()
            || !self.terminal.replies.is_empty()
        {
            return (0, 0);
        }
        if self.osc.pending() {
            // The retained request resumes before any following text or query,
            // including an empty continuation after the child's final write.
            self.osc
                .sync_endpoints(&mut self.terminal, false, &mut self.bus);
            self.osc
                .dispatch(&mut self.terminal, &mut self.replies, &mut self.bus);
            self.flush_parser_replies();
            if self.osc.pending() {
                return (0, 0);
            }
        }
        while parsed < self.pending.len() {
            // A slice never exceeds the §6.4 cap, even if the host hands
            // this call several megabytes in a single transport packet.
            let end = (parsed + 65_536).min(self.pending.len());
            let parsing = self.begin_feed_phase();
            if self.image_decode_pending() || self.osc.pending() || !self.replies.can_parse() {
                break;
            }
            let used = self.advance_pending(parsed, end);
            self.end_feed_phase(1, parsing);
            parsed += used;
            slices += 1;
            // A reply must leave before later slices can mutate the grid
            // again; the VT reply buffer retains capacity across calls.
            let replying = self.begin_feed_phase();
            self.flush_parser_replies();
            self.end_feed_phase(2, replying);
            if used == 0 || self.services.clock.elapsed_since(start) >= self.parse_budget {
                break;
            }
        }
        (parsed, slices)
    }

    /// Uses the existing ordered writer, also when a resumed OSC consumed no bytes.
    pub(super) fn flush_parser_replies(&mut self) {
        if let Err(error) = self
            .replies
            .replies(self.services.transport.as_mut(), &mut self.terminal.replies)
        {
            // A disconnected peer cannot consume more replies. Report it instead
            // of clearing bytes and pretending that enqueue meant successful I/O.
            tracing::warn!(event = "session.reply_write_failed", %error);
            self.close_requested = true;
        }
        // Source cursors retain their original query position before later input.
        // They encode only as much as the current owner turn can admit.
        self.flush_reply_sources();
    }

    /// Large VT replies enter the same FIFO before subsequent program output.
    pub(super) fn flush_reply_sources(&mut self) {
        while let Some(source) = self.terminal.take_reply_source() {
            if let Err(error) = self
                .replies
                .stream(self.services.transport.as_mut(), source)
            {
                tracing::warn!(event = "session.reply_write_failed", %error);
                self.close_requested = true;
                break;
            }
        }
    }

    /// Service costs belong to the same owner turn, even when VT work is small.
    fn finish_feed(&mut self, parsed: usize, start: Instant) {
        let waking = self.begin_feed_phase();
        if parsed > 0 {
            // Terminal damage is still parsed while synchronized output is
            // active; only its presentation is postponed by the scheduler.
            self.output_changed(start);
            self.overlay_changed(puzzle_overlay::provider::EventKinds::GRID);
        }
        self.end_feed_phase(3, waking);
        let reconciling = self.begin_feed_phase();
        self.reconcile_images();
        if parsed > 0 {
            self.reconcile_selection();
            self.search.dirty = true;
        }
        self.reconcile_viewport(parsed > 0);
        self.forward_events();
        self.end_feed_phase(4, reconciling);
        // Hooks may queue follow-up commands, but each drain has the
        // bus's existing sixteen-round cascade guard (§5.3).
        let pumping = self.begin_feed_phase();
        self.pump(start);
        self.end_feed_phase(5, pumping);
    }
}
