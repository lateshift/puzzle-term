//! Real Session selection readbacks at both DPRs, with optional foreground (§6.9).
//!
//! Each image is compared cell by cell with an unselected frame: every pixel
//! outside selected cells must remain identical, while every selected row has
//! the authored background. The simple case selects half of a -> ligature;
//! this guards clipping and rejects a whole-ligature recolor or double blending.
//! A warmed published snapshot must rebuild zero text rows after selecting.
//!
//! Fixture text contains a -> ligature, a word followed by slash punctuation,
//! a logical line wrapped over two rows, colored text and explicit hard breaks.
//! Each kind creates a new Session so no previous gesture can affect its range.
//! Both DPR paths own their fonts and atlas, exercising hinted and unhinted ink.
//! These bytes are self-authored and contain no captured user or shell data.
//!
//! The custom fixture theme inherits an existing bundled palette. Its paper
//! and selection colors are fixed bytes so readback has independent targets.
//! The no-foreground variant inherits an absent token, preserving SGR ink;
//! the other variant explicitly requests white ink, including colored cells.
//! No live user config or OS appearance enters this deterministic scenario.
//!
//! A baseline frame initializes the persistent atlas through real uploads.
//! Selection is then submitted as a User command and accepted by Session.
//! The baseline and selected samples use independent snapshot-copy histories,
//! while their builder deliberately shares its retained geometry. This catches
//! reliance on a slot's copy flags in place of the last built row versions.
//! Each sample owns its neutral frame until real GPU submission consumes uploads;
//! comparing a cold renderer would conceal missing uploads on the selected frame.
//! Pointer regions are outside this fixture: no submission acknowledgement is
//! invented merely to advance the source's search or selection bookkeeping.
//! Publication captures its range together with cell metadata,
//! just as the desktop session owner does at a slice boundary. The builder
//! must reuse the warmed rows despite the new highlight and foreground clips.
//!
//! Expected selected cells are authored separately from the engine's result.
//! The pixel checker must not ask that result which cells should change:
//! otherwise an incorrectly expanded word would make its own test pass.
//! Every pixel outside the expected cells is compared with the baseline,
//! including all padding and the unselected half of the simple ligature.
//! Within selected cells, exact selection paper and non-paper glyph samples
//! prove both the fill and text exist; background alone cannot count as ink.
//!
//! Candidate PNGs are written only after those readback checks succeed.
//! Their numbering follows DPR, foreground policy and gesture kind order.
//! The golden runner pins the same sixteen names rather than discovering
//! arbitrary output files, which prevents stale screenshots concealing a gap.
//! Human appearance confirmation remains a separate pending review item.

use super::{
    Scenario,
    scroll::{Manual, Sink},
};
use puzzle_render::{FrameRenderer, RgbaImage};
use puzzle_term::frame::RenderFrame;
use puzzle_term::{
    Origin, Session, SessionConfig,
    frame_builder::{FrameBuilder, context::TextContext},
    frame_snapshot::FrameSnapshot,
    messages::{Command, Value},
    platform::Services,
    selection::SelectionCommand,
    settings::Settings,
    text::{FontData, font_set::FontSet},
    vt::selection::{SelectPoint, SelectionKind},
};
use std::{collections::BTreeMap, time::Instant};
/// Runs all gesture kinds with and without a selection-ink token at 1× and 2×.
pub(super) fn run(state: &mut Scenario) -> Result<(), String> {
    // Each DPR exercises its own font hinting decision and device-cell geometry.
    for dpr in [1.0, 2.0] {
        // An absent token must preserve SGR colors; an explicit token overrides them.
        for foreground in [false, true] {
            // Four independently authored ranges protect gesture expansion and clipping.
            for kind in [
                SelectionKind::Simple,
                SelectionKind::Word,
                SelectionKind::Line,
                SelectionKind::Block,
            ] {
                sample(state, dpr, foreground, kind, false)?;
            }
        }
    }
    Ok(())
}
/// OSC 17/19 selection references exercise query-independent rendering at both DPRs.
pub(super) fn osc_colours(state: &mut Scenario) -> Result<(), String> {
    // The OSC variant starts without a theme ink override, so OSC 19 is observable.
    // Its rectangle matches the existing block fixture, keeping expected cells independent.
    for dpr in [1.0, 2.0] {
        sample(state, dpr, true, SelectionKind::Block, true)?;
    }
    Ok(())
}
/// Constructs only authored colors on existing bundled outlines, with no new asset.
fn settings(foreground: bool, osc: bool) -> Settings {
    // This pack theme has no inherited selection foreground, making absence observable.
    // Fixed colors let readback assert exact paper without asking the renderer policy.
    let mut theme = BTreeMap::from([
        ("extends".into(), Value::String("gruvbox-dark".into())),
        ("foreground".into(), Value::String("#e5e9f0".into())),
        ("background".into(), Value::String("#0e121a".into())),
        (
            "selectionBackground".into(),
            Value::String(if osc { "#ff0000" } else { "#35465d" }.into()),
        ),
    ]);
    // Only half the cases opt into recoloring; both halves share identical VT bytes.
    if foreground {
        theme.insert(
            "selectionForeground".into(),
            Value::String("#ffffff".into()),
        );
    }
    // Default separators plus slash isolate bar while preserving the arrow separator.
    // Keep user configuration out of these reproducible headless runs.
    Settings {
        theme_dark: "selection-fixture".into(),
        selection_word_separators: r#" `│|:"'()[]{}<>,;/"#.into(),
        themes: Value::Map(BTreeMap::from([(
            "selection-fixture".into(),
            Value::Map(theme),
        )])),
        ..Default::default()
    }
}
/// Feeds one wrapped logical line and two hard lines through the production parser.
fn sample(
    state: &mut Scenario,
    dpr: f32,
    foreground: bool,
    kind: SelectionKind,
    osc: bool,
) -> Result<(), String> {
    // Load the approved bundled font through the same text context as the live host.
    // No synthesized glyphs or mock placement can hide a broken atlas address.
    let bytes =
        std::fs::read("assets/fonts/JetBrainsMono-Variable.ttf").map_err(|e| e.to_string())?;
    let mut context = TextContext::new(
        FontSet::new(FontData::new(bytes, 0), None, None)?,
        14.0,
        dpr,
    )?;
    // Cell dimensions are device pixels, already resolved for this DPR.
    let now = Instant::now();
    let mut session = selection_scene(&context, foreground && !osc, osc, now);
    if osc {
        // Both values differ from the theme: a missing override cannot pass readback.
        session.feed(b"\x1b]17;#203040\x07\x1b]19;#ffffff\x1b\\", now);
        session.feed(b"\x1b]17;#35465d\x07", now);
    }
    // Reuse one GPU renderer across baseline and selected frames so uploads matter.
    let mut gpu = FrameRenderer::new(&state.renderer, wgpu::TextureFormat::Rgba8Unorm);
    // Warm ordinary row caches before changing only the selection model.
    let mut builder = FrameBuilder::new(&context);
    let baseline = baseline_image(
        state,
        &mut session,
        &mut context,
        &mut builder,
        &mut gpu,
        now,
    )?;
    select_fixture(&mut session, kind, now)?;
    present_selection(
        state,
        &mut session,
        &mut context,
        &mut builder,
        &mut gpu,
        &baseline,
        kind,
        foreground,
        dpr,
        now,
    )
}

/// Authors wrapped and colored rows through the same parser and policy as interactive selection.
fn selection_scene(context: &TextContext, foreground: bool, osc: bool, now: Instant) -> Session {
    // The twelve-column grid deliberately wraps the first logical line once.
    // Four physical rows retain all fixture text without hidden history in this sample.
    let mut session = Session::new(
        SessionConfig {
            columns: 12,
            rows: 4,
            ..Default::default()
        },
        Services::new(Box::new(Sink), Box::new(Manual)),
    );
    // Source policy and host builder must agree on measured cells and atlas incarnation.
    session.set_text_context(context);
    session.apply_settings(&settings(foreground, osc), now);
    // Hide the cursor to make outside-selection pixel identity independent of blink.
    // The red hard line provides colored ink inside the block range.
    session.feed(
        b"\x1b[?25lfoo ->bar/baztail\r\n\x1b[31mred->green\r\n\x1b[0mthird row",
        now,
    );
    // Both frames use these unchanged VT row versions; only the selection model differs.
    session
}

/// Warms the retained row cache and real GPU stores before the selection changes.
fn baseline_image(
    state: &Scenario,
    session: &mut Session,
    context: &mut TextContext,
    builder: &mut FrameBuilder,
    gpu: &mut FrameRenderer,
    now: Instant,
) -> Result<RgbaImage, String> {
    // This fresh copy history must not force misses in the already retained builder.
    let mut snapshot = FrameSnapshot::default();
    // The diagnostic host owns the cache and consumes the same sealed publication as live rendering.
    if session.publish_frame(
        now,
        &mut snapshot,
        context.atlas_generation(),
        builder.style_epoch(),
    )? != puzzle_term::PublishOutcome::Published
    {
        // Without a baseline, later unchanged-pixel comparisons would have no reference.
        return Err("selection baseline absent".into());
    }
    // Preserve initial uploads through real GPU submission rather than clearing them at build.
    let mut initial = RenderFrame::default();
    builder.build_published(context, &mut snapshot, &mut initial)?;
    let (baseline, _) = gpu
        .render_to_image(&state.renderer, &mut initial)
        .map_err(|e| e.to_string())?;
    // The returned pixels include exactly the atlas incarnation used by the selected frame.
    // They are an independent paper/ink baseline, not a snapshot of expected selection state.
    Ok(baseline)
}

/// Applies an authored gesture to retained rows after the baseline warmed caches.
fn select_fixture(session: &mut Session, kind: SelectionKind, now: Instant) -> Result<(), String> {
    // IDs come from retained document rows, not assumed zero-based identities.
    let ids: Vec<_> = session
        .buffer()
        .lines(session.buffer().line_ids().ok_or("rows absent")?)
        .map(|row| row.id())
        .collect();
    // These clicked cells differ from expected expanded cells below by design.
    // In particular word and logical-line tests must not derive their own expectations.
    let (a, b) = match kind {
        SelectionKind::Simple => ((0, 4), (0, 4)),
        SelectionKind::Word => ((0, 7), (0, 7)),
        SelectionKind::Line => ((0, 2), (0, 2)),
        SelectionKind::Block => ((0, 2), (2, 5)),
        _ => return Err("unsupported selection fixture kind".into()),
    };
    // Dispatch through Select to include validation, facts and marker reconciliation.
    session.submit(
        Command::Select(SelectionCommand::Range {
            kind,
            start: SelectPoint {
                line: ids[a.0],
                column: a.1,
            },
            end: SelectPoint {
                line: ids[b.0],
                column: b.1,
            },
        }),
        Origin::User,
    );
    // Complete the accepted Select cascade before late snapshot capture.
    session.pump(now);
    // The request changes only model state; rendering remains refresh-driven.
    Ok(())
}

/// Renders a late snapshot and checks the changed pixels against independent cells.
#[allow(
    clippy::too_many_arguments,
    reason = "diagnostic rendering carries one sample’s explicit context"
)]
fn present_selection(
    state: &mut Scenario,
    session: &mut Session,
    context: &mut TextContext,
    builder: &mut FrameBuilder,
    gpu: &mut FrameRenderer,
    baseline: &RgbaImage,
    kind: SelectionKind,
    foreground: bool,
    dpr: f32,
    now: Instant,
) -> Result<(), String> {
    // Resolve metrics after the sample’s DPR/font context is final.
    let metrics = context.metrics();
    // Capture after pump into the diagnostic host’s own free slot.
    let mut snapshot = FrameSnapshot::default();
    // Instance assembly reads only sealed row values.
    // The warmed builder retains the actual glyph geometry from the baseline.
    let mut frame = RenderFrame::default();
    // The host keeps its row cache while Session publishes a new immutable sample.
    if session.publish_frame(
        now,
        &mut snapshot,
        context.atlas_generation(),
        builder.style_epoch(),
    )? != puzzle_term::PublishOutcome::Published
    {
        return Err("selection frame absent".into());
    }
    // This snapshot contains the selected range and the unchanged row versions.
    builder.build_published(context, &mut snapshot, &mut frame)?;
    // Selection cannot enter the cache key; moving a fill must not shape text.
    if builder.rebuilt_rows() != 0 {
        return Err("selection invalidated glyph row caches".into());
    }
    // Real GPU bytes expose missing text as well as incorrectly scoped recoloring.
    let (image, _) = gpu
        .render_to_image(&state.renderer, &mut frame)
        .map_err(|e| e.to_string())?;
    // This diagnostic checks pixels only; no pointer target can be presented accidentally.
    check_cells(baseline, &image, kind, metrics.width_px, metrics.height_px)?;
    // Only mechanically checked candidates reach the screenshot writer.
    state.image = Some(image);
    state.preview_ready = true;
    state.screenshot(&format!(
        "{kind:?}-{}-{dpr:.0}x",
        // Only half the cases opt into recoloring; both halves share identical VT bytes.
        if foreground { "ink" } else { "original" }
    ))
}
/// Independent cell expectations catch missing fills and foreground escaping an edge.
fn selected(kind: SelectionKind, row: u32, column: u32) -> bool {
    // Authored geometry is deliberately independent of SelectionRange::columns.
    match kind {
        SelectionKind::Simple => row == 0 && column == 4,
        SelectionKind::Word => row == 0 && (6..9).contains(&column),
        SelectionKind::Line => row <= 1,
        SelectionKind::Block => row <= 2 && (2..6).contains(&column),
        _ => false,
    }
}
/// Compares actual GPU bytes, not the range metadata or CPU rectangle list.
fn check_cells(
    base: &RgbaImage,
    image: &RgbaImage,
    kind: SelectionKind,
    width: u32,
    height: u32,
) -> Result<(), String> {
    // Per-cell counters reject a fill-only image and a glyph-only highlight.
    // Stack storage matches this fixed twelve-by-four diagnostic grid.
    let mut paper = [[0u32; 12]; 4];
    let mut ink = [[0u32; 12]; 4];
    for y in 0..image.height {
        for x in 0..image.width {
            // Readback rows are tightly packed RGBA8; compare complete pixels outside.
            let at = ((y * image.width + x) * 4) as usize;
            // Fixed padding belongs to the fixture, not to selected-cell coordinates.
            let in_grid = x >= 16 && y >= 16 && x < 16 + 12 * width && y < 16 + 4 * height;
            let (row, col) = if in_grid {
                ((y - 16) / height, (x - 16) / width)
            } else {
                (4, 12)
            };
            // A selected pixel must contribute either exact paper or visible glyph coverage.
            if in_grid && selected(kind, row, col) {
                if image.pixels[at..at + 3] == [53, 70, 93] {
                    paper[row as usize][col as usize] += 1;
                } else {
                    ink[row as usize][col as usize] += 1;
                }
            // Includes padding and the unselected half of the arrow ligature.
            // No changed pixel outside the authored cells is tolerated here.
            } else if image.pixels[at..at + 4] != base.pixels[at..at + 4] {
                return Err(format!("selection changed an outside pixel at {x},{y}"));
            }
        }
    }
    // Require evidence in every authored selected cell rather than only one per row.
    for row in 0..4 {
        for col in 0..12 {
            if selected(kind, row, col) && paper[row as usize][col as usize] == 0 {
                return Err(format!("selection paper missing in cell {row},{col}"));
            }
            // Authored fixture cells containing visible glyphs must retain actual ink.
            // The second physical segment is ztail; blank padding needs paper only.
            let nonblank = match row {
                0 => col != 3,
                1 => col < 5,
                2 => col < 10,
                _ => col < 9 && col != 5,
            };
            if selected(kind, row, col) && nonblank && ink[row as usize][col as usize] == 0 {
                return Err(format!("selection glyph ink missing in cell {row},{col}"));
            }
        }
    }
    Ok(())
}
