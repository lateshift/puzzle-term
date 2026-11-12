//! Main-thread CALayer subclass: only geometry changes admit the D-60 wait.
//!
//! The existing native Metal layer becomes a retained container. Installing a
//! child preserves host layer identity when panes move; no drawable is acquired.
//! The child uses host physical extents, preserving fractional logical sizes.
//!
//! A native window owns the container bounds; Puzzle owns only this child and
//! its immutable contents. The child inherits the container's effective flipping
//! and selects the visual top-left corner using the existing D-54 orientation
//! rule. Resizing never scales an old image, even after a timeout.
//!
//! Host physical geometry is authoritative, including while CA layout lags.
//! The display callback applies that extent exactly rather than letting CA round it.
//! Scale is forwarded before the transaction, and is part of the presentation
//! key even when the physical extent happens to remain equal.
//!
//! A geometry request is a nonblocking render message. The condition variable
//! protects ready-image ownership, not Session data, and its guard is released
//! before rendering is requested or any Core Animation property is changed.
//! Completion signals it before posting the ordinary event-loop wake. Thus the
//! main thread can commit a finished resize image without first draining tick.
//!
//! The deadline is one observed display period, with the scheduler's existing
//! unknown-rate fallback. No work extends that deadline. A cold glyph build may
//! exceed it; the callback then returns with the previous contents still pinned
//! to the visual top left, and the later completion follows the async route.
/// Explicit sRGB layer backing for newly exposed resize space.
mod background;
/// Native hierarchy and pixel geometry are read without a mailbox guard.
mod layout;
/// Transaction-local background, immutable image adoption and activity receipts.
mod present;
use super::{Geometry, Observe, Request, Shared, State, Target, Wake, pool};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObjectProtocol},
};
use objc2_core_foundation::CGSize;
use objc2_quartz_core::{CAAutoresizingMask, CALayer, CATransaction};
use std::{
    cell::Cell,
    sync::{Arc, Condvar, Mutex},
};

/// All native callback state is owned by the layer, never a borrowed host pointer.
struct Ivars {
    /// Avoid allocating a native colour when the default background is unchanged.
    background: Cell<Option<[f32; 4]>>,
    /// GPU completion publishes immutable images into this bounded mailbox.
    shared: Arc<Shared>,
    /// Only posts coalesced geometry and a render deadline.
    request: Request,
    /// Main-thread evidence and presented-region acknowledgement endpoint.
    observe: Observe,
    /// Actual last committed geometry; a bounds change alone cannot update it.
    committed: Cell<Option<Geometry>>,
    /// Ancestor changes may reverse effective contents orientation without resizing.
    flip: Cell<Option<bool>>,
    /// Only main commits this image; render/GPU completion cannot publish thumbnails.
    snapshot: super::Snapshot,
}
/// The class macro implements Objective-C dispatch; every method remains host-affine.
#[allow(
    unsafe_code,
    reason = "R8/D-60: CALayer subclass with retained Rust callback state"
)]
mod native {
    use super::*;
    define_class!(
        // SAFETY: CALayer permits subclassing display; ivars own their callbacks.
        // MainThreadOnly confines all layer state and contents writes to AppKit.
        #[unsafe(super(CALayer))]
        #[thread_kind = MainThreadOnly]
        #[ivars = Ivars]
        pub(super) struct PresentationLayer;
        // SAFETY: NSObjectProtocol has no additional implementation requirements.
        unsafe impl NSObjectProtocol for PresentationLayer {}
        impl PresentationLayer {
            /// Core Animation calls this inside its transaction after bounds layout.
            // SAFETY: display takes no arguments and returns void, as CALayer declares.
            #[unsafe(method(display))]
            fn display_callback(&self) { self.present(true); }
            /// Parent layout schedules display without CALayer's point rounding.
            // SAFETY: resizeWithOldSuperlayerSize: takes CGSize and returns void.
            #[unsafe(method(resizeWithOldSuperlayerSize:))]
            fn parent_resized(&self, _old: CGSize) {
                // Do not call super: its autoresizing calculation rounds half-points.
                // Display applies the latest host pixel extent in this transaction.
                self.setNeedsDisplay();
            }
            /// No implicit contents/bounds animation may stretch the terminal image.
            // SAFETY: actionForKey: takes an Objective-C object and returns one;
            // the string argument is ignored; NSNull terminates action lookup.
            #[unsafe(method(actionForKey:))]
            fn action(&self, _key: &AnyObject) -> *mut AnyObject {
                // Use an explicit no-action sentinel, including bounds
                // changes initiated outside our own disabled-actions transaction.
                let Some(class) = objc2::runtime::AnyClass::get(c"NSNull") else {
                    return std::ptr::null_mut();
                };
                // The singleton is borrowed, so returning it needs no allocation or retain.
                // SAFETY: Foundation's NSNull null selector takes no arguments
                // and returns its process-lifetime singleton Objective-C object.
                unsafe { msg_send![class, null] }
            }
        }
    );
    impl PresentationLayer {
        /// Initializes the superclass after installing wholly owned callback ivars.
        pub(super) fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(ivars);
            // SAFETY: superclass CALayer's init consumes this allocated instance;
            // its inherited Objective-C signature returns the initialized object.
            unsafe { msg_send![super(this), init] }
        }
    }
}
use native::PresentationLayer;

/// UI-owned attachment; dropping it clears contents and cancels pending callbacks.
pub struct Host {
    /// Retains the subclass through explicit close; native parents may retain it longer.
    layer: Retained<PresentationLayer>,
}
impl Host {
    /// Installs a host-sized child with native resize notifications, without rounding.
    #[allow(
        unsafe_code,
        reason = "R8/D-60: borrow the retained Metal container on main"
    )]
    pub fn attach(
        surface: &wgpu::Surface<'_>,
        scale: f64,
        millihertz: u32,
        latency: u32,
        request: Request,
        wake: Wake,
        observe: Observe,
    ) -> Result<(Self, Target), String> {
        let mtm = MainThreadMarker::new().ok_or("CALayer attachment requires macOS main thread")?;
        // SAFETY: Surface outlives this borrowed HAL guard. The native container
        // is retained, not replaced/destroyed; no drawable or HAL queue is touched.
        let hal = unsafe { surface.as_hal::<wgpu::hal::api::Metal>() }
            .ok_or("IOSurface needs a Metal surface")?;
        // Retain the original layer instead of replacing the host's backing layer.
        // This lets a tab tear-off keep the same pointer and native ownership.
        let parent = hal.render_layer().lock().clone();
        // No HAL mutex is held while creating/attaching Objective-C UI objects.
        drop(hal);
        // This mailbox is the only shared mutable presentation ownership. Neither
        // callback owns a Session or needs to borrow the host App to make progress.
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                retired: std::collections::VecDeque::with_capacity(latency as usize + 1),
                ..State::default()
            }),
            ready: Condvar::new(),
            wake: super::wake::Gate::new(wake),
            scale: std::sync::atomic::AtomicU64::new(scale.to_bits()),
            rate: std::sync::atomic::AtomicU32::new(millihertz),
        });
        let layer = PresentationLayer::new(
            mtm,
            Ivars {
                background: Cell::new(None),
                shared: Arc::clone(&shared),
                request,
                observe,
                committed: Cell::new(None),
                flip: Cell::new(None),
                snapshot: super::Snapshot::default(),
            },
        );
        // The image is anchored inside the existing host coordinate system.
        // Bounds/position preserve fractional points; setFrame/autoresizing may
        // round a 495.5-point pane to 496 and permanently block first presentation.
        layer.setAnchorPoint(objc2_core_foundation::CGPoint::new(0., 0.));
        layer.setBounds(parent.bounds());
        layer.setPosition(parent.bounds().origin);
        layer.setContentsScale(scale);
        // Keep native resize notification, but our override never rounds/sizes it.
        layer.setAutoresizingMask(
            CAAutoresizingMask::LayerWidthSizable | CAAutoresizingMask::LayerHeightSizable,
        );
        // Core Animation must call display in the transaction that changes bounds.
        // A normal later event-loop wake would commit the window without its ink.
        layer.setNeedsDisplayOnBoundsChange(true);
        parent.addSublayer(&layer);
        crate::layer::set_layer(&layer)?;
        Ok((Self { layer }, pool::target(shared, latency)))
    }
    /// Set initial/theme backing inside a disabled-actions main-thread transaction.
    pub fn background(&self, colour: [f32; 4]) {
        self.layer.ivars().shared.lock().background = Some(colour);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.layer.apply_background();
        CATransaction::commit();
    }
    /// Geometry forwarding never waits; only the later CA display callback may wait.
    pub fn geometry(&self, scale: f64, millihertz: u32) {
        self.control().geometry(scale, millihertz);
        if self.layer.contentsScale() != scale {
            self.layer.setContentsScale(scale);
            self.layer.setNeedsDisplay();
        }
    }
    /// A numeric endpoint remains safe to clone through native render mailboxes.
    /// It retains no Objective-C object; closing the UI owner still detaches the
    /// layer immediately even when an old checked Surface capability survives.
    pub fn control(&self) -> super::Control {
        super::Control(Arc::clone(&self.layer.ivars().shared))
    }
    /// A retained immutable image capability contains no Objective-C UI owner.
    pub fn snapshot(&self) -> super::Snapshot {
        self.layer.ivars().snapshot.clone()
    }
    /// Called by App::tick after a GPU wake; unchanged-geometry frames never wait.
    pub fn flush(&self) {
        self.layer.present(false);
    }
    /// Invalidates callbacks before native-target release or a render-thread join.
    #[allow(
        unsafe_code,
        reason = "R8/D-60: main-thread contents reset cancels compositor ownership"
    )]
    pub fn close(&self) {
        // A queued Metal completion may retain Shared after native detach. Wait
        // only for an already admitted host wake, then forbid every later one.
        self.layer.ivars().shared.wake.close();
        {
            let mut state = self.layer.ivars().shared.lock();
            state.closed = true;
            state.ready = None;
        }
        // Wake a geometry waiter before releasing compositor/native ownership.
        // Retained native transactions can call display later, but closed is sticky.
        self.layer.ivars().shared.ready.notify_all();
        // SAFETY: this main-thread layer is live; nil releases its retained contents.
        unsafe { self.layer.setContents(None) };
        self.layer.ivars().shared.lock().current = None;
        self.layer.ivars().snapshot.clear();
        // Detachment prevents a closed pane from drawing again after a tab move.
        // The GPU callback uses weak ownership and cannot resurrect this layer.
        self.layer.removeFromSuperlayer();
    }
}
impl Drop for Host {
    /// A retained layer in a host transaction must become inert even after drop.
    fn drop(&mut self) {
        self.close();
    }
}
