use crate::{
    metadata_runtime, platform_secret, stage_renderer, text_service::TextReplacementBridge,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use astra_emu_manager::modal_service::ModalBridge;
#[cfg(not(target_os = "android"))]
use astra_emu_manager::{HostWake, ManagerController, run_manager_with_initial_state};
use astra_emu_manager_core::model::{
    FamilyEvent, FamilyStatus, KeyCode, KeyModifiers, KeyState, ProbeRequest, WindowState,
};
use astra_emu_manager_core::{
    FamilyPluginDescriptor, FamilyProbeCandidate, FamilyProbeSelection, FamilyProviderRegistry,
    GameRecord, GamepadDeadzone, GamepadInput, Library, NativeOpenRequest, NativeSession,
    PlaySessionEndReason, default_vn_preset, input_key_code as key_code,
};
use astra_emu_manager_ui_slint::{
    AppearanceViewModel, GameCardViewModel, GamepadBindingViewModel, GenericConfigFieldViewModel,
    InputConfigViewModel, ManagerViewModel, MatchReviewViewModel, PlaySessionViewModel,
};
use astra_emu_metadata::{
    BangumiPlayStatus, BangumiPlayUpdate, CompatibilityDatabase, CompatibilityEntry,
    CompatibilityFetch, CoverAsset, DEFAULT_COMPATIBILITY_SOURCE_URL, MetadataProviderId,
    MetadataRecord, MetadataSearchQuery,
};
use astra_emu_translation_openai_compatible::{
    SecretResolver, TranslationEndpointKind, TranslationProfile, TranslationProtocol,
};
use metadata_runtime::{MetadataCommand, MetadataCommandKind, MetadataPayload, MetadataRuntime};
use platform_secret::ManagerSecretStore;
use stage_renderer::{FrameMailbox, ManagerStageRenderer};

const MAX_SCAN_DIRECTORIES: usize = 4_096;
const MAX_SCAN_DEPTH: usize = 5;

mod connection_test;
mod controller;
mod extensions;
mod family_config;
mod filters;
mod input;
mod library;
mod metadata;
mod metadata_actions;
#[cfg(test)]
mod metadata_tests;
mod model;
mod musica_profiles;
mod navigation;
mod owner;
mod owner_worker;
mod preferences;
#[cfg(test)]
mod probe_state_tests;
mod runtime;
mod session;
mod support;
mod translation;
mod view_fields;
pub(super) fn run_extension_command(command: crate::ExtensionCommand) -> Result<(), String> {
    extensions::run(command)
}
use session::ActiveFamilySession;
use support::*;

struct PendingMetadata {
    game_id: Option<String>,
}

struct AstraEmuManagerController {
    library: Library,
    registry: owner::FamilyOwner,
    candidates: BTreeMap<String, FamilyProbeCandidate>,
    probe_choices: BTreeMap<String, Vec<FamilyProbeCandidate>>,
    selected_case_id: Option<String>,
    search_query: String,
    diagnostic: String,
    plugin_errors: BTreeMap<String, String>,
    data_dir: PathBuf,
    _extensions: crate::text_service::ExtensionRuntime,
    game_roots: Vec<PathBuf>,
    mailbox: FrameMailbox,
    active: Option<owner::ActiveSessionView>,
    active_play_session: Option<String>,
    pending_events: Vec<FamilyEvent>,
    window_state: Option<WindowState>,
    host_wake: Option<HostWake>,
    physical_keys: Vec<KeyCode>,
    library_sort: String,
    compatibility_filter: String,
    compatibility_database: Option<CompatibilityDatabase>,
    compatibility_hash: Option<String>,
    compatibility_fetched_at_unix_ms: Option<i64>,
    pinned_releases: BTreeMap<String, String>,
    filter_settings: astra_emu_manager_core::FilterSettings,
    input_mapping: astra_emu_manager_core::InputMapping,
    input_config: InputConfigViewModel,
    appearance: astra_emu_manager_core::AppearanceSettings,
    family_options: BTreeMap<String, String>,
    musica_import: Option<musica_profiles::ProfileImport>,
    filter_options: BTreeMap<String, String>,
    metadata: MetadataRuntime,
    metadata_sequence: u64,
    metadata_status: BTreeMap<String, String>,
    pending_metadata: BTreeMap<String, PendingMetadata>,
    match_candidates: BTreeMap<String, (String, MetadataRecord)>,
    metadata_consent: BTreeMap<String, bool>,
    metadata_tokens: BTreeMap<String, String>,
    sensitive_covers: bool,
    bangumi_play_status: String,
    bangumi_rating: i32,
    bangumi_note: String,
    releases: BTreeMap<String, Vec<(String, String)>>,
    translation_consent: bool,
    connection_test: Option<connection_test::ConnectionTest>,
    held_controls: BTreeSet<String>,
    key_modifiers: KeyModifiers,
    pointer_position: (f32, f32),
}

impl Drop for AstraEmuManagerController {
    fn drop(&mut self) {
        if let Err(error) = self.close_active(PlaySessionEndReason::Shutdown) {
            tracing::error!(event = "astra.emu.manager.shutdown_failed", diagnostic_code = %error);
        }
        if let Err(error) = self.registry.shutdown() {
            tracing::error!(event = "astra.emu.manager.owner_shutdown_failed", diagnostic_code = %error);
        }
        if let Err(error) = self.finish_play_session(PlaySessionEndReason::Shutdown) {
            tracing::error!(event = "astra.emu.manager.play_shutdown_failed", diagnostic_code = %error);
        }
    }
}

pub(super) fn run_application() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = platform_data_dir()?;
    fs::create_dir_all(&data_dir)?;
    let filter = match std::env::var("RUST_LOG") {
        Ok(filter) => filter,
        Err(std::env::VarError::NotPresent) => "info".to_owned(),
        Err(error) => return Err(error.into()),
    };
    let mut observability = astra_observability::HostObservabilityConfig::for_cli(filter);
    observability.role = astra_observability::HostRole::Manager;
    observability.console = false;
    observability.log_dir = Some(data_dir.join("diagnostics"));
    let _observability = std::sync::Arc::new(astra_observability::init_host(observability)?);
    let filter_guard = std::sync::Arc::downgrade(&_observability);
    astra_emu_manager_core::set_diagnostic_enabled(move |level, target| {
        filter_guard
            .upgrade()
            .is_some_and(|guard| guard.external_enabled(level, target))
    });
    let emit_guard = std::sync::Arc::downgrade(&_observability);
    astra_emu_manager_core::set_diagnostic_emit(move |record| {
        if let Some(guard) = emit_guard.upgrade() {
            if let Err(error) =
                guard.record_external(record.level, &record.target, &record.event, record.fields)
            {
                tracing::error!(event = "astra.emu.diagnostics.record_failed", error = %error);
            }
        }
    });
    let flush_guard = std::sync::Arc::downgrade(&_observability);
    astra_emu_manager_core::set_diagnostic_flush(move || {
        if let Some(guard) = flush_guard.upgrade() {
            if let Err(error) = guard.flush() {
                tracing::error!(event = "astra.emu.diagnostics.flush_failed", error = %error);
            }
        }
    });
    let result = (|| {
        let mailbox = FrameMailbox::new();
        let controller = AstraEmuManagerController::open(mailbox.clone())?;
        run_manager_with_initial_state(controller, ManagerStageRenderer::new(mailbox), false)?;
        Ok(())
    })();
    if result.is_err() {
        tracing::error!(
            event = "astra.emu.manager.fatal",
            diagnostic_code = "ASTRA_EMU_MANAGER_STARTUP_FAILED"
        );
    }
    result
}
