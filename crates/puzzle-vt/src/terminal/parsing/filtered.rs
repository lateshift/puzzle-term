//! One APC filter wraps bulk, synchronized and OSC-extension parser entry points.
//! Complete graphics yields stop before later queries, allowing Session to send
//! replies in source order. The plain VT API continues discarding graphics until
//! a Session consumes them, just as it discards other unsupported APC commands.
use crate::{Terminal, apc::ApcSink, osc::OscRequest};

/// Short-lived adapter holds no storage beyond the caller's reusable requests.
struct Sink<'a, 'b> {
    /// vte callbacks mutate only this terminal owner.
    terminal: &'a mut Terminal,
    /// No registered OSC handlers means the original bulk parser is used.
    osc: Option<(&'b [u16], &'b mut OscRequest)>,
    /// Stop at the semantic end of an initially active synchronized update.
    sync: bool,
    /// Session's graphics request buffer; None preserves VT-only discard policy.
    graphics: Option<&'b mut Vec<u8>>,
    /// Each completed extension boundary yields before any subsequent reply.
    osc_ready: bool,
    /// A graphics body is ready only after the two-byte ST reached vte.
    graphics_ready: bool,
}

/// Passing a partial prefix leaves the suffix entirely in the caller's ownership.
impl ApcSink for Sink<'_, '_> {
    /// Dispatch the accepted span through the caller's original semantic policy.
    fn passthrough(&mut self, bytes: &[u8]) -> usize {
        // A terminator at the end of the preceding span may already have
        // completed this turn. Do not admit the next APC's G or a later query.
        if self.osc_ready
            || self.terminal.reply_boundary_ready()
            || self.sync && self.terminal.modes.get(true, 2026) != Some(true)
        {
            return 0;
        }
        if let Some((numbers, request)) = &mut self.osc {
            // Raw OSC observation sees exactly the VT bytes that vte sees;
            // graphic pixels cannot masquerade as a registered OSC payload.
            let (used, captured) = self
                .terminal
                .advance_vt_until_osc(bytes, numbers, request, self.sync);
            self.osc_ready = captured;
            used
        } else if self.sync {
            // The existing vte callback detects mode completion, not a search
            // for a particular byte spelling that could occur inside a string.
            self.terminal.advance_vt_to_sync_end(bytes)
        } else {
            // Preserve the fast non-yielding parser for unregistered sessions.
            self.terminal.advance_vt(bytes)
        }
    }

    /// Retain just one request or preserve the VT-only unsupported-APC policy.
    fn graphics(&mut self, body: &[u8]) -> bool {
        let Some(request) = &mut self.graphics else {
            return true;
        };
        // A single bounded request is retained; no unbounded event queue can
        // accumulate image chunks ahead of Session's storage/decode budget.
        request.clear();
        request.extend_from_slice(body);
        self.graphics_ready = true;
        // The application may need a reply before the next DA/DSR is parsed.
        false
    }
}

/// Public parser entry points share framing and differ only in their yield policy.
impl Terminal {
    /// Advances VT bytes, consuming and discarding unsupported graphics bodies.
    pub fn advance(&mut self, bytes: &[u8]) {
        if self.parsing.framing.apc.plain(bytes) {
            // Preserve vte's optimized contiguous UTF-8/ASCII dispatch when
            // this slice cannot introduce graphics or finish a pending APC.
            self.advance_vt(bytes);
            return;
        }
        self.filtered(bytes, None, None, false);
    }

    /// Stops at the end of the active synchronized transaction, retaining its suffix.
    pub fn advance_to_sync_end(&mut self, bytes: &[u8]) -> usize {
        // Only an initially active transaction defines a stopping boundary.
        let sync = self.modes.get(true, 2026) == Some(true);
        self.filtered(bytes, None, None, sync).0
    }

    /// Stops at a registered OSC, with complete wire framing and ordered replies.
    pub fn advance_until_osc(
        &mut self,
        bytes: &[u8],
        numbers: &[u16],
        request: &mut OscRequest,
        stop_at_sync_end: bool,
    ) -> (usize, bool) {
        // Graphics framing still runs, but a host not consuming images retains
        // the original behavior of ignoring all unsupported graphics requests.
        let result = self.filtered(bytes, Some((numbers, request)), None, stop_at_sync_end);
        (result.0, result.1)
    }

    /// Yields one graphics or registered OSC request before later application bytes.
    /// Returns (source bytes consumed, OSC ready, graphics ready). Both request
    /// buffers are reused; an overlong or cancelled APC never produces a request.
    pub fn advance_until_graphics(
        &mut self,
        bytes: &[u8],
        numbers: &[u16],
        osc: &mut OscRequest,
        graphics: &mut Vec<u8>,
        stop_at_sync_end: bool,
    ) -> (usize, bool, bool) {
        // Empty OSC interests must not impose raw OSC observation on ordinary output.
        let interests = if numbers.is_empty() {
            None
        } else {
            Some((numbers, osc))
        };
        self.filtered(bytes, interests, Some(graphics), stop_at_sync_end)
    }

    /// Session's writer-credit fence stops after one reply-producing callback.
    /// This borrowed entry returns the exact accepted prefix; the caller keeps
    /// every suffix until its reply can enter the ordered completion lane.
    /// Graphics and optional OSC yields keep their existing independent barriers.
    pub fn advance_to_reply_boundary(
        &mut self,
        bytes: &[u8],
        numbers: &[u16],
        request: &mut OscRequest,
        graphics: Option<&mut Vec<u8>>,
        sync: bool,
    ) -> (usize, bool, bool) {
        // The flag exists only for this bounded parser borrow. Ordinary Terminal
        // APIs never leave a silently unconsumed suffix behind their void return.
        // Query fences share the filter so a reply never skips an earlier APC.
        // The same prefix count drives host credit and parser-buffer consumption.
        self.parsing.reply_boundaries = true;
        let interests = if numbers.is_empty() {
            None
        } else {
            Some((numbers, request))
        };
        let result = self.filtered(bytes, interests, graphics, sync);
        // Restore standalone consumption semantics before returning to the caller.
        // No clock, I/O callback or waiting primitive is introduced into VT.
        self.parsing.reply_boundaries = false;
        result
    }

    /// Temporarily moves the filter so parser callbacks can borrow the terminal.
    fn filtered(
        &mut self,
        bytes: &[u8],
        osc: Option<(&[u16], &mut OscRequest)>,
        graphics: Option<&mut Vec<u8>>,
        sync: bool,
    ) -> (usize, bool, bool) {
        // Most output has no APC introducer. Keep it in one contiguous vte call.
        // Checking first also avoids moving the reusable filter on that hot path.
        if self.parsing.framing.apc.can_skip(bytes) {
            // A synchronous/OSC boundary may leave a suffix; only the accepted
            // prefix can update APC framing for the next parser call.
            let mut sink = Sink {
                terminal: self,
                osc,
                sync,
                graphics,
                osc_ready: false,
                graphics_ready: false,
            };
            let used = sink.passthrough(bytes);
            let ready = sink.osc_ready;
            self.parsing.framing.apc.passed(&bytes[..used]);
            return (used, ready, false);
        }
        // Borrowing the filter and terminal at once would create a self-reference.
        // Moving its small owner value retains every allocated body buffer.
        let mut filter = std::mem::take(&mut self.parsing.framing.apc);
        let mut sink = Sink {
            terminal: self,
            osc,
            sync,
            graphics,
            osc_ready: false,
            graphics_ready: false,
        };
        let used = filter.advance(bytes, &mut sink);
        // Snapshot only ready flags before restoring ownership of the filter.
        let result = (used, sink.osc_ready, sink.graphics_ready);
        self.parsing.framing.apc = filter;
        result
    }
}
