use astra_core::SchemaVersion;
use astra_plugin_abi::{RuntimeOutputDomain, RuntimeOutputEnvelope};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Effect {
    reached: String,
}

#[astra_headless_test::test]
fn runtime_output_envelope_is_bound_to_domain_schema_codec_and_hash() {
    let envelope = RuntimeOutputEnvelope::postcard(
        RuntimeOutputDomain::Effect,
        "astra.test.effect.v1",
        SchemaVersion::new(1, 0, 0),
        &Effect {
            reached: "route.a".into(),
        },
    )
    .unwrap();

    assert_eq!(
        envelope
            .decode_postcard::<Effect>(
                RuntimeOutputDomain::Effect,
                "astra.test.effect.v1",
                SchemaVersion::new(1, 0, 0)
            )
            .unwrap(),
        Effect {
            reached: "route.a".into()
        }
    );
    assert_eq!(
        envelope
            .decode_postcard::<Effect>(
                RuntimeOutputDomain::Trace,
                "astra.test.effect.v1",
                SchemaVersion::new(1, 0, 0)
            )
            .unwrap_err()
            .code(),
        "ASTRA_RUNTIME_ENVELOPE_DOMAIN"
    );
    assert_eq!(
        envelope
            .decode_postcard::<Effect>(
                RuntimeOutputDomain::Effect,
                "astra.unknown",
                SchemaVersion::new(1, 0, 0)
            )
            .unwrap_err()
            .code(),
        "ASTRA_RUNTIME_ENVELOPE_SCHEMA"
    );

    let mut wire = serde_json::to_value(envelope).unwrap();
    wire["bytes"].as_array_mut().unwrap().push(0.into());
    let error = serde_json::from_value::<RuntimeOutputEnvelope>(wire).unwrap_err();
    assert!(error.to_string().contains("ASTRA_RUNTIME_ENVELOPE_HASH"));
}

#[astra_headless_test::test]
fn runtime_output_envelope_reuses_immutable_postcard_storage() {
    let encoded: std::sync::Arc<[u8]> = postcard::to_allocvec(&Effect {
        reached: "route.a".into(),
    })
    .unwrap()
    .into();
    let envelope = RuntimeOutputEnvelope::postcard_bytes(
        RuntimeOutputDomain::Effect,
        "astra.test.effect.v1",
        SchemaVersion::new(1, 0, 0),
        std::sync::Arc::clone(&encoded),
    );
    assert!(std::sync::Arc::ptr_eq(envelope.bytes(), &encoded));
    assert_eq!(
        envelope
            .decode_postcard::<Effect>(
                RuntimeOutputDomain::Effect,
                "astra.test.effect.v1",
                SchemaVersion::new(1, 0, 0),
            )
            .unwrap(),
        Effect {
            reached: "route.a".into(),
        }
    );
}
