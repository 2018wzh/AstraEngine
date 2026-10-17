use super::*;
use astra_emu_manager_core::model::{
    ConfigEntry, ConfigField, FamilyPointerRequest, FamilyWindowIcon,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use std::sync::Arc;
use std::thread::JoinHandle;

const COMMAND_CAPACITY: usize = 8;

type RegistryJob = Box<dyn FnOnce(&mut FamilyProviderRegistry) + Send>;
type Metadata = BTreeMap<String, (FamilyPluginDescriptor, Vec<ConfigField>)>;

pub(super) enum Command {
    Registry(RegistryJob),
    Open(Box<OpenSession>),
    Events {
        epoch: u64,
        events: Vec<FamilyEvent>,
    },
    Close {
        epoch: u64,
        events: Vec<FamilyEvent>,
    },
    Wake(HostWake),
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
    metadata: Metadata,
    modal: Mutex<Option<ModalBridge>>,
    window_service: Mutex<Option<astra_emu_manager::window_service::WindowBridge>>,
    text: Mutex<Option<TextReplacementBridge>>,
    cancellation: Mutex<Option<astra_emu_manager_core::NativeCancellation>>,
}

impl FamilyOwner {
    /// Construct the registry on its final thread; Rc-backed plugins are not Send.
    pub(super) fn start(
        directories: Vec<PathBuf>,
        mailbox: FrameMailbox,
    ) -> Result<(Self, BTreeMap<String, String>), String> {
        let (commands, receiver) = bounded(COMMAND_CAPACITY);
        let (ready, readiness) = bounded(1);
        let updates = Arc::new(Mutex::new(Update::default()));
        let shared = updates.clone();
        let worker = std::thread::Builder::new()
            .name("astra-family-owner".into())
            .stack_size(super::super::FAMILY_OWNER_STACK_BYTES)
            .spawn(move || super::owner_worker::run(directories, mailbox, receiver, shared, ready))
            .map_err(|error| format!("ASTRA_EMU_OWNER_START: {error}"))?;
        let initialized = readiness
            .recv()
            .map_err(|_| "ASTRA_EMU_OWNER_START_DISCONNECTED".to_owned());
        match initialized.and_then(|result| result) {
            Ok((metadata, errors)) => Ok((
                Self {
                    commands,
                    updates,
                    worker: Some(worker),
                    metadata,
                    modal: Mutex::new(None),
                    window_service: Mutex::new(None),
                    text: Mutex::new(None),
                    cancellation: Mutex::new(None),
                },
                errors,
            )),
            Err(error) => {
                let _ = worker.join();
                Err(error)
            }
        }
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

    pub(super) fn probe(
        &self,
        request: &ProbeRequest,
        plugin: Option<&str>,
        family: Option<&str>,
    ) -> Result<FamilyProbeSelection, String> {
        let request = request.clone();
        let plugin = plugin.map(str::to_owned);
        let family = family.map(str::to_owned);
        self.registry_call(move |registry| {
            registry
                .probe(&request, plugin.as_deref(), family.as_deref())
                .map_err(|error| error.to_string())
        })
    }

    fn registry_call<T: Send + 'static>(
        &self,
        call: impl FnOnce(&mut FamilyProviderRegistry) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        // Registry RPC is only admitted while idle. Playback never waits on UI RPC.
        let update = self.updates.lock();
        if update.epoch != 0 && !update.closed {
            return Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into());
        }
        drop(update);
        let (reply, response) = bounded(1);
        self.send(Command::Registry(Box::new(move |registry| {
            let _ = reply.send(call(registry));
        })))?;
        response
            .recv()
            .map_err(|_| "ASTRA_EMU_OWNER_DISCONNECTED".to_owned())?
    }

    #[cfg(test)]
    pub(super) fn register_provider(
        &mut self,
        factory: impl FnOnce() -> astra_emu_manager_core::LoadedFamilyPlugin + Send + 'static,
    ) -> Result<(), String> {
        self.metadata = self.registry_call(move |registry| {
            registry
                .register_provider(factory())
                .map_err(|error| error.to_string())?;
            collect_metadata(registry)
        })?;
        Ok(())
    }

    pub(super) fn open(&self, mut request: OpenSession) -> Result<(), String> {
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

    pub(super) fn set_wake(&self, wake: HostWake) -> Result<(), String> {
        self.send(Command::Wake(wake))
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
        let send = self.commands.send(Command::Shutdown);
        let result = worker
            .join()
            .map_err(|_| "ASTRA_EMU_OWNER_PANICKED".to_owned())?;
        result?;
        send.map_err(|_| "ASTRA_EMU_OWNER_DISCONNECTED".into())
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

pub(super) type Readiness = Sender<Result<(Metadata, BTreeMap<String, String>), String>>;
pub(super) type Commands = Receiver<Command>;

#[cfg(test)]
#[path = "owner_tests.rs"]
mod tests;
