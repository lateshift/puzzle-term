//! Provider-facing immutable path cache (design §11.2).
//!
//! A key identifies one path for this Session. The closure runs only on a miss;
//! strokes and SVG parsing therefore stay off animated build paths. Preparation
//! into device-bounded chunks belongs to the render-owned shared text context.
//! Arc keeps a path alive while a retained view draw references
//! it, without copying its commands or publishing atlas addresses to providers.
use kurbo::BezPath;
use puzzle_slug::ShapeKey;
use std::{cell::Cell, sync::Arc};

/// Immutable outline; GPU addresses remain private to the shared text context.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeRef(pub Arc<BezPath>);

/// Bounded path owners, lazily populated by dirty provider callbacks.
pub struct ShapeCache {
    /// Temporary ownership permits synchronous &ViewCtx callbacks without locks.
    paths: Cell<Option<Vec<(ShapeKey, ShapeRef)>>>,
}

/// Cached outlines are stable until explicitly removed or the Session is dropped.
impl ShapeCache {
    /// Returns a shared path, executing `build` only for an absent key.
    /// Rejects excessive descriptors before retaining untrusted extension geometry.
    pub fn get_or_prepare(
        &self,
        key: ShapeKey,
        build: impl FnOnce() -> BezPath,
    ) -> Result<ShapeRef, String> {
        let Some(mut paths) = self.paths.take() else {
            return Err("recursive overlay path preparation is not supported".into());
        };
        // Restore ownership on every ordinary result, including resource rejection.
        let result = if let Some((_, shape)) = paths.iter().find(|(id, _)| *id == key) {
            // The Arc clone changes ownership only; no path command allocation occurs.
            Ok(shape.clone())
        } else if paths.len() >= 256 {
            Err("overlay path cache reached 256 retained outlines".into())
        } else {
            // Geometry is constructed at the provider's cold boundary, never on a hit.
            let path = build();
            if path.elements().len() > 65_536 {
                Err("overlay outline exceeds 65536 path commands".into())
            } else {
                let shape = ShapeRef(Arc::new(path));
                // Old draw lists may keep this exact outline through later key removal.
                paths.push((key, shape.clone()));
                Ok(shape)
            }
        };
        self.paths.set(Some(paths));
        result
    }

    /// Allows an edited path to reuse a key; old draws keep their immutable source.
    pub fn remove(&mut self, key: ShapeKey) {
        if let Some(paths) = self.paths.get_mut() {
            paths.retain(|(id, _)| *id != key);
        }
    }
}

/// Empty cache is available for its first synchronous provider borrow.
impl Default for ShapeCache {
    fn default() -> Self {
        // Some(empty) differs from None, which denotes an active preparation closure.
        Self {
            paths: Cell::new(Some(Vec::new())),
        }
    }
}
