use super::*;

pub(super) fn restore_session(
    session: &mut VnSession,
    blob: SaveBlob,
) -> Result<(u64, u64), CoreVnError> {
    let package = session.engine.world().package_handle().cloned();
    let owner = session.owner;
    let expected_seed = session.engine.seed();
    let compiled = Arc::clone(&session.compiled);
    let index = Arc::clone(&session.runtime_index);
    let (_, (runtime, step, seed)) = session
        .engine
        .world_mut()
        .load_with_validation(
            blob,
            &astra_core::SchemaMigrationRegistry::default(),
            |snapshot| {
                prepare_runtime(
                    snapshot,
                    package.as_ref(),
                    owner,
                    expected_seed,
                    compiled,
                    index,
                )
            },
        )
        .map_err(|error| CoreVnError::message(error.to_string()))?;
    session.runtime = runtime;
    session.step_complexity = None;
    Ok((step, seed))
}

pub(super) fn validate_session_save(
    session: &VnSession,
    blob: &SaveBlob,
) -> Result<(), CoreVnError> {
    let mut snapshot =
        astra_runtime::read_runtime_save(blob, &astra_core::SchemaMigrationRegistry::default())
            .map_err(|error| CoreVnError::message(error.to_string()))?;
    snapshot
        .awaits
        .validate()
        .map_err(|error| CoreVnError::message(format!("{}: {}", error.code, error.message)))?;
    prepare_runtime(
        &mut snapshot,
        session.engine.world().package_handle(),
        session.owner,
        session.engine.seed(),
        Arc::clone(&session.compiled),
        Arc::clone(&session.runtime_index),
    )
    .map_err(|error| CoreVnError::message(error.to_string()))?;
    Ok(())
}

fn prepare_runtime(
    snapshot: &mut RuntimeSnapshot,
    package: Option<&PackageHandle>,
    owner: ActorId,
    expected_seed: u64,
    compiled: Arc<CoreCompiledStory>,
    index: Arc<CoreVnRuntimeIndex>,
) -> Result<(CoreVnRuntime, u64, u64), RuntimeError> {
    if snapshot.machines != astra_runtime::StateMachineStore::default() {
        return Err(RuntimeError::message("ASTRA_NATIVE_VN_RESTORE_LEGACY_MACHINE: NativeVN no longer executes saved forwarding state machines"));
    }
    if snapshot.config.seed != expected_seed {
        return Err(RuntimeError::message(
            "ASTRA_NATIVE_VN_RESTORE_SEED: save belongs to a different session seed",
        ));
    }
    if snapshot.package.as_ref() != package {
        return Err(RuntimeError::message(
            "ASTRA_NATIVE_VN_RESTORE_PACKAGE: save belongs to a different package",
        ));
    }
    let candidates = snapshot
        .actors
        .component_ids_for_actor_schema(owner, &VN_RUNTIME_STATE_SCHEMA.to_string());
    let [component_id] = candidates.as_slice() else {
        return Err(RuntimeError::message(
            "ASTRA_NATIVE_VN_RESTORE_STATE_SET: exactly one materialized VN state is required",
        ));
    };
    let component = snapshot.actors.component(*component_id).ok_or_else(|| {
        RuntimeError::message("ASTRA_NATIVE_VN_RESTORE_STATE_SET: VN state is missing")
    })?;
    if component.payload.version() != SchemaVersion::new(VN_RUNTIME_STATE_SCHEMA_MAJOR, 0, 0) {
        return Err(RuntimeError::message(
            "ASTRA_NATIVE_VN_RESTORE_STATE_VERSION: unsupported VN state version",
        ));
    }
    let state: VnRuntimeState = component.payload.decode()?;
    let runtime = CoreVnRuntime::from_shared_state_indexed(compiled, index, state)
        .map_err(|error| RuntimeError::message(error.to_string()))?;
    snapshot.actors.detach_component(*component_id);
    Ok((runtime, snapshot.step, snapshot.config.seed))
}
