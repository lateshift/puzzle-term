//! Native handle translation; the host retains the actual view through release.
//!
//! wgpu stores an Arc of this value, which owns only opaque handle identities.
//! The C host owns their validity until synchronous surface_free/app_free returns.
//! No render-worker path calls a toolkit method through these handles.
//!
//! The wrapper deliberately owns no Objective-C, Win32 or compositor object.
//! Creating an Arc of numeric identities does not extend those objects' lives;
//! that lifetime comes solely from the C caller and the synchronous free barrier.
//! WindowHandle::borrow_raw is unsafe for this reason, despite the wrapper itself
//! being transferable between threads. No pointer is dereferenced by this module.
//!
//! AppKit uses NSView, Win32 uses HWND, Xlib uses Window plus its Display, and
//! Wayland needs both wl_surface and wl_display. Runtime kind validation prevents
//! a Linux token from accidentally being interpreted as an Objective-C object.
//! Connection pointers are required only for the connection-based native APIs.
//! The Xlib screen index must be nonnegative; absent required objects fail before
//! any GPU surface creation or shell process starts.
//!
//! Geometry is copied independently of native object identity. Width and height
//! are device pixels while scale converts logical input coordinates. Minimization
//! is represented by a zero extent, not a new native target. Resize therefore
//! preserves the text context when font settings and scale have not changed.
//! macOS display numbers fit CGDirectDisplayID and are checked before narrowing.
//! Raw handles remain borrowed through wgpu destruction, including failed startup.
//! Hosts must perform creation and destruction on the App's owning UI thread.
//!
//! A CAMetalLayer is attached directly, unlike an NSView-backed target which
//! lets wgpu select its own backing layer. AppKit pane transfers keep that exact
//! layer alive and only forward new display and geometry observations. The
//! shared App instance supplies the device; native attachment cannot create an
//! independent font system or render worker for each host pane (D-30/D-41).
//! The C boundary establishes the borrowed lifetime before entering safe embed.
//! Closing joins native release before the host may destroy its layer.
use crate::abi::{Geometry, ResultCode, SurfaceOptions};
use puzzle_embed::{Extent, SurfaceInfo};
use std::{num::NonZeroIsize, ptr::NonNull};
use wgpu::rwh::*;

/// Opaque native identities are Send/Sync integers; no toolkit ownership moves.
pub(crate) struct Native {
    /// Supported raw-window-handle family, checked for this build before use.
    kind: u32,
    /// NSView/CAMetalLayer, HWND, Xlib Window or wl_surface identity.
    view: usize,
    /// Xlib/Wayland connection borrowed from the host.
    display: usize,
    /// Xlib screen index associated with that connection.
    screen: i32,
}
impl Native {
    /// Rejects unsupported platforms and null required handles before wgpu sees them.
    pub(crate) fn new(value: SurfaceOptions) -> Result<Self, ResultCode> {
        // A valid token for another OS is still not a native object on this target.
        let supported = match value.native_kind {
            1 | 5 => cfg!(target_os = "macos"),
            2 => cfg!(target_os = "windows"),
            3 | 4 => cfg!(target_os = "linux"),
            _ => false,
        };
        if !supported {
            return Err(ResultCode::Unsupported);
        }
        if value.native_view.is_null()
            || (matches!(value.native_kind, 3 | 4) && value.native_display.is_null())
            || value.native_screen < 0
        {
            return Err(ResultCode::InvalidArgument);
        }
        Ok(Self {
            kind: value.native_kind,
            view: value.native_view as usize,
            display: value.native_display as usize,
            screen: value.native_screen,
        })
    }
    /// Creates a borrowed layer target on the App's shared instance, never a second device.
    #[allow(
        unsafe_code,
        reason = "D-43/D-56: host CAMetalLayer lifetime is the C attachment contract"
    )]
    pub(crate) fn attach(
        self,
        app: &mut puzzle_embed::App,
        info: SurfaceInfo,
        options: puzzle_embed::SurfaceOptions,
    ) -> Result<puzzle_embed::Surface, puzzle_embed::ApiError> {
        // Attached layers start drawable; host visibility updates suspend inactive tabs.
        let visibility = puzzle_embed::Visibility {
            visible: Some(true),
            ..Default::default()
        };
        #[cfg(target_os = "macos")]
        if self.kind == 5 {
            return app.create_surface_from(
                |instance| {
                    // SAFETY: C supplies a live CAMetalLayer retained until synchronous
                    // surface_free/app_free returns; wgpu owns drawable configuration only.
                    unsafe {
                        instance.create_surface_unsafe(
                            wgpu::SurfaceTargetUnsafe::CoreAnimationLayer(self.view as *mut _),
                        )
                    }
                    .map_err(|error| error.to_string())
                },
                info,
                visibility,
                options,
            );
        }
        app.create_surface(std::sync::Arc::new(self), info, visibility, options)
    }
}
impl HasWindowHandle for Native {
    /// Borrows an object whose lifetime is guaranteed by the C attachment contract.
    #[allow(unsafe_code, reason = "R8/D-43: native C handle borrowed for wgpu")]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = match self.kind {
            1 => RawWindowHandle::AppKit(AppKitWindowHandle::new(
                NonNull::new(self.view as *mut _).ok_or(HandleError::Unavailable)?,
            )),
            2 => RawWindowHandle::Win32(Win32WindowHandle::new(
                NonZeroIsize::new(self.view as isize).ok_or(HandleError::Unavailable)?,
            )),
            3 => RawWindowHandle::Xlib(XlibWindowHandle::new(self.view as _)),
            4 => RawWindowHandle::Wayland(WaylandWindowHandle::new(
                NonNull::new(self.view as *mut _).ok_or(HandleError::Unavailable)?,
            )),
            _ => return Err(HandleError::NotSupported),
        };
        // SAFETY: host retains a valid native object until synchronous render teardown returns.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}
impl HasDisplayHandle for Native {
    /// Display connections follow the same host-owned native lifetime as the view.
    #[allow(
        unsafe_code,
        reason = "D-43: native connection borrowing at the C boundary"
    )]
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = match self.kind {
            1 => RawDisplayHandle::AppKit(AppKitDisplayHandle::new()),
            2 => RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
            3 => RawDisplayHandle::Xlib(XlibDisplayHandle::new(
                NonNull::new(self.display as *mut _),
                self.screen,
            )),
            4 => RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
                NonNull::new(self.display as *mut _).ok_or(HandleError::Unavailable)?,
            )),
            _ => return Err(HandleError::NotSupported),
        };
        // SAFETY: retained C connection is valid for every use of the attached surface.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}
/// Geometry is validated before an attachment or resize can divide by scale.
pub(crate) fn geometry(value: Geometry) -> Result<SurfaceInfo, ResultCode> {
    if !value.scale.is_finite()
        || value.scale <= 0.0
        || value.scale > f64::from(f32::MAX)
        || value.display > u64::from(u32::MAX)
    {
        return Err(ResultCode::InvalidArgument);
    }
    Ok(SurfaceInfo {
        extent: Extent {
            width: value.width,
            height: value.height,
        },
        scale: value.scale,
        display: value.display,
        millihertz: value.millihertz,
    })
}
