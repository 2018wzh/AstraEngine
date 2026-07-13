use astra_core::{Hash256, SchemaVersion};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use astra_plugin::{
    AsyncProductRuntimeHost, ProductRuntimeHost, ProductRuntimeProvider, RuntimeHostSchemaRegistry,
};
use astra_plugin_abi::*;

#[derive(Default)]
struct Provider {
    opened: bool,
    wrong_identity: bool,
}

struct CreateFailureProvider {
    malformed_report: bool,
    destroy_calls: Arc<AtomicUsize>,
}

impl ProductRuntimeProvider for CreateFailureProvider {
    fn create_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if self.malformed_report {
            Ok(RuntimeProviderInstanceReport {
                instance_id,
                status: "unknown".into(),
                diagnostics: vec![],
            })
        } else {
            Err("partial create failure".into())
        }
    }

    fn destroy_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        self.destroy_calls.fetch_add(1, Ordering::SeqCst);
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&mut self, _: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Err("unreachable".into())
    }
    fn probe(&mut self, _: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        Err("unreachable".into())
    }
    fn open(&mut self, _: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String> {
        Err("unreachable".into())
    }
    fn step(&mut self, _: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        Err("unreachable".into())
    }
    fn save(&mut self, _: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        Err("unreachable".into())
    }
    fn restore(&mut self, _: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        Err("unreachable".into())
    }
    fn shutdown(&mut self, _: GameRuntimeSessionId) -> Result<RuntimeShutdownReport, String> {
        Err("unreachable".into())
    }
}

impl ProductRuntimeProvider for Provider {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Ok(provider_descriptor())
    }

    fn create_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".into(),
            diagnostics: vec![],
        })
    }

    fn destroy_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&mut self, _: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Ok(RuntimePrepareReport {
            runtime_id: "test".into(),
            provider_id: if self.wrong_identity {
                "test.provider.wrong".into()
            } else {
                "test.provider".into()
            },
            status: "ready".into(),
            diagnostics: vec![],
        })
    }

    fn probe(&mut self, _: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        Ok(RuntimeProbeReport {
            runtime_id: "test".into(),
            provider_id: "test.provider".into(),
            status: "available".into(),
            diagnostics: vec![],
        })
    }

    fn open(&mut self, request: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String> {
        self.opened = true;
        Ok(RuntimeOpenReport {
            session_id: GameRuntimeSessionId(format!("session-{}", request.seed)),
            runtime_id: "test".into(),
            provider_id: "test.provider".into(),
            diagnostics: vec![],
        })
    }

    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        assert!(self.opened);
        if input.action == "panic" {
            panic!("provider panic fixture");
        }
        if input.action == "slow" {
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(RuntimeStepOutput {
            session_id: input.session_id,
            status: "blocked".into(),
            outputs: vec![RuntimeOutputEnvelope::postcard(
                RuntimeOutputDomain::Effect,
                "astra.test.effect.v1",
                SchemaVersion::new(1, 0, 0),
                &7_u32,
            )
            .unwrap()],
            diagnostics: vec![],
        })
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        Ok(RuntimeSaveSections {
            session_id: request.session_id,
            sections: if request.slot == "bad" {
                vec![RuntimeSectionPayload {
                    section_id: "state".into(),
                    schema: "test.state.v1".into(),
                    version: SchemaVersion::new(1, 0, 0),
                    codec: RuntimeSectionCodec::Postcard,
                    hash: Hash256::from_sha256(b"different"),
                    bytes: vec![],
                }]
            } else {
                vec![]
            },
            diagnostics: vec![],
        })
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        Ok(RuntimeRestoreReport {
            session_id: request.session_id,
            status: "restored".into(),
            diagnostics: vec![],
        })
    }

    fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        self.opened = false;
        Ok(RuntimeShutdownReport {
            session_id,
            status: "shutdown".into(),
            diagnostics: vec![],
        })
    }
}

fn provider_descriptor() -> ProductRuntimeDescriptor {
    ProductRuntimeDescriptor {
        runtime_id: "test".into(),
        product_kind: "test".into(),
        provider_id: "test.provider".into(),
        supported_targets: vec!["test".into()],
        capabilities: vec!["runtime.test".into()],
        package_sections: vec![],
        release_checks: vec![],
        output_schemas: vec![RuntimeOutputSchemaDescriptor {
            domain: RuntimeOutputDomain::Effect,
            schema: "astra.test.effect.v1".into(),
            version: SchemaVersion::new(1, 0, 0),
            codec: RuntimeOutputCodec::Postcard,
        }],
    }
}

fn bound_selection() -> ValidatedRuntimeProviderSelection {
    let context = |capability: &str| ProviderBindingContext {
        package_id: "test.package".into(),
        target: "test".into(),
        profile: "release".into(),
        required_capability: capability.into(),
        engine_version: "0.1.0".into(),
        rustc_fingerprint: "rustc-stable".into(),
        feature_fingerprint: "runtime-envelope-v2".into(),
        abi_fingerprint: "astra-plugin-abi-v2".into(),
    };
    let presentation =
        ProviderBinding::new("presentation", "test.renderer", context("renderer2d.test")).unwrap();
    let runtime = ProviderBinding::new(
        GAME_RUNTIME_PROVIDER_SLOT,
        "test.provider",
        context("runtime.test"),
    )
    .unwrap();
    let registry = PluginExtensionRegistrySnapshot {
        schema: PLUGIN_EXTENSION_REGISTRY_SCHEMA.into(),
        providers: vec![
            ProviderExtensionRecord {
                slot: "presentation".into(),
                provider_id: "test.renderer".into(),
                capability: "renderer2d.test".into(),
                phase: LoadPhase::Runtime,
                packaged: true,
                engine_version: "0.1.0".into(),
                rustc_fingerprint: "rustc-stable".into(),
                feature_fingerprint: "runtime-envelope-v2".into(),
                abi_fingerprint: "astra-plugin-abi-v2".into(),
            },
            ProviderExtensionRecord {
                slot: GAME_RUNTIME_PROVIDER_SLOT.into(),
                provider_id: "test.provider".into(),
                capability: "runtime.test".into(),
                phase: LoadPhase::Runtime,
                packaged: true,
                engine_version: "0.1.0".into(),
                rustc_fingerprint: "rustc-stable".into(),
                feature_fingerprint: "runtime-envelope-v2".into(),
                abi_fingerprint: "astra-plugin-abi-v2".into(),
            },
        ],
        bindings: vec![presentation.clone(), runtime.clone()],
        conflicts: vec![],
    };
    let policy = ProviderPolicy {
        schema: PROVIDER_POLICY_SCHEMA.into(),
        profile: "release".into(),
        renderer: "test.renderer".into(),
        decode_fallback: "forbid".into(),
        runtime_provider: provider_descriptor(),
        bindings: vec![presentation, runtime],
    };
    registry
        .resolve_embedded_runtime_provider(&policy, "test.package", "release")
        .unwrap()
}

#[test]
fn bound_host_blocks_request_and_provider_report_identity_drift() {
    let selection = bound_selection();
    let schemas = RuntimeHostSchemaRegistry::from_descriptor(selection.descriptor());
    let mut host = ProductRuntimeHost::bound_in_process(
        "bound-instance",
        &selection,
        Provider::default(),
        schemas.clone(),
    )
    .unwrap();
    let mut wrong_context = prepare_request();
    wrong_context.target_id = "other".into();
    assert_eq!(
        host.prepare(wrong_context).unwrap_err().code(),
        "ASTRA_RUNTIME_HOST_BINDING_CONTEXT"
    );
    host.prepare(prepare_request()).unwrap();
    host.destroy().unwrap();

    let mut wrong_provider = ProductRuntimeHost::bound_in_process(
        "wrong-provider-instance",
        &selection,
        Provider {
            wrong_identity: true,
            ..Provider::default()
        },
        schemas,
    )
    .unwrap();
    assert_eq!(
        wrong_provider
            .prepare(prepare_request())
            .unwrap_err()
            .code(),
        "ASTRA_RUNTIME_HOST_PROVIDER_IDENTITY"
    );
    wrong_provider.cleanup_after_failure().unwrap();
}

fn prepare_request() -> RuntimePrepareRequest {
    RuntimePrepareRequest {
        target_id: "test".into(),
        profile: "release".into(),
        package_hash: "sha256:test".into(),
        section_ids: vec![],
    }
}

#[test]
fn in_process_host_owns_provider_lifecycle_and_validates_step_envelopes() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let mut host =
        ProductRuntimeHost::reference_in_process("instance", Provider::default(), schemas).unwrap();
    host.prepare(prepare_request()).unwrap();
    host.probe(RuntimeProbeRequest {
        target_id: "test".into(),
        profile: "release".into(),
        platform: None,
        section_ids: vec![],
    })
    .unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .unwrap();
    let output = host
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            action: "advance".into(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    assert_eq!(output.outputs.len(), 1);
    host.save(RuntimeSaveRequest {
        session_id: open.session_id.clone(),
        slot: "slot".into(),
    })
    .unwrap();
    host.restore(RuntimeRestoreRequest {
        session_id: open.session_id.clone(),
        sections: vec![],
    })
    .unwrap();
    host.shutdown().unwrap();
    host.destroy().unwrap();
}

#[test]
fn host_rolls_back_failed_and_malformed_instance_creation() {
    for malformed_report in [false, true] {
        let destroy_calls = Arc::new(AtomicUsize::new(0));
        let error = ProductRuntimeHost::reference_in_process(
            "instance",
            CreateFailureProvider {
                malformed_report,
                destroy_calls: Arc::clone(&destroy_calls),
            },
            RuntimeHostSchemaRegistry::new(),
        )
        .err()
        .expect("create must fail");
        assert_eq!(destroy_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            error.code(),
            if malformed_report {
                "ASTRA_RUNTIME_HOST_INSTANCE_REPORT_STATUS"
            } else {
                "ASTRA_RUNTIME_HOST_CREATE"
            }
        );
    }
}

#[test]
fn duplicate_open_rolls_back_and_blocks_use_until_cleanup() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let mut host =
        ProductRuntimeHost::reference_in_process("instance", Provider::default(), schemas).unwrap();
    let request = RuntimeOpenRequest {
        target_id: "test".into(),
        profile: "release".into(),
        locale: "und".into(),
        seed: 1,
        package_hash: "sha256:test".into(),
        sections: vec![],
    };
    let first = host.open(request.clone()).unwrap();
    let duplicate = host.open(request).unwrap_err();
    assert_eq!(duplicate.code(), "ASTRA_RUNTIME_HOST_SESSION_DUPLICATE");
    let blocked = host
        .step(RuntimeStepInput {
            session_id: first.session_id.clone(),
            fixed_step: 1,
            action: "advance".into(),
            payload: serde_json::json!({}),
        })
        .unwrap_err();
    assert_eq!(blocked.code(), "ASTRA_RUNTIME_HOST_LIFECYCLE");
    host.shutdown_session(first.session_id).unwrap();
    host.destroy().unwrap();
}

#[test]
fn host_blocks_unknown_step_schema() {
    let mut host = ProductRuntimeHost::reference_in_process(
        "instance",
        Provider::default(),
        RuntimeHostSchemaRegistry::new(),
    )
    .unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .unwrap();
    let error = host
        .step(RuntimeStepInput {
            session_id: open.session_id,
            fixed_step: 1,
            action: "advance".into(),
            payload: serde_json::json!({}),
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_RUNTIME_HOST_ENVELOPE_SCHEMA");
}

#[test]
fn host_blocks_output_count_and_payload_bounds() {
    let schemas = RuntimeHostSchemaRegistry::new()
        .allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1")
        .with_bounds(0, 1);
    let mut host =
        ProductRuntimeHost::reference_in_process("instance", Provider::default(), schemas).unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .unwrap();
    let error = host
        .step(RuntimeStepInput {
            session_id: open.session_id,
            fixed_step: 1,
            action: "advance".into(),
            payload: serde_json::json!({}),
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_RUNTIME_HOST_OUTPUT_COUNT");
}

#[test]
fn host_validates_save_and_restore_sections_before_accepting_state() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let mut host =
        ProductRuntimeHost::reference_in_process("instance", Provider::default(), schemas).unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .unwrap();
    let invalid_restore = RuntimeSectionPayload {
        section_id: "state".into(),
        schema: "test.state.v1".into(),
        version: SchemaVersion::new(1, 0, 0),
        codec: RuntimeSectionCodec::Postcard,
        hash: Hash256::from_sha256(b"different"),
        bytes: vec![],
    };
    assert_eq!(
        host.restore(RuntimeRestoreRequest {
            session_id: open.session_id.clone(),
            sections: vec![invalid_restore],
        })
        .unwrap_err()
        .code(),
        "ASTRA_RUNTIME_HOST_SECTION_HASH"
    );
    let save_error = host
        .save(RuntimeSaveRequest {
            session_id: open.session_id.clone(),
            slot: "bad".into(),
        })
        .unwrap_err();
    assert_eq!(save_error.code(), "ASTRA_RUNTIME_HOST_SECTION_HASH");
    assert_eq!(
        host.step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            action: "advance".into(),
            payload: serde_json::json!({}),
        })
        .unwrap_err()
        .code(),
        "ASTRA_RUNTIME_HOST_SESSION_POISONED"
    );
    host.shutdown_session(open.session_id).unwrap();
    host.destroy().unwrap();
}

#[test]
fn host_rejects_non_monotonic_fixed_steps_and_poisons_the_session() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let mut host =
        ProductRuntimeHost::reference_in_process("instance", Provider::default(), schemas).unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .unwrap();
    let input = RuntimeStepInput {
        session_id: open.session_id.clone(),
        fixed_step: 1,
        action: "advance".into(),
        payload: serde_json::json!({}),
    };
    host.step(input.clone()).unwrap();
    let error = host.step(input).unwrap_err();
    assert_eq!(error.code(), "ASTRA_RUNTIME_HOST_STEP_ORDER");

    let poisoned = host
        .save(RuntimeSaveRequest {
            session_id: open.session_id,
            slot: "slot".into(),
        })
        .unwrap_err();
    assert_eq!(poisoned.code(), "ASTRA_RUNTIME_HOST_SESSION_POISONED");
}

#[test]
fn host_requires_first_step_one_and_catches_provider_panics() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let mut host =
        ProductRuntimeHost::reference_in_process("instance", Provider::default(), schemas).unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .unwrap();
    let error = host
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 2,
            action: "advance".into(),
            payload: serde_json::json!({}),
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_RUNTIME_HOST_STEP_ORDER");
    host.shutdown_session(open.session_id).unwrap();
    host.destroy().unwrap();

    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let mut host =
        ProductRuntimeHost::reference_in_process("instance-panic", Provider::default(), schemas)
            .unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .unwrap();
    let error = host
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            action: "panic".into(),
            payload: serde_json::json!({}),
        })
        .unwrap_err();
    assert_eq!(error.code(), "ASTRA_RUNTIME_HOST_PROVIDER_PANIC");
    assert_eq!(
        host.save(RuntimeSaveRequest {
            session_id: open.session_id.clone(),
            slot: "slot".into(),
        })
        .unwrap_err()
        .code(),
        "ASTRA_RUNTIME_HOST_SESSION_POISONED"
    );
    host.shutdown_session(open.session_id).unwrap();
    host.destroy().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn async_host_supports_multiple_sessions_on_one_ordered_provider_worker() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let host = AsyncProductRuntimeHost::reference_in_process(
        "instance",
        Provider::default(),
        schemas,
        Duration::from_secs(1),
    )
    .unwrap();
    let request = |seed| RuntimeOpenRequest {
        target_id: "test".into(),
        profile: "release".into(),
        locale: "und".into(),
        seed,
        package_hash: "sha256:test".into(),
        sections: vec![],
    };
    let first = host.open(request(1)).await.unwrap();
    let second = host.open(request(2)).await.unwrap();
    assert_ne!(first.session_id, second.session_id);

    for session_id in [first.session_id.clone(), second.session_id.clone()] {
        host.step(RuntimeStepInput {
            session_id,
            fixed_step: 1,
            action: "advance".into(),
            payload: serde_json::json!({}),
        })
        .await
        .unwrap();
    }

    host.shutdown(first.session_id).await.unwrap();
    host.shutdown(second.session_id).await.unwrap();
    host.destroy().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn async_host_timeout_poisons_the_provider_instance() {
    let host = AsyncProductRuntimeHost::reference_in_process(
        "instance",
        Provider::default(),
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1"),
        Duration::from_millis(5),
    )
    .unwrap();
    let open = host
        .open(RuntimeOpenRequest {
            target_id: "test".into(),
            profile: "release".into(),
            locale: "und".into(),
            seed: 1,
            package_hash: "sha256:test".into(),
            sections: vec![],
        })
        .await
        .unwrap();
    let timed_out = host
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            action: "slow".into(),
            payload: serde_json::json!({}),
        })
        .await
        .unwrap_err();
    assert_eq!(timed_out.code(), "ASTRA_RUNTIME_HOST_TIMEOUT");

    let poisoned = host
        .save(RuntimeSaveRequest {
            session_id: open.session_id,
            slot: "slot".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(poisoned.code(), "ASTRA_RUNTIME_HOST_INSTANCE_POISONED");
    host.cleanup_after_failure().await.unwrap();
}
