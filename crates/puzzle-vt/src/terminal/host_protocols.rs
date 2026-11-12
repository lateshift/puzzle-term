//! Lazy host-facing protocol transactions owned by the pure Terminal.
//! Clipboard, notifications and drops allocate only after their own wire commands.
//! Keeping their optional storage together leaves screen construction independent
//! of how many host protocol families are implemented. No OS handle lives here.
//! Reset still follows each protocol's lifecycle rather than replacing this value:
//! drop generations survive RIS, while incomplete clipboard/notification data does not.
//! Wire interpretation stays in the defining origin modules (D-58).

/// Optional native-intent state; defaults allocate nothing and ordinary output skips it.
#[derive(Default)]
pub(crate) struct HostProtocols {
    /// Clipboard writes own bounded MIME assembly only while a transaction is active.
    pub(crate) clipboard_write: Option<Box<crate::clipboard::Write>>,
    /// Notification chunks retain IDs until completion or the bounded table evicts them.
    pub(crate) notifications: Option<Box<crate::protocols::kitty::notifications::Assembly>>,
    /// A persistent epoch accompanies lazy drag state so old callbacks stay invalid.
    pub(crate) dnd: crate::dnd::Registration,
}
