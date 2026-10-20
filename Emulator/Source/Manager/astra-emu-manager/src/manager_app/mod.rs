use crate::{
    core_package, metadata_cache, metadata_runtime, platform_secret, stage_renderer,
    text_service::TextReplacementBridge,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use astra_emu_manager::modal_service::ModalBridge;
#[cfg(not(target_os = "android"))]
use astra_emu_manager::{run_manager_with_initial_state, HostExit, HostWake, ManagerController};
use astra_emu_manager_core::model::{
    FamilyEvent, FamilyStatus, KeyCode, KeyModifiers, KeyState, ProbeRequest, WindowState,
};
use astra_emu_manager_core::{
    default_vn_preset, input_key_code as key_code, FamilyPluginDescriptor, FamilyProbeCandidate,
    FamilyProbeSelection, FamilyProviderRegistry, GameRecord, GamepadDeadzone, GamepadInput,
    Library, NativeOpenRequest, NativeSession, PlaySessionEndReason,
};
use astra_emu_manager_ui_slint::{
    AppearanceViewModel, ConfigFieldViewModel, GamepadBindingViewModel, ManagerViewModel,
};
use astra_emu_metadata::{
    BangumiPlayStatus, BangumiPlayUpdate, CompatibilityEntry, CoverAsset, MetadataProviderId,
    MetadataRecord, MetadataSearchQuery, DEFAULT_METADATA_BASE_URL,
};
use astra_emu_translation::{
    OpenAiApi, SecretResolver, TranslationProfile, TranslationProvider,
};
use metadata_runtime::{MetadataCommand, MetadataCommandKind, MetadataPayload, MetadataRuntime};
use platform_secret::ManagerSecretStore;
use stage_renderer::{FrameMailbox, ManagerStageRenderer};

const MAX_SCAN_DIRECTORIES: usize = 4_096;
const MAX_SCAN_DEPTH: usize = 5;

mod background;
mod connection_test;
mod controller;
mod cores;
mod extensions;
mod family_config;
mod file_patch;
mod filters;
mod input;
mod library;
mod metadata;
mod metadata_actions;
mod metadata_sync;
#[cfg(test)]
mod metadata_tests;
mod model;
mod navigation;
mod owner;
mod owner_worker;
mod preferences;
mod probe_cache;
#[cfg(test)]
mod probe_state_tests;
mod probe_worker;
mod render_extent;
mod runtime;
mod session;
mod snackbar;
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
    core: Option<String>,
}

struct AstraEmuManagerController {
    library: Library,
    registry: owner::FamilyOwner,
    candidates: BTreeMap<String, FamilyProbeCandidate>,
    startup_state: background::StartupState,
    probe_choices: BTreeMap<String, Vec<FamilyProbeCandidate>>,
    selected_case_id: Option<String>,
    search_query: String,
    diagnostic: String,
    diagnostic_severity: &'static str,
    plugin_errors: BTreeMap<String, String>,
    data_dir: PathBuf,
    extensions: Option<crate::text_service::ExtensionRuntime>,
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
    metadata_catalog: Option<metadata_cache::MetadataCatalog>,
    metadata_sync: metadata_sync::SyncState,
    pinned_releases: BTreeMap<String, String>,
    filter_settings: astra_emu_manager_core::FilterSettings,
    input_mapping: astra_emu_manager_core::InputMapping,
    appearance: astra_emu_manager_core::AppearanceSettings,
    session_mapping: Option<astra_emu_manager_core::InputMapping>,
    defaults_plugin: Option<String>,
    metadata: MetadataRuntime,
    metadata_sequence: u64,
    metadata_status: BTreeMap<String, String>,
    pending_metadata: BTreeMap<String, PendingMetadata>,
    match_candidates: BTreeMap<String, (String, MetadataRecord)>,
    network_allowed: bool,
    metadata_tokens: BTreeMap<String, String>,
    metadata_tokens_loaded: bool,
    sensitive_covers: bool,
    bangumi_play_status: String,
    bangumi_rating: i32,
    bangumi_note: String,
    releases: BTreeMap<String, Vec<(String, String)>>,
    translation_enabled: bool,
    translation_models: Vec<String>,
    translation_status: String,
    connection_test: Option<connection_test::ConnectionTest>,
    held_controls: BTreeSet<String>,
    key_modifiers: KeyModifiers,
    pointer_position: (f32, f32),
    action_message: String,
    action_serial: u32,
    patch_job: Option<file_patch::PatchJob>,
    patch_import: Option<file_patch::PendingImport>,
    patch_state: astra_emu_manager_ui_slint::PatchViewModel,
    patch_sequence: u64,
    snackbar: snackbar::Snackbar,
    prepare_scan_pending: Option<background::PendingScan>,
    prepare_progress: Option<owner::PrepareProgress>,
    scan_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    last_excluded: Option<String>,
    key_capture: Option<input::KeyCapture>,
}

impl Drop for AstraEmuManagerController {
    fn drop(&mut self) {
        if let Some(mut job) = self.patch_job.take() {
            job.cancel();
            if let Err(error) = job.join() {
                tracing::error!(event = "astra.emu.manager.patch_shutdown_failed", diagnostic_code = %error);
            }
        }
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
    astra_emu_manager::startup::begin();
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
    astra_emu_manager::startup::mark("process_entry");
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
        astra_emu_manager::startup::mark("cached_controller_ready");
        let exit =
            run_manager_with_initial_state(controller, ManagerStageRenderer::new(mailbox), false)?;
        if matches!(exit, HostExit::Restart) {
            std::process::Command::new(std::env::current_exe()?)
                .args(std::env::args_os().skip(1))
                .spawn()?;
        }
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
