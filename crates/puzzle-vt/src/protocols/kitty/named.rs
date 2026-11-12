//! Origin: kitty; <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>.
//! Named non-text keys assigned by kitty's functional-key table (§6.8).
//!
//! The enum keeps a key's physical identity separate from the wire number.
//! M7's DOM adapter can map KeyboardEvent.code to a variant, and tests
//! can independently compare each variant with the published assignment.
//! Source: https://sw.kovidgoyal.net/kitty/keyboard-protocol/#functional-key-definitions

/// Functional keys that lack an ordinary xterm byte sequence.
///
/// Explicit discriminants pin the protocol assignments across releases.
#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NamedKey {
    /// Caps Lock toggles uppercase keyboard state.
    CapsLock = 57358,
    /// Scroll Lock toggles legacy scroll state.
    ScrollLock = 57359,
    /// Num Lock changes keypad interpretation.
    NumLock = 57360,
    /// Print Screen has no printable scalar.
    PrintScreen = 57361,
    /// Pause is distinct from the media pause key.
    Pause = 57362,
    /// The context menu key has a dedicated identity.
    Menu = 57363,
    /// Decimal on the numeric keypad.
    KeypadDecimal = 57409,
    /// Division on the numeric keypad.
    KeypadDivide = 57410,
    /// Multiplication on the numeric keypad.
    KeypadMultiply = 57411,
    /// Subtraction on the numeric keypad.
    KeypadSubtract = 57412,
    /// Addition on the numeric keypad.
    KeypadAdd = 57413,
    /// Equals on the numeric keypad.
    KeypadEqual = 57415,
    /// Separator on the numeric keypad.
    KeypadSeparator = 57416,
    /// Left motion from the numeric keypad.
    KeypadLeft = 57417,
    /// Right motion from the numeric keypad.
    KeypadRight = 57418,
    /// Up motion from the numeric keypad.
    KeypadUp = 57419,
    /// Down motion from the numeric keypad.
    KeypadDown = 57420,
    /// Page Up from the numeric keypad.
    KeypadPageUp = 57421,
    /// Page Down from the numeric keypad.
    KeypadPageDown = 57422,
    /// Home from the numeric keypad.
    KeypadHome = 57423,
    /// End from the numeric keypad.
    KeypadEnd = 57424,
    /// Insert from the numeric keypad.
    KeypadInsert = 57425,
    /// Delete from the numeric keypad.
    KeypadDelete = 57426,
    /// Begin from the numeric keypad.
    KeypadBegin = 57427,
    /// Start media playback.
    MediaPlay = 57428,
    /// Pause media playback.
    MediaPause = 57429,
    /// Toggle media playback.
    MediaPlayPause = 57430,
    /// Reverse media playback.
    MediaReverse = 57431,
    /// Stop media playback.
    MediaStop = 57432,
    /// Fast forward media playback.
    MediaFastForward = 57433,
    /// Rewind media playback.
    MediaRewind = 57434,
    /// Skip to the next media track.
    MediaNext = 57435,
    /// Return to the previous media track.
    MediaPrevious = 57436,
    /// Start media recording.
    MediaRecord = 57437,
    /// Decrease output volume.
    LowerVolume = 57438,
    /// Increase output volume.
    RaiseVolume = 57439,
    /// Toggle output mute.
    MuteVolume = 57440,
    /// The physical left Shift key.
    LeftShift = 57441,
    /// The physical left Control key.
    LeftControl = 57442,
    /// The physical left Alt or Option key.
    LeftAlt = 57443,
    /// The physical left Super or Command key.
    LeftSuper = 57444,
    /// The physical left Hyper key.
    LeftHyper = 57445,
    /// The physical left Meta key.
    LeftMeta = 57446,
    /// The physical right Shift key.
    RightShift = 57447,
    /// The physical right Control key.
    RightControl = 57448,
    /// The physical right Alt or Option key.
    RightAlt = 57449,
    /// The physical right Super or Command key.
    RightSuper = 57450,
    /// The physical right Hyper key.
    RightHyper = 57451,
    /// The physical right Meta key.
    RightMeta = 57452,
    /// The ISO Level 3 shift key, often AltGr.
    IsoLevel3Shift = 57453,
    /// The ISO Level 5 shift key.
    IsoLevel5Shift = 57454,
}

impl NamedKey {
    /// Returns kitty's published Unicode Private Use Area code.
    #[must_use]
    pub fn code(self) -> u32 {
        self as u32
    }

    /// Looks up a published PUA assignment without accepting arbitrary codes.
    #[must_use]
    pub fn from_code(code: u32) -> Option<Self> {
        // The gaps in kitty's table remain invalid; treating all PUA
        // scalars as named keys would make hostile wire input look trusted.
        Some(match code {
            57358 => Self::CapsLock,
            57359 => Self::ScrollLock,
            57360 => Self::NumLock,
            57361 => Self::PrintScreen,
            57362 => Self::Pause,
            57363 => Self::Menu,
            57409 => Self::KeypadDecimal,
            57410 => Self::KeypadDivide,
            57411 => Self::KeypadMultiply,
            57412 => Self::KeypadSubtract,
            57413 => Self::KeypadAdd,
            57415 => Self::KeypadEqual,
            57416 => Self::KeypadSeparator,
            57417 => Self::KeypadLeft,
            57418 => Self::KeypadRight,
            57419 => Self::KeypadUp,
            57420 => Self::KeypadDown,
            57421 => Self::KeypadPageUp,
            57422 => Self::KeypadPageDown,
            57423 => Self::KeypadHome,
            57424 => Self::KeypadEnd,
            57425 => Self::KeypadInsert,
            57426 => Self::KeypadDelete,
            57427 => Self::KeypadBegin,
            57428 => Self::MediaPlay,
            57429 => Self::MediaPause,
            57430 => Self::MediaPlayPause,
            57431 => Self::MediaReverse,
            57432 => Self::MediaStop,
            57433 => Self::MediaFastForward,
            57434 => Self::MediaRewind,
            57435 => Self::MediaNext,
            57436 => Self::MediaPrevious,
            57437 => Self::MediaRecord,
            57438 => Self::LowerVolume,
            57439 => Self::RaiseVolume,
            57440 => Self::MuteVolume,
            57441 => Self::LeftShift,
            57442 => Self::LeftControl,
            57443 => Self::LeftAlt,
            57444 => Self::LeftSuper,
            57445 => Self::LeftHyper,
            57446 => Self::LeftMeta,
            57447 => Self::RightShift,
            57448 => Self::RightControl,
            57449 => Self::RightAlt,
            57450 => Self::RightSuper,
            57451 => Self::RightHyper,
            57452 => Self::RightMeta,
            57453 => Self::IsoLevel3Shift,
            57454 => Self::IsoLevel5Shift,
            _ => return None,
        })
    }
}
