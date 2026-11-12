//! Parser yield points join generic handlers to the existing Session feed loop.
//! Only registered protocols use this path; ordinary terminals retain bulk parsing.
//! Every yield forwards earlier facts, applies RIS, and dispatches before resuming.
use super::Session;

/// Keep the unregistered fast path and the registered callback path in one feed owner.
impl Session {
    /// Parses one budgeted pending slice with short borrows around extension callbacks.
    pub(super) fn advance_pending(&mut self, mut start: usize, end: usize) -> usize {
        // Native graphics framing shares the same ordered boundary lane as OSC.
        if self.graphics.is_some() {
            return self.advance_image_pending(start, end);
        }
        // Return a delta so queued byte accounting retains an incomplete image suffix.
        let initial = start;
        // Yield boundaries may split this slice many times without changing its budget.
        while start < end && !self.osc.pending() && self.replies.can_parse() {
            // The parser writes into reusable registry storage without borrowing a handler.
            let (consumed, captured, _) = self.terminal.advance_to_reply_boundary(
                &self.pending[start..end],
                &self.osc.numbers,
                &mut self.osc.request,
                None,
                false,
            );
            // This prefix belongs to VT; all following bytes remain behind the extension.
            start += consumed;
            // Release the pending-buffer borrow before trusted protocol code runs.
            self.reply_boundary(captured);
            // A missing parser cannot spin forever on an unchanged suffix.
            if consumed == 0 {
                break;
            }
        }
        start - initial
    }

    /// A synchronized transaction still yields at its original semantic end.
    pub(super) fn advance_external(&mut self, bytes: &[u8]) -> usize {
        // Native graphics framing shares the same ordered boundary lane as OSC.
        if self.graphics.is_some() {
            return self.advance_image_external(bytes);
        }
        // Capture the initial mode: feed_to_sync_end must stop at this transaction only.
        let sync = self.terminal.modes.get(true, 2026) == Some(true);
        // Count consumed bytes so the caller can retain an untouched transaction suffix.
        let mut total = 0;
        // An OSC yield is independent of the semantic synchronized-output boundary.
        while total < bytes.len() && !self.osc.pending() && self.replies.can_parse() {
            let (consumed, captured, _) = self.terminal.advance_to_reply_boundary(
                &bytes[total..],
                &self.osc.numbers,
                &mut self.osc.request,
                None,
                sync,
            );
            // Extension replies precede any query in the not-yet-consumed suffix.
            total += consumed;
            self.reply_boundary(captured);
            // Only a real mode transition ends sync parsing; do not search escape spellings.
            if consumed == 0 || sync && self.terminal.modes.get(true, 2026) != Some(true) {
                break;
            }
        }
        total
    }

    /// Only a real reply/policy boundary drains early; plain output retains its
    /// original viewport/selection reconciliation followed by the final bus pump.
    pub(super) fn reply_boundary(&mut self, captured: bool) {
        let policy = matches!(
            self.terminal.events.last(),
            Some(
                puzzle_vt::terminal::TerminalEvent::ClipboardRequest(_)
                    | puzzle_vt::terminal::TerminalEvent::DesktopNotification(_)
                    | puzzle_vt::terminal::TerminalEvent::Drop(_)
            )
        );
        if captured || policy || !self.terminal.replies.is_empty() {
            if captured {
                // Issue older staged receipts before a handler reserves a new one.
                self.flush_parser_replies();
            }
            self.extension_boundary(captured);
            if policy {
                self.pump(self.host_time.unwrap_or_else(web_time::Instant::now));
            }
            // Policy results use the same fence as parser-generated replies.
            // No second reply writer or completion counter exists for extensions.
            self.flush_parser_replies();
        }
        // A lazy clipboard/drop cursor can exist without any raw reply bytes.
        // Transfer it before the next parsed request, preserving stream order.
        // The transport admission lane, rather than this callback, controls encoding.
        self.flush_reply_sources();
    }

    /// No borrow of pending bytes survives into host protocol code.
    pub(super) fn extension_boundary(&mut self, captured: bool) {
        // A preceding RIS must clear plugin state before a subsequent report replaces it.
        self.osc
            .reset_if_changed(self.terminal.reset_epoch, &mut self.bus);
        // Prompt markers preceding this report expire busy records in source order.
        self.forward_events();
        // Raw candidate scanning alone never invokes plugin code.
        if captured {
            self.osc
                .sync_endpoints(&mut self.terminal, false, &mut self.bus);
            self.osc
                .dispatch(&mut self.terminal, &mut self.replies, &mut self.bus);
        }
    }
}

/// Hosts opt in explicitly; simply linking a handler changes no parser behaviour.
impl Session {
    /// Installs one optional OSC protocol. Built-in or duplicate numbers return an error.
    /// Registration moves ownership; host calls use the corresponding Session message.
    pub fn register_osc_handler(
        &mut self,
        number: u16,
        handler: Box<dyn crate::osc::OscHandler>,
    ) -> Result<(), String> {
        // The registry owns the handler for this Session, independent of active screen.
        let visual = self
            .osc
            .register(number, handler, &mut self.terminal, &mut self.overlays)?;
        if visual {
            // A late registration transfers its first view even on an idle terminal.
            self.needs_frame = true;
            self.services.waker.request_frame();
        }
        Ok(())
    }
}

/// Private host controls follow the same ordered Session message seam as input.
impl Session {
    /// Applies a control only to the named registered owner, never after EOF.
    /// A handler rejects wrong typed payloads before creating any authority.
    pub fn control_osc_handler(
        &mut self,
        number: u16,
        control: &mut puzzle_bus::wire::Extension,
    ) -> Result<(), String> {
        if self.exited.is_some() {
            // A held final screen is readable; it cannot acquire a fresh grant.
            return Err("session has exited".into());
        }
        // A host clear/reset may have run without any subsequent application
        // bytes. Bring lifecycle state current before installing new authority.
        self.osc
            .reset_if_changed(self.terminal.reset_epoch, &mut self.bus);
        self.osc
            .sync_endpoints(&mut self.terminal, false, &mut self.bus);
        self.osc
            .control(number, control, &mut self.terminal, &mut self.bus)?;
        // A revoke/epoch replacement may retire the exact request that was
        // waiting for writer storage. Make it eligible for one normal owner
        // continuation without polling or replenishing this turn's credits.
        self.osc.reply_ready();
        Ok(())
    }
}

/// Source publication and genuine-user actions share registered ownership.
mod presentation;
