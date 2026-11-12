// The standard source tree includes only its base protocol families.
/// Every shipped VT and Session test still runs.
fn origin_tests() -> Result<(), String> {
    crate::commands::quiet("cargo", &["test", "-p", "puzzle-vt", "-p", "puzzle-term"])
}
