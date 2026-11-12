//! Raster instance byte layout for the future atlas producer.

/// The tint and kind words follow eight floats with no implicit padding.
#[test]
fn raster_instance_is_forty_bytes() {
    assert_eq!(size_of::<super::RasterInstance>(), 40);
}
