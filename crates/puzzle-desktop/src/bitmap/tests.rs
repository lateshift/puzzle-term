//! Installed emoji CLI readback remains a harness contract.
#![allow(
    clippy::expect_used,
    reason = "installed readback failure must fail this fixture"
)]

#[test]
/// Installed emoji must show authored ink in exactly two cells at both DPRs.
fn installed_emoji_two_cells() {
    let _gpu = crate::gpu_test::lock();
    crate::bitmap_probe::run().expect("installed bitmap emoji readback");
}
