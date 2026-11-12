//! Public provider seam and injected animation requests (design §11.2, §11.5).
//!
//! Session-owned models publish immutable state; render-owned views produce
//! logical draw commands. Shaping and atlas writes stay with the render owner.
//! The sealed visible BufferView is borrowed only during each view callback.
use crate::draw::CursorIcon;
use std::cell::Cell;

/// Coarse changes that invalidate provider output; no per-cell bus traffic.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EventKinds(pub u64);
/// Defines coarse interests independently of terminal bus payload types.
impl EventKinds {
    /// Terminal rows or shell metadata changed.
    pub const GRID: Self = Self(1);
    /// Composition or suggestion changed, without mutating a terminal row.
    pub const INPUT: Self = Self(128);
    /// Bell or application progress changed independently of grid damage.
    pub const STATUS: Self = Self(256);
    /// Surface, cell metrics or display scale changed.
    pub const GEOMETRY: Self = Self(2);
    /// Viewport position changed.
    pub const SCROLL: Self = Self(4);
    /// Pointer entered, moved, left or changed buttons.
    pub const POINTER: Self = Self(8);
    /// Theme or reduced-motion policy changed.
    pub const THEME: Self = Self(16);
    /// Selection or search presentation changed.
    pub const QUERY: Self = Self(32);
    /// Every defined coarse event.
    pub const ALL: Self = Self(511);
    /// Declarative annotations were added, updated, removed or invalidated.
    pub const DECORATIONS: Self = Self(64);
    /// Tests overlapping interests without needing a crate-specific bus type.
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}
/// Animation demands are returned to Session's existing Waker seam.
#[derive(Default)]
pub struct Requests {
    /// A single refresh is enough when no animation interval was requested.
    pub frame: Cell<bool>,
    /// Latest animation end, in the same injected seconds as ModelCtx::time.
    pub until: Cell<f64>,
    /// One-shot future rebuild; zero means no deadline and creates no timer.
    pub wake: Cell<f64>,
    /// At most one typed model action from the current pointer callback.
    pub action: Cell<Option<crate::models::Action>>,
    /// Optional source owner receives only input from presented provider regions.
    pub source_pointer: Cell<Option<SourcePointer>>,
}
/// A presented hit addressed to its source owner, never a drawing callback.
#[derive(Clone, Copy, Debug)]
pub struct SourcePointer {
    /// Registered source channel; interpreted only by the composition owner.
    pub channel: u16,
    /// Immutable presented generation or provider-local token.
    pub region: crate::draw::RegionId,
    /// Logical input copied by the model, preserving the actual user gesture.
    pub event: PointerEvent,
}
/// Portable theme tokens supplied by the composition layer.
#[derive(Clone, Debug, Default)]
pub struct Theme {
    /// Stable semantic token names paired with straight RGBA8 colors.
    pub tokens: Vec<(&'static str, u32)>,
}
/// Token lookup exposes resolved colors without owning settings.
impl Theme {
    /// Resolves a token; an absent name falls back to the caller's chosen color.
    pub fn color(&self, name: &str, fallback: u32) -> u32 {
        self.tokens
            .iter()
            .find(|(key, _)| *key == name)
            .map_or(fallback, |(_, value)| *value)
    }
}
/// Synthetic pointer kinds are shared by desktop and DOM adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerKind {
    /// Pointer crossed into a region.
    Enter,
    /// Pointer crossed out of a region.
    Leave,
    /// Pointer moved within a region or a captured drag.
    Move,
    /// A button was pressed.
    Press,
    /// A button was released.
    Release,
}
/// Coordinates have already been converted to logical surface pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerEvent {
    /// Routing kind, including synthesized enter/leave transitions.
    pub kind: PointerKind,
    /// Logical surface coordinate, including padding.
    pub point: [f32; 2],
    /// Primary=0, middle=1, secondary=2; absent for ordinary hover.
    pub button: Option<u8>,
    /// Portable Shift/Alt/Ctrl/Super modifier mask.
    pub modifiers: u8,
}
/// A provider must explicitly consume an event to prevent terminal input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Handled {
    /// Selection or application reporting may continue.
    No,
    /// The overlay owns the event; no selection or report is permitted.
    Yes,
}
/// Pointer routing result, including the shell's current cursor shape.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerResult {
    /// True prevents selection and application mouse reporting.
    pub handled: bool,
    /// Current topmost region's portable cursor shape.
    pub cursor: CursorIcon,
}
