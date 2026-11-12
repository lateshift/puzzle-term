//! Session-local drop lifetime; only registration allocates this owner.
//! The host captures data eagerly like the reference terminal, so native completion never waits
//! for a terminal program. A later t=r conclusion merely releases held bytes.
//!
//! The registration epoch is not part of State: unregister releases this allocation,
//! but a queued native callback still carries its old epoch and must be rejected.
//! A new registration always gets a new epoch, including after a terminal reset.
//! The client number is different: it is the protocol's multiplexer identifier.
//! It comes from the first registration packet and is echoed on movement/data replies.
//!
//! A hover carries MIME names and geometry, with no bytes or file descriptors.
//! Only Drop installs immutable native representations that a later read can access.
//! The host may release its OS drag objects immediately after this eager capture.
//! Leave and conclusion release all representations while preserving registration.
//! A subsequent hover starts a fresh drag and discards any abandoned previous drop.
//!
//! The complete offer is validated before changing either permissions or reply state.
//! Duplicate MIME names would make the protocol's index ambiguous, so they reject.
//! Control characters and metadata separators cannot appear in MIME names.
//! A maximum representation count bounds both lookup work and native object capture.
//! The aggregate byte bound includes every alternative, even shared byte allocations.
//!
//! Movement reports omit unchanged MIME lists, as required by the wire protocol.
//! Drop always repeats the full list because clients index reads against that report.
//! Coordinates have already been mapped through the presented grid by Session.
//! This model does not consult font caches, snapshot slots or the operating system.
//! Its work therefore occurs only on drag messages and never on text/frame paths.
//! Invalid host input is returned to the caller without admitting a partial gesture.
//! Taking State temporarily avoids shared mutable access while the reply FIFO grows.
//! Restoring State on both success and failure preserves its registration lifetime.

use super::{Effect, Input, MAX_BYTES, MAX_ITEMS};
use crate::{Terminal, clipboard::MimeData, terminal::TerminalEvent};
/// Optional chunk storage and one registered target share one bounded allocation.
#[derive(Default)]
pub(crate) struct State {
    /// Registration identity is echoed on every response to a multiplexed program.
    pub(crate) client: u32,
    /// Complete registration MIME list, empty until the final chunk arrives.
    pub(crate) registered: bool,
    /// Only an explicitly chunked registration/status allocates payload storage.
    pub(crate) partial: Option<(crate::protocols::kitty::dnd::Metadata, Vec<u8>)>,
    /// True only after a native Move; Drop/Leave end movement for that drag.
    hovering: bool,
    /// Offered names permit change-only MIME-list emission on subsequent moves.
    offered: String,
    /// Bytes are retained only after an actual user Drop, never from a Move.
    pub(crate) items: Option<Vec<MimeData>>,
}
impl Terminal {
    /// Unregisters on RIS or trusted child exit, invalidating old host callbacks.
    pub fn clear_drop_target(&mut self) {
        if self.host_protocols.dnd.state.take().is_some() {
            // A host observes disposal even if a reset discards earlier VT facts.
            self.events.push(TerminalEvent::Drop(Effect::Registration {
                generation: self.host_protocols.dnd.generation,
                mimes: None,
            }));
        }
    }
    /// Applies one native gesture on the sole owner. Returns false for stale input.
    /// Invalid names, bounds or hover data never become program-readable bytes.
    pub fn drop_input(&mut self, generation: u64, input: Input) -> Result<bool, String> {
        if generation != self.host_protocols.dnd.generation
            || !self
                .host_protocols
                .dnd
                .state
                .as_ref()
                .is_some_and(|s| s.registered)
        {
            return Ok(false);
        }
        // Take ownership so reply encoding can use the existing terminal FIFO.
        let Some(mut state) = self.host_protocols.dnd.state.take() else {
            return Ok(false);
        };
        // Validation runs while the old state is exclusively owned, before publishing bytes.
        let result = state.input(self, input);
        // Error returns keep the same target lifetime; a later valid gesture can retry.
        self.host_protocols.dnd.state = Some(state);
        result.map(|()| true)
    }
}
impl State {
    /// New completed registration starts with no accepted gesture or retained data.
    pub(crate) fn registered(client: u32) -> Self {
        Self {
            client,
            registered: true,
            ..Default::default()
        }
    }

    /// Validates a complete native observation before changing the active drag.
    fn input(&mut self, term: &mut Terminal, input: Input) -> Result<(), String> {
        let (drop, position, operations, items) = match input {
            Input::Leave => {
                // Only a real hover emits leave, preventing fabricated drag sequences.
                if self.hovering {
                    crate::protocols::kitty::dnd::encode(
                        term,
                        "t=m:x=-1:y=-1",
                        self.client,
                        b"",
                        false,
                    );
                }
                self.clear();
                return Ok(());
            }
            Input::Move {
                position,
                operations,
                items,
            } => (false, position, operations, items),
            Input::Drop {
                position,
                operations,
                items,
            } => (true, position, operations, items),
        };
        validate(&items, drop, operations)?;
        let offered = items
            .iter()
            .map(|i| i.mime.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        // MIME order matters: subsequent reads use one-based indices into this list.
        let changed = offered != self.offered || !self.hovering;
        if !self.hovering {
            self.items = None;
        }
        let [x, y, px, py] = position;
        let kind = if drop { 'M' } else { 'm' };
        let header = format!("t={kind}:x={x}:y={y}:X={px}:Y={py}:o={operations}");
        crate::protocols::kitty::dnd::encode(
            term,
            &header,
            self.client,
            if drop || changed {
                offered.as_bytes()
            } else {
                b""
            },
            false,
        );
        self.offered = offered;
        self.hovering = !drop;
        // A drop owns an immutable copy; following output does not mutate its bytes.
        if drop {
            self.items = Some(items);
        }
        Ok(())
    }
    /// Errors, leave and conclusion release all drag data without unregistering.
    pub(crate) fn clear(&mut self) {
        self.items = None;
        self.hovering = false;
        self.offered.clear();
    }
}
/// Validate all representations atomically; no partial native drop is admitted.
fn validate(items: &[MimeData], drop: bool, operations: u32) -> Result<(), String> {
    if !(1..=3).contains(&operations) || items.len() > MAX_ITEMS {
        return Err("invalid drop operations or MIME count".into());
    }
    let mut bytes = 0usize;
    for (index, item) in items.iter().enumerate() {
        // Whitespace/control/separator names could forge MIME indices or OSC payloads.
        if !crate::protocols::kitty::dnd::valid_mime(&item.mime)
            || items[..index].iter().any(|prior| prior.mime == item.mime)
            || !drop && !item.data.is_empty()
        {
            return Err("invalid or duplicate drop MIME representation".into());
        }
        bytes = bytes.saturating_add(item.data.len());
        if bytes > MAX_BYTES {
            return Err("drop exceeds 64-MiB cap".into());
        }
    }
    Ok(())
}
