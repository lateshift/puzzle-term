//! One native attachment transaction for ordinary shells and approved producers.
//!
//! Configuration is completed before the native target or child is created.
//! A failed C handle insertion synchronously releases the newly attached surface.
//! No partially configured launch can escape through an asynchronous callback.
//!
//! The registry protects C capabilities, never terminal state. Affinity and the
//! active-tick guard are checked before borrowing arguments or attaching wgpu.
//! A creation callback can adjust owned launch policy but cannot access a Session.
//! This keeps ordinary shell creation and the opt-in producer path identical
//! for geometry, settings defaults, native lifetime and error rollback.
//!
//! Preparation copies strings while the C call owns their readable lifetime.
//! Attachment transfers native ownership to the App before publishing an ID.
//! If capability allocation fails, the synchronous release barrier unwinds the
//! new native target and its workers; no unreachable child can survive failure.
//! Output storage is validated first and is written only after the identity map
//! contains the new surface, so callbacks always have a corresponding C token.
use crate::{
    abi::*,
    handles::Table,
    memory,
    native::Native,
    registry::{self, Registry, SurfaceSlot},
};

/// Checked native identity plus owned launch arguments, not a borrowed C structure.
struct Prepared {
    /// Borrowed host object, retained by the C lifetime contract.
    native: Native,
    /// Copied backing geometry, validated before wgpu attachment.
    geometry: puzzle_embed::SurfaceInfo,
    /// Owned settings passed to the safe embedding construction path.
    options: puzzle_embed::SurfaceOptions,
}

/// Copies call-scoped strings and rejects invalid transport/geometry before spawning.
///
#[doc = "# Safety"]
/// The structure and its buffers remain readable for this call.
#[allow(unsafe_code, reason = "D-43: checked creation input marshalling")]
unsafe fn prepare(raw: *const SurfaceOptions) -> Result<Prepared, ResultCode> {
    // SAFETY: caller guarantees initialized call-scoped options; shape is checked.
    let raw = unsafe { memory::read(raw)? };
    let native = Native::new(raw)?;
    let geometry = crate::native::geometry(raw.geometry)?;
    // SAFETY: shell bytes are live and readable until creation returns.
    let shell = unsafe { memory::text(raw.shell)? };
    // SAFETY: cwd has the same borrowed byte contract as shell.
    let cwd = unsafe { memory::text(raw.cwd)? };
    let transport = match raw.transport {
        0 => puzzle_embed::TransportMode::Pty,
        1 => puzzle_embed::TransportMode::Host,
        _ => return Err(ResultCode::InvalidArgument),
    };
    Ok(Prepared {
        native,
        geometry,
        options: puzzle_embed::SurfaceOptions {
            transport,
            shell: (!shell.is_empty()).then(|| shell.into()),
            cwd: (!cwd.is_empty()).then(|| cwd.into()),
            ..Default::default()
        },
    })
}

impl Prepared {
    /// Installs a native surface and rolls it back if no C capability can be issued.
    fn attach(self, registry: &mut Registry, app_id: u64) -> Result<u64, ResultCode> {
        let app = registry.app(app_id)?;
        // Surface-local patches start from exactly the defaults used at creation.
        let settings = app.settings.clone();
        let surface = self
            .native
            .attach(&mut app.app, self.geometry, self.options)
            .map_err(ResultCode::from)?;
        let internal = surface.id();
        let id = match registry.surfaces.insert(SurfaceSlot {
            app: app_id,
            surface,
            settings,
            markers: Table::new(),
        }) {
            Ok(id) => id,
            Err((error, _)) => {
                // The host cannot close a surface for which it received no handle.
                registry
                    .app(app_id)?
                    .app
                    .release_surface(internal)
                    .map_err(ResultCode::from)?;
                return Err(error);
            }
        };
        // Actor IDs never repeat; C additionally checks the slot generation.
        registry.app(app_id)?.identities.insert(internal, id);
        Ok(id)
    }
}

/// Validates affinity/output first, then configures and attaches one safe Surface.
///
#[doc = "# Safety"]
/// Options and all referenced inputs are readable; out is one writable u64.
#[allow(unsafe_code, reason = "D-43: shared C creation transaction")]
pub(crate) unsafe fn create(
    app_id: u64,
    options: *const SurfaceOptions,
    out: *mut u64,
    configure: impl FnOnce(&mut puzzle_embed::SurfaceOptions) -> Result<(), ResultCode>,
) -> ResultCode {
    registry::result((|| {
        memory::pointer(out)?;
        let mut registry = registry::lock()?;
        if registry.app(app_id)?.delivering {
            return Err(ResultCode::Busy);
        }
        // SAFETY: the exported caller forwards its call-scoped readable contract.
        let mut prepared = unsafe { prepare(options)? };
        // Approval errors cannot leave a native target, process or bootstrap behind.
        configure(&mut prepared.options)?;
        let id = prepared.attach(&mut registry, app_id)?;
        // SAFETY: output was validated before starting any native lifecycle work.
        unsafe { memory::write(out, id) }
    })())
}
