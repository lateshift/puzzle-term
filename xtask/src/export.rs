//! Export policy belongs to the source distribution maintainer, not the terminal engine.
/// Public source checkouts can be copied directly; this private curation recipe is not shipped.
pub(crate) fn run(_: &[String]) -> Result<(), String> {
    Err("this source tree is already the curated distribution".into())
}
