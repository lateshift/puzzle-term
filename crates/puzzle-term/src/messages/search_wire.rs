//! Exact literal-search maps at the public bus boundary (§5.7, §6.10).
//!
//! Query strings are literal, with nullable explicit case sensitivity. Unknown
//! fields are rejected so a caller cannot accidentally believe regex options
//! are supported. Empty text means clear through the same typed request.
use crate::{search::SearchCommand, vt::search::FindOptions};
use puzzle_bus::{Value, value::ValidationReport, wire::reject};
use std::collections::BTreeMap;
/// Allocates a plain map only when an external observer requests it.
pub(super) fn to_value(request: &SearchCommand) -> Value {
    // External payload maps are cold; the typed engine never stores wire values.
    let mut map = BTreeMap::new();
    // Stable operation names distinguish query changes from UI focus intent.
    let operation = match request {
        SearchCommand::Find { query, options } => {
            // Literal strings stay unchanged at the wire seam; normalization is VT work.
            map.insert("query".into(), Value::String(query.clone()));
            // Null requests smart case; false must remain distinct from omission.
            map.insert(
                "caseSensitive".into(),
                options.case_sensitive.map_or(Value::Null, Value::Bool),
            );
            "find"
        }
        SearchCommand::Clear => "clear",
        SearchCommand::Request => "request",
    };
    // The operation discriminator is required even for zero-argument commands.
    map.insert("operation".into(), Value::String(operation.into()));
    Value::Map(map)
}
/// Validates options before constructing the closed query operation.
pub(super) fn from_value(value: &Value) -> Result<SearchCommand, ValidationReport> {
    // Non-map input cannot construct a search command or mutate the old query.
    let Value::Map(map) = value else {
        return Err(reject("search must be a map"));
    };
    // Never coerce numbers or null into operation strings.
    let Some(Value::String(operation)) = map.get("operation") else {
        return Err(reject("search operation missing"));
    };
    // Exact field counts reject accidental regex and misspelled option names.
    match operation.as_str() {
        "clear" if map.len() == 1 => Ok(SearchCommand::Clear),
        "request" if map.len() == 1 => Ok(SearchCommand::Request),
        "find" if map.len() == 3 => {
            // Value-tree validation already bounds string and map sizes (§5.7).
            let Some(Value::String(query)) = map.get("query") else {
                return Err(reject("literal query missing"));
            };
            // Only a nullable boolean expresses the three supported case policies.
            let case_sensitive = match map.get("caseSensitive") {
                Some(Value::Null) => None,
                Some(Value::Bool(case)) => Some(*case),
                _ => return Err(reject("caseSensitive must be boolean or null")),
            };
            // Construct typed state only after every expected field is validated.
            Ok(SearchCommand::Find {
                query: query.clone(),
                options: FindOptions { case_sensitive },
            })
        }
        // Preserve the old query when an unsupported operation arrives.
        _ => Err(reject("unknown search operation or fields")),
    }
}
