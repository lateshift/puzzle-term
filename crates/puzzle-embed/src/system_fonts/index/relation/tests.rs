//! Counting inversion must preserve the relation used by whole-cluster fallback.
#[test]
fn inversion_matches_sorted_deduplicated_pairs() {
    let mut pairs = Vec::new();
    for face in 0..23 {
        for scalar in [0, 65, 65, 0x4e00 + face, 0x10ffff] {
            pairs.push((scalar, face));
        }
    }
    let mut expected = pairs.clone();
    expected.sort_unstable();
    expected.dedup();
    let (offsets, faces) = super::invert(pairs);
    let actual: Vec<_> = offsets
        .windows(2)
        .enumerate()
        .flat_map(|(scalar, range)| {
            faces[range[0] as usize..range[1] as usize]
                .iter()
                .map(move |face| (scalar as u32, *face))
        })
        .collect();
    assert_eq!(actual, expected);
}
