use astra_property::{AstraProperty, PropertyDescribe};

#[derive(AstraProperty)]
#[allow(dead_code)]
struct SpriteComponent {
    visible: bool,
    layer: i64,
}

#[derive(AstraProperty)]
#[allow(dead_code)]
struct HandleComponent<T>
where
    T: Clone,
{
    value: T,
}

#[astra_headless_test::test]
fn property_metadata_is_stable_and_explicit() {
    let metadata = SpriteComponent::property_metadata();
    assert_eq!(metadata.type_name, "SpriteComponent");
    assert_eq!(metadata.fields.len(), 2);
    assert_eq!(metadata.fields[0].name, "visible");
    assert_eq!(metadata.fields[0].rust_type, "bool");
    assert!(metadata.fields[0].save.included);
    assert_eq!(metadata.fields[1].name, "layer");

    let generic = HandleComponent::<String>::property_metadata();
    assert_eq!(generic.type_name, "HandleComponent");
    assert_eq!(generic.fields[0].rust_type, "T");
}
