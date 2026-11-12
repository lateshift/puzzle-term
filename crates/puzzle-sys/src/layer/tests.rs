//! D-54 selects a visual corner from effective contents orientation, including ancestors.
use super::*;

#[test]
fn visual_top_left_accounts_for_host_and_layer_flips() -> Result<(), String> {
    // Winit uses an unflipped Metal child of a flipped backing layer. A host
    // may instead supply its backing layer directly, or nest a second flip.
    for host_flipped in [false, true] {
        for layer_flipped in [false, true] {
            let host = CALayer::new();
            host.setGeometryFlipped(host_flipped);
            let layer = objc2_quartz_core::CAMetalLayer::new();
            layer.setGeometryFlipped(layer_flipped);
            host.addSublayer(&layer);
            // Core Animation resolves inherited orientation, not only the leaf's
            // geometryFlipped. Two flips cancel; one reverses gravity's names.
            let effective_flip = host_flipped ^ layer_flipped;
            assert_eq!(layer.contentsAreFlipped(), effective_flip);
            let reading = set_layer(&layer)?;
            let expected = if effective_flip {
                "bottomLeft"
            } else {
                "topLeft"
            };
            assert_eq!(
                layer.contentsGravity().to_string(),
                expected,
                "visual top left for host={host_flipped}, layer={layer_flipped}"
            );
            assert!(reading.top_left);
            assert_eq!(
                reading.corner,
                if effective_flip {
                    GravityCorner::BottomLeft
                } else {
                    GravityCorner::TopLeft
                }
            );
            assert_eq!(reading.geometry_flipped, layer_flipped);
            assert_eq!(reading.contents_flipped, effective_flip);
            assert_eq!(reading.host_geometry_flipped, Some(host_flipped));
            assert_eq!(reading.host_contents_flipped, Some(host_flipped));
            // Selecting an alignment must not flip rendered pixels or the host.
            assert_eq!(host.isGeometryFlipped(), host_flipped);
            assert_eq!(layer.isGeometryFlipped(), layer_flipped);
        }
    }
    Ok(())
}

#[test]
fn visual_corner_is_reselected_after_reparenting() -> Result<(), String> {
    let layer = objc2_quartz_core::CAMetalLayer::new();
    let upright = CALayer::new();
    let flipped = CALayer::new();
    flipped.setGeometryFlipped(true);
    // A window/tab move can replace the host without replacing the Metal layer.
    // Each configuration must observe today's hierarchy, not a cached flip bit.
    for host in [&upright, &flipped, &upright] {
        host.addSublayer(&layer);
        let reading = set_layer(&layer)?;
        assert_eq!(reading.contents_flipped, host.isGeometryFlipped());
        assert_eq!(
            layer.contentsGravity().to_string(),
            if host.isGeometryFlipped() {
                "bottomLeft"
            } else {
                "topLeft"
            }
        );
        assert!(reading.top_left);
    }
    Ok(())
}
