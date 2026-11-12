//! Plain-data values for bus messages and future settings (design §5.7).
//!
//! The wire tree is independent of JSON and browser objects. Platforms may
//! translate those formats at their edge, then hand this tree to a bounded
//! validator. Maps use sorted keys so diagnostics and documentation are
//! deterministic. Bytes stay bytes rather than being coerced into strings.
use std::collections::BTreeMap;

/// A platform-neutral value with no parser or serialization dependency.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// An absent or intentionally empty value.
    Null,
    /// A Boolean flag.
    Bool(bool),
    /// A finite floating-point number after validation.
    Number(f64),
    /// Unicode text, never assumed to be a terminal byte stream.
    String(String),
    /// Opaque bytes such as an extension payload.
    Bytes(Vec<u8>),
    /// Ordered values whose order carries meaning.
    List(Vec<Self>),
    /// Named fields in deterministic key order.
    Map(BTreeMap<String, Self>),
}

/// One readable problem found while validating external data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Dot-separated location in the submitted value.
    pub path: String,
    /// The expected shape or violated bound.
    pub message: String,
}

/// Accumulates validation problems without panicking on malformed input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    /// Every problem the decoder could identify safely.
    pub issues: Vec<ValidationIssue>,
}

/// Helpers shared by each public wire decoder.
impl ValidationReport {
    /// Adds a location-specific error to a rejected payload.
    pub fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(ValidationIssue {
            path: path.into(),
            message: message.into(),
        });
    }

    /// Reports whether conversion may proceed to dispatch.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Rejects excessive nesting and collection sizes before typed conversion.
///
/// The iterative walk avoids consuming the call stack on a hostile tree.
/// Limits bound validation work even when an external adapter supplies a
/// large object. Adapters should apply the same depth limit while building
/// the tree, since dropping a deeply nested owned tree may itself recurse.
pub fn validate_tree(value: &Value) -> Result<(), ValidationReport> {
    // The worklist makes validation stack-safe for values built by an
    // external adapter, even when the input nests many containers.
    let mut report = ValidationReport::default();
    let mut pending = vec![(value, 0_usize)];
    let mut visited = 0_usize;
    while let Some((node, depth)) = pending.pop() {
        visited += 1;
        // Cap both depth and aggregate work; a small branching factor can
        // otherwise still make a deeply supplied tree expensive to inspect.
        if depth > 64 || visited > 65_536 {
            report.push("payload", "value tree exceeds depth or node limit");
            break;
        }
        match node {
            Value::Number(number) if !number.is_finite() => {
                report.push("payload", "number must be finite");
            }
            Value::String(text) if text.len() > 1_048_576 => {
                report.push("payload", "string exceeds 1 MiB");
            }
            Value::Bytes(bytes) if bytes.len() > 1_048_576 => {
                report.push("payload", "bytes exceed 1 MiB");
            }
            Value::List(items) => {
                if items.len() > 16_384 {
                    report.push("payload", "list exceeds 16,384 entries");
                } else {
                    pending.extend(items.iter().map(|item| (item, depth + 1)));
                }
            }
            Value::Map(fields) => {
                if fields.len() > 16_384 {
                    report.push("payload", "map exceeds 16,384 fields");
                } else {
                    pending.extend(fields.values().map(|item| (item, depth + 1)));
                }
            }
            _ => {}
        }
    }
    if report.is_valid() {
        Ok(())
    } else {
        Err(report)
    }
}

#[cfg(test)]
/// Properties of the bounded neutral value tree.
mod tests;
