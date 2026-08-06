use astra_core::SchemaVersion;
use astra_plugin_abi::{RuntimeOutputDomain, RuntimePersistedOutput};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Effect {
    reached: String,
}

#[astra_headless_test::test]
fn runtime_persisted_output_is_bound_to_domain_schema_and_codec() {
    let persisted = RuntimePersistedOutput::postcard(
        RuntimeOutputDomain::Effect,
        "astra.test.effect.v1",
        SchemaVersion::new(1, 0, 0),
        &Effect {
            reached: "route.a".into(),
        },
    )
    .unwrap();

    assert_eq!(
        persisted
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
        persisted
            .decode_postcard::<Effect>(
                RuntimeOutputDomain::Trace,
                "astra.test.effect.v1",
                SchemaVersion::new(1, 0, 0)
            )
            .unwrap_err()
            .code(),
        "ASTRA_RUNTIME_PERSISTED_DOMAIN"
    );
    assert_eq!(
        persisted
            .decode_postcard::<Effect>(
                RuntimeOutputDomain::Effect,
                "astra.unknown",
                SchemaVersion::new(1, 0, 0)
            )
            .unwrap_err()
            .code(),
        "ASTRA_RUNTIME_PERSISTED_SCHEMA"
    );
}

#[astra_headless_test::test]
fn runtime_persisted_output_reuses_immutable_postcard_storage() {
    let encoded: std::sync::Arc<[u8]> = postcard::to_allocvec(&Effect {
        reached: "route.a".into(),
    })
    .unwrap()
    .into();
    let persisted = RuntimePersistedOutput::postcard_bytes(
        RuntimeOutputDomain::Effect,
        "astra.test.effect.v1",
        SchemaVersion::new(1, 0, 0),
        std::sync::Arc::clone(&encoded),
    );
    assert!(std::sync::Arc::ptr_eq(persisted.bytes(), &encoded));
    assert_eq!(
        persisted
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
