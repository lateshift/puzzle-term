//! Header discovery rejects a missing or extra ABI function without a compiler plugin.
use super::catalogue::names;
use std::collections::BTreeSet;
#[test]
fn exported_names_ignore_types_and_comments() {
    let rust = "pub unsafe extern \"C\" fn puzzle_surface_key(\n// puzzle_fake(\npub extern \"C\" fn puzzle_app_tick(id: u64)";
    let actual = names(rust, true);
    assert_eq!(
        actual,
        BTreeSet::from(["puzzle_app_tick".into(), "puzzle_surface_key".into()])
    );
    let header = "puzzle_surface_key(\npuzzle_app_tick(\n/* puzzle_fake( */";
    // A comment's token is never at the start of a genuine declaration.
    assert_eq!(names(header, false), actual);
}
