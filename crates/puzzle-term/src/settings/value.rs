//! Converts neutral wire values before they touch typed effective settings.
//!
//! JSON and JS adapters are outside this crate. Rejecting non-finite and
//! ill-typed values here keeps every source under the same schema policy.
//!
//! The bus `Value` tree is deliberately broader than any one setting:
//! bytes and nested maps can cross the wire, but a boolean option accepts
//! only a boolean. Each codec rejects a mismatched variant before the
//! typed effective settings snapshot is changed (§14.3).
//! Numeric range validation runs before decoding. Integer conversion
//! still checks exact integrality, because accepting 1.5 as 1 would make
//! a JSON configuration behave differently from its visible value.
use super::schema::Validator;
use puzzle_bus::Value;

/// A typed schema field that can cross the neutral wire boundary.
pub(super) trait SettingValue: Sized + Clone {
    /// Human-readable type used by generated documentation and reports.
    const KIND: &'static str;
    /// Decodes a validated value without panicking on external input.
    fn decode(value: &Value) -> Result<Self, String>;
    /// Describes the typed value in the common plain-data representation.
    fn encode(&self) -> Value;
}

/// Boolean fields keep their JSON and JS spelling without coercion.
impl SettingValue for bool {
    /// The schema emits a boolean control for this field.
    const KIND: &'static str = "boolean";
    /// A string such as `"true"` must be reported rather than coerced.
    fn decode(value: &Value) -> Result<Self, String> {
        match value {
            Value::Bool(value) => Ok(*value),
            _ => Err("expected a boolean".into()),
        }
    }
    /// The neutral tree preserves the field's exact two-state value.
    fn encode(&self) -> Value {
        Value::Bool(*self)
    }
}

/// Floating fields use f32 in the core but f64 on the wire.
impl SettingValue for f32 {
    /// Docs present the external representation as a number.
    const KIND: &'static str = "number";
    /// Rejects NaN, infinity and values outside f32's finite range.
    fn decode(value: &Value) -> Result<Self, String> {
        match value {
            Value::Number(number) if number.is_finite() && number.abs() <= f32::MAX as f64 => {
                Ok(*number as f32)
            }
            _ => Err("expected a finite number".into()),
        }
    }
    /// Widening a finite f32 retains its value exactly on the wire.
    fn encode(&self) -> Value {
        Value::Number(f64::from(*self))
    }
}

/// Counts and timeouts require an exact nonnegative integer.
impl SettingValue for u32 {
    /// Generated JSON Schema can distinguish this from a decimal field.
    const KIND: &'static str = "integer";
    /// Fractional values cannot silently truncate a row or byte budget.
    fn decode(value: &Value) -> Result<Self, String> {
        match value {
            Value::Number(number)
                if number.is_finite()
                    && *number >= 0.0
                    && *number <= f64::from(u32::MAX)
                    && number.fract() == 0.0 =>
            {
                Ok(*number as u32)
            }
            _ => Err("expected a nonnegative integer".into()),
        }
    }
    /// Every u32 is exactly representable by the wire's f64 number.
    fn encode(&self) -> Value {
        Value::Number(f64::from(*self))
    }
}

/// Text fields share §16's four-kibibyte input bound.
impl SettingValue for String {
    /// String metadata drives a text control in generated settings UIs.
    const KIND: &'static str = "string";
    /// The byte bound holds even when a field contains multi-byte Unicode.
    fn decode(value: &Value) -> Result<Self, String> {
        match value {
            Value::String(text) if text.len() <= 4096 => Ok(text.clone()),
            _ => Err("expected a string of at most 4096 bytes".into()),
        }
    }
    /// Export copies the owned setting so the caller can retain the tree.
    fn encode(&self) -> Value {
        Value::String(self.clone())
    }
}

/// Lists are checked element by element before any field is replaced.
impl SettingValue for Vec<String> {
    /// Lists of feature tags and fallback faces use the same wire shape.
    const KIND: &'static str = "string[]";
    /// One invalid entry rejects the list instead of dropping that entry.
    fn decode(value: &Value) -> Result<Self, String> {
        let Value::List(items) = value else {
            return Err("expected a string list".into());
        };
        if items.len() > 4096 {
            // A settings adapter may be fed by an untrusted page or file;
            // bounding list length prevents an unbounded validation walk.
            return Err("list exceeds 4096 entries".into());
        }
        items.iter().map(String::decode).collect()
    }
    /// Each element uses the same codec as an individual string field.
    fn encode(&self) -> Value {
        Value::List(self.iter().map(String::encode).collect())
    }
}

/// Rich theme/profile maps retain their neutral form for a later owner.
impl SettingValue for Value {
    /// The schema advertises an object rather than exposing Rust internals.
    const KIND: &'static str = "object";
    /// The shared wire-tree bound still applies to nested unknown data.
    fn decode(value: &Value) -> Result<Self, String> {
        puzzle_bus::value::validate_tree(value)
            .map_err(|_| "value tree exceeds the wire limits".to_string())?;
        Ok(value.clone())
    }
    /// Unknown subkeys survive export for a newer Puzzle version.
    fn encode(&self) -> Value {
        self.clone()
    }
}

/// Applies a field's range or choices before typed conversion.
///
/// Range checks clamp rather than reject so an outdated font size remains
/// usable; choice checks reject because an unknown enum has no safe
/// behavior to select. Both paths return whether clamping occurred for
/// `ApplyReport`, which the settings UI can show to the user (§14.3).
pub(super) fn validate(value: Value, rule: Validator) -> Result<(Value, bool), String> {
    match rule {
        Validator::Any => Ok((value, false)),
        Validator::Range { minimum, maximum } => {
            let Value::Number(number) = value else {
                return Err("expected a number".into());
            };
            if !number.is_finite() {
                // Neither a clamp nor an f32 cast has useful semantics
                // for NaN, and infinity must not leak into geometry.
                return Err("number must be finite".into());
            }
            // The submitted value is preserved when already in range; a
            // clamped report identifies only an actual boundary change.
            let bounded = number.clamp(minimum, maximum);
            Ok((Value::Number(bounded), bounded != number))
        }
        Validator::Choice(allowed) => {
            // A typo must not silently choose another policy, especially
            // for clipboard access or a renderer preference.
            let Value::String(text) = &value else {
                return Err("expected a string choice".into());
            };
            if allowed.contains(&text.as_str()) {
                Ok((value, false))
            } else {
                Err(format!("expected one of {}", allowed.join(", ")))
            }
        }
        Validator::CursorColor => {
            let Value::String(text) = &value else {
                return Err("expected a cursor color string".into());
            };
            let hex = text.strip_prefix('#').is_some_and(|digits| {
                digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
            if text == "theme" || text == "inverse" || hex {
                Ok((value, false))
            } else {
                Err("expected theme, inverse or #rrggbb".into())
            }
        }
    }
}
