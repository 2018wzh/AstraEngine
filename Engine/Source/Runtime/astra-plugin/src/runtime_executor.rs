use std::{future::Future, sync::mpsc, thread};

use super::RuntimeHostError;

type Job = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send>;

/// Own the synchronous facade's executor on a thread where both blocking
/// execution and runtime destruction are valid, including for async callers.
pub(super) struct RuntimeExecutor {
    sender: Option<mpsc::Sender<Job>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl RuntimeExecutor {
    pub(super) fn new() -> Result<Self, RuntimeHostError> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let (ready, started) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("astra-runtime-host".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(1)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        let _ = ready.send(Err(executor_error("executor initialization failed")));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while let Ok(job) = receiver.recv() {
                    job(&runtime);
                }
                // Runtime drops here, outside any entered async context.
            })
            .map_err(|_| executor_error("executor thread creation failed"))?;
        let executor = Self {
            sender: Some(sender),
            worker: Some(worker),
        };
        started
            .recv()
            .map_err(|_| executor_error("executor stopped during initialization"))??;
        Ok(executor)
    }

    pub(super) fn block_on<T, F>(&self, future: F) -> Result<T, RuntimeHostError>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, RuntimeHostError>> + Send + 'static,
    {
        let (reply, result) = mpsc::sync_channel(1);
        self.sender
            .as_ref()
            .ok_or_else(|| executor_error("executor is closed"))?
            .send(Box::new(move |runtime| {
                let _ = reply.send(runtime.block_on(future));
            }))
            .map_err(|_| executor_error("executor worker stopped"))?;
        result
            .recv()
            .map_err(|_| executor_error("executor stopped before completing the operation"))?
    }
}

impl Drop for RuntimeExecutor {
    fn drop(&mut self) {
        self.sender.take();
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            tracing::error!(
                event = "runtime.host.executor.shutdown_failed",
                diagnostic_code = "ASTRA_RUNTIME_HOST_EXECUTOR",
                "runtime executor worker panicked"
            );
        }
    }
}

fn executor_error(message: &str) -> RuntimeHostError {
    RuntimeHostError::new("ASTRA_RUNTIME_HOST_EXECUTOR", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[astra_headless_test::tokio_test]
    async fn sync_executor_runs_and_drops_inside_async_context() {
        let executor = RuntimeExecutor::new().unwrap();
        assert_eq!(executor.block_on(async { Ok(42_u32) }).unwrap(), 42);
        drop(executor);
        tokio::task::yield_now().await;
    }
}
