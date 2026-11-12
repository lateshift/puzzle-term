//! Bounded declarations and one-time SVG parsing at the API boundary (§16).
//!
//! No provider parses or validates strings on a clean frame. Mutation errors
//! leave the old declaration intact, and numeric validation prevents NaN from
//! reaching affine shader inputs. Bounds account for each retained definition.
//!
//! Text, tooltip and icon limits are deliberately distinct: a short gutter
//! label must not become a large unbounded panel. Control characters cannot
//! introduce another line or an application escape sequence in a label.
//! SVG has its own byte and element limits before it becomes an Arc path.
//!
//! All validation runs before private VT witness handles are registered.
//! A malformed replacement consequently needs no rollback of tracked slots.
//! Numeric validation rejects nonfinite sizes, radii and affine coefficients
//! before the vector compiler can copy them into render instances.
//!
//! Glyph opacity is finite here; the shared effect compiler performs the
//! documented 0..1 clamp. Style classes use the same 0..3 catalogue as grid
//! text, and optional face IDs are resolved by the shared text context.
//! Parsing SVG once avoids rediscovering syntax errors on every scroll frame.
//! Fixed viewport anchors receive the same finite-coordinate validation as affines.
//! SVG whitespace remains legal even though single-line label controls are rejected.
//! Validating the complete replacement first preserves the previous live definition.
use puzzle_overlay::{decoration::*, draw::Effect, shapes::ShapeRef};
use puzzle_text::geometry::{Affine, BezPath};
use std::sync::Arc;
/// Validates one declaration and returns its optional immutable SVG source.
pub(super) fn definition(value: &Decoration) -> Result<Option<ShapeRef>, String> {
    if let puzzle_overlay::anchor::Anchor::Viewport(rect) = value.anchor {
        // Fixed screen anchors are untrusted too; reject empty-negative or nonfinite geometry.
        if rect
            .origin
            .iter()
            .chain(&rect.size)
            .any(|v| !v.is_finite() || v.abs() > 1e6)
            || rect.size.iter().any(|v| *v < 0.0)
        {
            return Err("invalid decoration viewport rectangle".into());
        }
    }
    if let Some(RegionSpec::Tooltip(text)) = &value.region {
        bounded(text, 4096)?;
    }
    match &value.kind {
        DecorationKind::InlineText { text, style, .. } => {
            bounded(text, 4096)?;
            if !style.size.is_finite() || !(1.0..=200.0).contains(&style.size) || style.style > 3 {
                return Err("invalid decoration text size or style".into());
            }
        }
        DecorationKind::GutterBadge { text, .. } => bounded(text, 256)?,
        DecorationKind::Box { radius, .. } if !radius.is_finite() || *radius < 0.0 => {
            return Err("invalid decoration corner radius".into());
        }
        DecorationKind::Shape { svg_path, fit, .. } => {
            // SVG whitespace includes tabs/newlines; its parser validates syntax separately.
            if svg_path.len() > 65536 {
                return Err("SVG exceeds 65536 bytes".into());
            }
            if let ShapeFit::Custom(matrix) = fit {
                affine(*matrix)?;
            }
            // Parse before creating a lifetime witness or publishing a replacement.
            let path = BezPath::from_svg(svg_path).map_err(|e| e.to_string())?;
            if path.elements().len() > 4096 {
                return Err("SVG exceeds 4096 elements".into());
            }
            return Ok(Some(ShapeRef(Arc::new(path))));
        }
        DecorationKind::GlyphEffect(Effect {
            opacity, transform, ..
        }) => {
            affine(*transform)?;
            if !opacity.is_finite() {
                return Err("nonfinite decoration opacity".into());
            }
        }
        _ => {}
    }
    Ok(None)
}
/// Strings are single-line annotations with explicit per-definition limits.
fn bounded(text: &str, limit: usize) -> Result<(), String> {
    if text.len() > limit || text.chars().any(char::is_control) {
        return Err(format!(
            "annotation exceeds {limit} bytes or contains controls"
        ));
    }
    Ok(())
}
/// All affine coefficients must be representable in shader coordinate space.
fn affine(matrix: Affine) -> Result<(), String> {
    if matrix
        .as_coeffs()
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1e6)
    {
        return Err("invalid decoration transform".into());
    }
    Ok(())
}
