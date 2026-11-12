//! Session-side services enqueue policy-approved effects, never native callbacks.
//!
//! The Session applies security and rate-limit policy before invoking these
//! services. This module does not duplicate that policy or perform native IO.
//! Clipboard requests use the core's existing deferred transaction queue;
//! URLs and notifications carry owned strings into the App's host inbox.
//!
//! A worker can finish a command while the host is inside an OS modal loop.
//! Retaining values here preserves that work without blocking the Session.
//! Only wakeup may run on a worker; actual UI operations happen during tick.
//! Stale surface effects are discarded there before any native view is used.
//! MIME effects carry an owned transaction to the host event loop.
//! The service reports successful enqueueing only, never completed clipboard I/O.
//! The host must send an Answer after its native operation finishes.
use super::{AppEvent, Posted};
use crate::render_thread::surface::SurfaceId;
use puzzle_term::platform::{Clipboard, ClipboardEffect, Notifier, UrlOpener};
use std::{
    collections::VecDeque,
    sync::{Arc, mpsc},
};

/// Session queues transactions rather than invoking OS APIs on its worker.
#[derive(Default)]
pub(super) struct DeferredClipboard(VecDeque<ClipboardEffect>);
impl Clipboard for DeferredClipboard {
    /// MIME work waits for the UI owner; success is acknowledged only by its answer.
    fn mime(
        &mut self,
        transaction: puzzle_term::clipboard::Transaction,
    ) -> Result<(), puzzle_term::clipboard::Status> {
        self.0.push_back(ClipboardEffect::Mime(transaction));
        Ok(())
    }
    /// Text reaches the native clipboard only after the queued policy turn.
    fn write_text(&mut self, text: &str) -> Result<(), String> {
        self.0.push_back(ClipboardEffect::Write(text.into()));
        Ok(())
    }
    /// Both representations travel as one transaction.
    fn write_html(&mut self, html: &str, text: &str) -> Result<(), String> {
        self.0.push_back(ClipboardEffect::WriteHtml {
            html: html.into(),
            text: text.into(),
        });
        Ok(())
    }
    /// The UI supplies actual clipboard text with a later Paste message.
    fn request_text(&mut self) -> Result<Option<String>, String> {
        // None means asynchronous ownership, rather than an empty clipboard string.
        self.0.push_back(ClipboardEffect::Read);
        Ok(None)
    }
    /// Each transaction is moved once, without downcasting or shared ownership.
    fn take_effect(&mut self) -> Option<ClipboardEffect> {
        self.0.pop_front()
    }
}

/// URL and notification transactions carry only owned strings to the host thread.
#[derive(Clone)]
pub(super) struct Effects {
    /// Surface identity is never reused by the App.
    pub(super) id: SurfaceId,
    /// Queue nodes contain no foreign native-window pointer.
    pub(super) posts: mpsc::Sender<Posted>,
    /// This is the only host-supplied function allowed on a worker.
    pub(super) wake: Arc<dyn Fn() + Send + Sync>,
}
impl Effects {
    /// Publish before waking, so an immediately draining host cannot lose this effect.
    pub(super) fn post(&self, event: AppEvent) -> Result<(), String> {
        self.posts
            .send(Posted::Event(self.id, event))
            .map_err(|e| e.to_string())?;
        (self.wake)();
        Ok(())
    }
}
impl UrlOpener for Effects {
    /// Scheme and gesture policy has already run on the Session owner.
    fn open(&mut self, url: &str) -> Result<(), String> {
        self.post(AppEvent::OpenLink(url.into()))
    }
}
impl Notifier for Effects {
    /// UI callbacks receive metadata; protocol replies remain on the Session owner.
    fn notification(&mut self, effect: puzzle_term::notification::Effect) -> Result<(), String> {
        self.post(AppEvent::DesktopNotification(effect))
    }
    /// Rate limiting and control stripping remain core policy, shared with the web.
    fn notify(&mut self, title: &str, body: &str) -> Result<(), String> {
        self.post(AppEvent::Notification {
            title: title.into(),
            body: body.into(),
        })
    }
}
