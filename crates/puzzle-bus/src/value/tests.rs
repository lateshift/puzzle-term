//! Bounded validation is the first defense against untrusted wire values.
use super::{Value, validate_tree};
use proptest::prelude::*;

/// Non-finite values cannot survive conversion to a JS or JSON number.
#[test]
fn non_finite_number_is_reported() {
    assert!(validate_tree(&Value::Number(f64::NAN)).is_err());
    assert!(validate_tree(&Value::Number(f64::INFINITY)).is_err());
}

proptest! {
    /// Validation of arbitrary scalar input always returns a result.
    #[test]
    fn scalar_values_never_panic(number in any::<f64>(), text in ".{0,128}") {
        let _ = validate_tree(&Value::Number(number));
        let _ = validate_tree(&Value::String(text));
    }
}
