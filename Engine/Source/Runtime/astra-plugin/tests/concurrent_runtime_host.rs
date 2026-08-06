use std::{
    sync::{Arc, Barrier},
    time::Duration,
};

use astra_plugin::{
    ConcurrentProductRuntimeHost, ProductRuntimeProviderFactory, ProductRuntimeSession,
    RuntimeHostSchemaRegistry,
};
use astra_plugin_abi::*;

struct Factory {
    step_barrier: Arc<Barrier>,
}

struct Session {
    step_barrier: Arc<Barrier>,
}

impl ProductRuntimeProviderFactory for Factory {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Ok(ProductRuntimeDescriptor {
            runtime_id: "test.concurrent".into(),
            product_kind: "test".into(),
            provider_id: "test.concurrent.provider".into(),
            supported_targets: vec!["test".into()],
            capabilities: vec!["runtime.test".into()],
            package_sections: vec![],
            release_checks: vec![],
            output_schemas: vec![],
        })
    }

    fn create_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".into(),
            diagnostics: vec![],
        })
    }

    fn destroy_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&self, _: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        unreachable!()
    }

    fn probe(&self, _: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        unreachable!()
    }

    fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<(RuntimeOpenReport, Box<dyn ProductRuntimeSession>), String> {
        let session_id = GameRuntimeSessionId(format!("session-{}", request.seed));
        Ok((
            RuntimeOpenReport {
                session_id,
                runtime_id: "test.concurrent".into(),
                provider_id: "test.concurrent.provider".into(),
                diagnostics: vec![],
            },
            Box::new(Session {
                step_barrier: Arc::clone(&self.step_barrier),
            }),
        ))
    }
}

impl ProductRuntimeSession for Session {
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        if input.action == "fail" {
            return Err("ASTRA_TEST_SESSION_FAILURE".into());
        }
        self.step_barrier.wait();
        Ok(RuntimeStepOutput {
            session_id: input.session_id,
            status: "idle".into(),
            live: Default::default(),
            persisted: vec![],
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
            restored_fixed_step: 0,
            session_seed: 0,
            status: "restored".into(),
            diagnostics: vec![],
        })
    }

    fn shutdown(
        self: Box<Self>,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        Ok(RuntimeShutdownReport {
            session_id,
            status: "shutdown".into(),
            diagnostics: vec![],
        })
    }
}

fn open_request(seed: u64) -> RuntimeOpenRequest {
    RuntimeOpenRequest {
        target_id: "test".into(),
        profile: "evidence".into(),
        locale: "und".into(),
        seed,
        integrity_mode: RuntimeTickIntegrityMode::Evidence,
        executor: RuntimeExecutorConfig::parallel(2),
        package_hash: "sha256:test".into(),
        sections: vec![],
    }
}

fn step_input(session_id: GameRuntimeSessionId, seed: u64) -> RuntimeStepInput {
    RuntimeStepInput {
        session_id,
        fixed_step: 1,
        delta_ns: 16_666_667,
        session_seed: seed,
        mode: RuntimeStepMode::Live,
        action: "advance".into(),
        ..RuntimeStepInput::default()
    }
}

fn step_at(session_id: GameRuntimeSessionId, seed: u64, fixed_step: u64) -> RuntimeStepInput {
    RuntimeStepInput {
        fixed_step,
        ..step_input(session_id, seed)
    }
}

#[tokio::test]
async fn different_sessions_execute_provider_steps_concurrently() {
    let factory = Factory {
        step_barrier: Arc::new(Barrier::new(2)),
    };
    let host = ConcurrentProductRuntimeHost::new(
        "instance",
        factory,
        RuntimeHostSchemaRegistry::new(),
        Duration::from_secs(2),
    )
    .unwrap();
    let first = host.open(open_request(1)).await.unwrap();
    let second = host.open(open_request(2)).await.unwrap();

    let (first_output, second_output) = tokio::join!(
        host.step(step_input(first.session_id.clone(), 1)),
        host.step(step_input(second.session_id.clone(), 2))
    );
    assert_eq!(first_output.unwrap().session_id, first.session_id);
    assert_eq!(second_output.unwrap().session_id, second.session_id);

    host.shutdown(first.session_id).await.unwrap();
    host.shutdown(second.session_id).await.unwrap();
    host.destroy().await.unwrap();
}

#[tokio::test]
async fn provider_failure_poisons_only_the_failing_session() {
    let factory = Factory {
        step_barrier: Arc::new(Barrier::new(1)),
    };
    let host = ConcurrentProductRuntimeHost::new(
        "instance",
        factory,
        RuntimeHostSchemaRegistry::new(),
        Duration::from_secs(2),
    )
    .unwrap();
    let failed = host.open(open_request(1)).await.unwrap();
    let healthy = host.open(open_request(2)).await.unwrap();

    let mut failed_input = step_input(failed.session_id.clone(), 1);
    failed_input.action = "fail".into();
    assert_eq!(
        host.step(failed_input).await.unwrap_err().code(),
        "ASTRA_RUNTIME_HOST_STEP"
    );
    assert_eq!(
        host.step(step_input(failed.session_id.clone(), 1))
            .await
            .unwrap_err()
            .code(),
        "ASTRA_RUNTIME_HOST_SESSION_POISONED"
    );
    assert!(host
        .step(step_input(healthy.session_id.clone(), 2))
        .await
        .is_ok());

    // A poisoned session is still explicitly shut down during cleanup.
    host.shutdown(failed.session_id).await.unwrap();
    host.shutdown(healthy.session_id).await.unwrap();
    host.destroy().await.unwrap();
}

#[tokio::test]
async fn same_session_mailbox_preserves_fifo_step_order() {
    let factory = Factory {
        step_barrier: Arc::new(Barrier::new(1)),
    };
    let host = ConcurrentProductRuntimeHost::new(
        "instance",
        factory,
        RuntimeHostSchemaRegistry::new(),
        Duration::from_secs(2),
    )
    .unwrap();
    let opened = host.open(open_request(7)).await.unwrap();
    let first_host = host.clone();
    let first_session = opened.session_id.clone();
    let first = tokio::spawn(async move { first_host.step(step_at(first_session, 7, 1)).await });
    tokio::task::yield_now().await;
    let second = host.step(step_at(opened.session_id.clone(), 7, 2));
    let (first, second) = tokio::join!(first, second);
    assert!(first.unwrap().is_ok());
    assert!(second.is_ok());

    host.shutdown(opened.session_id).await.unwrap();
    host.destroy().await.unwrap();
}
