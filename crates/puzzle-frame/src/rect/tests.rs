//! Rect instance layout pinned against §10.2's GPU contract.

/// A padding change would shift all following GPU instance attributes.
#[test]
fn rect_instance_is_thirty_two_bytes() {
    assert_eq!(size_of::<super::RectInstance>(), 32);
}
