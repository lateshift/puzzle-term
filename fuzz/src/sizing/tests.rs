//! Exhausted Arbitrary primitives cannot manufacture unbounded zero operations.
use super::*;

#[test]
fn exhausted_or_truncated_input_has_at_most_one_operation_per_byte() {
    assert_eq!(text_sizing(&[]), 0);
    for data in [
        include_bytes!("../../seeds/text_sizing/rectangles").as_slice(),
        include_bytes!("../../seeds/text_sizing/fractional").as_slice(),
        &[0],
        &[1],
        &[0, 0],
    ] {
        assert!(text_sizing(data) <= data.len());
    }
}
