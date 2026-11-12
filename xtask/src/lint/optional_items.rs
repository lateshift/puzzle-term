// The standard graph has no additional dependency ownership rules.
/// Ordinary dependencies are checked by the base platform catalogue.
fn extra_dependency(_: &str, _: &str) -> Option<bool> {
    None
}
