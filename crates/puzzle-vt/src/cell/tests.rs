//! Layout checks for the grid's compact storage contract.
use super::*;

#[test]
fn cell_has_sixteen_bytes() {
    assert_eq!(std::mem::size_of::<Cell>(), 16);
}

#[test]
fn underline_style_keeps_other_attributes() {
    let mut attrs = Attrs::BOLD | Attrs::ITALIC;
    attrs.set_underline_style(5);
    assert_eq!(attrs.underline_style(), 5);
    assert!(attrs.contains(Attrs::BOLD | Attrs::ITALIC));
}
