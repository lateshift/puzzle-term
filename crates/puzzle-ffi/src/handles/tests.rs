//! Generation and namespace checks pin the C use-after-free contract.
#![allow(
    clippy::unwrap_used,
    reason = "handle fixture setup failures are test failures"
)]
use super::*;
#[test]
fn stale_and_foreign_ids_never_alias_replacements() {
    let mut apps = Table::<u32, false>::new();
    let mut surfaces = Table::<u32, true>::new();
    let a = apps.insert(7).unwrap();
    let s = surfaces.insert(9).unwrap();
    assert_eq!(apps.get(s), Err(ResultCode::InvalidHandle));
    assert_eq!(surfaces.get(a), Err(ResultCode::InvalidHandle));
    assert_eq!(apps.remove(a), Ok(7));
    let replacement = apps.insert(11).unwrap();
    assert_ne!(a, replacement);
    assert_eq!(apps.get(a), Err(ResultCode::InvalidHandle));
    assert_eq!(apps.get(0), Err(ResultCode::InvalidHandle));
    assert_eq!(apps.get(replacement), Ok(&11));
    // Exhaustion retires the slot instead of wrapping back to an issued identity.
    apps.slots[0].generation = 0x7fff_ffff;
    let final_id = (0x7fff_ffff_u64 << 32) | 1;
    assert_eq!(apps.remove(final_id), Ok(11));
    assert_ne!(apps.insert(12).unwrap() as u32, 1);
}
