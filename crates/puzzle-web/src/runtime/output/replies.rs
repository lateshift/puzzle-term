//! Several actual writers must consume a reply before its shared receipt completes.
//!
//! One Session ticket may fan out to a WebSocket and a JS callback.
//! Each route reports once; the count is the number still outstanding.
//! Most completions hit the front, keeping the normal FIFO path constant-time.
//! Promise microtasks can finish later writes first, requiring bounded lookup.
//! Such completion zeros its entry but cannot advance the Session ledger.
//! Only a contiguous zero-count prefix returns writer credit to Session.
//! Duplicate/unknown route IDs leave the admission counters unchanged.
//! Payload ownership stays in each route; this ledger stores scalar receipts.
//! Completion never copies bytes or allocates a replacement protocol buffer.
//! Entries are bounded by the Session's admitted program-reply bytes.
use super::*;

impl Runtime {
    /// All actual writer routes must complete before Session can reuse their credit.
    /// Promise completions may arrive out of order, but Session receipts never do.
    pub(in crate::runtime) fn complete_reply_route(&mut self, id: u64) -> Result<(), String> {
        let index = if self
            .reply_routes
            .front()
            .is_some_and(|(next, _)| *next == id)
        {
            0
        } else {
            self.reply_routes
                .iter()
                .position(|(next, _)| *next == id)
                .ok_or("unknown reply writer route")?
        };
        let routes = &mut self.reply_routes[index].1;
        if *routes == 0 {
            return Err("duplicate reply writer completion".into());
        }
        *routes -= 1;
        // This is an actual writer-progress event, unlike an empty host flush.
        // Retry a leased-buffer wait once; another live route can still keep it busy.
        self.session.reply_storage_ready();
        while self
            .reply_routes
            .front()
            .is_some_and(|(_, count)| *count == 0)
        {
            if let Some((id, _)) = self.reply_routes.pop_front() {
                self.session.reply_completed(id)?;
            }
        }
        Ok(())
    }
}
