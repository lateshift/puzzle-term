//! Checked message capabilities for one native surface (D-43, D-50).
//!
//! Clones expose no VT, GPU, native window or mutable model reference. Thread
//! affinity and closed-state checks run before submission or pixel access.
//! The thumbnail capability only pins the immutable image already presented;
//! it grants no rendering authority and is cleared before native teardown.
//!
//! Input order belongs to the Session control inbox, not to App::tick. A
//! host sends a key immediately even if it has not yet consumed title or
//! rendering notifications. Queueing never borrows a snapshot or waits for
//! a text context, so the GPU cannot hold a key behind a drawable acquisition.
//!
//! Origin is assigned by the API operation. Generic dispatch is Api; a key,
//! committed text or paste gesture is User; focus is a Platform observation.
//! These distinctions preserve the same interceptor/security behavior that
//! the native harness used before it became an embedding host.
//!
//! The validity flag is lifecycle state only. It contains no Session data,
//! and its sole transition is live to closed. App identities are never reused,
//! so a queued result or retained clone cannot silently target another view.
//! Native resources remain owned by App and its workers until joined teardown.
//!
//! Geometry validates scale before queueing. A zero extent is a legitimate
//! minimized surface, whose configure is deferred by the existing driver.
//! No host operation mutates font caches or performs terminal history work.
use super::*;
/// Hosts acknowledge seen output without querying worker-owned state.
mod activity;
/// Annotations and markers are owned messages with asynchronous identity results.
mod decorations;
/// Native drop gestures copy MIME data into the same Session control inbox.
mod dnd;
/// Native callbacks enter through typed surface methods, never generic actor mailboxes.
mod input;
/// Host UI exclusions are copied into the Session presentation model.
mod occlusion;
/// Optional protocols enter through the same checked host capability.
mod osc;
/// Host-supplied physical refreshes share the native render actor's pacing seam.
mod refresh;
/// Per-surface query UI and asynchronous selection-text reads.
mod search;
/// Presented-image copies never submit work to a Session or renderer.
mod snapshot;
use crate::{
    render_thread::surface::{SurfaceInfo, SurfaceUpdate},
    session_thread::Handle,
};
use puzzle_term::{
    Origin, SessionMessage,
    messages::{Command, PreeditState},
    search::SearchCommand,
};
pub use snapshot::{SnapshotError, SnapshotSize};
use std::sync::atomic::{AtomicBool, Ordering};

/// Host-thread endpoint; a stale clone remains invalid after its App closes the surface.
#[derive(Clone)]
pub struct Surface {
    /// The App never reuses this identity, even after startup failure.
    pub(super) id: SurfaceId,
    /// Runtime checking also protects the future C translation boundary.
    pub(super) owner: ThreadId,
    /// Invalidated before cancellation so no new work can follow close.
    pub(super) valid: Arc<AtomicBool>,
    /// Only the Session owner can execute commands from this capability.
    pub(super) session: Handle,
    /// Present only for a host source; Session never locks this admission capability.
    pub(super) feed: Option<super::transport::Feed>,
    /// Render updates are values merged by the actor's existing mailbox.
    pub(super) render: SurfaceHandle,
    /// Lifecycle requests are executed on App::tick, keeping native teardown host-affine.
    pub(super) posts: mpsc::Sender<Posted>,
    /// A posted close must wake an idle host just like a worker event.
    pub(super) wake: Arc<dyn Fn() + Send + Sync>,
    /// Installed once at native attachment; clones share only immutable front ownership.
    #[cfg(target_os = "macos")]
    pub(super) snapshot: Arc<std::sync::OnceLock<puzzle_sys::presentation::Snapshot>>,
}
impl Surface {
    /// Stable identity for associating App::tick events with the host's own view.
    pub fn id(&self) -> SurfaceId {
        self.id
    }
    /// Rejects foreign-thread or closed-surface work before it can reach either owner.
    pub(super) fn check(&self) -> Result<(), ApiError> {
        if thread::current().id() != self.owner {
            return Err(ApiError::WrongThread);
        }
        if !self.valid.load(Ordering::Acquire) {
            return Err(ApiError::Closed);
        }
        Ok(())
    }
    /// Lifecycle invalidation cannot be reversed by a retained host clone.
    pub(super) fn invalidate(&self) {
        self.valid.store(false, Ordering::Release);
    }
    /// Sends a typed core message without invoking policy on the host thread.
    pub(crate) fn send(&self, message: SessionMessage) -> Result<(), ApiError> {
        self.check()?;
        // Surface identity accompanies optional clocks, never user input contents.
        if let Some(mut timing) = crate::InputTiming::capture() {
            timing.surface = Some(self.id.0);
            return self
                .session
                .send_timed(message, timing)
                .map_err(ApiError::Platform);
        }
        // Thread/lifetime checks precede even allocation of a control-channel node.
        self.session.send(message).map_err(ApiError::Platform)
    }
    /// Coalesces surface facts without waiting for a GPU frame or Session turn.
    pub(crate) fn update(&self, update: SurfaceUpdate) -> Result<(), ApiError> {
        self.check()?;
        // The actor merges latest geometry and independent damage in a reusable mailbox.
        self.render.update(update);
        Ok(())
    }
    /// Sets dim suggestion ink outside VT; None clears it without sending input.
    pub fn set_ghost_text(&self, text: Option<String>) -> Result<(), ApiError> {
        self.send(SessionMessage::GhostText(text))
    }
    /// Explicitly accepts a suggestion through the Session's committed-input policies.
    pub fn accept_ghost_text(&self) -> Result<(), ApiError> {
        self.send(SessionMessage::AcceptGhostText)
    }
    /// Shows coarse completed-frame statistics; hidden surfaces add no measurement work.
    pub fn set_performance_hud(&self, shown: bool) -> Result<(), ApiError> {
        self.send(SessionMessage::PerformanceHud(shown))
    }
    /// Enables the cross-platform in-window notification UI used by the harness.
    pub fn notification_toasts(&self) -> Result<(), ApiError> {
        self.send(SessionMessage::NotificationToasts)
    }
    /// Returns native activation/dismissal without executing protocol logic in the host.
    pub fn notification_feedback(
        &self,
        token: String,
        outcome: crate::NotificationFeedback,
    ) -> Result<(), ApiError> {
        if token.len() > 64 {
            return Err(ApiError::InvalidArgument("notification token too long"));
        }
        self.send(SessionMessage::NotificationFeedback(token, outcome))
    }
    /// Closes asynchronously; retained clones reject further input immediately.
    pub fn close(&self) -> Result<(), ApiError> {
        self.check()?;
        self.invalidate();
        // Posting retains no native view; App owns cancellation and eventual joins.
        self.posts
            .send(Posted::Close(self.id))
            .map_err(|e| ApiError::Platform(e.to_string()))?;
        (self.wake)();
        Ok(())
    }
    /// API-dispatched commands retain policy and cannot forge user or terminal origin.
    pub fn dispatch(&self, command: Command) -> Result<(), ApiError> {
        self.send(SessionMessage::Command(command, Origin::Api))
    }
    /// Physical/layout keys and committed text are routed by the current Session input mode.
    pub fn key(&self, key: puzzle_term::vt::input::KeyEvent) -> Result<(), ApiError> {
        // Search and query-field focus is decided by Session at application time.
        self.send(SessionMessage::NativeKey(key, Origin::User))
    }
    /// Committed IME text enters normal core policy, distinct from provisional composition.
    pub fn text(&self, text: String) -> Result<(), ApiError> {
        self.send(SessionMessage::Command(Command::Text(text), Origin::User))
    }
    /// The caret is a UTF-8 byte offset; the Session owns provisional input mode state.
    pub fn preedit(&self, state: PreeditState) -> Result<(), ApiError> {
        self.send(SessionMessage::Command(
            Command::Preedit(state),
            Origin::User,
        ))
    }
    /// Clipboard answers and paste gestures share sanitization and bracketed-paste handling.
    pub fn paste(&self, text: String) -> Result<(), ApiError> {
        // Sanitization and bracketed-paste encoding remain core operations.
        self.send(SessionMessage::Command(Command::Paste(text), Origin::User))
    }
    /// Resize and display/scale changes keep only the latest host observation.
    pub fn resize(&self, info: SurfaceInfo) -> Result<(), ApiError> {
        let _stage = crate::resize_timing::Stage::new("main_surface_resize");
        tracing::trace!(target:"puzzle_desktop::resize",event="embed.resize_requested",?info);
        self.check()?;
        validate(info)?;
        self.update(SurfaceUpdate::Geometry(info))
    }
    /// Search navigation runs with the current model on its exclusive owner.
    pub fn search_next(&self) -> Result<(), ApiError> {
        self.dispatch(Command::ScrollTo(
            puzzle_term::viewport::ScrollTarget::Match(1),
        ))
    }
    /// Previous-match navigation uses the same incremental query results as the built-in bar.
    pub fn search_prev(&self) -> Result<(), ApiError> {
        self.dispatch(Command::ScrollTo(
            puzzle_term::viewport::ScrollTarget::Match(-1),
        ))
    }
    /// Focus changes are messages, never synchronous reads of terminal state.
    pub fn focus(&self, focused: bool) -> Result<(), ApiError> {
        self.send(SessionMessage::Command(
            Command::Focus(focused),
            Origin::Platform,
        ))
    }
    /// Hidden and occluded observations stop refresh subscriptions through the existing driver.
    pub fn visibility(
        &self,
        visible: crate::render_thread::host::Visibility,
    ) -> Result<(), ApiError> {
        self.send(SessionMessage::NotificationVisibility(
            visible.visible != Some(false) && visible.occluded != Some(true),
        ))?;
        self.update(SurfaceUpdate::Visibility(visible))
    }
    /// Search is incremental on the Session owner; no history scan occurs on the host.
    /// App::tick delivers HostEvent::SearchState for changed counts and completion.
    pub fn search(&self, search: SearchCommand) -> Result<(), ApiError> {
        self.dispatch(Command::Search(search))
    }
    /// Releases matches on their owner and publishes the empty count to the host.
    pub fn search_clear(&self) -> Result<(), ApiError> {
        self.search(SearchCommand::Clear)
    }
    /// Keys keep their current binding policy while font/GPU changes run on rendering.
    pub fn settings(&self, settings: DesktopSettings) -> Result<(), ApiError> {
        self.check()?;
        self.session
            .input_policy(settings.store.effective().option_as_alt())?;
        // Input policy is submitted before render-side font work is requested.
        // Later layout consumes only measurements; no host waits for a reflow.
        self.send(SessionMessage::Keybindings(settings.store.keybindings()))?;
        self.send(SessionMessage::Settings(Box::new(
            settings.store.effective().clone(),
        )))?;
        self.update(SurfaceUpdate::Settings(Box::new(settings)))
    }
}

/// Zero extents mean minimized; scale must remain finite and physically meaningful.
pub(super) fn validate(info: SurfaceInfo) -> Result<(), ApiError> {
    if !info.scale.is_finite() || info.scale <= 0.0 || info.scale > f64::from(f32::MAX) {
        return Err(ApiError::InvalidArgument(
            "scale must be finite and positive",
        ));
    }
    Ok(())
}

/// Host pointer coordinates have one library-owned geometry conversion.
mod pointer;
pub use pointer::SurfacePointer;
