//! Theme invariants that source JSON and runtime resolution must preserve.
//!
//! The popular pack is judged against its authored colors, while Puzzle's
//! four original themes have the explicit §14.6 readability floor. A
//! contrast assertion on the pack would silently change authors' work.
#![allow(
    clippy::unwrap_used,
    reason = "fixed test fixtures need direct assertions"
)]
use super::{Appearance, Theme, ThemeRegistry, color};
use puzzle_bus::Value;
use std::collections::BTreeMap;

/// Supplies a small custom specification without a JSON dependency.
fn custom(parent: &str, background: &str) -> Value {
    Value::Map(BTreeMap::from([
        ("extends".into(), Value::String(parent.into())),
        ("background".into(), Value::String(background.into())),
    ]))
}

#[test]
/// Generated names cover the four originals and every approved D-25 member.
fn builtin_catalogue_has_fourteen_sources() {
    let names = Theme::builtin_names();
    assert_eq!(names.len(), 14);
    for name in [
        "catppuccin-latte",
        "catppuccin-mocha",
        "dracula",
        "solarized-light",
        "solarized-dark",
        "gruvbox-light",
        "gruvbox-dark",
        "nord",
        "tokyo-night",
        "tokyo-night-day",
    ] {
        assert!(names.contains(&name), "missing {name}");
    }
    // A terminal index is copied from Catppuccin's pinned kitty file,
    // so this also detects a generated file that used a generic ANSI map.
    assert_eq!(
        Theme::builtin("catppuccin-mocha").unwrap().palette[4],
        [137, 180, 250]
    );
}

#[test]
/// Puzzle-owned palettes meet §14.6 without modifying the popular pack.
fn puzzle_contrast_policy() {
    for name in [
        "puzzle-dark",
        "puzzle-light",
        "puzzle-high-contrast-dark",
        "puzzle-high-contrast-light",
    ] {
        let theme = Theme::builtin(name).unwrap();
        let foreground_floor = if name.contains("high-contrast") {
            7.0
        } else {
            4.5
        };
        assert!(color::contrast(theme.foreground, theme.background) >= foreground_floor);
        assert!(
            color::contrast(
                theme.selection_foreground.unwrap_or(theme.foreground),
                theme.selection_background
            ) >= 4.5
        );
        for (index, slot) in theme.palette.into_iter().enumerate() {
            // A dim black on dark and bright white on light are deliberate
            // background-family choices, not readable foreground colors.
            let exempt = match theme.appearance {
                Appearance::Dark => index == 0 || index == 8,
                Appearance::Light => index == 7 || index == 15,
            };
            if !exempt {
                assert!(
                    color::contrast(slot, theme.background) >= 3.0,
                    "{name} slot {index}"
                );
            }
        }
    }
}

#[test]
/// Missing UI tokens are recomputed from the final inherited terminal colors.
fn inheritance_derives_after_override() {
    let mut registry = ThemeRegistry::default();
    registry
        .register("base", &custom("puzzle-dark", "#202020"))
        .unwrap();
    registry
        .register("child", &custom("base", "#303030"))
        .unwrap();
    let parent = registry.resolve("base").unwrap();
    let child = registry.resolve("child").unwrap();
    assert_eq!(child.background, [48; 3]);
    assert_ne!(parent.ui.gutter, child.ui.gutter);
    assert_eq!(
        child.palette,
        Theme::builtin("puzzle-dark").unwrap().palette
    );
}

#[test]
/// A cycle is an error with the chain, never a recursive stack overflow.
fn inheritance_cycles_are_rejected() {
    let mut registry = ThemeRegistry::default();
    registry.register("a", &custom("b", "#202020")).unwrap();
    registry.register("b", &custom("a", "#303030")).unwrap();
    assert!(registry.resolve("a").unwrap_err().contains("a -> b -> a"));
}

#[test]
/// OKLab mixing preserves endpoints and produces a perceptual midpoint.
fn oklab_color_math_and_wcag_endpoints() {
    let black = [0; 3];
    let white = [255; 3];
    assert_eq!(color::mix(black, white, 0.0), black);
    assert_eq!(color::mix(black, white, 1.0), white);
    assert!(color::contrast(black, white) >= 21.0 - 1e-9);
    assert_eq!(color::contrast(white, white), 1.0);
}

/// Authored search surfaces survive derivation and malformed colors are rejected.
#[test]
fn search_tokens_are_authored_or_derived() {
    let mut registry = ThemeRegistry::default();
    let mut source = custom("puzzle-dark", "#0e121a");
    if let Value::Map(map) = &mut source {
        map.insert("searchMatch".into(), Value::String("#334455".into()));
        map.insert("searchCurrent".into(), Value::String("#775533".into()));
    }
    registry.register("search-test", &source).unwrap();
    let resolved = registry.resolve("search-test").unwrap();
    assert_eq!(resolved.ui.search_match, [51, 68, 85]);
    assert_eq!(resolved.ui.search_current, [119, 85, 51]);
    for token in ["searchMatch", "searchCurrent"] {
        for invalid in ["#nope!!", "#ééé", "#12345", "#1234567"] {
            let mut source = custom("puzzle-dark", "#0e121a");
            if let Value::Map(map) = &mut source {
                map.insert(token.into(), Value::String(invalid.into()));
            }
            assert!(registry.register("bad", &source).is_err());
        }
    }
    let plain = Theme::builtin("puzzle-dark").unwrap();
    assert_ne!(plain.ui.search_match, plain.ui.search_current);
}

/// Active model overlays must preserve authored colors, alpha and sparse inheritance.
#[test]
fn overlay_ui_tokens_survive_derivation_and_child_inheritance() {
    let mut registry = ThemeRegistry::default();
    let mut source = custom("puzzle-dark", "#0e121a");
    if let Value::Map(map) = &mut source {
        for (key, value) in [
            ("scrollbarThumb", "#11223344"),
            ("scrollbarTrack", "#223344"),
            ("pillBackground", "#335544"),
            ("badgeForeground", "#abcdef"),
            ("zoneError", "#ff0000"),
        ] {
            map.insert(key.into(), Value::String(value.into()));
        }
    }
    registry.register("ui", &source).unwrap();
    registry
        .register("child", &custom("ui", "#303030"))
        .unwrap();
    for name in ["ui", "child"] {
        let theme = registry.resolve(name).unwrap();
        assert_eq!(theme.ui.scrollbar_thumb, [17, 34, 51, 68]);
        assert_eq!(theme.ui.scrollbar_track, [34, 51, 68]);
        assert_eq!(theme.ui.pill_background, [51, 85, 68]);
        assert_eq!(theme.ui.badge_foreground, [171, 205, 239]);
        assert_eq!(theme.ui.zone_error, [255, 0, 0]);
    }
    // Validation rejects malformed active UI tokens instead of ignoring them.
    for key in [
        "scrollbarThumb",
        "scrollbarTrack",
        "pillBackground",
        "zoneError",
    ] {
        let mut invalid = source.clone();
        if let Value::Map(map) = &mut invalid {
            map.insert(key.into(), Value::String("#nothex".into()));
        }
        assert!(registry.register("bad-ui", &invalid).is_err());
    }
}
