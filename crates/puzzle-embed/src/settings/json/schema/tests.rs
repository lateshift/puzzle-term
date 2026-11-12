//! Generated metadata must include the current settings catalogue.
//!
//! A missing nested leaf makes --init-config's adjacent schema misleading
//! even when the runtime store accepts that same config.json key.
use super::document;

#[test]
/// The editor schema reaches nested camelCase leaves and mixed blink.
///
/// These keys exercise number, mixed string/bool and enum codecs, rather
/// than merely counting the generated properties.
fn schema_has_current_keys() {
    let schema = document();
    assert_eq!(
        schema["properties"]["font"]["properties"]["size"]["default"],
        14.0
    );
    assert!(schema["properties"]["cursor"]["properties"]["blink"]["anyOf"].is_array());
    assert_eq!(
        schema["properties"]["keyboard"]["properties"]["optionAsAlt"]["default"],
        "none"
    );
}
