//! Latest-wins geometry within an owner turn, with input as an ordering barrier.
//!
//! Reflow, surface pixels and PTY SIGWINCH have independent final values. A frame
//! request only asks for publication after the turn, so it is not a barrier;
//! keys, pointer input and every other mutation are. Sorting three retained
//! messages by their arrival preserves the relationship between grid and pixels.
use super::endpoint::HostMessage;
use puzzle_term::SessionMessage;

/// Three fixed slots replace arbitrarily many queued intermediate native sizes.
#[derive(Default)]
pub(super) struct Batch {
    /// Grid, surface and transport observations retain their last arrival only.
    values: [Option<(usize, HostMessage)>; 3],
    /// Ordering is local to this finite drain, never a persistent identity.
    next: usize,
}
/// Owner-local replacement never allocates or enters the terminal while collecting.
impl Batch {
    /// Non-geometry values return untouched for immediate ordered dispatch.
    pub(super) fn push(&mut self, message: HostMessage) -> Option<HostMessage> {
        // The three families have different effects: surface pixels must not
        // force reflow, and a delayed PTY ioctl must retain its own final size.
        let index = match &message.value {
            SessionMessage::GridResize { .. } => 0,
            SessionMessage::SurfaceSize(_) => 1,
            SessionMessage::PtyResize(_) => 2,
            _ => return Some(message),
        };
        // Replacing a value drops only superseded geometry. Sender clocks on
        // the final message survive; no borrowed Session state crosses this seam.
        self.values[index] = Some((self.next, message));
        self.next += 1;
        None
    }
    /// At most one real reflow and one ioctl are applied between input barriers.
    pub(super) fn flush(&mut self, mut apply: impl FnMut(HostMessage)) {
        if self.next != 0 {
            tracing::trace!(target:"puzzle_desktop::resize",event="embed.geometry_batch",
                received=self.next,applied=self.values.iter().filter(|v|v.is_some()).count());
        }
        // Only three slots participate, so a minimum search needs no allocation
        // or general-purpose sort. The arrival order matters when a newer surface
        // observation follows a grid resize carrying an older backing extent.
        while let Some(index) = self
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.as_ref().map(|(order, _)| (i, *order)))
            .min_by_key(|(_, order)| *order)
            .map(|(i, _)| i)
        {
            if let Some((_, value)) = self.values[index].take() {
                apply(value);
            }
        }
        // Input barriers start a fresh sequence; orders never escape this drain.
        self.next = 0;
    }
}

/// Functional ordering checks use no timing or native resources.
#[cfg(test)]
mod tests;
