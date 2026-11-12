//! Shared Shift ownership policy for xterm XTSHIFTESCAPE (M18.2).
//!
//! A Boolean is a default that a program may override; always/never are
//! user-enforced choices. Keeping this distinction typed prevents adapters
//! from mistaking an explicit program release for an absent request.
// Settings codec trait implemented by every typed setting.
use super::SettingValue;
// Wire value type shared with the settings bus.
use puzzle_bus::Value;

/// Whether an application may receive Shift-modified mouse gestures.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShiftCapture {
    /// Select locally unless the application explicitly requests capture.
    #[default]
    False,
    /// Capture unless the application explicitly releases Shift.
    True,
    /// Capture regardless of application requests.
    Always,
    /// Select locally regardless of application requests.
    Never,
}

impl ShiftCapture {
    /// Resolves the configured default against the current terminal request.
    pub fn captures(self, request: Option<bool>) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            // An explicit release is as authoritative as capture; OR would lose it.
            Self::False | Self::True => request.unwrap_or(self == Self::True),
        }
    }
}

impl SettingValue for ShiftCapture {
    /// JSON/JS use real Booleans and two fixed-policy string names.
    const KIND: &'static str = "boolean|always|never";
    /// Rejects coercions so a misspelled forced policy cannot enable capture.
    fn decode(value: &Value) -> Result<Self, String> {
        match value {
            Value::Bool(false) => Ok(Self::False),
            Value::Bool(true) => Ok(Self::True),
            Value::String(name) if name == "always" => Ok(Self::Always),
            Value::String(name) if name == "never" => Ok(Self::Never),
            _ => Err("expected false, true, always or never".into()),
        }
    }
    /// Exports precisely the forms accepted by the shared codec.
    fn encode(&self) -> Value {
        match self {
            Self::False => Value::Bool(false),
            Self::True => Value::Bool(true),
            Self::Always => Value::String("always".into()),
            Self::Never => Value::String("never".into()),
        }
    }
}
