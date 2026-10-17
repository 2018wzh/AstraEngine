use super::owner::{collect_metadata, Command, Commands, Readiness, Update};
use super::*;
use crossbeam_channel::RecvTimeoutError;
use parking_lot::Mutex;
use std::sync::Arc;

pub(super) fn run(
    directories: Vec<PathBuf>,
    mailbox: FrameMailbox,
    commands: Commands,
    shared: Arc<Mutex<Update>>,
    ready: Readiness,
) -> Result<(), String> {
    let mut registry = FamilyProviderRegistry::new();
    let initialization = initialize(&mut registry, &directories);
    match initialization {
        Ok(errors) => {
            let metadata = collect_metadata(&registry);
            let successful = metadata.is_ok();
            let _ = ready.send(metadata.map(|metadata| (metadata, errors)));
            if !successful {
                return Ok(());
            }
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return Ok(());
        }
    }
    let mut active: Option<ActiveFamilySession> = None;
    let mut wake: Option<HostWake> = None;
    let mut pending = Vec::new();
    loop {
        let command = if let Some(session) = active.as_ref() {
            commands.recv_timeout(
                session
                    .wake_deadline()
                    .saturating_duration_since(Instant::now()),
            )
        } else {
            commands.recv().map_err(|_| RecvTimeoutError::Disconnected)
        };
        match command {
            Ok(Command::Registry(call)) => call(&mut registry),
            Ok(Command::Wake(value)) => wake = Some(value),
            Ok(Command::Open(request)) => {
                pending.clear();
                let epoch = request.epoch;
                wake = Some(request.wake.clone());
                let audio = request
                    .candidate
                    .descriptor
                    .has_capability(astra_emu_manager_core::FamilyCapability::PcmAudio)
                    .then(|| {
                        audio_executor::HostAudioExecutor::new(request.audio_device, wake.clone())
                    });
                match ActiveFamilySession::open(
                    (&request.game, request.configuration, epoch),
                    &request.candidate,
                    &mut registry,
                    mailbox.clone(),
                    request.window,
                    super::session::SessionServices {
                        text: request.text,
                        audio,
                        modal: request.modal,
                    },
                ) {
                    Ok(mut session) => {
                        publish(&mut session, &shared);
                        active = Some(session);
                    }
                    Err(error) => closed(epoch, Some(error), &shared),
                }
                notify(&wake);
            }
            Ok(Command::Events { epoch, events }) => {
                if let Some(session) = active.as_mut().filter(|session| session.epoch() == epoch) {
                    if pending.len() + events.len() > astra_emu_family_api::MAX_EVENTS_PER_ADVANCE {
                        finish(
                            &mut active,
                            Some("ASTRA_EMU_INPUT_QUEUE_FULL".into()),
                            &shared,
                        );
                        pending.clear();
                        notify(&wake);
                    } else {
                        let lifecycle = events.iter().any(|event| {
                            matches!(
                                event,
                                FamilyEvent::WindowFocused { .. }
                                    | FamilyEvent::WindowVisibility { .. }
                                    | FamilyEvent::WindowSuspended { .. }
                            )
                        });
                        pending.extend(events);
                        if lifecycle {
                            let result = session.advance_host_events(&pending);
                            pending.clear();
                            if let Err(error) = result {
                                finish(&mut active, Some(error), &shared);
                            } else if let Some(session) = active.as_mut() {
                                publish(session, &shared);
                            }
                            notify(&wake);
                        }
                    }
                }
            }
            Ok(Command::Close { epoch, events }) => {
                if let Some(session) = active.as_mut().filter(|session| session.epoch() == epoch) {
                    let result = if pending.is_empty() {
                        Ok(())
                    } else {
                        session.advance_host_events(&pending)
                    };
                    pending.clear();
                    let result = result.and_then(|()| {
                        if events.is_empty() {
                            Ok(())
                        } else {
                            session.advance_host_events(&events)
                        }
                    });
                    finish(&mut active, result.err(), &shared);
                    notify(&wake);
                }
            }
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(mut session) = active.take() {
                    session.close()?;
                }
                return shared.lock().error.take().map_or(Ok(()), Err);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if active
            .as_ref()
            .is_some_and(|session| session.status == FamilyStatus::Finished)
        {
            finish(&mut active, None, &shared);
            pending.clear();
            notify(&wake);
        }
        // Commands cannot starve the logical clock. UI/render cadence is irrelevant.
        if let Some(session) = active.as_mut() {
            let result = service_due(session, &mut pending, Instant::now());
            let finished = session.status == FamilyStatus::Finished;
            match result {
                Err(error) => {
                    pending.clear();
                    finish(&mut active, Some(error), &shared);
                    notify(&wake);
                }
                Ok(_) if finished => {
                    finish(&mut active, None, &shared);
                    notify(&wake);
                }
                Ok(true) => {
                    if let Some(session) = active.as_mut() {
                        publish(session, &shared);
                    }
                    notify(&wake);
                }
                Ok(false) => {}
            }
        }
    }
}

/// GPU completion has its own wake; it must not consume a VM tick.
pub(super) fn service_due(
    session: &mut ActiveFamilySession,
    pending: &mut Vec<FamilyEvent>,
    now: Instant,
) -> Result<bool, String> {
    let mut changed = false;
    if session
        .frame_poll_deadline
        .is_some_and(|deadline| now >= deadline)
    {
        changed = session.poll_pending_frame()?;
    }
    // A successful restore resumes time only after its first output is ready.
    if !session.clock_reset_pending && now >= session.next_deadline {
        let elapsed = session
            .last_tick
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX)) as u64;
        let result = session
            .audio
            .as_ref()
            .map_or(Ok(()), |audio| audio.check_health())
            .and_then(|()| session.advance(elapsed.max(1), pending));
        pending.clear();
        result?;
        changed = true;
    }
    Ok(changed)
}

fn initialize(
    registry: &mut FamilyProviderRegistry,
    directories: &[PathBuf],
) -> Result<BTreeMap<String, String>, String> {
    #[cfg(target_os = "android")]
    registry
        .register_provider(astra_emu_fvp::FvpProvider::default())
        .map_err(|error| error.to_string())?;
    let mut errors = BTreeMap::new();
    #[cfg(not(target_os = "android"))]
    for failure in registry
        .load_directories(directories)
        .map_err(|error| error.to_string())?
        .errors
    {
        let file = failure
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("<unknown-core>")
            .to_owned();
        let diagnostic = format!(
            "{}: {}",
            failure.error.diagnostic_code(),
            failure.error.human_message()
        );
        tracing::error!(event = "astra.emu.plugin.discovery_failed", core = %file, diagnostic = %diagnostic);
        errors.insert(file, diagnostic);
    }
    #[cfg(target_os = "android")]
    let _ = directories;
    Ok(errors)
}

fn publish(session: &mut ActiveFamilySession, shared: &Mutex<Update>) {
    let mut update = shared.lock();
    if update.epoch != session.epoch() {
        return;
    }
    if !update.opened {
        update.opened = true;
        update.dirty = true;
    }
    if update.fullscreen != session.fullscreen {
        update.fullscreen = session.fullscreen;
        update.dirty = true;
    }
    if update.title != session.title {
        update.title.clone_from(&session.title);
        update.title_changed = true;
        update.dirty = true;
    }
    if let Some(icon) = session.pending_icon.take() {
        update.icon = Some(icon);
        update.dirty = true;
    }
    if let Some(pointer) = session.pending_pointer.take() {
        update
            .pointer
            .get_or_insert_with(Default::default)
            .merge(pointer);
        update.dirty = true;
    }
}

fn finish(
    active: &mut Option<ActiveFamilySession>,
    primary: Option<String>,
    shared: &Mutex<Update>,
) {
    if let Some(mut session) = active.take() {
        let epoch = session.epoch();
        let cleanup = session.close().err();
        let error = match (primary, cleanup) {
            (Some(first), Some(second)) => Some(format!("{first}; {second}")),
            (first, second) => first.or(second),
        };
        closed(epoch, error, shared);
    }
}

fn closed(epoch: u64, error: Option<String>, shared: &Mutex<Update>) {
    let mut update = shared.lock();
    if update.epoch != epoch {
        return;
    }
    update.closed = true;
    update.error = error;
    update.dirty = true;
}

fn notify(wake: &Option<HostWake>) {
    if let Some(wake) = wake {
        wake();
    }
}
