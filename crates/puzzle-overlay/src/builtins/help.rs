//! Leader hints and searchable shortcut help use only public overlay primitives.
//!
//! Session owns the editing mode and effective binding table. These views cannot
//! perform shortcuts or send application input. A finite clip keeps large tables
//! inside the surface; filtering makes any effective entry directly discoverable.
//! Every retained row includes its configuration source, including defaults.
//!
//! Both overlays occupy Top, leaving grid placement and copied text unchanged.
//! The leader uses a cursor anchor so it follows the same sealed caret as preedit.
//! A cheat-sheet panel uses surface coordinates and remains clipped to its bounds.
//! Long entries cannot paint over neighboring windows or escape the surface scissor.
//! Natural text uses the render owner's shared primary and fallback faces.
//! Hidden table rows are skipped before text shaping, not merely discarded by GPU.
//! Filtering can select any row whose group would otherwise be below the fold.
//! Neither view receives scheduling capabilities, editable keys or a live Session.
//! Showing help adds no keyboard-focus API for arbitrary extension providers.
//!
//! Leader paper and caption share the Cursor anchor. Its origin stays local to
//! that anchor, so converting it to a viewport rectangle would detach the pill
//! during scroll or cursor movement. The short vertical offset clears grid ink.
//! Rounded paper uses the existing analytic rectangle pipeline, not another mesh.
//! A nested clip constrains captions whose many continuations exceed the surface.
//! The measured natural width includes modifiers and complete action arguments.
//! No ellipsis mutates the matcher labels or hides an entry from introspection.
//!
//! The cheat sheet is a surface panel with an independent filter line. Its caret,
//! horizontal scroll and provisional underline use the search field's painter.
//! Keeping those paths shared prevents IME placement drifting between modes.
//! The result count describes the filtered effective table, including hidden rows.
//! Rows are grouped by action purpose rather than by configuration provenance;
//! a layer label remains beside each row so overriding a default is discoverable.
//! The filter searches trigger, action, group and source on the Session owner.
//! Filtering therefore reaches entries below the fold without drawing all rows.
//! Each group header consumes its own line before that group's first entry.
//! Row labels keep their canonical chord and action argument without executing it.
//! The outer clip also applies to the panel's pointer region at small sizes.
//! Its model consumes pointers but has no keyboard callback: Session routes keys.
//! Closing or changing the filter is ordinary accepted input, not a view mutation.
//! A unit payload is allocated once and reused because all editable data already
//! lives in the immutable source model delivered with each frame publication.
//!
//! Native UI exclusions affect only a floating hint's offset, never its caret.
//! The caret origin and clip come from one sealed frame, preserving hit alignment.
//! Corner help uses the same placement policy as the search field and HUD.
use super::*;
use crate::{model::ViewState, view::OverlayView};

/// A leader hint is anchored to the same cursor geometry as composition.
pub struct LeaderView;
impl OverlayView for LeaderView {
    /// Lists precisely the still-valid continuations sampled by the matcher owner.
    fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let help = &ctx.models.help;
        if help.leader.is_empty() || help.open || ctx.models.search.open {
            return;
        }
        // The cursor-relative pill uses natural text metrics, then clips long continuations.
        let bounds = Rect {
            origin: [0.0, 18.0],
            size: [
                (super::query_field::width(ctx, &help.leader) + 16.0)
                    .min(ctx.geometry.viewport.size[0]),
                24.0,
            ],
        };
        let Some(bounds) = super::placement::cursor(ctx, bounds) else {
            return;
        };
        out.push(
            Anchor::Cursor,
            ZSlot::Top,
            Primitive::Rect(RectSpec {
                rect: bounds,
                color: ctx.theme.color("pillBackground", 0xff202020),
                radius: 6.0,
                stroke: 0.0,
            }),
        );
        out.push(Anchor::Cursor, ZSlot::Top, Primitive::ClipPush(bounds));
        out.push(
            Anchor::Cursor,
            ZSlot::Top,
            Primitive::Text(TextSpec {
                text: help.leader.clone(),
                size: 12.0,
                face: None,
                style: 0,
                color: ctx.theme.color("pillForeground", 0xffffffff),
                transform: kurbo::Affine::translate((
                    f64::from(bounds.origin[0] + 8.0),
                    f64::from(bounds.origin[1] + 4.0),
                )),
            }),
        );
        out.push(Anchor::Cursor, ZSlot::Top, Primitive::ClipPop);
    }
}

/// Searchable effective bindings are a Session query mode, not provider focus.
pub struct HelpView;
impl OverlayView for HelpView {
    /// Draws filtered rows grouped by semantic action, with every source visible.
    fn build(&mut self, _: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        let help = &ctx.models.help;
        if !help.open {
            return;
        }
        let Some(bounds) = panel(ctx, 680.0, ctx.geometry.viewport.size[1] - 24.0) else {
            return;
        };
        rect(
            out,
            bounds,
            ctx.theme.color("badgeBackground", 0xff202020),
            6.0,
        );
        push(out, Primitive::ClipPush(bounds));
        let color = ctx.theme.color("badgeForeground", 0xffffffff);
        // The field's source is the same bounded Unicode editor as the search bar.
        super::query_field::draw(
            ctx,
            out,
            bounds,
            color,
            super::query_field::Field {
                label: "Keybindings",
                text: &help.filter,
                preedit: &help.preedit,
                cursor: help.cursor,
            },
        );
        // UI clicks cannot leak through the panel into selection or application reports.
        push(
            out,
            Primitive::Region(
                bounds,
                crate::draw::RegionId(0),
                crate::draw::CursorIcon::Text,
            ),
        );
        text(
            out,
            format!("{} matches · Escape closes", help.rows.len()),
            [bounds.origin[0] + 10.0, bounds.origin[1] + 28.0],
            color,
        );
        rows(ctx, out, bounds, color);
        push(out, Primitive::ClipPop);
    }
}

/// Group headers share table order so filtering never mixes unrelated shortcuts.
fn rows(ctx: &ViewCtx<'_>, out: &mut DrawList, bounds: Rect, color: u32) {
    let mut y = bounds.origin[1] + 52.0;
    let mut group = "";
    for row in &ctx.models.help.rows {
        if row.group != group {
            group = row.group;
            text(
                out,
                group.into(),
                [bounds.origin[0] + 10.0, y],
                ctx.theme.color("accent", color),
            );
            y += 20.0;
        }
        text(
            out,
            format!("{} = {} [{}]", row.trigger, row.action, row.source),
            [bounds.origin[0] + 18.0, y],
            color,
        );
        y += 18.0;
        // Clipping is authoritative; skipping wholly hidden rows avoids shaping them.
        if y >= bounds.origin[1] + bounds.size[1] {
            break;
        }
    }
}

/// Interaction model consumes panel pointers; Session owns every editing key.
#[derive(Default)]
pub struct Help {
    /// One reusable payload keeps dirty state capture allocation-free.
    state: Option<crate::model::SharedState>,
}
/// No general keyboard focus or scheduling capability is delegated to this model.
impl crate::model::OverlayModel for Help {
    /// Distinct identity from the search field and leader pill.
    fn name(&self) -> &'static str {
        "keybinding-help"
    }
    /// Queries and source edges invalidate this purely declarative presentation.
    fn wants(&self) -> crate::provider::EventKinds {
        crate::provider::EventKinds::ALL
    }
    /// A unit state suffices because immutable source models already carry the table.
    fn view_state(&mut self) -> crate::model::SharedState {
        self.state
            .get_or_insert_with(|| std::sync::Arc::new(()))
            .clone()
    }
    /// A visible panel owns input over its rectangle, avoiding PTY mouse reports.
    fn on_pointer(
        &mut self,
        _: &crate::provider::PointerEvent,
        _: crate::draw::RegionId,
        _: &mut crate::model::ModelCtx<'_>,
    ) -> crate::provider::Handled {
        crate::provider::Handled::Yes
    }
}
