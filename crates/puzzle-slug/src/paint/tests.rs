//! Gradient semantics and bounded generational paint-store contracts (§9.5).
#![allow(
    clippy::unwrap_used,
    reason = "small authored paint fixtures must fail loudly"
)]
use super::*;
/// Transparent/opaque interpolation must remain premultiplied in sRGB.
fn gradient(brush: Brush, extend: Extend) -> Paint {
    Paint {
        brush,
        foreground: false,
        inverse: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        extend,
        stops: vec![
            Stop {
                offset: 0.0,
                color: [1.0, 0.0, 0.0, 1.0],
                foreground: false,
            },
            Stop {
                offset: 1.0,
                color: [0.0; 4],
                foreground: false,
            },
        ],
        clips: vec![],
    }
}
#[test]
fn color_line_and_geometry_contract() {
    let linear = Brush::Linear {
        p0: [0.0; 2],
        p1: [1.0, 0.0],
    };
    for (extend, x, expected) in [
        (Extend::Pad, -0.25, 1.0),
        (Extend::Pad, 1.25, 0.0),
        (Extend::Repeat, -0.25, 0.25),
        (Extend::Reflect, -0.25, 0.75),
        (Extend::Reflect, 1.25, 0.25),
    ] {
        let c = gradient(linear.clone(), extend).sample([x, 0.0]);
        assert_eq!(c, [expected, 0.0, 0.0, expected]);
    }
    let radial = gradient(
        Brush::Radial {
            c0: [0.0; 2],
            c1: [0.0; 2],
            radii: [0.0, 1.0],
        },
        Extend::Pad,
    );
    assert_eq!(radial.sample([0.5, 0.0]), [0.5, 0.0, 0.0, 0.5]);
    let sweep = gradient(
        Brush::Sweep {
            center: [0.0; 2],
            angles: [0.0, 360.0],
        },
        Extend::Pad,
    );
    assert_eq!(sweep.sample([0.0, -1.0]), [0.75, 0.0, 0.0, 0.75]);
    let mut transformed = gradient(linear, Extend::Pad);
    transformed.inverse = [2.0, 0.0, 0.0, 2.0, -1.0, 0.0];
    assert_eq!(transformed.sample([0.75, 0.0]), [0.5, 0.0, 0.0, 0.5]);
    // Duplicate stops select the last exact offset; zero-width repeat is empty.
    transformed.stops[1].offset = 0.0;
    assert_eq!(transformed.sample([0.5, 0.0]), [0.0; 4]);
    transformed.extend = Extend::Repeat;
    assert_eq!(transformed.sample([0.0, 0.0]), [0.0; 4]);
}
#[test]
fn paint_growth_eviction_restore_contract() {
    let p = gradient(
        Brush::Linear {
            p0: [0.0; 2],
            p1: [1.0, 0.0],
        },
        Extend::Pad,
    );
    let mut table = table::PaintTable::new(4 * 4096);
    let old = table.prepare(1, &p, 0).unwrap();
    let survivor = table.prepare(2, &p, 0).unwrap();
    table.prepare(3, &p, 0).unwrap();
    assert_eq!(table.allocated_bytes(), 4 * 4096);
    let initial = table.take_dirty();
    assert_eq!(initial.len(), 3);
    assert!(table.get(2, 700).is_some());
    let new = table.prepare(4, &p, 700).unwrap();
    assert!(!table.contains(old));
    assert!(!table.contains(survivor));
    assert!(table.contains(new));
    assert!(table.get(1, 700).is_none());
    let compacted = table.take_dirty();
    assert_eq!(compacted.len(), 2);
    // Device restoration reproduces the same addresses and exact row bytes.
    table.restore();
    assert_eq!(compacted, table.take_dirty());
    let mut too_many = p;
    too_many.stops.resize(17, too_many.stops[0]);
    assert!(table.prepare(5, &too_many, 700).is_err());
}

/// A live insertion cannot relocate addresses after instances have captured them.
#[test]
fn live_eviction_precedes_instance_capture() {
    let p = gradient(
        Brush::Linear {
            p0: [0.0; 2],
            p1: [1.0, 0.0],
        },
        Extend::Pad,
    );
    let mut table = table::PaintTable::new(4 * 4096);
    let cold = table.prepare_live(1, &p, 0).unwrap();
    let warm = table.prepare_live(2, &p, 0).unwrap();
    table.prepare_live(4, &p, 0).unwrap();
    table.mark_index(warm.index, 700);
    assert!(table.prepare_live(3, &p, 700).is_err());
    assert!(table.contains(cold) && table.contains(warm));
    assert!(table.begin_frame(700).unwrap());
    assert!(!table.contains(cold) && !table.contains(warm));
    assert!(table.get(1, 700).is_none());
    let survivor = table.get(2, 700).unwrap();
    table.prepare_live(3, &p, 700).unwrap();
    assert!(table.contains(survivor));
}

/// Opacity leaves dynamic foreground tint available to selection and cursor redraws.
#[test]
fn effect_alpha_preserves_foreground_sentinels_and_palette_stops() {
    let mut p = gradient(
        Brush::Linear {
            p0: [0.0; 2],
            p1: [1.0, 0.0],
        },
        Extend::Pad,
    );
    p.stops[0].foreground = true;
    p.stops[0].color = [0.5; 4];
    p.stops[1].color = [0.0, 0.0, 0.25, 0.25];
    let mut table = table::PaintTable::default();
    let source = table.prepare_live(1, &p, 0).unwrap();
    let original = table.take_dirty().remove(0).1;
    table
        .prepare_effect(2, source.index, 0xffffffff, None, 0.5, 0)
        .unwrap();
    let opacity = table.take_dirty().remove(0).1;
    // Dynamic stops use the already alpha-adjusted instance; authored palette uses row alpha.
    assert_eq!(opacity[8][1], 1.0);
    assert_eq!(opacity[9], original[9]);
    assert_eq!(opacity[11], [0.0, 0.0, 0.125, 0.125]);
    table
        .prepare_effect(3, source.index, 0xffffffff, Some(0x8000ff00), 0.5, 0)
        .unwrap();
    let recolored = table.take_dirty().remove(0).1;
    assert_eq!(recolored[8][1], 0.0);
    for (index, alpha) in [(9, 0.5), (11, 0.25)] {
        let expected = alpha * (128.0 / 255.0) * 0.5;
        assert_eq!(recolored[index], [0.0, expected, 0.0, expected]);
    }
    // Geometry, gradient offsets and extend stay byte-identical across both variants.
    assert_eq!(&original[..8], &opacity[..8]);
    assert_eq!(&original[..8], &recolored[..8]);
}
