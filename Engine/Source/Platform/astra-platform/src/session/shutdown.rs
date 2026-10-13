use super::*;

impl PlatformHostClient {
    pub async fn shutdown(&self) -> Result<(), PlatformError> {
        if self
            .shutdown
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.shutdown",
                "platform host has already been shut down",
            ));
        }
        let (reply, response) = oneshot::channel();
        if let Err(error) = self.try_send(HostCommand::Shutdown { reply }) {
            self.shutdown.store(false, Ordering::Release);
            return Err(error);
        }
        match response.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.shutdown.store(false, Ordering::Release);
                Err(error)
            }
            Err(_) => {
                self.shutdown.store(false, Ordering::Release);
                Err(queue_closed("host.shutdown"))
            }
        }
    }

    pub(super) fn ensure_running(&self, operation: &'static str) -> Result<(), PlatformError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                operation,
                "platform host is shutting down",
            ));
        }
        Ok(())
    }

    pub(super) fn try_send(&self, command: HostCommand) -> Result<(), PlatformError> {
        let operation = command.operation();
        self.command_tx
            .try_send(command)
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    operation,
                    "platform command queue is full",
                ),
                mpsc::error::TrySendError::Closed(_) => queue_closed(operation),
            })?;
        if let Some(wake) = &self.command_wake {
            wake.wake(operation);
        }
        Ok(())
    }

    pub(super) async fn send_unit(
        &self,
        command: HostCommand,
        response: oneshot::Receiver<Result<(), PlatformError>>,
        operation: &'static str,
    ) -> Result<(), PlatformError> {
        self.ensure_running(operation)?;
        self.try_send(command)?;
        response.await.map_err(|_| queue_closed(operation))?
    }
}

pub(super) type PlatformCommandWakeCallback = dyn Fn() + Send + Sync + 'static;

/// Opaque in-process bridge used by native event-loop hosts to wake command
/// processing immediately after a successful queue submission. It contains no
/// platform handle and is never serialized or exposed through plugin ABIs.
#[derive(Clone, Default)]
pub struct PlatformCommandWakeRegistration {
    pub(super) callback: Arc<Mutex<Option<Arc<PlatformCommandWakeCallback>>>>,
}

impl PlatformCommandWakeRegistration {
    pub fn bind(&self, callback: impl Fn() + Send + Sync + 'static) -> Result<(), PlatformError> {
        let mut slot = self.callback.lock().map_err(|_| {
            PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.command_wake.bind",
                "platform command wake registration is poisoned",
            )
        })?;
        if slot.is_some() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "host.command_wake.bind",
                "platform command wake callback is already bound",
            ));
        }
        *slot = Some(Arc::new(callback));
        Ok(())
    }

    pub(super) fn wake(&self, operation: &'static str) {
        let callback = match self.callback.lock() {
            Ok(slot) => slot.clone(),
            Err(_) => {
                tracing::error!(
                    event = "platform.host.command_wake.poisoned",
                    operation,
                    "platform command wake registration is poisoned"
                );
                return;
            }
        };
        if let Some(callback) = callback {
            callback();
        }
    }

    /// Notify the owning event loop about work completed by a host worker.
    ///
    /// Queue submissions call this internally.  Platform workers (for
    /// example HTTPS package completion or decoder completion) use this
    /// explicit edge notification so the event loop does not need a timer to
    /// discover completed work.
    pub fn notify(&self) {
        self.wake("host.command_wake.notify");
    }
}

pub struct PlatformEventStream {
    pub(super) event_rx: mpsc::Receiver<PlatformEvent>,
    pub(super) last_sequence: u64,
}

impl PlatformEventStream {
    pub async fn recv(&mut self) -> Result<PlatformEvent, PlatformError> {
        let event = self
            .event_rx
            .recv()
            .await
            .ok_or_else(|| queue_closed("event.recv"))?;
        if event.sequence <= self.last_sequence {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidState,
                "event.recv",
                "platform event sequence is not strictly increasing",
            ));
        }
        self.last_sequence = event.sequence;
        Ok(event)
    }
}

pub struct PlatformBackendChannels {
    pub(super) command_rx: mpsc::Receiver<HostCommand>,
    pub(super) event_tx: mpsc::Sender<PlatformEvent>,
    pub(super) last_event_sequence: Arc<std::sync::atomic::AtomicU64>,
    pub(super) audio_wake: AudioWakeRegistration,
}

impl PlatformBackendChannels {
    pub fn audio_wake(&self) -> AudioWakeRegistration {
        self.audio_wake.clone()
    }

    pub async fn next_command(&mut self) -> Option<HostCommand> {
        self.command_rx.recv().await
    }

    pub fn try_next_command(&mut self) -> Result<Option<HostCommand>, PlatformError> {
        match self.command_rx.try_recv() {
            Ok(command) => Ok(Some(command)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(queue_closed("command.recv")),
        }
    }

    pub fn emit_event(&mut self, event: PlatformEvent) -> Result<(), PlatformError> {
        let mut previous = self.last_event_sequence.load(Ordering::Acquire);
        loop {
            if event.sequence <= previous {
                return Err(PlatformError::new(
                    PlatformErrorCode::InvalidState,
                    "event.emit",
                    "platform event sequence is not strictly increasing",
                ));
            }
            match self.last_event_sequence.compare_exchange(
                previous,
                event.sequence,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => previous = actual,
            }
        }
        self.event_tx
            .try_send(event.clone())
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    "event.emit",
                    "platform event queue is full",
                ),
                mpsc::error::TrySendError::Closed(_) => queue_closed("event.emit"),
            })?;
        Ok(())
    }

    pub fn event_emitter(&self) -> PlatformEventEmitter {
        PlatformEventEmitter {
            event_tx: self.event_tx.clone(),
            last_event_sequence: Arc::clone(&self.last_event_sequence),
        }
    }
}

#[derive(Clone)]
pub struct PlatformEventEmitter {
    pub(super) event_tx: mpsc::Sender<PlatformEvent>,
    pub(super) last_event_sequence: Arc<std::sync::atomic::AtomicU64>,
}

impl PlatformEventEmitter {
    pub fn emit(&self, kind: PlatformEventKind) -> Result<u64, PlatformError> {
        let sequence = self.last_event_sequence.fetch_add(1, Ordering::AcqRel) + 1;
        self.event_tx
            .try_send(PlatformEvent::new(sequence, kind))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PlatformError::new(
                    PlatformErrorCode::QueueOverflow,
                    "event.emit",
                    "platform event queue is full",
                ),
                mpsc::error::TrySendError::Closed(_) => queue_closed("event.emit"),
            })?;
        Ok(sequence)
    }
}

pub struct PlatformHostSession {
    pub client: PlatformHostClient,
    pub events: PlatformEventStream,
    pub profile: HostLaunchProfile,
}

pub fn host_channel(
    profile: impl Into<HostLaunchProfile>,
    command_capacity: usize,
    event_capacity: usize,
) -> Result<
    (
        PlatformHostClient,
        PlatformBackendChannels,
        PlatformEventStream,
    ),
    PlatformError,
> {
    host_channel_internal(profile, command_capacity, event_capacity, None)
}

pub fn host_channel_with_command_wake(
    profile: impl Into<HostLaunchProfile>,
    command_capacity: usize,
    event_capacity: usize,
    command_wake: PlatformCommandWakeRegistration,
) -> Result<
    (
        PlatformHostClient,
        PlatformBackendChannels,
        PlatformEventStream,
    ),
    PlatformError,
> {
    host_channel_internal(
        profile,
        command_capacity,
        event_capacity,
        Some(command_wake),
    )
}

pub(super) fn host_channel_internal(
    profile: impl Into<HostLaunchProfile>,
    command_capacity: usize,
    event_capacity: usize,
    command_wake: Option<PlatformCommandWakeRegistration>,
) -> Result<
    (
        PlatformHostClient,
        PlatformBackendChannels,
        PlatformEventStream,
    ),
    PlatformError,
> {
    let profile = profile.into();
    profile.validate()?;
    if command_capacity == 0 || event_capacity == 0 {
        return Err(PlatformError::new(
            PlatformErrorCode::InvalidProfile,
            "host.channel",
            "platform channel capacity must be non-zero",
        ));
    }
    let (command_tx, command_rx) = mpsc::channel(command_capacity);
    let (event_tx, event_rx) = mpsc::channel(event_capacity);
    let audio_wake = AudioWakeRegistration::default();
    Ok((
        PlatformHostClient {
            command_tx,
            command_wake,
            audio_wake: audio_wake.clone(),
            shutdown: Arc::new(AtomicBool::new(false)),
            profile: Arc::new(profile),
        },
        PlatformBackendChannels {
            command_rx,
            event_tx,
            last_event_sequence: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            audio_wake,
        },
        PlatformEventStream {
            event_rx,
            last_sequence: 0,
        },
    ))
}

pub(super) fn queue_closed(operation: &'static str) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::QueueClosed,
        operation,
        "platform host queue is closed",
    )
}

pub(super) fn validate_rgba_frame(
    width: u32,
    height: u32,
    rgba8: &[u8],
    max_bytes: usize,
) -> Result<(), PlatformError> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if width == 0 || height == 0 || expected != Some(rgba8.len()) || rgba8.len() > max_bytes {
        return Err(PlatformError::new(
            PlatformErrorCode::IntegrityMismatch,
            "surface.frame.validate",
            "RGBA frame dimensions and byte length do not match",
        ));
    }
    Ok(())
}
