use crate::{
    audio_executor, metadata_runtime, platform_secret, stage_renderer,
    text_service::TextReplacementBridge,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use abi_stable::std_types::ROption;
use astra_emu_family_api::{
    FamilyEvent, FamilyHostServices, FamilySession, FamilyStatus, KeyCode, KeyModifiers, KeyState,
    OpenRequest, ProbeRequest, WindowState,
};
#[cfg(not(target_os = "android"))]
use astra_emu_manager::{run_manager_with_initial_state, HostWake, ManagerController};
use astra_emu_manager_core::{
    default_vn_preset, input_key_code as key_code, FamilyPluginDescriptor, FamilyProbeCandidate,
    FamilyProbeSelection, FamilyProviderRegistry, GameRecord, GamepadDeadzone, GamepadInput,
    Library, PlaySessionEndReason,
};
use astra_emu_manager_ui_slint::{
    AppearanceViewModel, GameCardViewModel, GamepadBindingViewModel, GenericConfigFieldViewModel,
    InputConfigViewModel, ManagerViewModel, MatchReviewViewModel, PlaySessionViewModel,
};
use astra_emu_metadata::{
    BangumiPlayStatus, BangumiPlayUpdate, CompatibilityDatabase, CompatibilityEntry,
    CompatibilityFetch, CoverAsset, MetadataProviderId, MetadataRecord, MetadataSearchQuery,
    DEFAULT_COMPATIBILITY_SOURCE_URL,
};
use astra_emu_translation_openai_compatible::{
    SecretResolver, TranslationEndpointKind, TranslationProfile, TranslationProtocol,
};
use metadata_runtime::{MetadataCommand, MetadataCommandKind, MetadataPayload, MetadataRuntime};
use platform_secret::ManagerSecretStore;
use stage_renderer::{FrameCollector, FrameMailbox, ManagerStageRenderer};

const FIXED_FRAME_NS: u64 = 16_666_667;
const MAX_SCAN_DIRECTORIES: usize = 4_096;
const MAX_SCAN_DEPTH: usize = 5;

mod connection_test;
mod controller;
mod filters;
mod input;
mod installation;
mod library;
mod metadata;
mod metadata_actions;
#[cfg(test)]
mod metadata_tests;
mod model;
mod navigation;
mod preferences;
mod runtime;
mod session;
mod support;
mod translation;
mod view_fields;
use session::ActiveFamilySession;
use support::*;

struct PendingMetadata {
    game_id: Option<String>,
}

struct AstraEmuManagerController {
    library: Library,
    registry: FamilyProviderRegistry,
    candidates: BTreeMap<String, FamilyProbeCandidate>,
    probe_choices: BTreeMap<String, Vec<FamilyProbeCandidate>>,
    selected_case_id: Option<String>,
    search_query: String,
    diagnostic: String,
    data_dir: PathBuf,
    game_roots: Vec<PathBuf>,
    mailbox: FrameMailbox,
    active: Option<ActiveFamilySession>,
    audio_device: audio_executor::AudioDeviceKind,
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
    let _observability = astra_observability::init_host(observability)?;
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
