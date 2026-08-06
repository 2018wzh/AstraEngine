use astra_core::SchemaVersion;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, RuntimeOutputDomain,
    RuntimeOutputSchemaDescriptor, RuntimePersistedCodec, RuntimePersistedOutput,
    RuntimeStepOutput,
};

#[astra_headless_test::test]
fn step_output_keeps_persisted_output_separate_from_live_output() {
    let persisted = RuntimePersistedOutput::postcard(
        RuntimeOutputDomain::Effect,
        "astra.test.effect.v1",
        SchemaVersion::new(1, 0, 0),
        &7_u32,
    )
    .unwrap();
    let output = RuntimeStepOutput {
        session_id: GameRuntimeSessionId("session".into()),
        status: "blocked".into(),
        live: Default::default(),
        persisted: vec![persisted.clone()],
        diagnostics: vec![],
    };

    assert_eq!(output.persisted, [persisted]);
    assert_eq!(output.persisted[0].version, SchemaVersion::new(1, 0, 0));
}

#[astra_headless_test::test]
fn provider_descriptor_declares_every_allowed_output_schema() {
    let descriptor = ProductRuntimeDescriptor {
        runtime_id: "test".into(),
        product_kind: "fixture".into(),
        provider_id: "test.provider".into(),
        supported_targets: vec!["test".into()],
        capabilities: vec![],
        package_sections: vec![],
        release_checks: vec![],
        output_schemas: vec![RuntimeOutputSchemaDescriptor {
            domain: RuntimeOutputDomain::Effect,
            schema: "astra.test.effect.v1".into(),
            version: SchemaVersion::new(1, 0, 0),
            codec: RuntimePersistedCodec::Postcard,
        }],
    };
    assert_eq!(descriptor.output_schemas.len(), 1);
}

#[astra_headless_test::test]
fn persisted_output_rejects_a_wrong_schema_version() {
    let persisted = RuntimePersistedOutput::postcard(
        RuntimeOutputDomain::Trace,
        "astra.test.trace.v1",
        SchemaVersion::new(1, 0, 0),
        &"trace",
    )
    .unwrap();

    let error = persisted
        .decode_postcard::<String>(
            RuntimeOutputDomain::Trace,
            "astra.test.trace.v1",
            SchemaVersion::new(2, 0, 0),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_RUNTIME_PERSISTED_VERSION");
}
