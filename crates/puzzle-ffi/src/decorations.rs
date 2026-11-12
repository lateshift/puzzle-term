//! C declarations map one-to-one to the existing annotation controller (§11.3).
//!
//! Marker identities refer to tracked Session endpoints, never cells borrowed
//! from a render snapshot. Additions return correlated results during tick;
//! invalid SVG/text/geometry is rejected by the same core validator as Rust.
//!
//! The C declaration is plain data: numeric discriminants select the shared
//! Rust kind, anchor and slot. Invalid discriminants return InvalidArgument
//! rather than constructing an invalid Rust enum. Marker IDs are checked in
//! the current surface's table before creating a core anchor.
//! Cursor, viewport and pointer anchors need no retained marker. Cell/range/line
//! anchors refer to tracked positions owned by Session and therefore follow the
//! same scroll, reflow and eviction rules as the built-in overlays.
//!
//! Additions are asynchronous because only the Session owner can validate an
//! anchor against live contents. The caller's request number is returned with
//! either the decoration ID or the core's validation error during App::tick.
//! Updating passes one complete replacement through the same validator, so a
//! malformed path cannot partially overwrite an existing annotation.
//! Removal uses the core's idempotent declaration identity; marker removal is
//! checked independently and does not recycle any still-live annotation ID.
//!
//! Text labels, SVG paths and tooltips are UTF-8 copies, never foreign pointers
//! kept for a later frame. All drawing remains on the render thread through
//! Session's immutable view state, not inside these exported functions.
//! Packed colors retain the core's RGBA ordering. Affine coefficients use the
//! shared geometry library's six-value ordering without a second transform engine.
//! Only kind-specific fields are interpreted; unused fields may remain zero.
//! Inline style bits select regular/bold/italic combinations and are range-checked.
//! Shape fitting selects cell, range or explicit affine coordinates. Effects
//! preserve independent optional color, opacity, transform and hidden state.
//! Underline style numbers include None, matching the core catalogue exactly.
//! Neither the FFI registry nor this translation layer schedules overlay timers.
use crate::{
    abi::{self, ResultCode},
    input::boolean,
    memory,
    registry::{self, SurfaceSlot},
};
use puzzle_term::{
    frame::ZSlot,
    overlay::{
        anchor::{Anchor, MarkerRange},
        decoration::*,
        draw::{Effect, Paint},
        geometry::{Cell, Rect},
    },
    text::geometry::Affine,
    vt::tracked::LineId,
};

/// Converts numeric attachment kinds using only this surface's checked marker IDs.
fn anchor(slot: &SurfaceSlot, value: abi::Decoration) -> Result<Anchor, ResultCode> {
    // C selects an anchor family, never the representation of a Rust enum.
    Ok(match value.anchor_kind {
        0 => Anchor::Cursor,
        1 => Anchor::Viewport(Rect {
            origin: [value.rect[0], value.rect[1]],
            size: [value.rect[2], value.rect[3]],
        }),
        // Only tracked capabilities from this source surface may address cells.
        2 => Anchor::Cell(*slot.markers.get(value.start)?),
        3 => Anchor::Range(MarkerRange {
            start: *slot.markers.get(value.start)?,
            end: *slot.markers.get(value.end)?,
        }),
        4 => Anchor::Line(*slot.markers.get(value.start)?),
        // OSC 133 zones already have core-owned identities and eviction semantics.
        5 => Anchor::Zone(value.start),
        6 => Anchor::Pointer,
        _ => return Err(ResultCode::InvalidArgument),
    })
}
/// Z-order values are validated instead of transmuting an arbitrary C integer.
fn zslot(value: u32) -> Result<ZSlot, ResultCode> {
    Ok(match value {
        0 => ZSlot::Background,
        1 => ZSlot::BelowText,
        2 => ZSlot::Text,
        3 => ZSlot::TextRaster,
        4 => ZSlot::Cursor,
        5 => ZSlot::AboveText,
        6 => ZSlot::Top,
        _ => return Err(ResultCode::InvalidArgument),
    })
}
/// Underline catalogue accepts only the shared six style discriminants.
fn underline(value: u32) -> Result<UnderlineStyle, ResultCode> {
    Ok(match value {
        0 => UnderlineStyle::None,
        1 => UnderlineStyle::Single,
        2 => UnderlineStyle::Double,
        3 => UnderlineStyle::Curly,
        4 => UnderlineStyle::Dotted,
        5 => UnderlineStyle::Dashed,
        _ => return Err(ResultCode::InvalidArgument),
    })
}
/// Kind-specific payload mapping contains no rendering or lifetime policy.
fn kind(value: abi::Decoration, text: String) -> Result<DecorationKind, ResultCode> {
    Ok(match value.kind {
        0 => DecorationKind::Highlight { fill: value.color },
        1 => DecorationKind::Underline {
            style: underline(value.style)?,
            color: value.color,
        },
        2 => DecorationKind::Box {
            stroke: value.color,
            radius: value.radius,
        },
        3 => DecorationKind::GutterBadge {
            text,
            color: value.color,
        },
        4 => inline_text(value, text)?,
        5 => DecorationKind::Shape {
            svg_path: text,
            fit: match value.fit {
                0 => ShapeFit::Cell,
                1 => ShapeFit::Range,
                2 => ShapeFit::Custom(Affine::new(value.transform)),
                _ => return Err(ResultCode::InvalidArgument),
            },
            paint: Paint {
                color: value.color,
                // This declaration uses solid RGBA; gradient drawing remains a core primitive.
                gradient: None,
            },
        },
        // Effects modify existing ink and never insert replacement VT content.
        6 => DecorationKind::GlyphEffect(Effect {
            color: boolean(value.replace_color)?.then_some(value.color),
            opacity: value.opacity,
            transform: Affine::new(value.transform),
            hidden: boolean(value.hidden)?,
        }),
        _ => return Err(ResultCode::InvalidArgument),
    })
}
/// Inline text maps its style and relative placement without allocating render data.
fn inline_text(value: abi::Decoration, text: String) -> Result<DecorationKind, ResultCode> {
    if value.style > 3 {
        return Err(ResultCode::InvalidArgument);
    }
    // Placement offsets the resolved anchor; it does not create another tracked cell.
    let placement = match value.placement {
        0 => Placement::After,
        1 => Placement::Before,
        2 => Placement::Above,
        3 => Placement::Below,
        4 => Placement::EndOfLine,
        _ => return Err(ResultCode::InvalidArgument),
    };
    Ok(DecorationKind::InlineText {
        text,
        style: TextStyle {
            size: value.size,
            // Font identity stays with the surface's shared text context (D-30).
            face: None,
            style: value.style as u8,
            color: value.color,
        },
        placement,
    })
}
/// Copies strings before the declaration can be transferred to another owner.
#[doc = "# Safety"]
/// Value and its Bytes fields must remain initialized/readable throughout this call.
///
/// Kind-specific validation belongs to the shared decoration controller.
/// The copied value contains no reference into the caller's annotation allocation.
#[allow(unsafe_code, reason = "D-43: declarative C annotation input")]
unsafe fn declaration(
    slot: &SurfaceSlot,
    value: *const abi::Decoration,
) -> Result<Decoration, ResultCode> {
    // SAFETY: caller promises one initialized declaration; read validates pointer shape.
    let value = unsafe { memory::read(value)? };
    // Resolve foreign marker IDs before any annotation can enter the owner inbox.
    let anchor = anchor(slot, value)?;
    // Labels are copied on mutation only; clean overlay frames reuse view state.
    // The Session validator bounds SVG complexity and checks finite transforms.
    // Bad replacements leave the previous declaration and tracked positions intact.
    // C requests therefore follow the same asynchronous error path as Rust hosts.
    // The wrapper never prepares a shape or borrows a render-side cache.
    // SAFETY: label/path bytes are caller-owned and copied synchronously.
    let text = unsafe { memory::text(value.text)? };
    // SAFETY: tooltip range is borrowed only for its validated UTF-8 copy.
    let tooltip = unsafe { memory::text(value.tooltip)? };
    Ok(Decoration {
        anchor,
        kind: kind(value, text)?,
        slot: zslot(value.slot)?,
        auto_remove: boolean(value.auto_remove)?,
        // Empty tooltips create no hit region; nonempty text follows normal hover rules.
        region: (!tooltip.is_empty()).then_some(RegionSpec::Tooltip(tooltip)),
    })
}
/// Registers a tracked cell; surface/markerAdded returns its local checked ID or null.
///
/// The request token correlates the owner's eventual markerAdded result.
/// Unavailable source cells answer with a null marker rather than a dangling handle.
#[allow(unsafe_code, reason = "D-43: asynchronous marker creation export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_marker_add(
    id: u64,
    request: u64,
    line: u64,
    column: usize,
) -> ResultCode {
    registry::with_surface(id, |surface| {
        surface.add_marker(
            request,
            Cell {
                line: LineId(line),
                column,
            },
        )
    })
}
/// Releases a local marker; declarations using it become Gone through core ownership.
///
/// Only this surface's current marker generation may be released.
/// Tracked declaration ownership remains governed by the shared marker registry.
#[allow(unsafe_code, reason = "D-43: checked marker disposal export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_marker_remove(id: u64, marker: u64) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let slot = registry.surface(id)?;
        slot.surface
            .remove_marker(*slot.markers.get(marker)?)
            .map_err(ResultCode::from)?;
        // Retire the C incarnation only after its remove message was accepted.
        slot.markers.remove(marker)?;
        Ok(())
    })())
}
/// Adds any supported declaration; surface/decorationAdded returns its core result.
#[doc = "# Safety"]
/// Declaration and its nested text ranges are live readable input memory.
///
/// The caller supplies a correlation token because core addition is asynchronous.
/// DecorationAdded carries the original validator's error without inventing C-side policy.
#[allow(unsafe_code, reason = "R8/D-43: annotation addition through C")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_decoration_add(
    id: u64,
    request: u64,
    value: *const abi::Decoration,
) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let slot = registry.surface(id)?;
        // SAFETY: input contract is forwarded; no borrowed C data enters the Session message.
        let value = unsafe { declaration(slot, value)? };
        slot.surface
            .add_decoration(request, value)
            .map_err(ResultCode::from)
    })())
}
/// Atomically replaces an existing declaration, preserving it if core validation rejects.
#[doc = "# Safety"]
/// Replacement structure and buffers remain readable for this call.
///
/// A replacement retains the existing decoration if the shared validator rejects it.
/// No incremental field mutation can leave a partially updated draw declaration.
#[allow(unsafe_code, reason = "D-43: annotation replacement input")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn puzzle_surface_decoration_update(
    id: u64,
    decoration: u64,
    value: *const abi::Decoration,
) -> ResultCode {
    registry::result((|| {
        let mut registry = registry::lock()?;
        let slot = registry.surface(id)?;
        // SAFETY: replacement is copied under the same documented C declaration contract.
        let value = unsafe { declaration(slot, value)? };
        slot.surface
            .update_decoration(DecorationId(decoration), value)
            .map_err(ResultCode::from)
    })())
}
/// Removes a declaration idempotently; caller-owned marker IDs remain valid.
///
/// Core decoration IDs are monotonic; removing a missing ID is harmless.
/// Caller-owned tracked marker capabilities are independent of declaration lifetime.
#[allow(unsafe_code, reason = "D-43: declaration disposal export")]
#[unsafe(no_mangle)]
pub extern "C" fn puzzle_surface_decoration_remove(id: u64, decoration: u64) -> ResultCode {
    registry::with_surface(id, |surface| {
        surface.remove_decoration(DecorationId(decoration))
    })
}
