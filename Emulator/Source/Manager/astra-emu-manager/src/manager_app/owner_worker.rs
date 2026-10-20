use super::owner::{
    BackgroundEvent, BackgroundSender, Command, Commands, Update, collect_metadata,
};
use super::*;
use crossbeam_channel::RecvTimeoutError;
use parking_lot::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn run(
    data_dir: PathBuf,
    mailbox: FrameMailbox,
    commands: Commands,
    shared: Arc<Mutex<Update>>,
    results: BackgroundSender,
    wake: Arc<Mutex<Option<HostWake>>>,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut registry = FamilyProviderRegistry::new();
    astra_emu_manager::startup::mark("core_loading_started");
    let initialized = (|| {
        if cancelled.load(Ordering::Acquire) {
            return Err("ASTRA_EMU_OWNER_CANCELLED".into());
        }
        let cores = data_dir.join("cores");
        fs::create_dir_all(&cores).map_err(|_| "ASTRA_EMU_CORES_DIRECTORY_CREATE")?;
        let pending_failures = core_package::apply_pending(&cores);
        #[cfg(not(target_os = "android"))]
        let directories = core_directories(&cores)?;
        #[cfg(target_os = "android")]
        let directories = Vec::new();
        let mut errors = initialize(&mut registry, &directories)?;
        for (file, error) in pending_failures {
            errors.insert(file, error);
        }
        let metadata = collect_metadata(&registry, directories.get(1).map(PathBuf::as_path))?;
        Ok((directories, metadata, errors))
    })();
    let directories = match initialized {
        Ok((directories, metadata, errors)) => {
            astra_emu_manager::startup::mark("core_metadata_ready");
            let _ = results.send(BackgroundEvent::Ready(Ok((metadata, errors))));
            notify(&wake);
            directories
        }
        Err(error) => {
            let _ = results.send(BackgroundEvent::Ready(Err(error)));
            notify(&wake);
            return registry.shutdown().map_err(|error| error.to_string());
        }
    };
    let inventory = probe_cache::Inventory::read(&directories, &registry);
    let mut cache = probe_cache::ProbeCache::load(&data_dir, inventory);
    let mut active: Option<ActiveFamilySession> = None;
    let mut pending = Vec::new();
    loop {
        if cancelled.load(Ordering::Acquire) {
            let primary = shared.lock().error.take();
            let session_error = active.take().and_then(|mut session| session.close().err());
            let owner_error = registry.shutdown().err().map(|error| error.to_string());
            let errors: Vec<_> = [primary, session_error, owner_error]
                .into_iter()
                .flatten()
                .collect();
            return if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("; "))
            };
        }
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
            #[cfg(test)]
            Ok(Command::Registry(call)) => {
                let metadata = call(&mut registry);
                let _ = results.send(BackgroundEvent::Ready(
                    metadata.map(|metadata| (metadata, BTreeMap::new())),
                ));
                // A registry mutation invalidates hints, including duplicate rejection.
                cache = probe_cache::ProbeCache::load(&data_dir, None);
                notify(&wake);
            }
            Ok(Command::Scan(request)) => {
                let kind = request.kind;
                let result = if active.is_some() {
                    Err("ASTRA_EMU_FAMILY_SESSION_ALREADY_ACTIVE".into())
                } else {
                    probe_worker::scan(&registry, &mut cache, request, &cancelled)
                };
                let _ = results.send(BackgroundEvent::Scanned(kind, result));
                notify(&wake);
            }
            Ok(Command::Open(request)) => {
                pending.clear();
                let epoch = request.epoch;
                match ActiveFamilySession::open(
                    (&request.game, request.configuration, epoch),
                    &request.candidate,
                    &mut registry,
                    mailbox.clone(),
                    request.window,
                    request.render_extent,
                    super::session::SessionServices {
                        text: request.text,
                        window: request.window_service,
                        modal: request.modal,
                        cancellation: request.cancellation,
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
                    if pending.len() + events.len()
                        > astra_emu_manager_core::model::MAX_INPUT_EVENTS
                    {
                        finish(
                            &mut active,
                            Some("ASTRA_EMU_INPUT_QUEUE_FULL".into()),
                            &shared,
                        );
                        pending.clear();
                        notify(&wake);
                    } else {
                        pending.extend(events);
                        let result = session.input(&pending);
                        pending.clear();
                        if let Err(error) = result {
                            finish(&mut active, Some(error), &shared);
                        }
                        notify(&wake);
                    }
                }
            }
            Ok(Command::Action { epoch, control }) => {
                if let Some(session) = active.as_mut().filter(|session| session.epoch() == epoch) {
                    if let Err(error) = session.control(control) {
                        if session.poisoned() {
                            finish(&mut active, Some(error), &shared);
                        } else {
                            let mut update = shared.lock();
                            if update.epoch == epoch {
                                update.action_error = Some(error);
                                update.dirty = true;
                            }
                        }
                        notify(&wake);
                    }
                }
            }
            Ok(Command::Close { epoch, events }) => {
                if let Some(session) = active.as_mut().filter(|session| session.epoch() == epoch) {
                    let result = if pending.is_empty() {
                        Ok(())
                    } else {
                        session.input(&pending)
                    };
                    pending.clear();
                    let result = result.and_then(|()| {
                        if events.is_empty() {
                            Ok(())
                        } else {
                            session.input(&events)
                        }
                    });
                    finish(&mut active, result.err(), &shared);
                    notify(&wake);
                }
            }
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                let primary = shared.lock().error.take();
                let session_error = active.take().and_then(|mut session| session.close().err());
                let owner_error = registry.shutdown().err().map(|error| error.to_string());
                let errors: Vec<_> = [primary, session_error, owner_error]
                    .into_iter()
                    .flatten()
                    .collect();
                return if errors.is_empty() {
                    Ok(())
                } else {
                    Err(errors.join("; "))
                };
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
        // Polling observes native production; it never drives VM scheduling.
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

pub(super) fn service_due(
    session: &mut ActiveFamilySession,
    pending: &mut Vec<FamilyEvent>,
    now: Instant,
) -> Result<bool, String> {
    if !pending.is_empty() {
        session.input(pending)?;
        pending.clear();
    }
    if now >= session.wake_deadline() {
        session.poll()
    } else {
        Ok(false)
    }
}

fn initialize(
    registry: &mut FamilyProviderRegistry,
    directories: &[PathBuf],
) -> Result<BTreeMap<String, String>, String> {
    #[cfg(target_os = "android")]
    unsafe { registry.register_static(astra_emu_fvp::astra_emu_query) }
        .map_err(|error| error.to_string())?;
    let mut errors = BTreeMap::new();
    #[cfg(not(target_os = "android"))]
    {
        let discovery = astra_emu_manager_core::discover_core_directories(directories)
            .map_err(|error| error.to_string())?;
        let report = registry
            .load_libraries(discovery.libraries)
            .map_err(|error| error.to_string())?;
        for failure in report.errors.into_iter().chain(discovery.errors) {
            let stem = astra_emu_manager_core::library_stem(&failure.path);
            let diagnostic = format!(
                "{}: {}",
                failure.error.diagnostic_code(),
                failure.error.human_message()
            );
            tracing::error!(event = "astra.emu.plugin.discovery_failed", core = %stem, diagnostic = %diagnostic);
            errors.insert(stem, diagnostic);
        }
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
        update.surface = Some(session.surface);
        update.dirty = true;
    }
    if update.failure != session.failure {
        update.failure.clone_from(&session.failure);
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

fn notify(wake: &Mutex<Option<HostWake>>) {
    let callback = wake.lock().clone();
    if let Some(wake) = callback {
        wake();
    }
}
