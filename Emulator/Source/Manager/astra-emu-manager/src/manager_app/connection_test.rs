use super::*;
use astra_emu_translation::ConnectionTestResult;
use std::sync::{mpsc, Arc};

pub(super) struct ConnectionTest {
    receiver: mpsc::Receiver<Result<ConnectionTestResult, String>>,
    task: tokio::task::JoinHandle<()>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl ConnectionTest {
    pub(super) fn start(
        profile: TranslationProfile,
        secrets: Arc<dyn SecretResolver>,
        wake: HostWake,
    ) -> Result<Self, String> {
        let client = astra_emu_translation::TranslationClient::new(profile, secrets)
            .map_err(|e| e.to_string())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|_| "ASTRA_EMU_TRANSLATION_TEST_RUNTIME")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let task = runtime.spawn(async move {
            let result = client.test_connection().await.map_err(|e| e.to_string());
            if sender.send(result).is_ok() {
                wake();
            }
        });
        Ok(Self {
            receiver,
            task,
            runtime: Some(runtime),
        })
    }

    pub(super) fn poll(&self) -> Option<Result<ConnectionTestResult, String>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("ASTRA_EMU_TRANSLATION_TEST_WORKER_STOPPED".into()))
            }
        }
    }
}

impl Drop for ConnectionTest {
    fn drop(&mut self) {
        self.task.abort();
        drop(self.runtime.take());
    }
}
