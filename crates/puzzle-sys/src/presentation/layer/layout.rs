//! Main-thread physical geometry and hierarchy identity for native presentation.
use super::{CALayer, Retained};
use crate::presentation::Geometry;

/// Observe native bounds for bootstrap and diagnostics, never to veto a host key.
pub(super) fn geometry(layer: &CALayer) -> Option<Geometry> {
    // Before the host supplies geometry, native bounds provide the initial key.
    // Afterward apply_host has already sized this layer from the host pixels;
    // this observation is telemetry only and cannot replace that desired extent.
    // Invalid/minimized bounds are inert and cannot allocate a zero-size image.
    let bounds = layer.bounds();
    let scale = layer.contentsScale();
    let size = [bounds.size.width, bounds.size.height].map(|n| (n * scale).round());
    (scale.is_finite()
        && scale > 0.0
        && size
            .iter()
            .all(|n| n.is_finite() && *n >= 1.0 && *n <= f64::from(u32::MAX)))
    .then(|| Geometry {
        size: size.map(|n| n as u32),
        scale,
    })
}
/// Root identity detects reparenting without borrowing an NSView or window.
/// Only main reads the hierarchy; the number is never dereferenced off-thread.
pub(super) fn binding(layer: &CALayer) -> usize {
    let mut identity = layer as *const CALayer as usize;
    let mut parent = layer.superlayer();
    // The immediate container intentionally survives tab tear-off. Its root,
    // rather than that stable child pointer, identifies the enclosing CA host.
    while let Some(layer) = parent {
        identity = Retained::as_ptr(&layer) as usize;
        parent = layer.superlayer();
    }
    identity
}

/// A host pixel is the unit of authority; division preserves half-points at 2x.
/// This never rounds to a whole logical point or borrows the parent's new size.
pub(super) fn host_bounds(wanted: Geometry) -> objc2_core_foundation::CGRect {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    CGRect::new(
        CGPoint::new(0., 0.),
        CGSize::new(
            f64::from(wanted.size[0]) / wanted.scale,
            f64::from(wanted.size[1]) / wanted.scale,
        ),
    )
}

/// Set bounds/position directly: CALayer's frame/autoresize path rounds points.
/// The parent supplies only the coordinate origin, never an independent extent.
/// Native properties are changed on main, with no mailbox guard held across CA.
pub(super) fn apply_host(layer: &CALayer, wanted: Geometry) {
    // The integer pixel key is copied before entering this helper. No native
    // read can replace it with a parent size from an older/newer transaction.
    let bounds = host_bounds(wanted);
    // Scale and extent belong to the same host observation, including display
    // moves where the logical size stays fixed but the backing density changes.
    if layer.contentsScale() != wanted.scale {
        layer.setContentsScale(wanted.scale);
    }
    // A zero anchor makes position the frame origin. Keeping the default
    // centre anchor would move that origin when only the size changes.
    let anchor = objc2_core_foundation::CGPoint::new(0., 0.);
    if layer.anchorPoint() != anchor {
        layer.setAnchorPoint(anchor);
    }
    // Compare before mutating: ordinary tick must not dirty unchanged CA layout.
    if layer.bounds() != bounds {
        layer.setBounds(bounds);
    }
    // Parent coordinate origins may move during reparenting or view scrolling.
    // Its size is deliberately irrelevant to this authoritative child extent.
    if let Some(parent) = layer.superlayer() {
        let origin = parent.bounds().origin;
        if layer.position() != origin {
            layer.setPosition(origin);
        }
    }
}

/// Pixel-key round trips cannot silently accept logical-point quantisation.
#[cfg(test)]
mod tests;
