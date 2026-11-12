//! Shared clipboard values and bounded transaction storage, independent of origin.
//! Wire interpretation belongs to kitty; host permissions and I/O belong to Session.
//! Ordinary terminals retain only an empty Option, with no payload allocation.
/// Protocol minimum supported transaction size (kitty requires at least 64 MB).
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
/// A bounded number of alternatives prevents metadata-only memory exhaustion.
pub const MAX_TYPES: usize = 64;
/// One immutable MIME representation, shared cheaply with aliases and host effects.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MimeData {
    /// Validated UTF-8 MIME name, containing no whitespace or controls.
    pub mime: String,
    /// Binary data, never passed through text sanitation.
    pub data: std::sync::Arc<[u8]>,
}
/// A complete request is delivered only after wire validation succeeds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    /// Sanitized multiplexer identity echoed with every response.
    pub id: String,
    /// Primary maps to the same standard clipboard as existing OSC 52.
    pub primary: bool,
    /// Reads carry requested names; writes carry all alternatives atomically.
    pub operation: Operation,
    /// A credential is inert without Session's single-use, gesture-scoped grant.
    pub password: Option<Secret>,
}
/// Host work has a closed shape, without platform objects or callbacks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Operation {
    /// A period requests the MIME listing; other names request binary contents.
    Read(Vec<String>),
    /// An empty list clears the clipboard.
    Write(Vec<MimeData>),
}
/// Protocol-defined outcomes; unsupported MIME operations use Unavailable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    /// The host completed the transaction.
    Done,
    /// The selected clipboard or representation is unavailable.
    Unavailable,
    /// Read/write policy or the host denied permission.
    Denied,
    /// Another clipboard operation is outstanding.
    Busy,
    /// Native clipboard I/O failed.
    Io,
    /// Corrupt metadata or base64 aborted the write.
    Invalid,
    /// Transaction storage exceeded its bound.
    TooLarge,
}
/// An in-flight write is allocated only after an explicit start packet.
#[derive(Default)]
pub(crate) struct Write {
    /// The start packet determines reply identity and target for the whole write.
    pub(crate) id: String,
    /// Location survives data packets that omit their own location field.
    pub(crate) primary: bool,
    /// Completed MIME alternatives, charged once even when aliases share bytes.
    pub(crate) items: Vec<MimeData>,
    /// Alias names resolve at commit, including aliases preceding their target.
    pub(crate) aliases: Vec<(String, String)>,
    /// Current MIME has one continuous base64 stream across packet boundaries.
    pub(crate) mime: String,
    /// Decoded bytes for the current MIME, moved into shared ownership at commit.
    pub(crate) bytes: Vec<u8>,
    /// At most three encoded bytes survive a partial four-symbol group.
    pub(crate) tail: Vec<u8>,
    /// Padding terminates a MIME stream, not an individual data packet.
    pub(crate) padded: bool,
    /// Charge all bytes received, including overwritten representations.
    pub(crate) total: usize,
}

/// Encodes a validated host result in the defining protocol.
pub use crate::protocols::kitty::clipboard::{Response, paste_event, respond};

/// Opaque credential has deliberately redacted diagnostics, including nested requests.
#[derive(Clone, Eq, PartialEq)]
pub struct Secret(
    /// The wire encoder may access the credential; logging must use redacted Debug.
    pub String,
);
impl std::fmt::Debug for Secret {
    /// No Debug formatter may disclose a paste-event credential.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret([redacted])")
    }
}
