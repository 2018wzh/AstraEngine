use super::*;

pub(in super::super) fn retained_pointer_activation_target(
    semantics: &UiSemanticSnapshot,
    position: UiPoint,
) -> Option<&UiSemanticNode> {
    semantics.nodes.iter().rev().find(|node| {
        semantic_node_contains(node, Some(position))
            && node.enabled
            && !node.hidden
            && node
                .actions
                .contains(&astra_ui_core::UiSemanticAction::Activate)
    })
}

pub(in super::super) fn system_page_localization_key(
    page: SystemPageKind,
) -> Result<&'static str, NativeVnHostError> {
    match page {
        SystemPageKind::Title => Ok("system.title"),
        SystemPageKind::QuickPanel => Ok("system.quick_panel"),
        SystemPageKind::Save => Ok("system.save"),
        SystemPageKind::Load => Ok("system.load"),
        SystemPageKind::Config => Ok("system.config"),
        SystemPageKind::Gallery => Ok("system.gallery"),
        SystemPageKind::Replay => Ok("system.replay"),
        SystemPageKind::VoiceReplay => Ok("system.voice_replay"),
        SystemPageKind::RouteChart => Ok("system.route_chart"),
        SystemPageKind::Backlog => Ok("system.backlog"),
        SystemPageKind::LocalizationPreview => Ok("system.localization_preview"),
        SystemPageKind::Custom => Ok("system.custom"),
        SystemPageKind::Unknown => Err(NativeVnHostError::Input(
            "ASTRA_PLAYER_SYSTEM_PAGE: unknown system page".to_string(),
        )),
    }
}

pub(in super::super) fn save_slots_for_policy(
    policy: &SystemUiProfilePolicy,
) -> BTreeMap<String, SaveSlotViewModel> {
    policy
        .save_slot_ids
        .iter()
        .cloned()
        .map(|slot_id| {
            (
                slot_id.clone(),
                SaveSlotViewModel {
                    slot_id,
                    occupied: false,
                    thumbnail_asset: None,
                    has_thumbnail: false,
                    title_key: None,
                    timestamp_text: None,
                    playtime_text: None,
                    metadata_text: None,
                    can_write: true,
                    can_load: false,
                    migration_status: "empty".to_string(),
                },
            )
        })
        .collect()
}

pub(in super::super) fn format_playtime(playtime_ms: u64) -> String {
    let total_seconds = playtime_ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub(in super::super) fn decode_save_envelope(
    bytes: &[u8],
) -> Result<NativeVnPlayerSaveEnvelope, NativeVnHostError> {
    let (schema, _) = postcard::take_from_bytes::<String>(bytes).map_err(|error| {
        NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
    })?;
    if schema != "astra.player.native_vn_save.v10" {
        return Err(NativeVnHostError::Save("ASTRA_PLAYER_SAVE_SCHEMA: unsupported save version; retain the old file and use a new slot".into()));
    }
    let envelope: NativeVnPlayerSaveEnvelope = postcard::from_bytes(bytes).map_err(|error| {
        NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_INTEGRITY: {error}"))
    })?;
    if envelope.schema != "astra.player.native_vn_save.v10"
        || envelope.payload.schema != "astra.player.native_vn_save_payload.v10"
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_VERSION_UNSUPPORTED: save schema is not supported".into(),
        ));
    }
    Ok(envelope)
}

pub(in super::super) fn validate_save_metadata(
    metadata: &NativeVnSaveMetadata,
    expected_slot: &str,
) -> Result<(), NativeVnHostError> {
    if metadata.slot_id != expected_slot
        || metadata.thumbnail_asset != format!("astra.internal.save_thumbnail.{expected_slot}")
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_METADATA_IDENTITY: save metadata identity does not match its slot"
                .into(),
        ));
    }
    if metadata.timestamp_text.trim().is_empty()
        || metadata.timestamp_text.len() > 64
        || metadata.playtime_text.len() != 8
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_METADATA_TEXT: save metadata text is invalid".into(),
        ));
    }
    let expected_bytes = usize::try_from(metadata.thumbnail.width)
        .ok()
        .and_then(|width| {
            usize::try_from(metadata.thumbnail.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if metadata.thumbnail.width != 160
        || metadata.thumbnail.height != 120
        || expected_bytes != Some(metadata.thumbnail.rgba8.len())
    {
        return Err(NativeVnHostError::Save(
            "ASTRA_PLAYER_SAVE_METADATA_THUMBNAIL: save thumbnail integrity check failed".into(),
        ));
    }
    Ok(())
}

pub(in super::super) fn save_metadata_for_persistence(
    metadata: &NativeVnSaveMetadata,
) -> SavedNativeVnSaveMetadata {
    SavedNativeVnSaveMetadata {
        slot_id: metadata.slot_id.clone(),
        thumbnail_asset: metadata.thumbnail_asset.clone(),
        thumbnail_width: metadata.thumbnail.width,
        thumbnail_height: metadata.thumbnail.height,
        thumbnail_rgba8: metadata.thumbnail.rgba8.as_slice().to_vec(),
        timestamp_text: metadata.timestamp_text.clone(),
        playtime_text: metadata.playtime_text.clone(),
    }
}

pub(in super::super) fn restore_save_metadata(
    metadata: SavedNativeVnSaveMetadata,
) -> Result<NativeVnSaveMetadata, NativeVnHostError> {
    let thumbnail = TextureFrame::from_vec(
        metadata.thumbnail_width,
        metadata.thumbnail_height,
        metadata.thumbnail_rgba8,
    )
    .map_err(|error| {
        NativeVnHostError::Save(format!("ASTRA_PLAYER_SAVE_METADATA_THUMBNAIL: {error}"))
    })?;
    Ok(NativeVnSaveMetadata {
        slot_id: metadata.slot_id,
        thumbnail_asset: metadata.thumbnail_asset,
        thumbnail,
        timestamp_text: metadata.timestamp_text,
        playtime_text: metadata.playtime_text,
    })
}

pub(in super::super) fn system_page_binding_key(
    page: SystemPageKind,
) -> Result<&'static str, NativeVnHostError> {
    match page {
        SystemPageKind::Title => Ok("title"),
        SystemPageKind::QuickPanel => Ok("quick_panel"),
        SystemPageKind::Save => Ok("save"),
        SystemPageKind::Load => Ok("load"),
        SystemPageKind::Config => Ok("config"),
        SystemPageKind::Gallery => Ok("gallery"),
        SystemPageKind::Replay => Ok("replay"),
        SystemPageKind::VoiceReplay => Ok("voice_replay"),
        SystemPageKind::RouteChart => Ok("route_chart"),
        SystemPageKind::Backlog => Ok("backlog"),
        SystemPageKind::LocalizationPreview => Ok("localization_preview"),
        SystemPageKind::Custom => Ok("custom"),
        SystemPageKind::Unknown => Err(NativeVnHostError::Input(
            "ASTRA_PLAYER_SYSTEM_PAGE: unknown system page has no UI binding".into(),
        )),
    }
}

pub(in super::super) fn ui_string_argument<'a>(
    action: &'a astra_ui_core::UiActionEnvelope,
    name: &str,
) -> Result<&'a str, NativeVnHostError> {
    match action.arguments.get(name) {
        Some(UiValue::String(value)) if !value.is_empty() => Ok(value),
        _ => Err(NativeVnHostError::Input(format!(
            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: action {} requires string argument {name}",
            action.action_id
        ))),
    }
}

pub(in super::super) fn ui_bool_argument(
    action: &astra_ui_core::UiActionEnvelope,
    name: &str,
) -> Result<bool, NativeVnHostError> {
    match action.arguments.get(name) {
        Some(UiValue::Bool(value)) => Ok(*value),
        _ => Err(NativeVnHostError::Input(format!(
            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: action {} requires boolean argument {name}",
            action.action_id
        ))),
    }
}

pub(in super::super) fn ui_value_to_scalar(value: &UiValue) -> Result<String, NativeVnHostError> {
    match value {
        UiValue::String(value) => Ok(value.clone()),
        UiValue::Bool(value) => Ok(value.to_string()),
        UiValue::Integer(value) => Ok(value.to_string()),
        UiValue::Number(value) if value.is_finite() => Ok(value.to_string()),
        _ => Err(NativeVnHostError::Input(
            "ASTRA_PLAYER_UI_ACTION_ARGUMENT: config value must be scalar".into(),
        )),
    }
}

pub(in super::super) fn resolve_localized<'a>(
    localization: &'a VnLocalizationTable,
    key: &str,
) -> Result<&'a str, NativeVnHostError> {
    localization
        .resolve(key)
        .map_err(|error| NativeVnHostError::Localization(error.to_string()))
}

pub(in super::super) fn validate_story_text(
    compiled: &CompiledStory,
    localization: &VnLocalizationTable,
    provider: &CosmicTextLayoutProvider,
    font_families: &[String],
) -> Result<(), NativeVnHostError> {
    let mut keys = BTreeSet::new();
    for command in compiled
        .states
        .values()
        .flat_map(|state| &state.scenes)
        .flat_map(|scene| &scene.commands)
    {
        match command {
            CompiledCommand::Dialogue { key, speaker, .. } => {
                keys.insert(key.clone());
                if let Some(speaker) = speaker {
                    keys.insert(format!("speaker.{speaker}"));
                }
            }
            CompiledCommand::Choice { key, options, .. } => {
                keys.insert(key.clone());
                keys.extend(options.iter().map(|option| option.key.clone()));
            }
            CompiledCommand::SystemPage { page, .. } => {
                keys.insert(system_page_localization_key(*page)?.to_string());
                keys.insert("system.back".to_string());
            }
            CompiledCommand::Presentation { command, .. } => match command {
                PresentationCommand::Dialogue { key, speaker, .. } => {
                    keys.insert(key.clone());
                    if let Some(speaker) = speaker {
                        keys.insert(format!("speaker.{speaker}"));
                    }
                }
                PresentationCommand::Choice { key, options } => {
                    keys.insert(key.clone());
                    keys.extend(options.iter().map(|option| option.key.clone()));
                }
                PresentationCommand::SystemPage { page } => {
                    keys.insert(system_page_localization_key(*page)?.to_string());
                    keys.insert("system.back".to_string());
                }
                PresentationCommand::SystemOption { option } => {
                    keys.insert(option.key.clone());
                }
                PresentationCommand::Stage(_)
                | PresentationCommand::Extension(_)
                | PresentationCommand::Marker { .. } => {}
            },
            _ => {}
        }
    }
    for key in keys {
        let text = resolve_localized(localization, &key)?;
        let request_key = format!("preflight.{key}");
        provider.layout(&text_request(TextRequestSpec {
            key: &request_key,
            text,
            locale: &localization.locale,
            font_families,
            max_width: 4096,
            font_size: 28.0,
            max_lines: 16,
            direction: TextDirection::Auto,
        }))?;
    }
    Ok(())
}

pub(in super::super) fn validate_story_presentation(
    compiled: &CompiledStory,
    assets: &PackageAssetStore,
    manifest: &VnPresentationProviderManifest,
    profile: &str,
) -> Result<(), NativeVnHostError> {
    for command in compiled
        .states
        .values()
        .flat_map(|state| &state.scenes)
        .flat_map(|scene| &scene.commands)
    {
        let CompiledCommand::Presentation { command, .. } = command else {
            continue;
        };
        match command {
            PresentationCommand::Stage(stage) => {
                validate_stage_command_policy(stage, manifest, profile)?;
                match stage {
                    StageCommand::Preload { asset } => {
                        if !assets.contains_image(asset) && !assets.contains_media(asset) {
                            return Err(NativeVnHostError::Asset(format!(
                                "ASTRA_PLAYER_PRELOAD_ASSET_MISSING: {asset}"
                            )));
                        }
                    }
                    StageCommand::Background { asset: asset_id, .. }
                    | StageCommand::Show {
                        asset: asset_id, ..
                    } => {
                        if !assets.contains_image(asset_id) {
                            return Err(missing_texture(asset_id));
                        }
                    }
                    StageCommand::Configure { .. }
                    | StageCommand::Hide { .. }
                    | StageCommand::ClearLayer { .. }
                    | StageCommand::SetLayerVisibility { .. }
                    | StageCommand::Backdrop { .. }
                    | StageCommand::Shade { .. }
                    | StageCommand::SetSkipAllowed { .. }
                    | StageCommand::Move { .. }
                    | StageCommand::Audio(_)
                    | StageCommand::AudioControl(_)
                    | StageCommand::SetAudioBusEnabled { .. }
                    | StageCommand::Timeline(_)
                    | StageCommand::Transition { .. }
                    | StageCommand::Shake { .. } => {}
                    StageCommand::DeclareLayer { blend, .. } => {
                        stage_blend(*blend)?;
                    }
                    StageCommand::Camera { .. } => {}
                    StageCommand::Movie { fallback, .. } => {
                        if let Some(fallback) = fallback {
                            if !assets.contains_image(fallback) {
                                return Err(missing_texture(fallback));
                            }
                        }
                    }
                    StageCommand::Effect { filter, .. }
                        if matches!(
                            filter.as_str(),
                            "soft_glow" | "astra.filter.bloom" | "astra.filter.fade"
                        ) => {}
                    unsupported => {
                        return Err(NativeVnHostError::Asset(format!(
                            "ASTRA_PLAYER_PRESENTATION_UNSUPPORTED: typed stage command {} has no product renderer binding",
                            unsupported.kind()
                        )))
                    }
                }
            }
            PresentationCommand::Extension(extension)
                if extension.provider_id != astra_vn_policy::POLICY_ACTION_PROVIDER =>
            {
                return Err(NativeVnHostError::Asset(format!(
                    "ASTRA_PLAYER_EXTENSION_PROVIDER_UNWIRED: command {} requires provider {}",
                    extension.command, extension.provider_id
                )))
            }
            PresentationCommand::Extension(_) => {}
            _ => {}
        }
    }
    Ok(())
}

/// Returns the drawable portion of a previously submitted UI frame.
///
/// Resource lifetime commands are deliberately omitted: a cached UI frame is
/// only valid while the existing renderer residency remains live, and replaying
/// an `Upload*` command violates the retained-resource contract.
pub(in super::super) fn reusable_ui_draw_commands(draw: &[SceneCommand]) -> Vec<SceneCommand> {
    draw.iter()
        .filter(|command| !is_ui_resource_lifecycle(command))
        .cloned()
        .collect()
}

pub(in super::super) fn is_ui_resource_lifecycle(command: &SceneCommand) -> bool {
    matches!(
        command,
        SceneCommand::UploadTexture { .. }
            | SceneCommand::UploadGlyph { .. }
            | SceneCommand::UpdateTextureRegion { .. }
            | SceneCommand::ReleaseResource { .. }
    )
}

pub(in super::super) fn validate_stage_command_policy(
    command: &StageCommand,
    manifest: &VnPresentationProviderManifest,
    profile: &str,
) -> Result<(), NativeVnHostError> {
    let preset = match command {
        StageCommand::Background { preset, .. }
        | StageCommand::Show { preset, .. }
        | StageCommand::Hide { preset, .. }
        | StageCommand::Move { preset, .. }
        | StageCommand::Camera { preset, .. } => preset.as_deref(),
        StageCommand::Transition { preset, .. } => Some(preset.as_str()),
        _ => None,
    };
    if let Some(preset) = preset {
        manifest
            .resolve_preset(profile, command.kind(), preset)
            .map_err(|diagnostic| {
                NativeVnHostError::Package(format!("{}: {}", diagnostic.code, diagnostic.message))
            })?;
    }
    Ok(())
}
