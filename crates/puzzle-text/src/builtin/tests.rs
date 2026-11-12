//! Procedural glyph tests check boundaries instead of visual taste.

use super::BuiltinCell;
use kurbo::{PathEl, Shape};

/// Adjacent horizontal line cells cover both sides of a shared boundary.
#[test]
fn box_arms_touch_the_cell_edge() -> Result<(), String> {
    let cell = BuiltinCell::new(9, 17, 1)?;
    let ink = cell.ink('─').ok_or("missing light line")?;
    let bounds = ink.path.bounding_box();
    assert_eq!(bounds.x0, 0.0);
    assert_eq!(bounds.x1, 9.0 / 17.0);
    assert!(bounds.y0 <= 0.6 && bounds.y1 >= 0.4);
    Ok(())
}

/// Rounded corners share a substantial radius despite odd stroke placement.
#[test]
fn rounded_corners_have_one_half_cell_radius() -> Result<(), String> {
    // At 1× an even cell width places its one-pixel band off the geometric
    // centre. Mirroring that whole cell once gave different corner shapes.
    for cell in [BuiltinCell::new(8, 19, 1)?, BuiltinCell::new(16, 37, 2)?] {
        let mut radii = Vec::new();
        for ch in ['╭', '╮', '╯', '╰'] {
            let ink = cell.ink(ch).ok_or("rounded corner has no outline")?;
            let (Some(PathEl::LineTo(start)), Some(PathEl::CurveTo(_, _, end))) =
                (ink.path.elements().get(1), ink.path.elements().get(2))
            else {
                return Err("rounded corner lost its outer circular curve".into());
            };
            // The first curve spans the centre-line radius plus half the
            // stroke thickness, in em units scaled back to device pixels.
            let radius =
                (end.x - start.x).abs() * f64::from(cell.height) - f64::from(cell.stroke) / 2.0;
            assert!(radius >= f64::from(cell.width) * 0.35);
            radii.push(radius);
        }
        let smallest = radii.iter().copied().fold(f64::INFINITY, f64::min);
        let largest = radii.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(
            largest - smallest < 1e-6,
            "corner radii diverged: {radii:?}"
        );
    }
    Ok(())
}

/// Every assigned builtin range yields a deterministic Slug outline.
#[test]
fn assigned_ranges_have_paths() -> Result<(), String> {
    let cell = BuiltinCell::new(10, 20, 1)?;
    for code in (0x2500..=0x259f)
        .chain(0x2800..=0x28ff)
        .chain(0x1fb00..=0x1fb3b)
    {
        let ch = char::from_u32(code).ok_or("invalid test codepoint")?;
        assert!(cell.ink(ch).is_some(), "missing U+{code:04X}");
    }
    assert!(!cell.tofu(0x1f600).path.elements().is_empty());
    Ok(())
}

/// Shared parts must describe exactly the old outline, including clipping and zero labels.
#[test]
fn shared_tofu_preserves_complete_diagnostic_contours() -> Result<(), String> {
    for cell in [
        BuiltinCell::new(8, 17, 1)?,
        BuiltinCell::new(10, 20, 1)?,
        BuiltinCell::new(20, 40, 2)?,
    ] {
        for code in [0, 0x378, 0xffff, 0x1ffff, 0x10ffff, u32::MAX] {
            let mut combined = kurbo::BezPath::new();
            for id in cell.tofu_components(code) {
                combined.extend(cell.tofu_component(id)?.path.elements().iter().copied());
            }
            assert_eq!(
                combined,
                cell.tofu(code).path,
                "cell={cell:?} code={code:X}"
            );
        }
    }
    // The flood can address only the fixed border/alphabet set, independent of corpus size.
    let cell = BuiltinCell::new(20, 40, 1)?;
    let keys: std::collections::HashSet<_> = (0x10000..0x10000 + 46607)
        .flat_map(|code| cell.tofu_components(code))
        .collect();
    assert!(keys.len() <= 129);
    Ok(())
}
