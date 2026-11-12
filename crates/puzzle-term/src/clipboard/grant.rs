//! A paste gesture authorizes one read of one clipboard revision (§16).
//! There is no expiry timer. Input and mode changes revoke the capability;
//! the host checks its revision again before and after reading native data.
//! The grant is an in-memory capability, not a persistent permission preference.
//! Only a trusted host gesture can create it; command-wire callers cannot.
//! The credential is opaque and redacted whenever a containing value is formatted.
//! A valid read consumes the grant before waiting on host clipboard work.
//! The host must still prove the clipboard generation has not changed.
//! Keeping the mode epoch detects disable/re-enable within one parser slice.
//! The next gesture replaces the previous grant even if the clipboard is unchanged.
//! The ordinary text fallback belongs to Session's existing sanitized paste path.
use super::{MAX_TYPES, MimeData, Operation, Request, State};
use puzzle_vt::clipboard::Secret;

/// Trusted host paste observation, never constructible through the command wire.
#[derive(Clone, Debug)]
pub struct Paste {
    /// Native hosts generate this with the OS random source, never with a counter.
    pub secret: Secret,
    /// Monotonic clipboard change identity, scoped to this host's clipboard location.
    pub revision: u64,
    /// Standard clipboard or primary selection; grants never cross locations.
    pub primary: bool,
    /// Available representations only; listing a type does not disclose its data.
    pub mimes: Vec<String>,
    /// Ordinary paste text, used only if mode 5522 is not enabled on the owner.
    pub text: Option<String>,
}
/// A grant lives solely with the Session that received the user gesture.
pub(super) struct Grant {
    /// Unguessable credential is never included in diagnostic formatting.
    secret: Secret,
    /// A change and restoration still changes the native revision, preventing reuse.
    revision: u64,
    /// A program may not substitute another clipboard location.
    primary: bool,
    /// Reset/re-enable within one feed still invalidates the previous generation.
    epoch: u64,
}
impl State {
    /// Produces the unsolicited MIME listing and retains only one revocable grant.
    pub(crate) fn paste_event(&mut self, paste: Paste, epoch: u64, replies: &mut Vec<u8>) {
        self.grant = None;
        if paste.mimes.len() > MAX_TYPES
            || paste.secret.0.is_empty()
            || paste.secret.0.len() > 128
            || !super::valid_mimes(&paste.mimes)
        {
            return;
        }
        // The wire encoder alone sees the credential; diagnostic paths use Secret's redaction.
        let items = [MimeData {
            mime: ".".into(),
            data: paste.mimes.join(" ").into_bytes().into(),
        }];
        let request = Request {
            id: String::new(),
            primary: paste.primary,
            operation: Operation::Read(vec![".".into()]),
            password: None,
        };
        puzzle_vt::clipboard::paste_event(&request, &paste.secret, &items, replies);
        self.grant = Some(Grant {
            secret: paste.secret,
            revision: paste.revision,
            primary: paste.primary,
            epoch,
        });
    }
    /// A successful credential match consumes the grant before native work is queued.
    pub(super) fn consume_grant(&mut self, request: &Request) -> Option<u64> {
        let grant = self.grant.as_ref()?;
        if !matches!(request.operation, Operation::Read(_))
            || request.primary != grant.primary
            || request.password.as_ref() != Some(&grant.secret)
        {
            return None;
        }
        self.grant.take().map(|grant| grant.revision)
    }
    /// Any other user input ends a grant, even if a keybinding later consumes it.
    pub(crate) fn revoke_paste_grant(&mut self) {
        self.grant = None;
    }
    /// Revocation is structural rather than dependent on a reset's final mode value.
    pub(crate) fn reconcile_grant(&mut self, epoch: u64) {
        if self
            .grant
            .as_ref()
            .is_some_and(|grant| grant.epoch != epoch)
        {
            self.grant = None;
        }
    }
}
