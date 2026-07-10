use astra_core::SchemaVersion;
use std::time::Duration;

use astra_plugin::{
    AsyncProductRuntimeHost, ProductRuntimeHost, ProductRuntimeProvider, RuntimeHostSchemaRegistry,
};
use astra_plugin_abi::*;

#[derive(Default)]
struct Provider {
    opened: bool,
}

impl ProductRuntimeProvider for Provider {
    fn prepare(&mut self, _: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        Ok(RuntimePrepareReport {
            runtime_id: "test".into(),
            provider_id: "test.provider".into(),
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
            sections: vec![],
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
        ProductRuntimeHost::in_process("instance", Provider::default(), schemas).unwrap();
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
fn host_blocks_unknown_step_schema() {
    let mut host = ProductRuntimeHost::in_process(
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
        ProductRuntimeHost::in_process("instance", Provider::default(), schemas).unwrap();
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
fn host_rejects_non_monotonic_fixed_steps_and_poisons_the_session() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let mut host =
        ProductRuntimeHost::in_process("instance", Provider::default(), schemas).unwrap();
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

#[tokio::test(flavor = "current_thread")]
async fn async_host_supports_multiple_sessions_on_one_ordered_provider_worker() {
    let schemas =
        RuntimeHostSchemaRegistry::new().allow(RuntimeOutputDomain::Effect, "astra.test.effect.v1");
    let host = AsyncProductRuntimeHost::in_process(
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
    let host = AsyncProductRuntimeHost::in_process(
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
}
