//! Binary16 anchors keep the D-8 comparison from timing malformed texels.
use super::f32_to_f16;

/// Zero, one and the maximum half finite value pin exponent conversion.
#[test]
fn ieee_half_anchors() {
    assert_eq!(f32_to_f16(0.0), 0);
    assert_eq!(f32_to_f16(-0.0), 0x8000);
    assert_eq!(f32_to_f16(1.0), 0x3c00);
    assert_eq!(f32_to_f16(-1.0), 0xbc00);
    assert_eq!(f32_to_f16(65504.0), 0x7bff);
}
