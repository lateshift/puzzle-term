//! Checked native drop input; the host owns its OS drag session and captures data.
//! Session resolves view-local points and registration generations asynchronously.
//!
//! This API copies no platform drag object into the session thread.
//! The caller supplies owned MIME values captured from a real user gesture.
//! Affinity and liveness are checked before sending the nonblocking owner message.
//! Rejecting all alternatives atomically avoids a partially delivered native drop.
//! The byte allowance is aggregate across alternatives, not a limit per MIME value.
//! Hover values must be empty so advertising a type cannot accidentally disclose data.
//! Registration generations prevent a queued gesture from targeting a replacement.
//! The pure Session performs geometry conversion using the last presented frame.
//! Native hosts decide OS acceptance from the asynchronous program feedback callback.
//! Unregistered sessions keep host-specific paste-path behavior outside this method.

use super::{ApiError, SessionMessage, Surface};
impl Surface {
    /// Forwards a user drag/drop to the creating surface, never waits for the parser.
    /// Move carries names only; Drop holds at most 16 representations / 64 MiB.
    /// Before registration hosts keep their ordinary paste-the-path behavior.
    pub fn drop_input(&self, input: puzzle_term::dnd::Input) -> Result<(), ApiError> {
        self.check()?;
        if input.generation == 0
            || input.kind != puzzle_term::dnd::Kind::Leave && !(1..=3).contains(&input.operations)
            || input.kind == puzzle_term::dnd::Kind::Leave && !input.items.is_empty()
            || input.position.iter().any(|v| !v.is_finite())
            || input.items.len() > puzzle_term::dnd::MAX_ITEMS
        {
            return Err(ApiError::InvalidArgument(
                "invalid drop coordinates or count",
            ));
        }
        let mut bytes = 0usize;
        for (index, item) in input.items.iter().enumerate() {
            bytes = bytes.saturating_add(item.data.len());
            if item.mime.is_empty()
                || item.mime.len() > 256
                || !item.mime.contains('/')
                || !item
                    .mime
                    .bytes()
                    .all(|b| b.is_ascii_graphic() && !b";:".contains(&b))
                || input.items[..index]
                    .iter()
                    .any(|prior| prior.mime == item.mime)
                || bytes > puzzle_term::dnd::MAX_BYTES
                || input.kind == puzzle_term::dnd::Kind::Move && !item.data.is_empty()
            {
                return Err(ApiError::InvalidArgument(
                    "invalid drop representation or size",
                ));
            }
        }
        self.send(SessionMessage::Drop(input))
    }
}
