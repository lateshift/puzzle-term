//! Protocol examples and malformed cells must never enter glyph fallback.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests name malformed wire and fixture failures"
)]
use super::*;
use crate::{
    images::{DecodeResult, DecodedImage, Store},
    vt::Terminal,
};
/// Both create paths produce invisible prototypes and leave the cursor untouched.
#[test]
fn virtual_placements_do_not_draw_or_move_cursor() {
    for action in ["T", "t"] {
        let mut term = Terminal::new(10, 4, 4, 4096);
        let mut store = Store::new(4096, &term);
        store.command(
            format!("a={action},U=1,i=42,c=3,r=2,s=1,v=1;/wAA/w==").as_bytes(),
            &mut term,
        );
        let job = store.take_request().unwrap();
        store.complete(
            DecodeResult {
                ticket: job.ticket,
                image: Ok(DecodedImage {
                    size: [1, 1],
                    rgba: std::sync::Arc::from([255, 0, 0, 255]),
                }),
            },
            &mut term,
        );
        if action == "t" {
            store.command(b"a=p,U=1,i=42,c=3,r=2", &mut term);
        }
        assert_eq!((term.cursor.row, term.cursor.column), (0, 0));
        assert_eq!(store.placements().len(), 1);
    }
}
/// All 297 scalars and their wire order are pinned to the downloaded protocol list.
#[test]
fn exact_protocol_diacritic_table() {
    // FNV-1a over each scalar's little-endian u32, independently computed from
    // kitty's rowcolumn-diacritics.txt rather than from the Rust table.
    let digest = diacritics::TABLE
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    assert_eq!(digest, 0xfeb8357288a293a);
    assert!(diacritics::TABLE.windows(2).all(|p| p[0] < p[1]));
    // Every entry must survive VT clustering, including supplementary-plane marks.
    for (index, mark) in diacritics::TABLE.iter().enumerate() {
        let mut term = Terminal::new(2, 1, 0, 1024);
        term.advance(
            format!("\x1b[38;5;42m\u{10eeee}{}", char::from_u32(*mark).unwrap()).as_bytes(),
        );
        let tiles = tiles(term.screen().row(0).unwrap());
        assert_eq!(tiles[0].1.position, [index as u32, 0]);
    }
}
/// Decoding uses raw palette indices/RGB, independent of OSC recolouring and attributes.
#[test]
fn raw_colour_ids_and_high_byte() {
    for (sgr, image, placement) in [
        ("38;5;42;58;5;7", 42, 7),
        ("38;2;18;52;86;58;2;1;2;3", 0x123456, 0x010203),
    ] {
        let mut term = Terminal::new(8, 2, 0, 4096);
        term.advance(
            format!("\x1b]4;42;#ffffff\x1b\\\x1b[{sgr};7;2;8m\u{10eeee}\u{030d}\u{030e}\u{0310}")
                .as_bytes(),
        );
        let cells = tiles(term.screen().row(0).unwrap());
        assert_eq!(
            (cells[0].1.image, cells[0].1.placement, cells[0].1.position),
            (image | (3 << 24), placement, [1, 2])
        );
    }
}
/// Every indexed colour is an eight-bit identifier, including ANSI/bright aliases.
#[test]
fn all_indexed_image_and_placement_ids_are_raw() {
    for index in 0..=255 {
        let mut term = Terminal::new(2, 1, 0, 1024);
        // Palette overrides deliberately disagree with the encoded wire index.
        term.advance(
            format!(
                "\x1b]4;{index};#ffffff\x1b\\\x1b[38;5;{index};58;5;{}m\u{10eeee}",
                255 - index
            )
            .as_bytes(),
        );
        let cell = tiles(term.screen().row(0).unwrap())[0].1;
        assert_eq!(cell.image, index);
        assert_eq!(cell.placement, 255 - index);
    }
    for (sgr, id) in [("31", 1), ("91", 9)] {
        let mut term = Terminal::new(2, 1, 0, 1024);
        term.advance(format!("\x1b[{sgr}m\u{10eeee}").as_bytes());
        assert_eq!(tiles(term.screen().row(0).unwrap())[0].1.image, id);
    }
}
/// Protocol's three omission rules and every boundary must agree with left-to-right text.
#[test]
fn inheritance_resets_on_colour_text_and_row_boundaries() {
    let mut term = Terminal::new(16, 3, 0, 4096);
    term.advance(
        concat!(
            "\x1b[38;5;42;58;5;7m",
            "\u{10eeee}\u{030d}\u{0305}\u{030e}", // row1,col0,high2
            "\u{10eeee}",                         // no marks
            "\u{10eeee}\u{030d}",                 // row only
            "\u{10eeee}\u{030d}\u{0310}",         // adjacent explicit col3
            "\u{10eeee}\u{030d}\u{033d}",         // gap col5 -> high0
            "\u{10eeee}\u{030e}",                 // different row -> col0,high0
            "x\u{10eeee}",                        // normal text resets
            "\x1b[58;5;8m\u{10eeee}",             // underline change resets
            "\x1b[38;2;0;0;42m\u{10eeee}",        // equivalent RGB is a different tag
            "\r\n\u{10eeee}"
        )
        .as_bytes(),
    );
    let actual: Vec<_> = tiles(term.screen().row(0).unwrap())
        .iter()
        .map(|(_, t)| (t.position, t.image >> 24))
        .collect();
    assert_eq!(
        actual,
        [
            ([1, 0], 2),
            ([1, 1], 2),
            ([1, 2], 2),
            ([1, 3], 2),
            ([1, 5], 0),
            ([2, 0], 0),
            ([0, 0], 0),
            ([0, 0], 0),
            ([0, 0], 0)
        ]
    );
    assert_eq!(tiles(term.screen().row(1).unwrap())[0].1.position, [0, 0]);
}
/// Virtual prototypes survive physical deletes and ED, but image-ID deletes and RIS remove them.
#[test]
fn virtual_deletion_and_visible_admission() {
    let mut term = Terminal::new(10, 4, 4, 4096);
    let mut store = Store::new(4096, &term);
    crate::images::tests::transmit(
        &mut store,
        &mut term,
        b"a=T,U=1,i=42,p=7,c=2,r=2,s=1,v=1;/wAA/w==",
    );
    term.advance("\x1b[38;5;42;58;5;7m\u{10eeee}\u{0305}\u{0305}".as_bytes());
    assert!(store.visible(store.images()[0].generation, &term));
    for delete in [
        "a", "A", "c", "C", "p", "P", "q", "Q", "x", "X", "y", "Y", "z", "Z",
    ] {
        store.command(format!("a=d,d={delete},x=1,y=1,z=0").as_bytes(), &mut term);
        assert_eq!(store.placements().len(), 1, "d={delete}");
    }
    term.advance(b"\x1b[2J");
    store.reconcile(&mut term);
    assert_eq!(store.placements().len(), 1);
    assert!(!store.visible(store.images()[0].generation, &term));
    store.command(b"a=d,d=I,i=42,p=7", &mut term);
    assert!(store.images().is_empty());
    crate::images::tests::transmit(
        &mut store,
        &mut term,
        b"a=T,U=1,i=42,c=2,r=2,s=1,v=1;/wAA/w==",
    );
    term.advance(b"\x1bc");
    store.reconcile(&mut term);
    assert!(store.images().is_empty());
}
/// Collect the decoded wire cells without consulting rendered colours or fonts.
fn tiles(row: &crate::vt::row::Row) -> Vec<(usize, Tile)> {
    let mut result = Vec::new();
    visit(row, |column, tile| result.push((column, tile)));
    result
}
