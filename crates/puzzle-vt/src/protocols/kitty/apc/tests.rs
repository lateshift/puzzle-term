//! M15.1 framing, bounded storage and source-order witnesses.
//! Split tests use the same source at every boundary, including both ST bytes.
//! The transparency property compares raw bytes, so even malformed UTF-8 and
//! unrelated APC/DCS/OSC streams cannot silently change in the pre-filter.
use super::{ApcFilter, ApcSink, BODY_BYTES, CHUNK_BYTES, CONTROL_BYTES};
use proptest::prelude::*;

/// Retains exact consumer observations outside allocation-sensitive tests.
#[derive(Default)]
struct Collect {
    /// Every byte forwarded to the unchanged vte parser.
    vt: Vec<u8>,
    /// Complete graphics bodies in source order.
    graphics: Vec<Vec<u8>>,
    /// Optional semantic VT yield inside an offered span.
    limit: Option<usize>,
}
impl ApcSink for Collect {
    fn passthrough(&mut self, bytes: &[u8]) -> usize {
        let used = self.limit.unwrap_or(bytes.len()).min(bytes.len());
        self.vt.extend_from_slice(&bytes[..used]);
        used
    }
    fn graphics(&mut self, body: &[u8]) -> bool {
        self.graphics.push(body.to_vec());
        true
    }
}

proptest! {
    #[test]
    fn arbitrary_graphics_prefix_recovers_at_ris(bytes in prop::collection::vec(any::<u8>(), 0..12000), slice in 1usize..1025) {
        let mut source = b"\x1b_G".to_vec();
        source.extend_from_slice(&bytes);
        // RIS is an explicit new escape sequence, so an unterminated or
        // malformed string must not hide it or the output following it.
        source.extend_from_slice(b"\x1bcSAFE");
        let mut term = crate::Terminal::new(80, 24, 0, 0);
        for chunk in source.chunks(slice) {
            term.advance(chunk);
        }
        let Some(row) = term.screen().row(0) else {
            return Err(TestCaseError::fail("visible row absent after RIS"));
        };
        for (col, expected) in "SAFE".chars().enumerate() {
            prop_assert_eq!(row.cells[col].ch, expected as u32);
        }
    }

    #[test]
    fn non_graphics_bytes_pass_unchanged(bytes in prop::collection::vec(any::<u8>(), 0..8192), slice in 1usize..1025) {
        // Only an exact kitty introducer may remove any byte. A random other
        // APC, broken string or binary UTF-8 sequence must remain transparent.
        prop_assume!(!bytes.windows(3).any(|w| w == b"\x1b_G"));
        let mut filter = ApcFilter::default();
        let mut sink = Collect::default();
        for part in bytes.chunks(slice) {
            prop_assert_eq!(filter.advance(part, &mut sink), part.len());
        }
        prop_assert_eq!(sink.vt, bytes);
        prop_assert!(sink.graphics.is_empty());
    }
}

#[test]
fn maximum_chunk_crosses_kernel_reads_and_desktop_slices() {
    let mut source = vec![b'x'; 16382];
    source
        .extend_from_slice(format!("\x1b_Ga=T;{}\x1b\\after", "A".repeat(CHUNK_BYTES)).as_bytes());
    for size in [1024, 16384] {
        let mut filter = ApcFilter::default();
        let mut sink = Collect::default();
        for chunk in source.chunks(size) {
            assert_eq!(filter.advance(chunk, &mut sink), chunk.len());
        }
        assert_eq!(sink.graphics.len(), 1);
        assert_eq!(sink.graphics[0].len(), 4 + CHUNK_BYTES);
        assert!(sink.vt.ends_with(b"\x1b_G\x1b\\after"));
    }
}

#[test]
fn every_split_and_one_byte_reads_keep_graphics_and_other_apcs() {
    let source =
        b"before\x1b_other text\x1b\\\x1b_Ga=T,f=24,s=1,v=1;AAAA\x1b\\after\x1b_Ga=d\x1b\\";
    let expected = b"before\x1b_other text\x1b\\\x1b_G\x1b\\after\x1b_G\x1b\\";
    for split in 0..=source.len() {
        let mut filter = ApcFilter::default();
        let mut sink = Collect::default();
        for part in [&source[..split], &source[split..]] {
            assert_eq!(filter.advance(part, &mut sink), part.len());
        }
        assert_eq!(sink.vt, expected);
        assert_eq!(
            sink.graphics,
            [b"a=T,f=24,s=1,v=1;AAAA".to_vec(), b"a=d".to_vec()]
        );
    }
    let mut filter = ApcFilter::default();
    let mut sink = Collect::default();
    for byte in source.chunks(1) {
        assert_eq!(filter.advance(byte, &mut sink), 1);
    }
    assert_eq!(sink.vt, expected);
    assert_eq!(sink.graphics.len(), 2);
}

#[test]
fn payload_and_control_caps_discard_whole_and_resume_after_st() {
    for (header, count) in [("a=T;", CHUNK_BYTES + 1), ("", CONTROL_BYTES + 1)] {
        let source = format!("\x1b_G{header}{}\x1b\\visible", "A".repeat(count));
        let mut filter = ApcFilter::default();
        let mut sink = Collect::default();
        for chunk in source.as_bytes().chunks(1024) {
            assert_eq!(filter.advance(chunk, &mut sink), chunk.len());
        }
        assert!(sink.graphics.is_empty());
        assert_eq!(sink.vt, b"\x1b_G\x1b\\visible");
        assert!(filter.body.capacity() <= BODY_BYTES);
        assert!(filter.body.is_empty());
    }
    // The actual chunk maximum, including a split final ST, is accepted.
    let source = format!("\x1b_Ga=T;{}\x1b\\", "A".repeat(CHUNK_BYTES));
    let mut filter = ApcFilter::default();
    let mut sink = Collect::default();
    filter.advance(source.as_bytes(), &mut sink);
    assert_eq!(sink.graphics[0].len(), 4 + CHUNK_BYTES);
}

#[test]
fn cancellation_and_restarted_escape_do_not_swallow_following_output() {
    for cancel in [b"\x18".as_slice(), b"\x1a", b"\x1b[31m"] {
        let mut source = b"\x1b_Gunterminated".to_vec();
        source.extend_from_slice(cancel);
        source.extend_from_slice(b"visible\x1b_Ga=d\x1b\\");
        let mut filter = ApcFilter::default();
        let mut sink = Collect::default();
        for byte in source.chunks(1) {
            filter.advance(byte, &mut sink);
        }
        assert_eq!(sink.graphics, [b"a=d".to_vec()]);
        assert!(sink.vt.windows(7).any(|w| w == b"visible"));
    }
}

#[test]
fn vt_partial_consumption_preserves_unaccepted_candidate() {
    let source = b"text\x1b_Ga=q;AAAA\x1b\\suffix";
    let mut sink = Collect {
        limit: Some(2),
        ..Collect::default()
    };
    let mut filter = ApcFilter::default();
    let mut used = 0;
    while used < source.len() {
        let count = filter.advance(&source[used..], &mut sink);
        assert!(count > 0);
        used += count;
    }
    assert_eq!(sink.vt, b"text\x1b_G\x1b\\suffix");
    assert_eq!(sink.graphics, [b"a=q;AAAA".to_vec()]);
}

#[test]
fn terminal_graphics_yield_precedes_da_and_registered_osc() {
    let mut term = crate::Terminal::new(80, 24, 0, 0);
    let mut osc = crate::osc::OscRequest::default();
    let mut graphics = Vec::new();
    let source = b"\x1b_Ga=q,i=1;AAAA\x1b\\\x1b[c\x1b]7502;payload\x07";
    let (used, osc_ready, ready) =
        term.advance_until_graphics(source, &[7502], &mut osc, &mut graphics, false);
    assert!(ready && !osc_ready);
    assert_eq!(graphics, b"a=q,i=1;AAAA");
    assert!(term.replies.is_empty());
    // Simulate Session's first ordered protocol reply before later VT queries.
    term.replies.extend_from_slice(b"graphics reply");
    let (rest, osc_ready, ready) =
        term.advance_until_graphics(&source[used..], &[7502], &mut osc, &mut graphics, false);
    assert_eq!(used + rest, source.len());
    assert!(osc_ready && !ready);
    assert_eq!(osc.payload, b"payload");
    assert_eq!(term.replies, b"graphics reply\x1b[?62;22c");
}

#[test]
fn ordinary_terminal_feeds_allocate_nothing_after_warmup() {
    let mut term = crate::Terminal::new(80, 24, 100, 32 * 1024 * 1024);
    let bytes = b"\x1b[Htext_with_underscores \x1b[31mred\x1b[m\r\n";
    for _ in 0..200 {
        term.advance(bytes);
    }
    let counts = allocation_counter::measure(|| {
        for _ in 0..200 {
            term.advance(bytes);
        }
    });
    assert_eq!(counts.count_total, 0);
    // A fresh standalone filter also reserves nothing before a graphics prefix.
    assert_eq!(ApcFilter::default().body.capacity(), 0);
}
