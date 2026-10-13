use super::*;

pub(in super::super) fn validate_product_provider_bindings(
    package: &astra_package::PackageReader,
) -> Result<(), NativeVnHostError> {
    let policy = read_package_json(package, "provider.policy")?;
    let registry = read_package_json(package, "plugin.extension_registry")?;
    let headless_selected = policy.get("renderer").and_then(serde_json::Value::as_str)
        == Some("headless")
        || policy
            .get("bindings")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .any(|binding| {
                binding
                    .get("provider_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|provider| provider.contains("headless"))
            });
    if headless_selected {
        return Err(NativeVnHostError::ProviderBinding(
            "ASTRA_PLAYER_PRESENTATION_PROVIDER_INELIGIBLE: headless presentation is not packaged Player eligible"
                .to_string(),
        ));
    }
    {
        let (slot, provider_id, capability) =
            ("presentation", "astra.renderer.wgpu", "renderer2d.wgpu");
        let policy_bound = policy
            .get("bindings")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding.get("slot").and_then(serde_json::Value::as_str) == Some(slot)
                        && binding
                            .get("provider_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(provider_id)
                })
            });
        let registry_bound = registry
            .get("bindings")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|bindings| {
                bindings.iter().any(|binding| {
                    binding.get("slot").and_then(serde_json::Value::as_str) == Some(slot)
                        && binding
                            .get("provider_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(provider_id)
                })
            });
        let registered = registry
            .get("providers")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|providers| {
                providers.iter().any(|provider| {
                    provider.get("slot").and_then(serde_json::Value::as_str) == Some(slot)
                        && provider
                            .get("provider_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(provider_id)
                        && provider
                            .get("capability")
                            .and_then(serde_json::Value::as_str)
                            == Some(capability)
                        && provider
                            .get("packaged")
                            .and_then(serde_json::Value::as_bool)
                            == Some(true)
                })
            });
        if !policy_bound || !registry_bound || !registered {
            return Err(NativeVnHostError::ProviderBinding(format!(
                "ASTRA_PLAYER_PROVIDER_BINDING_INVALID: {slot} must bind packaged provider {provider_id}"
            )));
        }
    }
    let presentation: astra_vn_presentation::VnPresentationProviderManifest = package
        .container()
        .decode_postcard("vn.presentation_provider_manifest")
        .map_err(|error| NativeVnHostError::ProviderBinding(error.to_string()))?;
    if !presentation.validate_standard().passed
        || presentation.renderer_provider != "astra.renderer2d.wgpu"
    {
        return Err(NativeVnHostError::ProviderBinding(
            "ASTRA_PLAYER_PRESENTATION_PROVIDER_INELIGIBLE: package presentation provider is not the product wgpu path"
                .to_string(),
        ));
    }
    Ok(())
}

pub(in super::super) fn saved_runtime_state(
    blob: &astra_runtime::SaveBlob,
) -> Result<VnRuntimeState, NativeVnHostError> {
    let snapshot =
        astra_runtime::read_runtime_save(blob, &astra_core::SchemaMigrationRegistry::default())
            .map_err(|error| {
                NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
            })?;
    let mut states = snapshot
        .actors
        .actor_snapshots()
        .into_iter()
        .flat_map(|actor| snapshot.actors.component_snapshots(actor.actor_id))
        .filter(|component| component.payload.schema().as_str() == VN_RUNTIME_STATE_SCHEMA)
        .map(|component| {
            component
                .payload
                .decode::<VnRuntimeState>()
                .map_err(|error| {
                    NativeVnHostError::Save(format!(
                        "ASTRA_PLAYER_SAVE_RUNTIME_STATE_INVALID: {error}"
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if states.len() != 1 {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_RUNTIME_STATE_SET: runtime save must contain exactly one VN state"
                .into(),
        ));
    }
    Ok(states.remove(0))
}

pub(in super::super) fn unique_string_set(
    values: Vec<String>,
    code: &'static str,
) -> Result<BTreeSet<String>, NativeVnHostError> {
    let count = values.len();
    let values = values.into_iter().collect::<BTreeSet<_>>();
    if values.len() != count {
        return Err(typed_vn_state_error(code));
    }
    Ok(values)
}

pub(in super::super) fn typed_vn_state_error(code: &'static str) -> NativeVnHostError {
    NativeVnHostError::RuntimeEvidence(format!(
        "{code}: typed VN state contains duplicate identity"
    ))
}

pub(in super::super) fn player_timeline_task(
    task: astra_vn_core::VnTimelineTask,
) -> Result<PlayerTimelineTask, NativeVnHostError> {
    let (task_id, target, action, duration_ms, fence) = match task.command {
        TimelineCommand::Start(spec) => {
            let duration_ms = spec
                .tracks
                .iter()
                .flat_map(|track| track.keyframes.iter())
                .map(|keyframe| u64::from(keyframe.time_ms))
                .max();
            let target = spec.tracks.first().map(|track| track.target.clone());
            (
                spec.id,
                target,
                PlayerTimelineTaskAction::Start,
                duration_ms,
                spec.fence,
            )
        }
        TimelineCommand::Cancel { id, .. } => {
            (id, None, PlayerTimelineTaskAction::Cancel, None, None)
        }
    };
    Ok(PlayerTimelineTask {
        schema: "astra.player_timeline_task.v1".to_string(),
        task_id,
        target,
        action,
        duration_ms,
        fence,
    })
}

pub(in super::super) fn read_package_json(
    package: &astra_package::PackageReader,
    section: &str,
) -> Result<serde_json::Value, NativeVnHostError> {
    let bytes = package
        .container()
        .read_bounded(section, 256 * 1024)
        .map_err(|error| NativeVnHostError::ProviderBinding(error.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        NativeVnHostError::ProviderBinding(format!(
            "ASTRA_PLAYER_PROVIDER_SECTION_INVALID: {section}: {error}"
        ))
    })
}

pub(in super::super) fn cleanup_runtime_host(
    host: &mut NativeVnRuntimeHost,
    error: impl std::fmt::Display,
) -> NativeVnHostError {
    match host.cleanup_after_failure() {
        Ok(_) => NativeVnHostError::Package(error.to_string()),
        Err(cleanup_error) => NativeVnHostError::Package(format!(
            "ASTRA_PLAYER_RUNTIME_CLEANUP_FAILED: {error}; cleanup failed: {cleanup_error}"
        )),
    }
}

pub(in super::super) fn current_image_working_set(state: &ProductStageState) -> Vec<String> {
    let mut working_set = state
        .entities
        .values()
        .filter(|entity| entity.visible)
        .map(|entity| entity.asset.clone())
        .collect::<BTreeSet<_>>();
    working_set.extend(
        state
            .movies
            .values()
            .filter_map(|movie| movie.fallback.clone()),
    );
    working_set.into_iter().collect()
}

pub(in super::super) fn image_prefetch_windows(
    story: &CompiledStory,
    asset_store: &PackageAssetStore,
) -> Result<BTreeMap<String, Vec<String>>, NativeVnHostError> {
    const LOOKAHEAD_ASSET_COUNT: usize = 16;

    let authored_successors = story
        .stories
        .iter()
        .flat_map(|authored_story| authored_story.states.windows(2))
        .map(|pair| (pair[0].clone(), pair[1].clone()))
        .collect::<BTreeMap<_, _>>();
    let state_ids = story.states.keys().cloned().collect::<BTreeSet<_>>();
    let mut windows = BTreeMap::new();
    for authored_story in &story.stories {
        for state_id in &authored_story.states {
            let mut seen = BTreeSet::new();
            let mut visited_states = BTreeSet::new();
            let mut pending_states = VecDeque::from([state_id.clone()]);
            let mut assets = Vec::new();
            'states: while let Some(candidate_id) = pending_states.pop_front() {
                if !visited_states.insert(candidate_id.clone()) {
                    continue;
                }
                let state = story.states.get(&candidate_id).ok_or_else(|| {
                    NativeVnHostError::Asset(format!(
                        "ASTRA_PLAYER_IMAGE_PREFETCH_STATE_MISSING: {candidate_id}"
                    ))
                })?;
                for scene in &state.scenes {
                    for command in &scene.commands {
                        let CompiledCommand::Presentation {
                            command: PresentationCommand::Stage(stage),
                            ..
                        } = command
                        else {
                            continue;
                        };
                        let asset = match stage {
                            StageCommand::Preload { asset }
                            | StageCommand::Background { asset, .. }
                            | StageCommand::Show { asset, .. } => Some(asset),
                            StageCommand::Movie { fallback, .. } => fallback.as_ref(),
                            _ => None,
                        };
                        let Some(asset) = asset.filter(|asset| asset_store.contains_image(asset))
                        else {
                            continue;
                        };
                        if seen.insert(asset.clone()) {
                            assets.push(asset.clone());
                            if assets.len() == LOOKAHEAD_ASSET_COUNT {
                                break 'states;
                            }
                        }
                    }
                }
                for successor in state_prefetch_successors(state, &authored_successors) {
                    let successor_state = astra_vn_core::resolve_target(&successor, &state_ids);
                    if !story.states.contains_key(&successor_state) {
                        return Err(NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_IMAGE_PREFETCH_TARGET_MISSING: {successor}"
                        )));
                    }
                    if !visited_states.contains(&successor_state) {
                        pending_states.push_back(successor_state);
                    }
                }
            }
            windows.insert(state_id.clone(), assets);
        }
    }
    Ok(windows)
}

#[cfg(test)]
pub(in super::super) fn system_action_gameplay_entry_states(
    states: &BTreeMap<String, State>,
    actions: &BTreeMap<String, astra_vn_core::SystemActionProgram>,
    system_story_ids: &BTreeSet<&str>,
) -> Vec<String> {
    let state_ids = states.keys().cloned().collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    actions
        .values()
        .flat_map(|program| program.effects.iter())
        .filter_map(|effect| match effect {
            SystemActionEffect::Jump { target } => Some(target),
            _ => None,
        })
        // System actions use the same author-facing target grammar as story
        // commands. A raw lookup discards legal label aliases, leaving the
        // launch prewarm unable to cover the first physical title action.
        .map(|target| astra_vn_core::resolve_target(target, &state_ids))
        .filter(|state_id| {
            states
                .get(state_id)
                .is_some_and(|state| !system_story_ids.contains(state.story_id.as_str()))
        })
        .filter(|state_id| seen.insert(state_id.clone()))
        .collect()
}

pub(in super::super) fn default_runtime_launch_state(
    stories: &[astra_vn_core::Story],
) -> Option<String> {
    let story = stories
        .iter()
        .find(|story| story.id == "story.main")
        .or_else(|| stories.first())?;
    story
        .states
        .iter()
        .find(|state_id| state_id.as_str() == "state.prologue")
        .or_else(|| story.states.first())
        .cloned()
}

pub(in super::super) fn state_prefetch_successors(
    state: &State,
    authored_successors: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut successors = Vec::new();
    let mut replaces_fallthrough = false;
    let mut preserves_fallthrough = false;
    for command in state.scenes.iter().flat_map(|scene| &scene.commands) {
        match command {
            CompiledCommand::Choice { options, .. } => {
                replaces_fallthrough = true;
                successors.extend(options.iter().map(|option| option.target.clone()));
            }
            CompiledCommand::Jump { target, .. } => {
                replaces_fallthrough = true;
                successors.push(target.clone());
            }
            CompiledCommand::Branch {
                then_target,
                else_target,
                ..
            } => {
                replaces_fallthrough = true;
                successors.push(then_target.clone());
                successors.push(else_target.clone());
            }
            CompiledCommand::Call { target, .. } => {
                preserves_fallthrough = true;
                successors.push(target.clone());
            }
            _ => {}
        }
    }
    if !replaces_fallthrough || preserves_fallthrough {
        if let Some(successor) = authored_successors.get(&state.id) {
            successors.push(successor.clone());
        }
    }
    let mut seen = BTreeSet::new();
    successors.retain(|successor| seen.insert(successor.clone()));
    successors
}
