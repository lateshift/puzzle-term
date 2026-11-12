//! Shared §16 notification policy and asynchronous lifecycle (M18.4).
//!
//! OSC origins supply values; this owner alone admits them, assigns correlation
//! tokens and writes reports. A host never parses protocol metadata or waits on
//! Session. Unknown/stale completions are inert, including after RIS or replacement.
//! Ordinary output neither allocates this table nor scans it on frame production.
//!
//! Application identifiers name mutable deliveries. Host tokens instead name one
//! immutable generation: replacement allocates a new token and invalidates the old
//! callback even when the OS delivers dismissal after the new item was posted.
//! The host namespace disambiguates the same application ID in different surfaces.
//! Responses always return through the creating Session's terminal reply buffer.
//!
//! Admission is shared by all notification origins, including API notifications.
//! A denied occasion or rate-limited request cannot remove an existing notification.
//! Explicit close is independent of admission, so a client can always remove work
//! it previously posted without waiting for the one-second delivery allowance.
//! Queue eviction is observable as closure when the client requested close reports.
//!
//! System notifications and harness toasts share state and completion semantics.
//! The harness bypasses native posting but uses the same activation/close handling.
//! Its immutable source is rebuilt only after mutation and shared into snapshots.
//! No expiration timer is invented: the user or application decides the lifetime.
//! RIS invalidates all generations and asks the host to remove their deliveries.
//! A failed native post never becomes an active entry or consumes the rate allowance.
use crate::platform::Notifier;
use puzzle_vt::notification::{MAX_NOTIFICATIONS, MAX_TEXT, Notification, Occasion, Request};
use web_time::Instant;
/// Host work delivered on its UI thread; fields contain no terminal escape sequences.
#[derive(Clone, Debug)]
pub enum Effect {
    /// Post/update a notification. The token is opaque and must accompany feedback.
    Show {
        /// Unique within the creating host surface, including replacements and RIS.
        token: String,
        /// Sanitized title, body and all supported protocol options.
        notification: Notification,
    },
    /// Remove the native notification with this token; later feedback is harmless.
    Close(String),
    /// A user activation requested that the host focus this notification's surface.
    Focus,
}
/// OS or toast outcome, returned asynchronously to the same creating surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Feedback {
    /// User selected the notification body (default action).
    Activated,
    /// Dismissal or an acknowledged explicit close.
    Closed,
    /// Host can display, but cannot track notification closure.
    Untracked,
    /// Native delivery failed; no activation or close report is fabricated.
    Failed,
}
/// One admitted notification, retained until dismissal/replacement or bounded eviction.
struct Active {
    /// Host correlation is not the application's reusable identifier.
    token: String,
    /// Local region identity for presented toast hit testing.
    serial: u64,
    /// Report options belong to this generation, never to a later replacement.
    notification: Notification,
}
/// Session-local mutable state; native handles and clocks remain outside VT.
#[derive(Default)]
pub(crate) struct State {
    /// Host supplies its stable surface namespace before processing any output.
    namespace: u64,
    /// Never reused on replacement or reset, preventing stale report aliasing.
    next: u64,
    /// Only admitted notifications allocate entries; oldest entries evict first.
    active: Vec<Active>,
    /// Last admitted delivery across OSC 9, 777, 99 and API Notify.
    last: Option<Instant>,
    /// RIS invalidates callbacks before processing new events in the same slice.
    epoch: u64,
    /// Latest actual input focus, supplied through the normal host message lane.
    pub(crate) focused: bool,
    /// Hidden/minimized surfaces satisfy the invisible occasion.
    pub(crate) visible: bool,
    /// Harness opts into toasts; native hosts retain system notifications.
    pub(crate) toasts: bool,
    /// Register lazily on the first admitted toast; unused sessions have no extra painter.
    pub(crate) registered: bool,
    /// Only changes rebuild immutable overlay source values (P11).
    pub(crate) dirty: bool,
}
/// Lifecycle operations are serialized by Session; no OS handle is retained.
impl State {
    /// Sets host identity once at cold startup; no foreign pointer becomes a token.
    pub(crate) fn namespace(&mut self, value: u64) {
        self.namespace = value;
    }
    /// Applies one policy-approved request; closing is never delayed by rate limiting.
    pub(crate) fn request(
        &mut self,
        request: Request,
        now: Instant,
        host: &mut dyn Notifier,
        replies: &mut Vec<u8>,
    ) {
        // Close messages are control operations, never new notification admissions.
        let Request::Post(notification) = request else {
            if let Request::Close(id) = request
                && let Some(index) = self
                    .active
                    .iter()
                    .position(|a| a.notification.id.as_ref() == Some(&id))
            {
                // Resolve the reusable ID to its current generation only.
                self.remove(index, host, replies);
            }
            return;
        };
        self.post(notification, now, host, replies);
    }
    /// Applies shared sanitation and admission before touching an existing native delivery.
    fn post(
        &mut self,
        notification: Notification,
        now: Instant,
        host: &mut dyn Notifier,
        replies: &mut Vec<u8>,
    ) {
        // Sanitation and host occasion policy precede replacement side effects.
        let Some(notification) = self.admit(notification, now) else {
            return;
        };
        if let Some(id) = &notification.id {
            // Reusable wire identifiers never survive into a new host generation.
            if !valid_id(id) {
                return;
            }
            if let Some(index) = self
                .active
                .iter()
                .position(|a| a.notification.id.as_ref() == Some(id))
            {
                // Updating is not a close event (kitty: Updating an existing notification).
                // A fresh token makes the old OS dismissal callback stale by construction.
                let previous = self.active.remove(index);
                let _ = host.notification(Effect::Close(previous.token));
            }
        }
        // Arrival order bounds native work even if the host never sends feedback.
        if self.active.len() == MAX_NOTIFICATIONS {
            self.remove(0, host, replies);
        }
        // This counter is not reset by RIS; delayed callbacks cannot alias reset state.
        self.next = self.next.wrapping_add(1);
        // A token carries no title, body or application-provided reply syntax.
        let token = format!("{}:{}", self.namespace, self.next);
        // Toast presentation happens from the immutable source instead of an OS callback.
        if !self.toasts
            && let Err(error) = host.notification(Effect::Show {
                token: token.clone(),
                notification: notification.clone(),
            })
        {
            tracing::debug!(event="notification.delivery_failed", %error);
            return;
        }
        // Only successfully accepted delivery consumes the shared rate allowance.
        self.last = Some(now);
        // Retain report options with the generation that the user will actually activate.
        self.active.push(Active {
            token,
            serial: self.next,
            notification,
        });
        self.dirty = true;
    }
    /// Legacy body-only forms and OSC 99 obey one sanitized, bounded admission policy.
    fn admit(&self, mut notification: Notification, now: Instant) -> Option<Notification> {
        // Shared policy also sanitizes API/interceptor-authored requests.
        notification.title = clean(&notification.title);
        notification.body = clean(&notification.body);
        // OSC 9 supplies only a body. Promote it so every origin has a visible heading.
        if notification.title.is_empty() {
            std::mem::swap(&mut notification.title, &mut notification.body);
        }
        // API-authored requests receive the same closed urgency range as parsed requests.
        notification.urgency = notification.urgency.min(2);
        // Hidden occasions and too-frequent posts have no host side effects.
        if notification.title.is_empty()
            || !self.allowed(notification.occasion)
            || self
                .last
                .is_some_and(|last| now.saturating_duration_since(last).as_secs_f64() < 1.0)
        {
            return None;
        }
        Some(notification)
    }
    /// Occasion is checked against the latest host focus/visibility, not parser time.
    fn allowed(&self, occasion: Occasion) -> bool {
        match occasion {
            Occasion::Always => true,
            Occasion::Unfocused => !self.focused,
            Occasion::Invisible => !self.visible && !self.focused,
        }
    }
    /// Completes only this token; replies stay in the Session's existing writer FIFO.
    pub(crate) fn feedback(
        &mut self,
        token: &str,
        feedback: Feedback,
        host: &mut dyn Notifier,
        replies: &mut Vec<u8>,
    ) {
        // An unknown token includes callbacks for closed, reset or replaced generations.
        let Some(index) = self.active.iter().position(|a| a.token == token) else {
            return;
        };
        // Tracking refusal is not dismissal: explicit removal must remain available.
        if feedback == Feedback::Untracked {
            self.untracked(index, replies);
            return;
        }
        // Remove before reporting so a reentrant duplicate cannot report twice.
        let item = self.active.remove(index);
        let n = &item.notification;
        // Kitty uses zero for an anonymous notification in activation/close replies.
        let id = n.id.as_deref().unwrap_or("0");
        // Only actual user activation can focus a terminal or acknowledge an action.
        if feedback == Feedback::Activated {
            if n.report {
                replies.extend_from_slice(format!("\x1b]99;i={id};\x1b\\").as_bytes());
            }
            if n.focus {
                let _ = host.notification(Effect::Focus);
            }
        }
        // Delivery failure is not a user-visible closure and produces no close report.
        if n.close_report && feedback != Feedback::Failed {
            replies.extend_from_slice(format!("\x1b]99;i={id}:p=close;\x1b\\").as_bytes());
        }
        // Activation dismisses the toast too; a repeated native close cannot report twice.
        if feedback != Feedback::Untracked {
            let _ = host.notification(Effect::Close(item.token));
        }
        self.dirty = true;
    }
    /// Losing close tracking does not lose the ability to explicitly remove or activate.
    fn untracked(&mut self, index: usize, replies: &mut Vec<u8>) {
        // The entry remains active; disable only the close report that cannot be tracked.
        let n = &mut self.active[index].notification;
        if n.close_report {
            let id = n.id.as_deref().unwrap_or("0");
            replies.extend_from_slice(format!("\x1b]99;i={id}:p=close;untracked\x1b\\").as_bytes());
            // The unavailable report is single-use; later closure cannot duplicate it.
            n.close_report = false;
        }
    }
    /// Presented region IDs resolve only to still-active generations.
    pub(crate) fn region_feedback(
        &mut self,
        serial: u64,
        feedback: Feedback,
        host: &mut dyn Notifier,
        replies: &mut Vec<u8>,
    ) {
        // A stale presented region does not resolve after replacement or dismissal.
        if let Some(token) = self
            .active
            .iter()
            .find(|a| a.serial == serial)
            .map(|a| a.token.clone())
        {
            self.feedback(&token, feedback, host, replies);
        }
    }
    /// Close/eviction removes the live generation and emits its requested close report.
    fn remove(&mut self, index: usize, host: &mut dyn Notifier, replies: &mut Vec<u8>) {
        // Reuse the ordinary feedback path for explicit close and bounded eviction.
        let token = self.active[index].token.clone();
        self.feedback(&token, Feedback::Closed, host, replies);
    }
    /// RIS removes native notifications and invalidates old callback generations.
    pub(crate) fn reset(&mut self, epoch: u64, host: &mut dyn Notifier) {
        // Ordinary parser drains compare one integer, with no active-table traversal.
        if self.epoch == epoch {
            return;
        }
        self.epoch = epoch;
        // Reset removes OS work but does not fabricate callbacks to the reset program.
        for item in self.active.drain(..) {
            let _ = host.notification(Effect::Close(item.token));
        }
        self.dirty = true;
    }
    /// Capture only on source mutation; unchanged models merely share this immutable value.
    pub(crate) fn toast_source(
        &self,
    ) -> Option<std::sync::Arc<[puzzle_overlay::models::toast::Toast]>> {
        if !self.toasts || self.active.is_empty() {
            return None;
        }
        // Copy text only when the caller observes dirty state, never per snapshot.
        Some(
            self.active
                .iter()
                .map(|a| puzzle_overlay::models::toast::Toast {
                    serial: a.serial,
                    title: a.notification.title.clone(),
                    body: a.notification.body.clone(),
                    urgency: a.notification.urgency,
                })
                .collect::<Vec<_>>()
                .into(),
        )
    }
}
/// The reply grammar cannot contain controls, separators or an unbounded identifier.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-+.".contains(&b))
}
/// All notification sources obey the same length and control-character policy.
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}
