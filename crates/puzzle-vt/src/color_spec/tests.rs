//! Shared X11 grammar and allocation contracts, exercised independently of OSCs.
#![allow(
    clippy::expect_used,
    reason = "Approved source rows must contain three valid RGB channels"
)]
use super::parse;

/// Every upstream spelling and its space/case aliases must resolve identically.
#[test]
fn every_upstream_name_and_normalized_alias() {
    let source = include_str!("../../../../assets/colors/rgb.txt");
    assert_eq!(source.lines().count(), 782);
    for line in source.lines() {
        let mut parts = line.split_whitespace();
        let rgb = std::array::from_fn(|_| {
            parts
                .next()
                .expect("validated colour fixture")
                .parse::<u8>()
                .expect("validated colour fixture")
        });
        let name = parts.collect::<Vec<_>>().join(" ");
        assert_eq!(parse(name.as_bytes()), Some(rgb), "{name}");
        assert_eq!(
            parse(name.to_uppercase().replace(' ', "").as_bytes()),
            Some(rgb),
            "{name}"
        );
        assert_eq!(
            parse(format!(" {} ", name.to_uppercase()).as_bytes()),
            Some(rgb)
        );
    }
}

/// XParseColor's compact hex form differs intentionally from scaled rgb notation.
#[test]
fn numeric_forms_and_malformed_inputs() {
    for (spec, rgb) in [
        ("#3a7", [0x30, 0xa0, 0x70]),
        ("#123456", [0x12, 0x34, 0x56]),
        ("#123456789", [0x12, 0x45, 0x78]),
        ("#123456789abc", [0x12, 0x56, 0x9a]),
        ("rgb:3/a/7", [0x33, 0xaa, 0x77]),
        ("rgb:ff/0/1234", [255, 0, 18]),
        ("rgbi:-1/5e-1/+2", [0, 128, 255]),
        ("fOrEsT gReEn", [34, 139, 34]),
    ] {
        assert_eq!(parse(spec.as_bytes()), Some(rgb), "{spec}");
    }
    for spec in [
        "",
        "#12",
        "#aé",
        "#ggg",
        "rgb:1/2",
        "rgb:1/2/3/4",
        "rgb:+1/2/3",
        "rgbi:NaN/0/0",
        "rgbi:inf/0/0",
        "rgbi:1e999/0/0",
        "madeup",
    ] {
        assert_eq!(parse(spec.as_bytes()), None, "{spec}");
    }
}

/// Named/numeric decoding has no first-use setup or subsequent heap work (R4/P1).
#[test]
fn colour_parser_allocates_nothing_including_first_lookup() {
    let allocations = allocation_counter::measure(|| {
        for _ in 0..1000 {
            assert_eq!(parse(b"Medium Spring Green"), Some([0, 250, 154]));
            assert_eq!(parse(b"rgb:f/00/1234"), Some([255, 0, 18]));
            assert_eq!(parse(b"unknown colour"), None);
        }
    });
    assert_eq!(allocations.count_total, 0);
}
