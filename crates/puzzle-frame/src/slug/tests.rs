//! The instance layout is an upload protocol between Rust and WGSL (§9.5).
use super::SlugInstance;

/// A changed field count would shift shader attributes and scramble glyphs.
#[test]
fn instance_is_fifty_six_bytes() {
    assert_eq!(std::mem::size_of::<SlugInstance>(), 56);
}
