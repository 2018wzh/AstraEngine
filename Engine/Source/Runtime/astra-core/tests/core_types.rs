use astra_core::{
    AstraError, AstraResult, Diagnostic, DiagnosticSeverity, Hash128, Hash256,
    SchemaMigrationRegistry, SchemaVersion, SourceSpan, StableId, StableIdGenerator,
};

#[test]
fn core_types() {
    stable_id_roundtrips_as_uuid_string();
    deterministic_v7_generator_repeats();
    diagnostic_serializes_machine_readable_fields();
    schema_versions_order_and_migration_chain_is_checked();
    hashes_are_repeatable_and_hex_encoded();
    result_type_carries_core_error();
}

#[test]
fn stable_id_roundtrips_as_uuid_string() {
    let id = StableId::parse("018f10fb-4e00-7000-8000-00000000002a").unwrap();
    let encoded = serde_json::to_string(&id).unwrap();
    assert_eq!(encoded, "\"018f10fb-4e00-7000-8000-00000000002a\"");
    assert_eq!(serde_json::from_str::<StableId>(&encoded).unwrap(), id);
}

#[test]
fn deterministic_v7_generator_repeats() {
    let mut left = StableIdGenerator::new(42);
    let mut right = StableIdGenerator::new(42);
    left.set_step(7);
    right.set_step(7);
    assert_eq!(left.next_id(), right.next_id());
    assert_eq!(left.next_id(), right.next_id());
}

#[test]
fn diagnostic_serializes_machine_readable_fields() {
    let diag = Diagnostic::blocking("ASTRA_TEST", "blocked")
        .with_source(SourceSpan::new("scenario", 3, 4, 5))
        .with_field("step", "2");
    let yaml = serde_yaml::to_string(&diag).unwrap();
    assert!(yaml.contains("ASTRA_TEST"));
    assert_eq!(diag.severity, DiagnosticSeverity::Blocking);
}

#[test]
fn schema_versions_order_and_migration_chain_is_checked() {
    let current = SchemaVersion::new(1, 0, 0);
    let older = SchemaVersion::new(0, 9, 0);
    let next = SchemaVersion::new(1, 1, 0);
    assert!(older < current);

    let mut registry = SchemaMigrationRegistry::default();
    assert!(registry
        .validate_chain("runtime.world", older, next)
        .is_err());
    registry.register_identity("runtime.world", older, current);
    registry.register_identity("runtime.world", current, next);
    registry
        .validate_chain("runtime.world", older, next)
        .unwrap();
}

#[test]
fn hashes_are_repeatable_and_hex_encoded() {
    let left = Hash128::from_blake3(b"runtime");
    let right = Hash128::from_blake3(b"runtime");
    assert_eq!(left, right);
    assert_eq!(left.to_hex().len(), 32);

    let sha = Hash256::from_sha256(b"container");
    assert_eq!(sha.to_hex().len(), 64);
}

#[test]
fn result_type_carries_core_error() {
    let result: AstraResult<()> = Err(AstraError::message("failed"));
    assert!(result.is_err());
}
