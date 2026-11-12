//! Desktop search field draws Session's input mode through public primitives.
//!
//! Keyboard handling belongs to Session, not provider focus. The provider only
//! paints the sampled query/preedit and emits navigation or close on buttons.
//!
//! The painter reads the committed query, insertion byte boundary, provisional
//! composition, discovered count and current index from one frozen model sample.
//! It has no editable String of its own and never advances the search engine.
//! Native keyboards and IME commits already pass through Session's input mode.
//! A page's searchRequested handler remains the web interface under D-34.
//!
//! The query occupies the first line; result count and controls occupy the second.
//! An outer panel clip constrains all ink in a small window. A nested field clip
//! prevents long queries from covering result controls or neighboring grid text.
//! Nested ClipPop restores the panel scissor before the controls are submitted.
//! Every primitive uses Top so padding can contain the field without grid clipping.
//!
//! Query insertion is a UTF-8 byte boundary, never a terminal column. Prefix and
//! suffix slices place provisional text at that boundary, including mid-query
//! composition. Measurement uses the same 12-pixel natural layout as rendering:
//! scalar counts cannot predict widths for combining marks or fallback glyphs.
//! A translated line scrolls just enough to keep the insertion caret in the field.
//! The caret is one device pixel; its height belongs to this UI's logical line.
//! Composition receives a semantic underline and never changes the match count.
//!
//! Measurement borrows the render owner's shared font caches, independently of
//! the Session model. It runs only when dirty. No private font context, atlas
//! or glyph cache is created here. The source's terminal cells and cursor remain
//! unchanged, as do selection and copy extraction. The field caret has no timer.
//!
//! Regions are ordered after their paper. The parent field uses a text cursor;
//! later button regions win on overlap and use a pointer cursor. Region zero
//! consumes field clicks but cannot grant arbitrary provider keyboard focus.
//! Regions one through three return signed navigation or close requests. Session
//! applies each typed request after the model callback returns, through its bus.
//! Enter/Shift+Enter share the navigation operation used by these buttons.
//!
//! Partial incremental searches retain their discovered count with an ellipsis.
//! An absent current match displays zero; a discovered current index displays
//! one-based numbering. Closing the mode clears the results in Session, and the
//! next provider rebuild removes this entire panel and its hit regions.
use super::*;
use crate::{
    draw::{CursorIcon, RegionId},
    model::{ModelCtx, OverlayModel, SharedState, ViewState},
    models::Action,
    provider::*,
    view::{OverlayView, ViewCtx},
};
/// Interaction half for the one Session-owned query mode.
pub struct Search {
    /// One immutable unit payload is reused even on dirty grid/source frames.
    state: SharedState,
}
/// Installation is cold; rebuilding a stateless view never creates a new payload.
impl Default for Search {
    /// Allocate the unit state once for this registration's lifetime.
    fn default() -> Self {
        Self {
            state: std::sync::Arc::new(()),
        }
    }
}
impl OverlayModel for Search {
    /// Identifies the native built-in rather than the embedding page's search UI.
    fn name(&self) -> &'static str {
        "search-bar"
    }
    /// Query/result edges, geometry and theme invalidate the retained field.
    fn wants(&self) -> EventKinds {
        EventKinds::ALL
    }
    /// Source data lives in the Session snapshot; this model has no private render payload.
    fn view_state(&mut self) -> SharedState {
        self.state.clone()
    }
    /// Buttons return actions; typing is already intercepted by the Session mode.
    fn on_pointer(
        &mut self,
        ev: &PointerEvent,
        region: RegionId,
        ctx: &mut ModelCtx<'_>,
    ) -> Handled {
        // A release completes the captured click; hovering cannot navigate results.
        if ev.kind == PointerKind::Release && ev.button == Some(0) {
            ctx.requests.action.set(match region.0 {
                1 => Some(Action::SearchStep(-1)),
                2 => Some(Action::SearchStep(1)),
                3 => Some(Action::SearchClose),
                _ => None,
            });
        }
        // Hover on the field belongs to UI, preserving the application's mouse state.
        if matches!(
            ev.kind,
            PointerKind::Press | PointerKind::Release | PointerKind::Move
        ) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}

/// Stateless drawing half; source data and interaction remain in the Session.
pub struct SearchView;
/// The view can be recreated without losing any query, click or following state.
impl OverlayView for SearchView {
    /// Draws query/candidate, match count and three independently clickable controls.
    fn build(&mut self, _state: &dyn ViewState, ctx: &ViewCtx<'_>, out: &mut DrawList) {
        // A closed mode has neither ink nor regions in the retained list.
        let state = &ctx.models.search;
        if !state.open {
            return;
        }
        let Some(bounds) = panel(ctx, 420.0, 60.0) else {
            return;
        };
        rect(
            out,
            bounds,
            ctx.theme.color("badgeBackground", 0xff202020),
            6.0,
        );
        // Clip long Unicode queries to the field rather than spilling over controls.
        push(out, Primitive::ClipPush(bounds));
        let color = ctx.theme.color("badgeForeground", 0xffffffff);
        super::query_field::draw(
            ctx,
            out,
            bounds,
            color,
            super::query_field::Field {
                label: "Find",
                text: &state.query,
                preedit: &state.preedit,
                cursor: state.cursor,
            },
        );
        // Partial progress is visible without forcing a synchronous history scan.
        let count = format!(
            "{} / {}{}",
            state.current.map_or(0, |i| i + 1),
            state.count,
            if state.complete { "" } else { " …" }
        );
        text(
            out,
            count,
            [bounds.origin[0] + 10.0, bounds.origin[1] + 34.0],
            color,
        );
        // The field owns button input but never grants general provider keyboard focus.
        push(
            out,
            Primitive::Region(bounds, RegionId(0), CursorIcon::Text),
        );
        buttons(ctx, out, bounds, color);
        push(out, Primitive::ClipPop);
    }
}

/// Controls occupy a separate line so the clipped query never obscures navigation.
fn buttons(ctx: &ViewCtx<'_>, out: &mut DrawList, bounds: Rect, color: u32) {
    for (index, label) in ["Prev", "Next", "Close"].into_iter().enumerate() {
        // Keep the three targets on the right even when the panel width is clamped.
        let button = Rect {
            origin: [
                bounds.origin[0] + bounds.size[0] - 168.0 + index as f32 * 54.0,
                bounds.origin[1] + 30.0,
            ],
            size: [50.0, 24.0],
        };
        rect(
            out,
            button,
            ctx.theme.color("tooltipBackground", 0xff383838),
            3.0,
        );
        text(
            out,
            label.into(),
            [button.origin[0] + 6.0, button.origin[1] + 4.0],
            color,
        );
        // Later regions win picking, placing each control above the parent field.
        push(
            out,
            Primitive::Region(button, RegionId(index as u64 + 1), CursorIcon::Pointer),
        );
    }
}
