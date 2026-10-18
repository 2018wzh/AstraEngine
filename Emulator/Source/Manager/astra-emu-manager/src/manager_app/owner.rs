use super::*;
use astra_emu_manager_core::model::{
    ConfigEntry, ConfigField, FamilyPointerRequest, FamilyWindowIcon,
};
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

const COMMAND_CAPACITY: usize = 8;

#[cfg(test)]
type RegistryJob = Box<dyn FnOnce(&mut FamilyProviderRegistry) -> Result<Metadata, String> + Send>;
pub(super) type Metadata = BTreeMap<String, (FamilyPluginDescriptor, Vec<ConfigField>)>;

pub(super) enum BackgroundEvent {
    Ready(Result<(Metadata, BTreeMap<String, String>), String>),
    Scanned(
        super::probe_worker::ScanKind,
        Result<super::probe_worker::ScanReport, String>,
    ),
    Exited(String),
}

pub(super) enum Command {
    #[cfg(test)]
    Registry(RegistryJob),
    Scan(super::probe_worker::ScanRequest),
    Open(Box<OpenSession>),
    Events {
        epoch: u64,
        events: Vec<FamilyEvent>,
    },
    Close {
        epoch: u64,
        events: Vec<FamilyEvent>,
    },
    Shutdown,
}

pub(super) struct OpenSession {
    pub(super) epoch: u64,
    pub(super) game: GameRecord,
    pub(super) candidate: FamilyProbeCandidate,
    pub(super) configuration: Vec<ConfigEntry>,
    pub(super) window: WindowState,
    pub(super) text: Option<TextReplacementBridge>,
    pub(super) wake: HostWake,
    pub(super) modal: Option<ModalBridge>,
    pub(super) window_service: Option<astra_emu_manager::window_service::WindowBridge>,
    pub(super) cancellation: astra_emu_manager_core::NativeCancellation,
}

/// UI metadata only. Native sessions, providers and GPU owners never leave the worker.
pub(super) struct ActiveSessionView {
    pub(super) epoch: u64,
    pub(super) game_id: String,
    pub(super) opened: bool,
    pub(super) closing: Option<PlaySessionEndReason>,
    pub(super) fullscreen: bool,
    pub(super) title: Option<String>,
    pub(super) pending_icon: Option<FamilyWindowIcon>,
    pub(super) pending_pointer: Option<FamilyPointerRequest>,
    pub(super) failure: Option<super::session::NativeFailure>,
}

impl ActiveSessionView {
    pub(super) fn opening(epoch: u64, game_id: String) -> Self {
        Self {
            epoch,
            game_id,
            opened: false,
            closing: None,
            fullscreen: false,
            title: None,
            pending_icon: None,
            pending_pointer: None,
            failure: None,
        }
    }
}

#[derive(Default)]
pub(super) struct Update {
    pub(super) epoch: u64,
    pub(super) dirty: bool,
    pub(super) opened: bool,
    pub(super) closed: bool,
    pub(super) error: Option<String>,
    pub(super) failure: Option<super::session::NativeFailure>,
}

pub(super) struct FamilyOwner {
    commands: Sender<Command>,
    updates: Arc<Mutex<Update>>,
    worker: Option<JoinHandle<Result<(), String>>>,
    background: Receiver<BackgroundEvent>,
    deferred: Option<(PathBuf, FrameMailbox, Commands, Sender<BackgroundEvent>)>,
    wake: Arc<Mutex<Option<HostWake>>>,
    cancelled: Arc<AtomicBool>,
    ready: bool,
    exit_reported: bool,
    metadata: Metadata,
    modal: Mutex<Option<ModalBridge>>,
    window_service: Mutex<Option<astra_emu_manager::window_service::WindowBridge>>,
    text: Mutex<Option<TextReplacementBridge>>,
    cancellation: Mutex<Option<astra_emu_manager_core::NativeCancellation>>,
}

impl FamilyOwner {
    /// Construction is DB/settings-only; explicit core refresh starts the fixed native thread.
    pub(super) fn new(data_dir: PathBuf, mailbox: FrameMailbox) -> Self {
        let (commands, receiver) = bounded(COMMAND_CAPACITY);
        let (results, background) = unbounded();
        Self {
            commands,
            updates: Arc::new(Mutex::new(Update::default())),
            worker: None,
            background,
            deferred: Some((data_dir, mailbox, receiver, results)),
            wake: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
            ready: false,
            exit_reported: false,
            metadata: BTreeMap::new(),
            modal: Mutex::new(None),
            window_service: Mutex::new(None),
            text: Mutex::new(None),
            cancellation: Mutex::new(None),
        }
    }

    pub(super) fn start(&mut self) -> Result<(), String> {
        let Some((data_dir, mailbox, receiver, results)) = self.deferred.take() else {
            return Err("ASTRA_EMU_OWNER_ALREADY_STARTED".into());
        };
        let shared = self.updates.clone();
        let wake = self.wake.clone();
        let cancelled = self.cancelled.clone();
        self.worker = Some(
            std::thread::Builder::new()
                .name("astra-family-owner".into())
                .stack_size(super::super::FAMILY_OWNER_STACK_BYTES)
                .spawn(move || {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        super::owner_worker::run(
                            data_dir,
                            mailbox,
                            receiver,
                            shared.clone(),
                            results.clone(),
                            wake.clone(),
                            cancelled,
                        )
                    }))
                    .unwrap_or_else(|_| Err("ASTRA_EMU_OWNER_PANICKED".into()));
                    let diagnostic = result
                        .as_ref()
                        .err()
                        .cloned()
                        .unwrap_or_else(|| "ASTRA_EMU_OWNER_EXITED".into());
                    if let Err(error) = &result {
                        let mut update = shared.lock();
                        if update.epoch != 0 && !update.closed {
                            update.closed = true;
                            update.error = Some(error.clone());
                            update.dirty = true;
                        }
                    }
                    let _ = results.send(BackgroundEvent::Exited(diagnostic));
                    let callback = wake.lock().clone();
                    if let Some(wake) = callback {
                        wake();
                    }
                    result
                })
                .map_err(|error| format!("ASTRA_EMU_OWNER_START: {error}"))?,
        );
        Ok(())
    }

    pub(super) fn take_background(&mut self) -> Option<BackgroundEvent> {
        if let Ok(event) = self.background.try_recv() {
            if let BackgroundEvent::Ready(result) = &event {
                self.ready = result.is_ok();
                if let Ok((metadata, _)) = result {
                    self.metadata.clone_from(metadata);
                }
            }
            if matches!(&event, BackgroundEvent::Exited(_)) {
                self.ready = false;
                self.exit_reported = true;
            }
            return Some(event);
        }
        if !self.exit_reported && self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            self.exit_reported = true;
            self.ready = false;
            return Some(BackgroundEvent::Exited("ASTRA_EMU_OWNER_EXITED".into()));
        }
        None
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }

    pub(super) fn scan(&self, request: super::probe_worker::ScanRequest) -> Result<(), String> {
        self.ensure_idle()?;
        self.send(Command::Scan(request))
    }

    fn ensure_idle(&self) -> Result<(), String> {
        if !self.ready {
            return Err("ASTRA_EMU_FAMILY_INITIALIZING".into());
        }
        let update = self.updates.lock();
        if update.epoch != 0 && !update.closed {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        Ok(())
    }

    pub(super) fn descriptor(&self, id: &str) -> Option<&FamilyPluginDescriptor> {
        self.metadata.get(id).map(|(descriptor, _)| descriptor)
    }

    pub(super) fn descriptors(&self) -> impl Iterator<Item = &FamilyPluginDescriptor> {
        self.metadata.values().map(|(descriptor, _)| descriptor)
    }

    pub(super) fn configuration(&self, id: &str) -> Result<Vec<ConfigField>, String> {
        self.metadata
            .get(id)
            .map(|(_, schema)| schema.clone())
            .ok_or_else(|| "ASTRA_EMU_FAMILY_PROVIDER_MISSING".into())
    }

    #[cfg(test)]
    pub(super) fn register_provider(
        &mut self,
        factory: impl FnOnce() -> astra_emu_manager_core::LoadedFamilyPlugin + Send + 'static,
    ) -> Result<(), String> {
        self.send(Command::Registry(Box::new(move |registry| {
            registry
                .register_provider(factory())
                .map_err(|error| error.to_string())?;
            collect_metadata(registry)
        })))
    }

    pub(super) fn open(&self, mut request: OpenSession) -> Result<(), String> {
        self.ensure_idle()?;
        *self.wake.lock() = Some(request.wake.clone());
        let epoch = request.epoch;
        let mut shared = self.updates.lock();
        if shared.epoch != 0 && !shared.closed {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        let modal =
            ModalBridge::new(epoch, request.wake.clone()).map_err(|error| error.to_string())?;
        request.modal = Some(modal.clone());
        *self.modal.lock() = Some(modal.clone());
        let window_service = astra_emu_manager::window_service::WindowBridge::new(
            epoch,
            modal.clone(),
            request.wake.clone(),
        );
        request.window_service = Some(window_service.clone());
        *self.window_service.lock() = Some(window_service);
        *self.text.lock() = request.text.clone();
        *self.cancellation.lock() = Some(request.cancellation.clone());
        if let Err(error) = self.send(Command::Open(Box::new(request))) {
            modal.cancel_pending();
            self.modal.lock().take();
            return Err(error);
        }
        *shared = Update {
            epoch,
            ..Update::default()
        };
        Ok(())
    }

    pub(super) fn events(&self, epoch: u64, events: Vec<FamilyEvent>) -> Result<(), String> {
        if events.len() > astra_emu_manager_core::model::MAX_INPUT_EVENTS {
            return Err("ASTRA_EMU_INPUT_QUEUE_FULL".into());
        }
        self.send(Command::Events { epoch, events })
    }

    pub(super) fn close(&self, epoch: u64, events: Vec<FamilyEvent>) -> Result<(), String> {
        if let Some(window) = self.window_service.lock().as_ref() {
            window.close();
        }
        if let Some(text) = self.text.lock().as_ref() {
            text.cancel();
        }
        if let Some(cancellation) = self.cancellation.lock().as_ref() {
            cancellation.cancel();
        }
        let modal = self
            .modal
            .lock()
            .as_ref()
            .filter(|modal| modal.epoch() == epoch)
            .cloned();
        if let Some(modal) = modal {
            modal.cancel_pending();
        }
        self.send(Command::Close { epoch, events })
    }

    pub(super) fn set_wake(&self, wake: HostWake) {
        *self.wake.lock() = Some(wake);
    }

    pub(super) fn take_update(&self, epoch: u64) -> Option<Update> {
        let mut shared = self.updates.lock();
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) && !shared.closed {
            shared.closed = true;
            shared.error = Some("ASTRA_EMU_OWNER_EXITED".into());
            shared.dirty = true;
        }
        if !shared.dirty || shared.epoch != epoch {
            return None;
        }
        shared.dirty = false;
        Some(Update {
            epoch,
            dirty: false,
            opened: shared.opened,
            closed: shared.closed,
            error: shared.error.take(),
            failure: shared.failure.clone(),
        })
    }

    pub(super) fn take_modal_update(
        &self,
    ) -> Option<astra_emu_manager::modal_service::ModalUiUpdate> {
        self.modal
            .lock()
            .as_ref()
            .and_then(ModalBridge::take_ui_update)
    }

    pub(super) fn respond_modal(
        &self,
        id: astra_emu_manager::modal_service::ModalId,
        response: astra_emu_manager_core::model::ModalResponse,
    ) -> Result<(), String> {
        self.modal
            .lock()
            .as_ref()
            .ok_or_else(|| "ASTRA_EMU_MODAL_STALE_RESPONSE".to_owned())?
            .respond(id, response)
            .map_err(|error| error.to_string())
    }
    pub(super) fn take_window_request(
        &self,
    ) -> Option<astra_emu_manager::window_service::WindowPresentation> {
        self.window_service
            .lock()
            .as_ref()
            .and_then(|service| service.take())
    }
    pub(super) fn respond_window(
        &self,
        request: &astra_emu_manager::window_service::WindowPresentation,
        result: Result<astra_emu_manager::window_service::WindowOutcome, String>,
    ) -> Result<(), String> {
        self.window_service
            .lock()
            .as_ref()
            .ok_or("ASTRA_EMU_WINDOW_CLOSED")?
            .respond(request, result)
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                crossbeam_channel::TrySendError::Full(_) => "ASTRA_EMU_OWNER_QUEUE_FULL".into(),
                crossbeam_channel::TrySendError::Disconnected(_) => {
                    "ASTRA_EMU_OWNER_DISCONNECTED".into()
                }
            })
    }

    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        self.cancelled.store(true, Ordering::Release);
        self.deferred.take();
        if let Some(window) = self.window_service.lock().as_ref() {
            window.close();
        }
        if let Some(text) = self.text.lock().as_ref() {
            text.cancel();
        }
        if let Some(cancellation) = self.cancellation.lock().as_ref() {
            cancellation.cancel();
        }
        let modal = self.modal.lock().as_ref().cloned();
        if let Some(modal) = modal {
            modal.cancel_pending();
        }
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        // Final process teardown: ordered close, then join. Ordinary UI close is asynchronous.
        let _ = self.commands.try_send(Command::Shutdown);
        worker
            .join()
            .map_err(|_| "ASTRA_EMU_OWNER_PANICKED".to_owned())?
    }
}

impl Drop for FamilyOwner {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            tracing::error!(event = "astra.emu.owner.shutdown_failed", diagnostic_code = %error);
        }
    }
}

pub(super) fn collect_metadata(registry: &FamilyProviderRegistry) -> Result<Metadata, String> {
    registry
        .descriptors()
        .map(|descriptor| {
            let schema = registry
                .configuration(&descriptor.plugin_id)
                .map_err(|error| error.to_string())?;
            Ok((descriptor.plugin_id.clone(), (descriptor.clone(), schema)))
        })
        .collect()
}

pub(super) type BackgroundSender = Sender<BackgroundEvent>;
pub(super) type Commands = Receiver<Command>;

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
