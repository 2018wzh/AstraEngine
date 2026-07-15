use astra_property::{AstraProperty, PropertyDescribe};

#[derive(AstraProperty)]
#[allow(dead_code)]
struct ExpandSmoke {
    value: String,
}

#[astra_headless_test::test]
fn derive_output_exposes_metadata_without_global_registry() {
    let metadata = ExpandSmoke::property_metadata();
    assert_eq!(metadata.fields[0].name, "value");
    assert_eq!(metadata.fields[0].rust_type, "String");
}
