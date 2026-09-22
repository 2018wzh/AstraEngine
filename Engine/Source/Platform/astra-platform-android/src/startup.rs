use std::{
    io::Read,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

use android_activity::AndroidApp;
use astra_platform::{PlatformError, PlatformErrorCode};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
    platform::android::EventLoopBuilderExtAndroid,
    window::WindowId,
};

use super::{
    host::{create_host, AndroidHostApp},
    AndroidPreparedPlayer,
};

type Prepared = Result<AndroidPreparedPlayer, PlatformError>;
type Prepare = Arc<dyn Fn(Arc<Vec<u8>>, Arc<AtomicBool>) -> Prepared + Send + Sync>;
static RETRY: AtomicBool = AtomicBool::new(false);
static WAKE: std::sync::Mutex<Option<EventLoopProxy<()>>> = std::sync::Mutex::new(None);

pub(super) fn request_retry() {
    RETRY.store(true, Ordering::Release);
    if let Ok(wake) = WAKE.lock() {
        if let Some(wake) = wake.as_ref() {
            let _ = wake.send_event(());
        }
    }
}

pub(super) fn run<F>(app: AndroidApp, prepare: F) -> Result<(), PlatformError>
where
    F: Fn(Arc<Vec<u8>>, Arc<AtomicBool>) -> Prepared + Send + Sync + 'static,
{
    let mut builder = EventLoop::builder();
    builder.with_android_app(app.clone());
    let event_loop = builder
        .build()
        .map_err(|_| error("Android event loop creation failed"))?;
    let proxy = event_loop.create_proxy();
    let cancelled = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(1);
    let asset = app
        .asset_manager()
        .open(c"game.astrapkg")
        .ok_or_else(|| error("bundled game package is missing"))?;
    let total = asset.length();
    if total == 0 {
        return Err(error("bundled game package is empty"));
    }
    *WAKE.lock().expect("startup wake lock") = Some(proxy.clone());
    RETRY.store(false, Ordering::Release);
    let mut startup = Startup {
        app,
        bytes: Arc::new(Vec::with_capacity(total)),
        asset: Some(asset),
        total,
        buffer: vec![0; 1024 * 1024],
        prepare: Arc::new(prepare),
        tx: Some(tx),
        worker: None,
        rx,
        cancelled,
        proxy,
        host: None,
        resumed: false,
        result: None,
    };
    event_loop.set_control_flow(ControlFlow::Poll);
    let event_result = event_loop
        .run_app(&mut startup)
        .map_err(|_| error("Android event loop failed"));
    *WAKE.lock().expect("startup wake lock") = None;
    startup.cancelled.store(true, Ordering::Release);
    if let Some(worker) = startup.worker.take() {
        let _ = worker.join();
    }
    let player_result = startup.host.map(AndroidHostApp::finish).unwrap_or(Ok(()));
    event_result.and(startup.result.unwrap_or(player_result))
}

struct Startup {
    app: AndroidApp,
    bytes: Arc<Vec<u8>>,
    asset: Option<ndk::asset::Asset>,
    total: usize,
    buffer: Vec<u8>,
    prepare: Prepare,
    tx: Option<mpsc::SyncSender<Prepared>>,
    worker: Option<std::thread::JoinHandle<()>>,
    rx: mpsc::Receiver<Prepared>,
    cancelled: Arc<AtomicBool>,
    proxy: EventLoopProxy<()>,
    host: Option<AndroidHostApp>,
    resumed: bool,
    result: Option<Result<(), PlatformError>>,
}

impl Startup {
    fn reap_worker(&mut self) {
        if self
            .worker
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn retry(&mut self, event_loop: &ActiveEventLoop) {
        if !RETRY.swap(false, Ordering::AcqRel) || self.host.is_some() {
            return;
        }
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.cancelled = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx = Some(tx);
        self.rx = rx;
        self.result = None;
        self.bytes = Arc::new(Vec::with_capacity(self.total));
        self.asset = self.app.asset_manager().open(c"game.astrapkg");
        if self.asset.is_none() {
            super::set_loading_state(&self.app, "游戏包不可用，请返回", true);
            return;
        }
        super::set_loading_state(&self.app, "正在读取游戏包…", false);
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn read_chunk(&mut self, event_loop: &ActiveEventLoop) -> Result<(), PlatformError> {
        let Some(asset) = self.asset.as_mut() else {
            return Ok(());
        };
        if self.cancelled.load(Ordering::Acquire) {
            self.asset = None;
            return Ok(());
        }
        let count = asset
            .read(&mut self.buffer)
            .map_err(|_| error("game package read failed"))?;
        if count > 0 {
            let bytes =
                Arc::get_mut(&mut self.bytes).expect("reader exclusively owns package bytes");
            bytes.extend_from_slice(&self.buffer[..count]);
            if bytes.len() > self.total {
                return Err(error("game package length changed"));
            }
            if bytes.len().is_multiple_of(16 * 1024 * 1024) {
                super::set_loading_state(
                    &self.app,
                    &format!(
                        "正在读取游戏包：{} / {} MB",
                        bytes.len() / 1024 / 1024,
                        self.total.div_ceil(1024 * 1024)
                    ),
                    false,
                );
            }
            event_loop.set_control_flow(ControlFlow::Poll);
            return Ok(());
        }
        self.asset = None;
        if self.bytes.len() != self.total {
            return Err(error("game package is truncated"));
        }
        super::set_loading_state(&self.app, "正在验证游戏包…", false);
        let prepare = Arc::clone(&self.prepare);
        let bytes = Arc::clone(&self.bytes);
        let cancelled = Arc::clone(&self.cancelled);
        let tx = self.tx.take().expect("preparation starts once");
        let wake = self.proxy.clone();
        self.worker = Some(
            std::thread::Builder::new()
                .name("astra-android-package".into())
                .spawn(move || {
                    let result = prepare(bytes, Arc::clone(&cancelled));
                    if !cancelled.load(Ordering::Acquire) {
                        let _ = tx.send(result);
                    }
                    let _ = wake.send_event(());
                })
                .map_err(|_| error("Android package worker could not start"))?,
        );
        event_loop.set_control_flow(ControlFlow::Wait);
        Ok(())
    }

    fn accept_prepared(&mut self, event_loop: &ActiveEventLoop) {
        if self.host.is_some() || self.cancelled.load(Ordering::Acquire) {
            return;
        }
        let Ok(prepared) = self.rx.try_recv() else {
            return;
        };
        let result = prepared.and_then(|prepared| {
            create_host(
                self.app.clone(),
                prepared.profile,
                Arc::clone(&self.bytes),
                prepared.storage_hash,
                prepared.player,
                self.proxy.clone(),
            )
        });
        match result {
            Ok(mut host) => {
                super::set_loading_state(&self.app, "正在打开游戏…", false);
                if self.resumed {
                    host.resumed(event_loop);
                }
                self.host = Some(host);
            }
            Err(error) => {
                tracing::error!(event = "player.android.prepare.failed", operation = %error.operation, diagnostic_code = ?error.code, "Android package could not be opened");
                super::set_loading_state(&self.app, "游戏加载失败，请返回后重试", true);
                self.result = Some(Err(error));
                self.cancelled.store(true, Ordering::Release);
            }
        }
    }
}

impl ApplicationHandler for Startup {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed = true;
        if let Some(host) = self.host.as_mut() {
            host.resumed(event_loop);
        }
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed = false;
        if let Some(host) = self.host.as_mut() {
            host.suspended(event_loop);
        } else {
            self.cancelled.store(true, Ordering::Release);
            self.asset = None;
            event_loop.set_control_flow(ControlFlow::Wait);
            super::set_loading_state(&self.app, "加载已取消，请重试", true);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(host) = self.host.as_mut() {
            host.window_event(event_loop, id, event);
        }
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        self.retry(event_loop);
        self.accept_prepared(event_loop);
        if let Some(host) = self.host.as_mut() {
            host.user_event(event_loop, event);
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.reap_worker();
        if let Err(error) = self.read_chunk(event_loop) {
            self.cancelled.store(true, Ordering::Release);
            self.asset = None;
            self.result = Some(Err(error));
            super::set_loading_state(&self.app, "读取游戏包失败，请重试", true);
            event_loop.set_control_flow(ControlFlow::Wait);
        }
        self.accept_prepared(event_loop);
        if let Some(host) = self.host.as_mut() {
            host.about_to_wait(event_loop);
        }
    }
    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(host) = self.host.as_mut() {
            host.memory_warning(event_loop);
        }
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(host) = self.host.as_mut() {
            host.exiting(event_loop);
        }
    }
}

fn error(message: &str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::InvalidState,
        "player.android.prepare",
        message,
    )
}
