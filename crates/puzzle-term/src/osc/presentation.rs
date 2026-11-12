//! Registered source models publish through the existing D-50 overlay split.
//! No semantic vocabulary, protocol credential or renderer enters this seam.
//! Interaction is invoked only by trusted user routing, never by PTY dispatch.
use super::{OscContext, Registry};
use puzzle_overlay::{model::OverlayModel, provider::PointerEvent, view::OverlayView};

/// One existing model/view registration, transferred by value exactly once.
pub type OscOverlay = (Box<dyn OverlayModel>, Box<dyn OverlayView>);

/// The source slice the next publication intends to display, in retained-row order.
#[derive(Clone, Copy)]
pub struct SourceWindow {
    /// Physical retained index, never a numeric LineId approximation.
    pub top: usize,
    /// Physical extent of the visible source window, including omitted rows
    /// between projected endpoints and clipped fractional edges. Drawing uses
    /// the snapshot's actual visible identities, so omitted rows draw nothing.
    pub rows: usize,
}

/// Only the Session's actual user-input routes construct these callbacks.
#[derive(Clone, Copy)]
pub enum OscUserAction<'a> {
    /// A configured extension keybinding, after all input interceptors.
    Named(&'a str),
    /// A region from the last presented frame; stale tokens remain inert.
    Pointer {
        /// Provider-local immutable presented identity.
        region: puzzle_overlay::draw::RegionId,
        /// Real logical user gesture, including button and modifiers.
        event: PointerEvent,
        /// User copy policy sampled on the source owner at this gesture.
        trim_trailing: bool,
    },
}

/// A source owner requests ordinary viewport work, without borrowing Session.
#[derive(Default)]
pub struct OscUiEffect {
    /// Source validated by the registered owner; normal navigation policy applies.
    pub scroll_to: Option<puzzle_vt::tracked::LineId>,
    /// A model changed and needs one publication; no recurring timer is implied.
    pub changed: bool,
    /// Immutable optional meaning fact, delivered through the ordinary extension bus.
    /// A user action can report its outcome without exposing its source owner.
    pub event: Option<puzzle_bus::wire::Extension>,
}

impl Registry {
    /// Dirty source values enter existing models before snapshot versions are sealed.
    pub(crate) fn publish_overlays(
        &mut self,
        terminal: &mut puzzle_vt::Terminal,
        overlays: &mut puzzle_overlay::OverlayRegistry,
        window: SourceWindow,
    ) {
        for registration in &mut self.handlers {
            // Nonvisual handlers are skipped before constructing any source context.
            let Some(provider) = registration.provider else {
                continue;
            };
            // This exclusive borrow ends before fonts or view callbacks can run.
            let mut source = OscContext {
                terminal,
                endpoints: &mut registration.endpoints,
                replies: None,
            };
            // None means the already published immutable version is still current.
            // Its Arc remains in the model and each rotating slot without a copy.
            if let Some(state) = registration.handler.overlay_state(&mut source, window) {
                // The model owns input; only its accepted immutable value reaches drawing.
                overlays.replace_state(provider, state);
            }
        }
    }

    /// Named actions try registered owners; presented pointer actions name exactly one.
    pub(crate) fn user_action(
        &mut self,
        channel: Option<u16>,
        action: OscUserAction<'_>,
        terminal: &mut puzzle_vt::Terminal,
    ) -> Option<OscUiEffect> {
        // Registration order breaks ties for names; addressed pointer channels
        // cannot be offered to a sibling handler that happens to use the same token.
        for (number, registration) in self.numbers.iter().zip(&mut self.handlers) {
            if channel.is_some_and(|channel| channel != *number) {
                continue;
            }
            // A reset/screen switch earlier in this same input cascade has
            // not necessarily delivered its handler lifecycle callback yet.
            // Refusal preserves that later notification and cannot act on old text.
            if !registration.endpoints.is_current(terminal) {
                continue;
            }
            let mut source = OscContext {
                terminal,
                endpoints: &mut registration.endpoints,
                replies: None,
            };
            if let Some(effect) = registration.handler.user_action(action, &mut source) {
                return Some(effect);
            }
        }
        None
    }
}

/// A source interval clipped to one publication, with exclusive column bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    /// First displayed source row and its included starting column.
    pub start: (puzzle_vt::tracked::LineId, usize),
    /// Last displayed source row and its excluded ending column.
    pub end: (puzzle_vt::tracked::LineId, usize),
    /// Actual source order, independent of nonmonotonic LineIds after region edits.
    pub rows: (usize, usize),
}

/// Fixed watch metadata clips source to its upcoming publication window.
mod source;
