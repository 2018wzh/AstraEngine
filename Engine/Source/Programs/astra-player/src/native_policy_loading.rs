use astra_package::PackageReader;
use astra_platform::{
    PlatformError, PlatformErrorCode, PlatformEvent, PlatformEventKind, PlatformEventStream,
};
use astra_player_vn::{
    NativeVnDecodedCacheBudget, NativeVnHostOpenOptions, NativeVnRuntimeExecution,
    PreparedPolicyActions, DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES,
};
use astra_runtime::TaskScope;
use std::collections::VecDeque;

pub struct PreparedNativeVnOpen {
    pub options: NativeVnHostOpenOptions,
    pub events: VecDeque<PlatformEvent>,
}

struct LoadingWorker {
    scope: TaskScope,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for LoadingWorker {
    fn drop(&mut self) {
        self.scope.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Keep the real platform event stream responsive while authorized package text loads.
/// Closing or dropping startup cancels at a section boundary and joins its owned worker.
pub async fn prepare_native_vn_open(
    package: &PackageReader,
    events: &mut PlatformEventStream,
    execution: NativeVnRuntimeExecution,
) -> Result<Option<PreparedNativeVnOpen>, PlatformError> {
    execution.validate().map_err(error)?;
    let budget = NativeVnDecodedCacheBudget::partition(DEFAULT_NATIVE_VN_DECODED_CACHE_BYTES)
        .map_err(error)?;
    let scope = TaskScope::new();
    let worker_scope = scope.clone();
    let package = package.clone();
    let (send, mut result) = tokio::sync::oneshot::channel();
    let thread = std::thread::Builder::new()
        .name("astra-policy-preload".into())
        .spawn(move || {
            let _ = send.send(PreparedPolicyActions::load(
                &package,
                budget.asset_bytes,
                &worker_scope,
            ));
        })
        .map_err(error)?;
    let _worker = LoadingWorker {
        scope,
        thread: Some(thread),
    };
    let mut deferred = VecDeque::new();
    loop {
        tokio::select! {
            biased;
            event = events.recv() => {
                let event = event?;
                if matches!(event.kind, PlatformEventKind::WindowClosed { .. }) { return Ok(None); }
                if deferred.len() >= 1024 { return Err(error("startup event queue exhausted")); }
                deferred.push_back(event);
            }
            result = &mut result => {
                let policy_actions = result.map_err(error)?.map_err(error)?;
                return Ok(Some(PreparedNativeVnOpen { options: NativeVnHostOpenOptions {
                    max_asset_cache_bytes: budget.asset_bytes, max_glyph_cache_bytes: budget.glyph_bytes,
                    runtime_execution: execution, policy_actions,
                }, events: deferred }));
            }
        }
    }
}
fn error(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "player.policy.preload",
        error.to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropping_startup_worker_cancels_and_joins_before_returning() {
        let scope = TaskScope::new();
        let task_scope = scope.clone();
        let (ready, started) = std::sync::mpsc::channel();
        let (finished, joined) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap();
            ready.send(()).unwrap();
            runtime.block_on(task_scope.cancelled());
            finished.send(()).unwrap();
        });
        started
            .recv_timeout(std::time::Duration::from_secs(3))
            .unwrap();
        drop(LoadingWorker {
            scope,
            thread: Some(thread),
        });
        joined
            .try_recv()
            .expect("owned worker finished before startup cancellation returned");
    }
}
