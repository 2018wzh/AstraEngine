use astra_core::SchemaVersion;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, RuntimeOutputCodec, RuntimeOutputDomain,
    RuntimeOutputEnvelope, RuntimeOutputSchemaDescriptor, RuntimeStepOutput,
};

#[test]
fn step_output_uses_one_versioned_envelope_stream() {
    let envelope = RuntimeOutputEnvelope::postcard(
        RuntimeOutputDomain::Effect,
        "astra.test.effect.v1",
        SchemaVersion::new(1, 0, 0),
        &7_u32,
    )
    .unwrap();
    let output = RuntimeStepOutput {
        session_id: GameRuntimeSessionId("session".into()),
        status: "blocked".into(),
        outputs: vec![envelope.clone()],
        diagnostics: vec![],
    };

    assert_eq!(output.outputs, [envelope]);
    assert_eq!(output.outputs[0].version, SchemaVersion::new(1, 0, 0));
}

#[test]
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
            codec: RuntimeOutputCodec::Postcard,
        }],
    };
    assert_eq!(descriptor.output_schemas.len(), 1);
}

#[test]
fn envelope_rejects_a_wrong_schema_version() {
    let envelope = RuntimeOutputEnvelope::postcard(
        RuntimeOutputDomain::Trace,
        "astra.test.trace.v1",
        SchemaVersion::new(1, 0, 0),
        &"trace",
    )
    .unwrap();

    let error = envelope
        .decode_postcard::<String>(
            RuntimeOutputDomain::Trace,
            "astra.test.trace.v1",
            SchemaVersion::new(2, 0, 0),
        )
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_RUNTIME_ENVELOPE_VERSION");
}
