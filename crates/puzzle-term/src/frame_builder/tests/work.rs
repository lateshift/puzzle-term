//! M18.7 deterministic work at actual copy, paint, placement and sparse entry points.
//! Test-only thread-local counters disappear from native/wasm product builds.
#![allow(
    clippy::expect_used,
    reason = "fixtures and optional count receipts must report setup failures"
)]
use super::*;
use std::cell::Cell;

/// Copied cells, painted cells, glyph inputs, sparse capture rows/references,
/// sparse ink entries/preparations and emitted block extents, in that order.
type Counts = [usize; 8];
thread_local! {
    /// Fixed stack-sized observation adds no heap noise to allocation checks.
    static ACTIVE: Cell<Option<Counts>> = const { Cell::new(None) };
}
/// Charges work where it happens rather than predicting it from the fixture.
pub(crate) fn charge(kind: usize, amount: usize) {
    ACTIVE.with(|slot| {
        if let Some(mut counts) = slot.get() {
            counts[kind] += amount;
            slot.set(Some(counts));
        }
    });
}
/// Observes one synchronous core operation on this test thread.
fn measure(run: impl FnOnce()) -> Counts {
    ACTIVE.with(|slot| {
        assert!(slot.replace(Some([0; 8])).is_none());
    });
    run();
    ACTIVE.with(|slot| slot.take().expect("installed observer"))
}
/// Optional raw count receipts are written after observation, never in a hot path.
fn report(rows: &str) {
    if let Some(path) = std::env::var_os("PUZZLE_TEXT_COUNT_REPORT") {
        std::fs::write(path, rows).expect("count evidence");
    }
}
/// ASCII/Unicode full rebuilds retain one copy/paint/glyph input per cell;
/// warm publications and frame assembly add no copies, scans or allocations.
#[test]
fn text_sizing_ordinary_frames_keep_work_and_zero_allocations() {
    let mut csv = String::from(
        "columns,rows,dpr,text,copied,painted,glyphs,capture_rows,references,ink_entries,prepared,extents,sources,slugs,rasters\n",
    );
    for (columns, rows) in [(120, 40), (282, 63)] {
        for dpr in [1.0, 2.0] {
            for scalar in ['A', 'é'] {
                let values = ordinary(columns, rows, dpr, scalar);
                csv.push_str(&format!(
                    "{columns},{rows},{dpr},{scalar},{}\n",
                    values
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
        }
    }
    report(&csv);
}
/// Builds the same real-font fixture at each geometry and returns physical work.
fn ordinary(columns: usize, rows: usize, dpr: f32, scalar: char) -> Vec<usize> {
    let mut context = TextContext::new(fonts(), 14.0, dpr).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(columns, rows, 0, 0);
    terminal.advance(b"\x1b[?25l");
    terminal.advance(scalar.to_string().repeat(columns * rows).as_bytes());
    let mut snapshot = FrameSnapshot::default();
    let mut frame = RenderFrame::default();
    let mut draw = |builder: &mut FrameBuilder| {
        snapshot
            .capture(&terminal, builder.style_epoch())
            .expect("copy");
        builder
            .build_snapshot(&mut context, &snapshot, &mut frame)
            .expect("frame");
    };
    for _ in 0..3 {
        draw(&mut builder);
    }
    // Invalidate resolved instances, retaining every glyph and allocation as
    // in the release full-frame budget, rather than relying on a cache hit.
    builder.style_epoch += 1;
    let counts = measure(|| draw(&mut builder));
    assert_eq!(
        counts,
        [
            columns * rows,
            columns * rows,
            columns * rows,
            0,
            0,
            0,
            0,
            0
        ]
    );
    let warm = measure(|| draw(&mut builder));
    assert_eq!(warm, [0; 8]);
    let allocated = allocation_counter::measure(|| {
        for _ in 0..8 {
            draw(&mut builder);
        }
    });
    assert_eq!(
        allocated.count_total, 0,
        "{columns}x{rows}/{dpr}/{scalar}: {allocated:?}"
    );
    assert_eq!(builder.rebuilt_rows(), 0);
    assert!(snapshot.blocks().next().is_none());
    let mut values = counts.to_vec();
    values.extend([
        builder.rows[..rows].iter().map(|r| r.sources.len()).sum(),
        frame.slugs.len(),
        frame.rasters.len(),
    ]);
    values
}
/// Positive observation and retirement prove that zeros mean dispatch was avoided.
#[test]
fn text_sizing_sparse_observer_and_last_block_retirement() {
    let mut context = TextContext::new(fonts(), 14.0, 2.0).expect("metrics");
    let mut builder = FrameBuilder::new(&context);
    let mut terminal = Terminal::new(12, 5, 0, 0);
    terminal.advance(b"\x1b]66;s=2;A\x07\x1b[?25l");
    let mut snapshot = FrameSnapshot::default();
    let mut frame = RenderFrame::default();
    let counts = measure(|| {
        snapshot.capture(&terminal, 0).expect("copy");
        builder
            .build_snapshot(&mut context, &snapshot, &mut frame)
            .expect("frame");
    });
    assert_eq!(&counts[3..], &[5, 2, 2, 1, 1]);
    // Erase leaves the bounded free list alive. Its lack of live records must
    // select the ordinary publication path without walking empty row indices.
    terminal.advance(b"\x1b[2J");
    assert!(
        terminal
            .screen()
            .ring
            .multicells
            .as_ref()
            .expect("used store")
            .is_empty()
    );
    let retired = measure(|| {
        snapshot.capture(&terminal, 0).expect("retired copy");
        builder
            .build_snapshot(&mut context, &snapshot, &mut frame)
            .expect("retired frame");
    });
    assert_eq!(&retired[3..], &[0; 5]);
    assert!(snapshot.blocks().next().is_none());
    assert!(builder.geometry.blocks.is_empty());
    let allocated = allocation_counter::measure(|| {
        for _ in 0..8 {
            snapshot.capture(&terminal, 0).expect("warm copy");
            builder
                .build_snapshot(&mut context, &snapshot, &mut frame)
                .expect("warm frame");
        }
    });
    assert_eq!(allocated.count_total, 0, "{allocated:?}");
}

/// The ordinary-count probe uses the same bundled font as the budget fixture.
fn fonts() -> puzzle_text::font_set::FontSet {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/JetBrainsMono-Variable.ttf"
    );
    puzzle_text::font_set::FontSet::new(
        puzzle_text::FontData::new(std::fs::read(path).expect("font"), 0),
        None,
        None,
    )
    .expect("font set")
}
