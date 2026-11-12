//! C App lifecycle translates to the safe embedding owner without worker callbacks.
//!
//! All output pointers are validated before ownership transfers. Tick first
//! collects owned events, then releases the registry lock before delivering
//! borrowed C buffers. Recursive tick and app-free during delivery are Busy.
//!
//! The process registry is a C capability table, not a Session owner. Each App
//! stays on its creating thread even though its slot is protected by a mutex.
//! Native creation and teardown are cold operations; input always sends values.
//! app_free removes capabilities before dropping the App outside the registry,
//! then App cancellation wakes held child-final-screen and output-pool waits.
//! The render join cannot depend on callback delivery from the UI being blocked.
//!
//! A wakeup function is mandatory because a host must schedule tick after worker
//! events. Its userdata is captured as an opaque address and remains host-owned.
//! No thread invokes the general event callback except the creating thread inside
//! tick. That callback receives borrowed buffers whose lifetime ends on return.
//! Recursive tick, App destruction and native creation are rejected during delivery;
//! ordinary surface input can safely enqueue while the registry is unlocked.
//! A translation failure clears the delivery guard just like normal completion.
//!
//! App settings supply defaults for new surfaces and send snapshots to live ones.
//! Configuration parsing uses the same native adapter and validation schema as the
//! harness, so the C wrapper does not grow a second defaults or coercion policy.
//! Surface creation validates strings and geometry before ownership enters wgpu.
//! The returned ID is published only after Session, pipeline cancellation and
//! render-driver registration have all succeeded; failed startup owns its teardown.
//! A native object remains host-owned, including while a frame is in flight.
//! surface_free's D-56 barrier waits for driver destruction before the host may
//! release that object. The asynchronous release event is informational afterward.
//! String and structure output locations are checked before committing a mutation.
use crate::{
    abi::*,
    memory,
    registry::{self, AppSlot},
};
use puzzle_embed::{App, AppConfig, DesktopSettings};
use std::sync::Arc;

/// Both ordinary and approved launches share native attachment and rollback.
pub(crate) mod creation;

/// C hosts opt into input observations; debug builds also retain frame decisions.
mod trace_probe;

/// The output ID is initialized only after the App owns its render worker.
/// Creation failures release partial native ownership on the caller thread.
///
/// Creates the host-thread App; out receives a generation-checked ID on success.
#[doc = "# Safety"]
/// Options/out must be live readable/writable objects; callback userdata lives until app_free.
#[allow(unsafe_code, reason = "R8/D-43: exported C App creation")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_app_new(options: *const AppOptions, out: *mut u64) -> ResultCode {
    registry::result((|| {
        memory::pointer(out)?;
        trace_probe::install()?;
        // SAFETY: caller supplies an initialized AppOptions; read rejects null/misalignment.
        let options = unsafe { memory::read(options)? };
        if options.abi_version != 1 {
            return Err(ResultCode::InvalidArgument);
        }
        let wake = options
            .callbacks
            .wakeup
            .ok_or(ResultCode::InvalidArgument)?;
        // SAFETY: config path range follows the call-scoped readable input contract.
        let path = unsafe { memory::text(options.config_path)? };
        let (settings, warnings) =
            DesktopSettings::open((!path.is_empty()).then_some(path.as_str()), None, &[])
                .map_err(|_| ResultCode::InvalidArgument)?;
        for warning in warnings {
            tracing::warn!(event = "ffi.settings_warning", %warning);
        }
        let userdata = options.callbacks.userdata as usize;
        let app = App::new(AppConfig {
            settings: settings.clone(),
            wakeup: Arc::new(move || wake(userdata as *mut _)),
        })
        .map_err(ResultCode::from)?;
        let id = registry::lock()?
            .apps
            .insert(AppSlot::new(app, settings, options.callbacks))
            .map_err(|(error, _)| error)?;
        // SAFETY: destination was checked before App creation; caller retains writable storage.
        unsafe { memory::write(out, id) }
    })())
}

/// Native teardown is synchronous; a stale or foreign-thread ID changes nothing.
/// The callback userdata stays live until this cold operation returns.
///
/// Cancels and joins all surfaces; after return native views and userdata can be released.
#[allow(unsafe_code, reason = "D-43: unmangled C lifecycle export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_app_free(id: u64) -> ResultCode {
    registry::result((|| {
        // This guard protects capabilities only; Session work is never performed here.
        let mut registry = registry::lock()?;
        let app = registry.app(id)?;
        if app.delivering {
            return Err(ResultCode::Busy);
        }
        // Retained completion tokens may already refer to freed Surface slots.
        let surfaces: Vec<_> = app.identities.values().copied().collect();
        for surface in surfaces {
            let _ = registry.surfaces.remove(surface);
        }
        let app = registry.apps.remove(id)?;
        // Joining worker teardown cannot run a host callback under the registry lock.
        drop(registry);
        drop(app);
        Ok(())
    })())
}

/// Event data is borrowed only during delivery; callbacks must copy retained data.
/// Busy rejects reentrancy while still permitting nonblocking surface messages.
///
/// Drains queued events on the creating thread; callbacks may enqueue surface input.
#[allow(unsafe_code, reason = "D-43: exported host callback delivery")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_app_tick(id: u64) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let app = registry.app(id)?;
        if app.delivering {
            return Err(ResultCode::Busy);
        }
        // Collection owns every payload before C can reenter a surface operation.
        let mut events = Vec::new();
        app.app
            .tick(|surface, event| events.push((surface, event)))
            .map_err(ResultCode::from)?;
        // A nested tick would invalidate the active callback buffer lifetime.
        app.delivering = true;
        let callback = app.callbacks.event;
        let userdata = app.userdata;
        let events = match crate::events::translate(&mut registry, id, events) {
            Ok(events) => events,
            Err(error) => {
                // A recoverable conversion error must not permanently make the App Busy.
                registry.app(id)?.delivering = false;
                return Err(error);
            }
        };
        // Host callbacks may call surface methods; no registry borrow crosses C.
        drop(registry);
        if let Some(callback) = callback {
            for (surface, event) in events {
                // A prior callback may free a surface in this same tick batch. Its
                // queued geometry/first-frame must never update a replacement pane.
                if event.0 != "surface/released" && registry::lock()?.surface(surface).is_err() {
                    continue;
                }
                // Event backing bytes remain owned until this invocation has returned.
                let borrowed = Event {
                    name: memory::borrowed(event.0.as_bytes()),
                    payload: memory::borrowed(&event.1),
                };
                callback(userdata as *mut _, surface, &borrowed);
                if event.0 == "surface/cursorRect" {
                    // App tick collected it; now the actual C callback returned too.
                    tracing::debug!(target: "puzzle_desktop::caret",
                        event="ffi.caret_callback", surface);
                }
            }
        }
        registry::lock()?.app(id)?.delivering = false;
        Ok(())
    })())
}

/// Returns InvalidArgument for rejected fields; independently valid fields apply.
/// Clamping, unknown-key preservation and defaults come from the shared schema.
///
/// Applies a JSON settings patch through the native schema to every live/future surface.
#[doc = "# Safety"]
/// Patch follows the readable Bytes contract for this call only.
#[allow(unsafe_code, reason = "D-43: C configuration text borrowing")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_app_set_config(id: u64, patch: Bytes) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let app = registry.app(id)?;
        // SAFETY: caller retains the input bytes while this call copies and validates UTF-8.
        let patch = unsafe { memory::text(patch)? };
        // The schema applies independent fields even when another field is rejected.
        let mut settings = app.settings.clone();
        let report = settings
            .apply_json(&patch)
            .map_err(|_| ResultCode::InvalidArgument)?;
        app.app
            .settings(settings.clone())
            .map_err(ResultCode::from)?;
        app.settings = settings.clone();
        // Surface snapshots track app-wide updates without querying a Session.
        for (_, surface) in registry.surfaces.values_mut() {
            if surface.app == id {
                surface.settings = settings.clone();
            }
        }
        if !report.rejected.is_empty() {
            return Err(ResultCode::InvalidArgument);
        }
        Ok(())
    })())
}

/// A successful ID owns cancellation before any asynchronous host event is sent.
/// Native objects remain caller-owned until the synchronous free barrier.
///
/// Attaches a host native view; keep it alive until surface_free or app_free returns.
#[doc = "# Safety"]
/// Options/out must be valid objects; native handles must name live objects of the selected kind.
#[allow(unsafe_code, reason = "R8/D-43: native surface attachment from C")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_new(
    app_id: u64,
    options: *const SurfaceOptions,
    out: *mut u64,
) -> ResultCode {
    // SAFETY: options/out retain the exported call's readable/writable lifetime.
    unsafe { creation::create(app_id, options, out, |_| Ok(())) }
}

/// Success permits immediate native object destruction; later release is a token only.
/// Unlike input, this cold lifecycle call waits for the in-flight render turn.
///
/// Cancels the session and returns only after the native target is released (D-56).
#[allow(unsafe_code, reason = "D-43: checked C surface teardown export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_free(id: u64) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let surface = registry.surface(id)?;
        let app = surface.app;
        // D-56 cancellation precedes the driver-release barrier; no callback is needed.
        let internal = surface.surface.id();
        registry
            .app(app)?
            .app
            .release_surface(internal)
            .map_err(ResultCode::from)?;
        registry.surfaces.remove(id)?;
        Ok(())
    })())
}
