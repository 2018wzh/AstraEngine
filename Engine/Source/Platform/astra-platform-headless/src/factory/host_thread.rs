use super::{
    HeadlessHostProfile, HeadlessPlatformFactory, HostState, PlatformError, PlatformErrorCode,
};

pub(super) async fn spawn_host(
    factory: HeadlessPlatformFactory,
    profile: HeadlessHostProfile,
    backend: astra_platform::PlatformBackendChannels,
) -> Result<(), PlatformError> {
    let performance = factory.performance_observer.is_some();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name(if performance {
            "astra-headless-performance-host".into()
        } else {
            "astra-headless-host".into()
        })
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(thread_error("runtime", error.to_string())));
                    return;
                }
            };
            let scheduling = if performance {
                match astra_platform_common::PerformanceSchedulingGuard::activate() {
                    Ok(guard) => Some(guard),
                    Err(error) => {
                        let _ = ready_tx.send(Err(thread_error("scheduling", error)));
                        return;
                    }
                }
            } else {
                None
            };
            // Native decode and GPU resources stay on their owning executor,
            // including initialization failure and a cancelled start request.
            runtime.block_on(async move {
                match HostState::new(factory, profile, backend) {
                    Ok(state) => {
                        if ready_tx.send(Ok(())).is_ok() {
                            state.run().await;
                        }
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                    }
                }
            });
            if let Some(scheduling) = scheduling {
                if let Err(error) = scheduling.restore() {
                    tracing::error!(
                        event = "platform.headless.performance_scheduling.restore_failed",
                        diagnostic = %error,
                        "failed to restore Headless performance host scheduling policy"
                    );
                }
            }
        })
        .map_err(|error| thread_error("spawn", error.to_string()))?;
    ready_rx
        .await
        .map_err(|error| thread_error("handshake", error.to_string()))?
}

fn thread_error(stage: &str, diagnostic: String) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "headless.host.thread",
        "dedicated Headless host thread failed",
    )
    .with_field("stage", stage)
    .with_field("diagnostic", diagnostic)
}
