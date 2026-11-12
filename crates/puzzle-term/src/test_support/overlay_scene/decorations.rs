//! Bundled-font declaration specimen uses the public Session mutation surface.
//!
//! The specimen reserves one labeled source row for each declarative family.
//! Every source is an ordinary VT range rather than a special renderer fixture.
//! The labels remain in copied output; annotations never alter terminal content.
//! Rows between source lines provide space for Above and Below free text.
//! The fixed source columns make Before and After visually distinguishable.
//! Five separated runs on each row expose all nonempty underline styles.
//! The empty underline style has no specimen ink by definition.
//! Source text uses the primary bundled face at the harness's configured size.
//! The gutter badge uses the approved icon face through ordinary fallback.
//! No part of this fixture enumerates installed fonts or loads machine assets.
//!
//! Device scale changes recreate the Session and its font context in the harness.
//! Cell geometry therefore comes from production metrics at each scale.
//! Logical annotation sizes stay constant while device pixels scale with DPR.
//! The public shape compiler prepares the curly rule and the SVG contours.
//! Shapes are not pre-rendered into a fixture bitmap or directly injected.
//! The renderer receives retained primitives through the usual overlay registry.
//! The two resulting images can use the existing strict golden comparator.
//! Human reference approval remains pending independently of these pixel checks.
//!
//! Installation feeds all specimen bytes before creating the tracked endpoints.
//! This ensures that row IDs describe the actual rendered text after scrolling.
//! The public buffer iterator avoids assuming arithmetic adjacency of LineIds.
//! Each source range includes five cells, matching the visible BBBBB run.
//! The end marker points at the inclusive last cell rather than the next run.
//! Markers belong to Session and follow its existing reflow and eviction rules.
//! Auto-remove lets an overwritten sample release its private source witness.
//! The common tooltip attaches identical region behavior to each declaration.
//! Tooltip hover can be reviewed through the production pointer path later.
//!
//! BelowText places the highlight behind the source glyphs without covering them.
//! AboveText draws the borders and rules above the unchanged grid text.
//! Top allows gutter content in padding, beyond the ordinary grid scissor.
//! The fixture keeps these choices explicit to expose slot and clipping errors.
//! Paint values are packed straight RGBA8, as in the public primitive contract.
//! Each treatment has a distinct color so compositing mistakes are visible.
//! The fill is opaque and the glyph effect alone supplies a reduced opacity.
//! The source foreground remains the harness's normal terminal color.
//!
//! Inline annotations select primary fallback with no explicit face override.
//! Measurement therefore uses the same loaded fonts as their actual drawing.
//! Before depends on measured width rather than a fixed character-count shift.
//! Above depends on the free line's allocated height rather than its ink box.
//! Below advances one source-row height before placing the natural text line.
//! EndOfLine follows the final written run even though its anchor is earlier.
//! This catches positioning at the grid's far edge or the range's endpoint.
//! The text color is separate from the source's unchanged foreground.
//!
//! SVG Cell fit targets one cell and must preserve the authored aspect ratio.
//! Range fit targets five source cells, leaving centered space when necessary.
//! Custom fit applies a fixed affine relative to the first source-cell origin.
//! The same triangle path makes differences among those fits easy to compare.
//! Each declaration parses its SVG once at the public mutation boundary.
//! Subsequent scroll or hover builds reuse the immutable parsed path handle.
//! The solid brush needs no gradient row and exercises the normal paint lane.
//!
//! The final effect rotates source ink around its original glyph placement.
//! It cannot rotate grid cells, source markers, labels or copy extraction.
//! The opacity change makes an accidental untouched grid instance conspicuous.
//! The effect uses unhinted outlines at both device scales under §8.9.
//! The neighboring source runs remain unchanged for a direct visual comparison.
//! Errors propagate through the harness rather than silently dropping a sample.
//!
//! Every kind and fit is labeled in VT text. Three-row spacing exposes inline
//! Above/Below placement and glyph effects without overlapping another sample.
//! Gutter icons use the bundled Symbols Nerd Font, never a system font asset.
use crate::{
    Session,
    overlay::{
        anchor::{Anchor, MarkerRange},
        decoration::*,
        draw::{Effect, Paint},
        geometry::Cell,
    },
};
use kurbo::Affine;
use puzzle_frame::ZSlot;
use web_time::Instant;
/// Installs the declaration specimen at the current bundled-font/DPR context.
pub(super) fn install(session: &mut Session) -> Result<(), String> {
    let labels = [
        "highlight",
        "underline",
        "box",
        "badge/icon",
        "after/before",
        "above/below",
        "end-of-line",
        "SVG fits",
        "glyph effect",
    ];
    let mut output = String::from("\x1b[?25l\x1b[2J\x1b[H");
    for (index, label) in labels.iter().enumerate() {
        // Fixed source columns leave room for five underline styles and natural free text.
        if index > 0 {
            output.push_str("\r\n\r\n\r\n");
        }
        output.push_str(&format!(
            "{label:<16}BBBBB     BBBBB     BBBBB     BBBBB     BBBBB"
        ));
    }
    session.feed(output.as_bytes(), Instant::now());
    let ids = session.buffer().line_ids().ok_or("specimen has no rows")?;
    let lines: Vec<_> = session.buffer().lines(ids).map(|line| line.id()).collect();
    // Capture live source identities after all fixture output has been parsed.
    let mut add = |row: usize, column: usize, kind, slot| -> Result<(), String> {
        let line = *lines.get(row).ok_or("specimen row outside grid")?;
        let start = session
            .add_marker(Cell { line, column })
            .ok_or("source marker")?;
        let end = session
            .add_marker(Cell {
                line,
                column: column + 4,
            })
            .ok_or("end marker")?;
        session.decorations_mut().add(Decoration {
            anchor: Anchor::Range(MarkerRange { start, end }),
            kind,
            slot,
            auto_remove: true,
            region: Some(RegionSpec::Tooltip("declarative annotation".into())),
        })?;
        Ok(())
    };
    cell_kinds(&mut add)?;
    inline(&mut add)?;
    svg_effects(&mut add)?;
    Ok(())
}
/// Every placement appears independently, with natural measurement from bundled faces.
fn inline(
    add: &mut impl FnMut(usize, usize, DecorationKind, ZSlot) -> Result<(), String>,
) -> Result<(), String> {
    for (row, column, placement, label) in [
        (12, 16, Placement::After, "after"),
        (12, 36, Placement::Before, "before"),
        (15, 16, Placement::Above, "above"),
        (15, 36, Placement::Below, "below"),
        (18, 16, Placement::EndOfLine, "at EOL"),
    ] {
        // Annotation text never changes VT contents, advances or copy extraction.
        add(
            row,
            column,
            DecorationKind::InlineText {
                text: label.into(),
                style: TextStyle {
                    size: 12.0,
                    face: None,
                    style: 0,
                    color: 0xff99ccff,
                },
                placement,
            },
            ZSlot::Top,
        )?;
    }
    Ok(())
}

/// Cell-aligned samples compare background, underline and border treatments.
fn cell_kinds(
    add: &mut impl FnMut(usize, usize, DecorationKind, ZSlot) -> Result<(), String>,
) -> Result<(), String> {
    add(
        0,
        16,
        DecorationKind::Highlight { fill: 0xff553311 },
        ZSlot::BelowText,
    )?;
    for (index, style) in [
        UnderlineStyle::Single,
        UnderlineStyle::Double,
        UnderlineStyle::Curly,
        UnderlineStyle::Dotted,
        UnderlineStyle::Dashed,
    ]
    .into_iter()
    .enumerate()
    {
        add(
            3,
            16 + index * 10,
            DecorationKind::Underline {
                style,
                color: 0xff55aaff,
            },
            ZSlot::AboveText,
        )?;
    }
    add(
        6,
        16,
        DecorationKind::Box {
            stroke: 0xff77cc99,
            radius: 3.0,
        },
        ZSlot::AboveText,
    )?;
    add(
        9,
        16,
        DecorationKind::GutterBadge {
            text: "\u{f013}".into(),
            color: 0xff99ddff,
        },
        ZSlot::Top,
    )?;
    Ok(())
}
/// SVG fits and ink transforms preserve the surrounding VT specimen text.
fn svg_effects(
    add: &mut impl FnMut(usize, usize, DecorationKind, ZSlot) -> Result<(), String>,
) -> Result<(), String> {
    for (index, fit) in [
        ShapeFit::Cell,
        ShapeFit::Range,
        ShapeFit::Custom(Affine::scale(2.0)),
    ]
    .into_iter()
    .enumerate()
    {
        add(
            21,
            16 + index * 10,
            DecorationKind::Shape {
                svg_path: "M0 0 L8 0 L4 8 Z".into(),
                fit,
                paint: Paint {
                    color: 0xffdd9955,
                    gradient: None,
                },
            },
            ZSlot::AboveText,
        )?;
    }
    add(
        24,
        16,
        DecorationKind::GlyphEffect(Effect {
            opacity: 0.4,
            transform: Affine::rotate(0.15),
            ..Default::default()
        }),
        ZSlot::AboveText,
    )?;
    Ok(())
}
