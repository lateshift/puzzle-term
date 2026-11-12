//! Read-only shortcut presentation shares the matcher's actual prefix grammar.
//!
//! Release events remain buffered for ordered replay but never count as leader
//! steps. Presentation must apply physical precedence just as candidate() does;
//! otherwise the hint would offer actions that cannot complete the held leader.
//! Label construction happens only on a settings/input edge, never per frame.
//! The iterator borrows original bindings, preserving their effective source layer.
//! It excludes completed entries: a hint offers next steps, never repeats an action
//! that already consumed the prefix. With no held press the continuation set is empty.
//! Action grouping and labels remain available to CLI and embedding introspection,
//! rather than being private strings invented by the drawing view.
//!
//! The displayed prefix is a strict prefix: completed bindings are absent because
//! their action has already consumed the held keys. A release stays buffered for
//! application ordering, yet it never increases the grammar-step count shown here.
//! Physical precedence belongs to the current step within this continuation set;
//! an unrelated global physical binding cannot steal a pending logical sequence.
//! Calling the matcher candidate routine preserves that rule instead of introducing
//! a second shortcut grammar that could advertise an impossible continuation.
//! Entries remain borrowed, so opening a leader cannot copy the effective table.
//!
//! Groups follow the action catalogue in settings documentation. Every enum case
//! appears in the exhaustive grouping match, forcing new actions to choose a group.
//! Labels retain arguments such as zoom direction and the copy format because
//! omitting them would make distinct configured operations look identical in help.
//! Control bytes in sending actions are escaped visibly, never interpreted as ink.
//! Source provenance remains attached to Binding, not inferred from label spelling.
//! Canonical trigger formatting is shared with CLI introspection and configuration.
use super::{Action, Binding, Keybindings};
use crate::vt::input::KeyPhase;

impl Keybindings {
    /// Borrows exactly the bindings which can continue the current leader.
    pub fn continuations(&self) -> impl Iterator<Item = (&Binding, usize)> {
        // Count grammar steps without allocating a second pending-key vector.
        let count = self
            .pending
            .iter()
            .filter(|key| key.phase != KeyPhase::Release)
            .count();
        // The current accepted prefix's physical precedence is fixed before listing.
        let physical = self
            .candidate(&self.pending)
            .is_some_and(|binding| count != 0 && binding.steps[count - 1].physical);
        self.entries.iter().filter_map(move |binding| {
            (count != 0
                && binding.steps.len() > count
                && binding.steps[count - 1].physical == physical
                && binding
                    .steps
                    .iter()
                    .zip(
                        self.pending
                            .iter()
                            .filter(|key| key.phase != KeyPhase::Release),
                    )
                    .all(|(step, key)| step.matches(key)))
            .then_some((binding, count))
        })
    }
}

impl Action {
    /// Semantic grouping of the shared action catalogue, not a separate UI table.
    pub fn group(&self) -> &'static str {
        match self {
            Self::Copy(_)
            | Self::Paste
            | Self::PasteSelection
            | Self::SelectAll
            | Self::SelectCommandOutput
            | Self::CopyCommandOutput => "Clipboard and selection",
            Self::SendText(_) | Self::Csi(_) | Self::Esc(_) => "Sending input",
            Self::ClearScreen | Self::ClearScrollback | Self::Reset => "Screen",
            Self::ScrollLines(_)
            | Self::ScrollPages(_)
            | Self::ScrollTop
            | Self::ScrollBottom
            | Self::JumpToPrompt(_) => "Scrolling and navigation",
            Self::Search | Self::SearchNext | Self::SearchPrev => "Search",
            Self::Zoom(_)
            | Self::ZoomReset
            | Self::SetAppearance(_)
            | Self::ToggleAppearance
            | Self::Profile(_)
            | Self::ReloadSettings
            | Self::OpenSettings => "Appearance and settings",
            Self::ShowKeybindings | Self::ToggleScreenReaderMode => "Help and accessibility",
            Self::ToggleFullscreen | Self::NewWindow | Self::CloseWindow => "Window",
            Self::Ext(_) => "Extension",
            Self::Ignore | Self::Unbind => "Routing",
        }
    }
    /// Describes the effective action, retaining arguments needed to distinguish entries.
    pub fn label(&self) -> String {
        let name = self.info().name;
        match self {
            // Keep configured text as data; a draw list cannot execute its escape bytes.
            Self::Copy(value)
            | Self::SendText(value)
            | Self::Csi(value)
            | Self::Esc(value)
            | Self::Profile(value)
            | Self::Ext(value) => format!("{name}:{}", value.escape_debug()),
            Self::ScrollLines(value) | Self::JumpToPrompt(value) | Self::Zoom(value) => {
                format!("{name}:{value}")
            }
            Self::ScrollPages(value) => format!("{name}:{value}"),
            Self::SetAppearance(value) => format!("{name}:{value:?}"),
            _ => name.into(),
        }
    }
}
