//! Each shared text resource wakes its own weak session followers on completion.
//!
//! Listeners belong to the text resource and disappear with its last owner.
//! There is no extra global registry. Source drivers retain only the event's
//! Rust name so the core's Send seam never contains a browser object.
use super::*;
use wasm_bindgen::closure::Closure;
/// Resource-owned completion subscription, removed when its text context is released.
pub(super) struct Wake {
    /// Names this resource only, preventing unrelated contexts from receiving damage.
    pub(super) name: String,
    /// The window listener borrows no live Session while font work executes.
    callback: Closure<dyn FnMut(web_sys::Event)>,
}
/// Listener lifetime follows the shared text resource, rather than any single session.
impl Drop for Wake {
    /// A disposed terminal/context never leaves a persistent idle task listener behind.
    fn drop(&mut self) {
        // Last-owner release removes the callback before dropping its JS closure.
        // Pending idle jobs retain a name only, so they cannot keep this listener alive.
        if let Ok(window) = window() {
            let _ = window.remove_event_listener_with_callback(
                &self.name,
                self.callback.as_ref().unchecked_ref(),
            );
        }
    }
}
/// The resource connects completion notifications to its currently attached Sessions.
impl Text {
    /// Keeps only live followers and avoids duplicate wakes when a context is selected again.
    pub(in crate::runtime) fn follow(&self, owner: &Rc<RefCell<super::super::Runtime>>) {
        // Weak ownership lets disposal free Runtime without explicitly unregistering it.
        // This method compares identity only, allowing callers to hold their Runtime borrow.
        let weak = Rc::downgrade(owner);
        let mut followers = self.sessions.borrow_mut();
        // Released terminals and terminals selecting another resource leave no growing list.
        followers.retain(|old| old.strong_count() != 0 && !old.ptr_eq(&weak));
        followers.push(weak);
    }
    /// Installs one cold listener after the Rc identity exists.
    pub(super) fn install_fallback_wake(text: &Rc<Self>) -> Result<(), JsValue> {
        // Rc identity is assigned before installation; live resources cannot share it.
        // The name stays private to this platform service and never enters the wire API.
        let name = format!("puzzle-fallback-{}", Rc::as_ptr(text) as usize);
        let weak = Rc::downgrade(text);
        let callback = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if let Some(text) = weak.upgrade() {
                // Ready work requests rAF even after output stops. Font ownership
                // remains in its lane until the frame's publication budget admits it.
                // Weak followers are local to this text resource, never global session state.
                // Release the follower-list borrow before dispatching any Session work.
                // Platform wake fan-out is cold; normal warm frames allocate no owner list.
                let owners: Vec<_> = text
                    .sessions
                    .borrow()
                    .iter()
                    .filter_map(Weak::upgrade)
                    .collect();
                for owner in owners {
                    {
                        let mut runtime = owner.borrow_mut();
                        // A terminal may still exist after switching contexts or disposal.
                        // An old service must not dirty the replacement font context.
                        if runtime.disposed
                            || !runtime.text.as_ref().is_some_and(|t| Rc::ptr_eq(t, &text))
                        {
                            continue;
                        }
                        // Async results re-enter the synchronous core through the internal bus.
                        // The command requests damage only; font ownership stays in TextContext.
                        runtime.session.submit(
                            puzzle_term::messages::Command::GlyphRasterized,
                            puzzle_term::Origin::Platform,
                        );
                        runtime.session.pump(web_time::Instant::now());
                    }
                    // Scheduling occurs after releasing Runtime, avoiding callback re-entry
                    // under a mutable Session borrow during host notification.
                    crate::runtime::host::turn(&owner);
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        window()?.add_event_listener_with_callback(&name, callback.as_ref().unchecked_ref())?;
        // The resource now owns both the event name and removal guard. If installation
        // failed above, no partially registered driver is supplied to frame building.
        *text.fallback.borrow_mut() = Some(Wake { name, callback });
        Ok(())
    }
}

/// Supplies independent emoji/platform policies without holding JS state in core.
pub(super) fn install_sources(context: &mut TextContext, text: &Text) -> Result<(), JsValue> {
    // Install the listener first so even an immediate discovery completion can request
    // a frame. Source replacement uses the same listener with fresh policy queues.
    let wake = text.fallback.borrow();
    let name = &wake
        .as_ref()
        .ok_or_else(|| error("FontLoadFailed", "fallback wake missing"))?
        .name;
    let (emoji, emoji_stats) = crate::runtime::fallback::idle::source(&text.families, name);
    let (platform, platform_stats) = crate::runtime::fallback::idle::source(&text.families, name);
    // Diagnostics observe only the current policy sources, including after font takeover.
    *text.fallback_stats.borrow_mut() = vec![emoji_stats, platform_stats];
    // The two policies keep separate cluster decisions but the same nonblocking frame contract.
    context
        .set_cluster_sources(Box::new(emoji), Box::new(platform))
        .map_err(|e| error("FontLoadFailed", &e))?;
    context
        .set_preparation_driver(crate::runtime::fallback::loaded::driver(name))
        .map_err(|e| error("FontLoadFailed", &e))
}
