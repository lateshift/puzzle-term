//! Parser borrowing and extension yield points preserve source order (D-44).
//! A registered unknown OSC ends this parser turn at its real terminator.
//! Session handles it before parsing later bytes, so replies never overtake it.
//! With no registrations, pass-through spans use the original bulk parser.
//! The watch-mode choice happens outside vte's scalar/span loop. A watch
//! installed at an OSC yield applies only to the following accepted span.
use super::Terminal;
use crate::osc::OscRequest;
/// Strict protocols must see controls before vte removes them.
mod clipboard_wire;
/// Filter adapter preserves semantic VT and OSC yield points.
mod filtered;

/// Parser allocation and its raw observers have one construction/reset lifetime.
/// The owner temporarily loans only the parser while callbacks mutate Terminal.
pub(super) struct State {
    /// Fixed-size parser preserves split UTF-8/CSI across every feed span.
    pub(super) parser: Option<Box<vte::Parser<{ crate::osc::MAX_BUFFER_BYTES }>>>,
    /// Raw validity and APC/OSC delimiters survive the same split boundaries.
    pub(super) framing: StringFraming,
    /// Large protocol sources are lazy only when an I/O-owning Session opts in.
    reply_streaming: bool,
    /// Ordered streams preserve earlier host callbacks before later input.
    reply_sources: std::collections::VecDeque<Box<dyn crate::reply_stream::ReplySource>>,
    /// Session opts into yielding at each generated reply, before later text.
    pub(super) reply_boundaries: bool,
}

/// Allocate bounded storage once; resets reuse this owner rather than rebuilding it.
impl Default for State {
    fn default() -> Self {
        Self {
            parser: Some(Box::new(vte::Parser::new_with_size())),
            framing: StringFraming::default(),
            reply_boundaries: false,
            reply_streaming: false,
            reply_sources: Default::default(),
        }
    }
}

/// Both string observers retain bounded raw framing across arbitrary read slices.
#[derive(Default)]
pub(super) struct StringFraming {
    /// OSC's semantic callback is checked against the original unstripped bytes.
    osc: crate::osc::wire::Wire,
    /// Bounded raw validation; OSC 99/3008 retain exact payload delimiters.
    clipboard: clipboard_wire::Validator,
    /// APC graphics bodies are extracted before vte discards them.
    apc: crate::apc::ApcFilter,
    /// ED 2/alternate clear retire image placements without changing row identities.
    pub(super) image_clear: [u64; 2],
    /// Destructive source operations invalidate optional tracked-source namespaces.
    pub(super) source_clear: u64,
}

/// Parser ownership is temporarily moved out while callbacks borrow the terminal.
impl Terminal {
    /// A reply-producing semantic callback ends this borrowed parser span.
    pub(crate) fn reply_boundaries(&self) -> bool {
        self.parsing.reply_boundaries
    }

    /// Session-level policy can generate replies from these deferred VT requests.
    /// Yield before later queries so their policy errors keep source order too.
    pub(crate) fn reply_boundary_ready(&self) -> bool {
        #[cfg(test)]
        crate::print_work::note(crate::print_work::Operation::ReplyCheck);
        self.reply_boundaries()
            && (!self.replies.is_empty()
                || !self.parsing.reply_sources.is_empty()
                || matches!(
                    self.events.last(),
                    Some(
                        super::TerminalEvent::ClipboardRequest(_)
                            | super::TerminalEvent::DesktopNotification(_)
                            | super::TerminalEvent::Drop(_)
                    )
                ))
    }

    /// Enables lazy large-response encoding for an I/O-owning Session only.
    /// Standalone VT callers retain eager replies for existing parser/replay APIs.
    /// The Session transfers each cursor before accepting a subsequent request.
    pub fn enable_reply_streams(&mut self) {
        self.parsing.reply_streaming = true;
    }
    /// Moves one ordered packet generator without copying its immutable data.
    /// Packet encoding happens under owner credit rather than inside vte callbacks.
    /// This keeps a large authorized clipboard read from expanding the reply Vec.
    pub fn take_reply_source(&mut self) -> Option<Box<dyn crate::reply_stream::ReplySource>> {
        self.parsing.reply_sources.pop_front()
    }
    /// Origin handlers choose lazy output only when the owner can return writer credit.
    /// The flag is fixed at Session construction, independent of request contents.
    pub(crate) fn reply_streaming(&self) -> bool {
        self.parsing.reply_streaming
    }
    /// At most one parsed request reaches this queue before Session's next boundary.
    /// Ownership transfer leaves the accepted source ordered before later text.
    pub(crate) fn push_reply_source(&mut self, source: Box<dyn crate::reply_stream::ReplySource>) {
        self.parsing.reply_sources.push_back(source);
    }

    /// Generic destructive-clear identity; no protocol-specific state lives in VT.
    pub fn source_clear_epoch(&self) -> u64 {
        self.parsing.framing.source_clear
    }

    /// Called only by destructive source operations, never scalar printing (P1).
    pub(crate) fn source_cleared(&mut self) {
        self.clear_source_watches();
        // Exhaustion disables optional capture rather than reusing an old identity.
        self.parsing.framing.source_clear = self.parsing.framing.source_clear.saturating_add(1);
    }

    /// Per-screen full erase generations let Session retire graphics in source order.
    pub fn image_clear_epochs(&self) -> [u64; 2] {
        self.parsing.framing.image_clear
    }

    /// Advances the bounded parser across an arbitrary byte chunk.
    ///
    /// Taking the box temporarily permits callbacks to mutate terminal
    /// state without a self-reference, and does not allocate per feed.
    pub(super) fn advance_vt(&mut self, bytes: &[u8]) -> usize {
        self.parse_validated(bytes, false, None)
    }

    /// Exposes raw OSC validity before dispatch, retaining semantic yield boundaries.
    fn parse_validated(
        &mut self,
        bytes: &[u8],
        stop_at_sync_end: bool,
        osc: Option<&mut crate::osc::Capture<'_>>,
    ) -> usize {
        #[cfg(test)]
        crate::print_work::note(crate::print_work::Operation::WatchChoice);
        if self.write_clock.has_watches() {
            self.parse_validated_watched::<true>(bytes, stop_at_sync_end, osc)
        } else {
            self.parse_validated_watched::<false>(bytes, stop_at_sync_end, osc)
        }
    }

    /// One parser loop, specialised for optional write observation outside all scalars.
    fn parse_validated_watched<const WATCH: bool>(
        &mut self,
        bytes: &[u8],
        stop_at_sync_end: bool,
        mut osc: Option<&mut crate::osc::Capture<'_>>,
    ) -> usize {
        // Move the existing parser allocation; callback borrows exclude its owner.
        let Some(mut parser) = self.parsing.parser.take() else {
            return 0;
        };
        let mut used = 0;
        while used < bytes.len() {
            // A yield can stop before our observed span; restore its unconsumed suffix.
            let before = self.parsing.framing.clipboard.clone();
            // Stop at one raw OSC boundary so its validity is current at dispatch.
            let count = self.parsing.framing.clipboard.span(&bytes[used..]);
            let mut performer = crate::perform::Performer::<WATCH> {
                term: self,
                stop_at_sync_end,
                osc: osc.as_deref_mut(),
            };
            let accepted = if !stop_at_sync_end
                && performer.osc.is_none()
                && !performer.term.reply_boundaries()
            {
                // Preserve vte's optimized bulk path when no semantic yield is needed.
                parser.advance(&mut performer, &bytes[used..used + count]);
                count
            } else {
                parser.advance_until_terminated(&mut performer, &bytes[used..used + count])
            };
            // A sync end or registered extension may yield before the raw boundary.
            let stopped = vte::Perform::terminated(&performer);
            if accepted < count {
                // Replay only framing state, never semantic mutations already applied.
                self.parsing.framing.clipboard = before;
                self.parsing
                    .framing
                    .clipboard
                    .span(&bytes[used..used + accepted]);
            }
            used += accepted;
            if stopped || accepted == 0 {
                break;
            }
        }
        // Return ownership even on semantic yield, retaining split UTF-8/CSI state.
        self.parsing.parser = Some(parser);
        used
    }

    /// Takes exact OSC 99/3008 data; other sequences allocate no duplicate payload.
    pub(crate) fn extended_osc_wire(&mut self) -> Option<Vec<u8>> {
        // An aborted OSC must not become a complete start/notification merely
        // because vte invokes its string callback on cancellation.
        let raw = self.parsing.framing.clipboard.payload.take();
        if self.parsing.framing.clipboard.cancelled {
            None
        } else {
            raw
        }
    }
    /// Reports raw string corruption only to protocols requiring strict base64.
    pub(crate) fn clipboard_wire_invalid(&self) -> bool {
        self.parsing.framing.clipboard.invalid
    }

    /// Consumes through the end of an active synchronized transaction (§10.4).
    /// The caller retains the suffix; no following transaction is consumed.
    pub(super) fn advance_vt_to_sync_end(&mut self, bytes: &[u8]) -> usize {
        if self.modes.get(true, 2026) != Some(true) {
            // Outside sync output, all of the supplied slice belongs to this turn.
            return self.parse_validated(bytes, false, None);
        }
        // Semantic callbacks still decide the stopping boundary, not byte spellings.
        self.parse_validated(bytes, true, None)
    }
}

/// Extension-enabled turns preserve semantic parser boundaries and exact wire bytes.
impl Terminal {
    /// Stops at the first registered unknown OSC, or an active sync transaction's end.
    /// `request` reuses its payload allocation. A true result means it is ready;
    /// the caller owns every unconsumed byte and handles the request before retrying.
    pub(super) fn advance_vt_until_osc(
        &mut self,
        bytes: &[u8],
        numbers: &[u16],
        request: &mut OscRequest,
        stop_at_sync_end: bool,
    ) -> (usize, bool) {
        if self.parsing.framing.osc.pending && !bytes.is_empty() {
            // vte yielded at ESC. Consume exactly its next byte before deciding
            // whether the candidate really had ST; later queries cannot overtake it.
            self.advance_vt(&bytes[..1]);
            self.parsing.framing.osc.pending = false;
            self.parsing.framing.osc.scan(&bytes[..1]);
            return (1, self.parsing.framing.osc.finish(request));
        }
        // Only the interests and reusable result are borrowed, never handler objects.
        let mut capture = crate::osc::Capture {
            numbers,
            request,
            ready: false,
        };
        // Both strict built-ins and extensions preserve the same source prefix.
        let consumed = self.parse_validated(bytes, stop_at_sync_end, Some(&mut capture));
        // End the callback borrows before inspecting the consumed raw prefix.
        let captured = capture.ready;
        // Controls stripped by vte still count against protocol validation and limits.
        self.parsing.framing.osc.scan(&bytes[..consumed]);
        // A raw candidate alone never dispatches: both observers must agree.
        (
            consumed,
            captured && self.parsing.framing.osc.finish(request),
        )
    }
}
