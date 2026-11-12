//! Session-owned clipboard permissions and asynchronous correlation, shared by hosts.
//! Wire assembly stays in the kitty origin module; this owner handles policy and I/O.
//! One outstanding request bounds native effects and rejects stale host answers.
//! There are two identities with different purposes: a monotonic transaction
//! number correlates native completion, while a secret authorizes one paste read.
//! Hosts see the former and the expected clipboard revision, never the secret.
//! The grant remains private to this Session and cannot cross a surface boundary.
//! One pending transaction bounds the host callback queue even under hostile OSC.
//! A second request receives EBUSY rather than waiting behind an OS clipboard call.
//! Terminal events submit typed commands so existing interceptors can veto writes
//! or replace MIME representations without moving policy into the native host.
//! The command's token must match the pending transaction before it can issue I/O.
//! A veto leaves issued=false; the drain then sends EPERM to the original client.
//! Posting a host request is not success. Only an Answer produces DONE/data.
//! Stale completions are ignored after reset or a newer transaction begins.
//! The monotonic counter survives reset, so an old answer cannot alias new work.
//! Counter exhaustion refuses further requests rather than recycling identities.
//! Clipboard contents are never interpreted as ordinary Paste on a read completion.
//! The answer is framed by the kitty encoder and queued with other owner replies.
//! Returned alternatives are filtered against the originally authorized read names.
//! A buggy host cannot disclose an extra clipboard representation accidentally.
//! Host and interceptor data are validated again because they bypass wire assembly.
//! Aliases sharing one Arc are charged once; independently allocated data is charged
//! separately even if its bytes happen to be equal.
//! Clipboard generations prevent a change-and-restore race during a paste read.
//! A granted host read must observe the expected generation before and after I/O.
//! Missing or changed generations convert a nominal host success into denial.
//! The ten-second deadline bounds an unresponsive host, not a gesture capability.
//! Gesture grants have no timer and instead expire on explicit ownership events.
//! RIS discards pending host transactions; later callbacks cannot revive them.
//! A public API write uses token zero and has no terminal acknowledgement to send.
//! That path still passes through the same write interceptors and payload bounds.
//! The clipboard service defaults to unsupported on hosts without MIME support.
//! No native clipboard object or blocking synchronization lives in this module.
//! The permission decision is complete before any request reaches native code.
/// Gesture-scoped capability state is independent of asynchronous transaction state.
mod grant;
use crate::{
    messages::{ClipboardContent, Command, Event},
    platform::Clipboard,
};
pub use grant::Paste;
use puzzle_bus::{Bus, Origin};
pub use puzzle_vt::clipboard::{MAX_BYTES, MAX_TYPES, MimeData, Operation, Request, Status};
/// Host completion carries binary representations without treating them as pasted text.
#[derive(Clone, Debug)]
pub struct Answer {
    /// Monotonic per-session token from the request effect.
    pub token: u64,
    /// Revision observed before and after a granted read; None cannot complete a grant.
    pub revision: Option<u64>,
    /// Native completion, including explicit unsupported and permission errors.
    pub status: Status,
    /// Read results only; writes must return an empty list.
    pub items: Vec<MimeData>,
}
/// All native clipboard work has one bounded correlation identity.
#[derive(Clone, Debug)]
pub struct Transaction {
    /// Hosts echo this token; a later operation cannot accept a stale result.
    pub token: u64,
    /// Policy-approved request, with primary mapped consistently with OSC 52.
    pub request: Request,
    /// A granted read must check this revision before and after native clipboard I/O.
    pub expected_revision: Option<u64>,
}
/// Clipboard state is small and allocation-free until a program requests native work.
pub(crate) struct State {
    /// One outstanding operation prevents an unbounded callback queue.
    pending: Option<Transaction>,
    /// Completed read data waits for incremental writer credit, never eager Base64.
    response: Option<puzzle_vt::clipboard::Response>,
    /// Unused until a trusted paste gesture arrives in mode 5522.
    grant: Option<grant::Grant>,
    /// A command veto leaves this false and is answered EPERM after draining policy.
    issued: bool,
    /// Tokens never reuse an earlier request's identity after RIS.
    next: u64,
    /// Existing OSC 52 settings govern both protocol paths.
    pub(crate) allow_read: bool,
    /// Default clipboard writes remain allowed as before.
    pub(crate) allow_write: bool,
    /// Only this reset epoch can invalidate an outstanding native response.
    epoch: u64,
    /// A stalled host cannot retain a transaction forever.
    deadline: Option<web_time::Instant>,
}
impl Default for State {
    /// No OS read is permitted until a trusted setting explicitly enables it.
    fn default() -> Self {
        Self {
            pending: None,
            response: None,
            grant: None,
            issued: false,
            next: 0,
            allow_read: false,
            allow_write: true,
            epoch: 0,
            deadline: None,
        }
    }
}
impl State {
    /// Queues a typed policy request, or replies without reading private host data.
    pub(crate) fn forward(
        &mut self,
        request: Request,
        bus: &mut Bus<Command, Event>,
        replies: &mut Vec<u8>,
    ) {
        // Consume the single-use capability before any native operation can begin.
        let expected_revision = self.consume_grant(&request);
        // Busy refusal also ends a correctly redeemed grant; retry needs another gesture.
        if self.pending.is_some() {
            puzzle_vt::clipboard::respond(&request, Status::Busy, &[], replies);
            return;
        }
        // A valid grant bypasses only read policy, never general write permissions.
        let read = matches!(request.operation, Operation::Read(_));
        if read && !self.allow_read && expected_revision.is_none() || !read && !self.allow_write {
            puzzle_vt::clipboard::respond(&request, Status::Denied, &[], replies);
            return;
        }
        // Exhaustion refuses new work rather than reusing a correlation identity.
        let Some(next) = self.next.checked_add(1) else {
            puzzle_vt::clipboard::respond(&request, Status::Busy, &[], replies);
            return;
        };
        // Retain the new correlation number even if a later interceptor vetoes it.
        self.next = next;
        let command = match &request.operation {
            Operation::Read(_) => Command::ClipboardRead,
            Operation::Write(items) => Command::ClipboardWrite(ClipboardContent::Mime {
                token: self.next,
                items: items.clone(),
            }),
        };
        // Keep the original read names and revision immutable across host completion.
        self.pending = Some(Transaction {
            token: self.next,
            request,
            expected_revision,
        });
        self.issued = false;
        // Terminal origin distinguishes application requests from trusted user gestures.
        bus.submit(command, Origin::Terminal);
    }
    /// Runs only after interceptors; terminal writes share the existing write mask.
    pub(crate) fn issue(
        &mut self,
        command: &Command,
        clipboard: &mut dyn Clipboard,
        replies: &mut Vec<u8>,
    ) {
        if let Command::ClipboardWrite(ClipboardContent::Mime { token: 0, items }) = command {
            issue_api_write(items, clipboard);
            return;
        }
        // Clone Arc payload ownership for the host, not the potentially large byte buffers.
        let Some(mut transaction) = self.pending.clone() else {
            return;
        };
        // Interceptors may rewrite representations, but cannot impersonate a token.
        match command {
            Command::ClipboardWrite(ClipboardContent::Mime { token, items })
                if *token == transaction.token =>
            {
                transaction.request.operation = Operation::Write(items.clone());
            }
            Command::ClipboardRead
                if matches!(transaction.request.operation, Operation::Read(_)) => {}
            _ => return,
        }
        // Middleware cannot bypass the same limits enforced on terminal-originated data.
        if !valid_items(match &transaction.request.operation {
            Operation::Write(items) => items,
            Operation::Read(_) => &[],
        }) {
            self.answer(
                Answer {
                    token: transaction.token,
                    revision: None,
                    status: Status::TooLarge,
                    items: Vec::new(),
                },
                replies,
            );
            return;
        }
        // Issuance only means pending OS work; acknowledgement remains asynchronous.
        self.issued = true;
        if let Err(status) = clipboard.mime(transaction.clone()) {
            self.answer(
                Answer {
                    token: transaction.token,
                    revision: None,
                    status,
                    items: Vec::new(),
                },
                replies,
            );
        }
    }
    /// Validates native data before constructing a potentially large reply stream.
    pub(crate) fn answer(&mut self, answer: Answer, replies: &mut Vec<u8>) {
        // No live request means this completion has already expired or been cancelled.
        let Some(pending) = &self.pending else {
            return;
        };
        // Never consume a current request on an unrelated or duplicate host answer.
        if pending.token != answer.token {
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        // Completion cancels the hang guard regardless of the OS result.
        self.deadline = None;
        let mut status = answer.status;
        // Refuse changed contents even if a host accidentally returns them as success.
        if pending.expected_revision.is_some() && pending.expected_revision != answer.revision {
            status = Status::Denied;
        }
        if !valid_items(&answer.items) {
            status = Status::TooLarge;
        }
        // A host cannot leak extra types beyond the policy-approved read request.
        // Enforce the approved alternatives even when the native host returns extra data.
        let items: Vec<_> = match &pending.request.operation {
            Operation::Read(names) => answer
                .items
                .into_iter()
                .filter(|item| names.contains(&item.mime))
                .collect(),
            Operation::Write(_) => Vec::new(),
        };
        if status == Status::Done && matches!(pending.request.operation, Operation::Read(_)) {
            self.response = Some(puzzle_vt::clipboard::Response::new(
                pending.request,
                status,
                items,
            ));
        } else {
            puzzle_vt::clipboard::respond(&pending.request, status, &items, replies);
        }
    }
    /// Transfers the one validated read result without copying its shared data.
    /// Binary storage already passed policy and size checks before cursor creation.
    /// Only used wire packets spend generic writer credit during continuation.
    /// The response retains its immutable source until its last DATA packet.
    pub(crate) fn take_response(&mut self) -> Option<puzzle_vt::clipboard::Response> {
        self.response.take()
    }
    /// RIS invalidates pending answers; an interceptor veto must still acknowledge denial.
    pub(crate) fn finish(&mut self, epoch: u64, now: web_time::Instant, replies: &mut Vec<u8>) {
        // A reset invalidates the request before timeout or interceptor bookkeeping.
        self.reset(epoch);
        if !self.issued
            && let Some(pending) = self.pending.take()
        {
            puzzle_vt::clipboard::respond(&pending.request, Status::Denied, &[], replies);
        }
        // Arm only after issuance, so policy denial needs no timer or native wake.
        if self.pending.is_some() && self.deadline.is_none() {
            // Ten seconds is a host hang guard, not a frame/input timing budget.
            self.deadline = Some(now + std::time::Duration::from_secs(10));
        }
        // A host that never answers cannot retain correlation/payload state indefinitely.
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            if let Some(pending) = self.pending.take() {
                puzzle_vt::clipboard::respond(&pending.request, Status::Busy, &[], replies);
            }
            self.deadline = None;
        }
    }
    /// A reset ends the protocol transaction, including an already-issued OS effect.
    pub(crate) fn reset(&mut self, epoch: u64) {
        // Epoch changes represent real protocol reset, not a render or theme rebuild.
        if self.epoch != epoch {
            self.epoch = epoch;
            self.pending = None;
            self.deadline = None;
        }
    }
    /// Native wake scheduling belongs to the Session, never the clipboard worker.
    pub(crate) fn deadline(&self) -> Option<web_time::Instant> {
        self.deadline
    }
}

/// Submits a policy-approved API write with no terminal transaction to acknowledge.
fn issue_api_write(items: &[MimeData], clipboard: &mut dyn Clipboard) {
    // Public API writes cross policy but have no program waiting for an OSC reply.
    if valid_items(items) {
        let transaction = Transaction {
            token: 0,
            request: Request {
                id: String::new(),
                primary: false,
                password: None,
                operation: Operation::Write(items.to_vec()),
            },
            expected_revision: None,
        };
        if let Err(status) = clipboard.mime(transaction) {
            tracing::debug!(event = "clipboard.api_write_failed", ?status);
        }
    }
}

/// Host and middleware payloads share the same aggregate bound as the wire decoder.
fn valid_items(items: &[MimeData]) -> bool {
    // Validate caller-owned alternatives before exposing them to a native clipboard.
    let mut bytes = 0usize;
    items.len() <= MAX_TYPES
        && items.iter().enumerate().all(|(index, item)| {
            // Aliases share one allocation, so charge their bytes only once.
            if !items[..index]
                .iter()
                .any(|previous| std::sync::Arc::ptr_eq(&previous.data, &item.data))
            {
                bytes = bytes.saturating_add(item.data.len());
            }
            // MIME names are metadata, not arbitrary terminal text or binary content.
            !item.mime.is_empty()
                && item.mime.len() <= 256
                && item.mime.bytes().all(|b| b.is_ascii_graphic())
                && bytes <= MAX_BYTES
        })
}

/// MIME lists have a metadata bound independent of their eventual binary data size.
fn valid_mimes(names: &[String]) -> bool {
    // Listings are metadata only; their eventual binary sizes are checked on completion.
    names.iter().all(|name| {
        !name.is_empty() && name.len() <= 256 && name.bytes().all(|byte| byte.is_ascii_graphic())
    })
}
