use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex},
};

use astra_core::{Hash256, SchemaVersion};
use astra_emu_family_api::*;
use rfvp::{
    host_api::{AudioSampleFormat, BlendMode, EncodedAudioKind, PixelFormat},
    integration::{RecordedRenderFrame, RuntimeSession},
    rfvp_audio::AudioCommand,
    script::{parser::Nls, Variant},
    subsystem::{
        resources::{
            input_manager::KeyCode,
            videoplayer::{HostMovieCommand, MovieMode},
        },
        world::{RuntimeVfs, SyscallJournalEntry},
    },
};
use serde::{Deserialize, Serialize};

use crate::{FvpArchive, FvpHcbScript, FvpNls, FVP_FAMILY_ID, FVP_PROVIDER_ID};

const MAX_CASE_FILES: usize = 65_536;
const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;
const MAX_RENDER_UPLOAD_BYTES_PER_STEP: usize = 256 * 1024 * 1024;
const MAX_EPHEMERAL_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct FvpCaseImage {
    pub case_fingerprint: Hash256,
    pub root_mount_id: String,
    pub script_bytes: Vec<u8>,
    pub nls: FvpNls,
    pub files: BTreeMap<String, Vec<u8>>,
}

struct CaseVfs {
    files: Option<BTreeMap<String, Vec<u8>>>,
    host: Option<(Arc<dyn LegacyVfsReader>, String)>,
    nls: FvpNls,
    archives: Mutex<BTreeMap<String, FvpArchive>>,
}

impl RuntimeVfs for CaseVfs {
    fn read_file(&self, path: &str) -> anyhow::Result<Vec<u8>> {
        let key = normalize_vfs_path(path).map_err(|message| anyhow::anyhow!(message))?;
        match self.read_direct(&key) {
            Ok(bytes) => return Ok(bytes),
            Err(error) if error.code() == "ASTRA_EMU_VFS_NOT_FOUND" => {}
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        }
        let (folder, entry) = key
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("RFVP_VFS_NOT_FOUND"))?;
        if folder.is_empty() || entry.is_empty() {
            anyhow::bail!("RFVP_VFS_ARCHIVE_PATH_INVALID");
        }
        let mut archives = self
            .archives
            .lock()
            .map_err(|_| anyhow::anyhow!("RFVP_VFS_ARCHIVE_LOCK_POISONED"))?;
        if !archives.contains_key(folder) {
            let archive_uri = format!("{folder}.bin");
            let archive = if self.files.is_some() {
                let archive_bytes = self
                    .read_direct(&archive_uri)
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                FvpArchive::parse(archive_bytes, self.nls, MAX_CASE_FILES)
            } else {
                let (host, mount_set_id) = self
                    .host
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("RFVP_VFS_HOST_UNAVAILABLE"))?;
                FvpArchive::open_host(
                    Arc::clone(host),
                    mount_set_id.clone(),
                    archive_uri,
                    self.nls,
                    MAX_CASE_FILES,
                )
            }
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            archives.insert(folder.into(), archive);
        }
        archives
            .get(folder)
            .ok_or_else(|| anyhow::anyhow!("RFVP_VFS_NOT_FOUND"))?
            .read(entry)
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
}

impl CaseVfs {
    fn read_direct(&self, key: &str) -> Result<Vec<u8>, LegacyProviderError> {
        if let Some(files) = &self.files {
            return files.get(key).cloned().ok_or_else(|| {
                invalid("ASTRA_EMU_VFS_NOT_FOUND", "case VFS entry is not present")
            });
        }
        let (host, mount_set_id) = self
            .host
            .as_ref()
            .ok_or_else(|| invalid("ASTRA_EMU_VFS_UNAVAILABLE", "case VFS host is unavailable"))?;
        host.read_file(mount_set_id, key, MAX_FILE_BYTES as u64)
    }
}

struct FvpSession {
    case_fingerprint: Hash256,
    runtime: RuntimeSession,
    last_step: u64,
    seed: u64,
    fixed_delta_ns: u64,
    compatibility_profile: String,
    instruction_count: u64,
    syscall_count: u64,
    pointer_x: i32,
    pointer_y: i32,
    pointer_in_screen: bool,
    stage_width: u32,
    stage_height: u32,
    poisoned: bool,
    ephemeral_text: BTreeMap<String, LegacyEphemeralText>,
    pending_movie: Option<PendingMovieV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingMovieV1 {
    playback_id: String,
    token_id: String,
    resource_uri: String,
    mode: LegacyVideoMode,
    stage_width: u32,
    stage_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FvpSessionSnapshotV1 {
    case_fingerprint: Hash256,
    runtime_bytes: Vec<u8>,
    last_step: u64,
    seed: u64,
    fixed_delta_ns: u64,
    compatibility_profile: String,
    instruction_count: u64,
    syscall_count: u64,
    pointer_x: i32,
    pointer_y: i32,
    pointer_in_screen: bool,
    stage_width: u32,
    stage_height: u32,
    pending_movie: Option<PendingMovieV1>,
}

#[derive(Default)]
pub struct FvpRuntimeProvider {
    cases: BTreeMap<Hash256, FvpCaseImage>,
    sessions: BTreeMap<String, FvpSession>,
    host_vfs: Option<Arc<dyn LegacyVfsReader>>,
}

impl FvpRuntimeProvider {
    pub fn with_vfs(host_vfs: Arc<dyn LegacyVfsReader>) -> Self {
        Self {
            host_vfs: Some(host_vfs),
            ..Self::default()
        }
    }

    pub fn has_active_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    pub fn register_case(&mut self, mut image: FvpCaseImage) -> Result<(), LegacyProviderError> {
        validate_symbol("root_mount_id", &image.root_mount_id)?;
        if image.script_bytes.len() > MAX_FILE_BYTES {
            return Err(invalid(
                "ASTRA_FVP_SCRIPT_BOUNDS",
                "HCB script exceeds the supported byte bound",
            ));
        }
        let script =
            FvpHcbScript::parse(image.script_bytes.clone(), image.nls).map_err(format_error)?;
        if image.case_fingerprint != script.header.content_hash {
            return Err(invalid(
                "ASTRA_FVP_CASE_FINGERPRINT",
                "case fingerprint does not match the HCB bytes",
            ));
        }
        if image.files.len() > MAX_CASE_FILES {
            return Err(invalid(
                "ASTRA_FVP_VFS_ENTRY_BOUNDS",
                "case VFS contains too many files",
            ));
        }
        let mut normalized = BTreeMap::new();
        for (path, bytes) in image.files {
            if bytes.len() > MAX_FILE_BYTES {
                return Err(invalid(
                    "ASTRA_FVP_VFS_FILE_BOUNDS",
                    "case VFS file exceeds the supported byte bound",
                ));
            }
            let path = normalize_vfs_path(&path)
                .map_err(|message| invalid("ASTRA_FVP_VFS_PATH", message))?;
            if normalized.insert(path, bytes).is_some() {
                return Err(invalid(
                    "ASTRA_FVP_VFS_DUPLICATE",
                    "case VFS contains a normalized path collision",
                ));
            }
        }
        image.files = normalized;
        if self
            .cases
            .values()
            .any(|case| case.root_mount_id == image.root_mount_id)
        {
            return Err(invalid(
                "ASTRA_FVP_MOUNT_DUPLICATE",
                "root mount id is already registered",
            ));
        }
        if self.cases.insert(image.case_fingerprint, image).is_some() {
            return Err(invalid(
                "ASTRA_FVP_CASE_DUPLICATE",
                "case fingerprint is already registered",
            ));
        }
        Ok(())
    }

    fn case_for_mount(&self, mount_id: &str) -> Result<&FvpCaseImage, LegacyProviderError> {
        let mut matches = self
            .cases
            .values()
            .filter(|case| case.root_mount_id == mount_id);
        let case = matches.next().ok_or_else(|| {
            invalid(
                "ASTRA_FVP_PROBE_SOURCE",
                "probe root mount is not registered",
            )
        })?;
        if matches.next().is_some() {
            return Err(invalid(
                "ASTRA_FVP_PROBE_AMBIGUOUS",
                "probe root mount resolves to multiple cases",
            ));
        }
        Ok(case)
    }
}

pub fn create_static_fvp_provider(
    vfs: Arc<dyn LegacyVfsReader>,
) -> Result<Box<dyn LegacyRuntimeProvider>, LegacyProviderError> {
    let provider = FvpRuntimeProvider::with_vfs(vfs);
    provider.descriptor().validate()?;
    Ok(Box::new(provider))
}

impl LegacyRuntimeProvider for FvpRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor {
        LegacyFamilyPluginDescriptor {
            family_id: FamilyId(FVP_FAMILY_ID.into()),
            plugin_id: "astra.emu.fvp".into(),
            provider_id: FVP_PROVIDER_ID.into(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            rustc_fingerprint: env!("ASTRA_FVP_RUSTC_FINGERPRINT").into(),
            feature_fingerprint: env!("ASTRA_FVP_FEATURE_FINGERPRINT").into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            supported_formats: vec![
                "fvp.hcb".into(),
                "fvp.bin".into(),
                "fvp.nvsg".into(),
                "fvp.hzc1".into(),
            ],
            permissions: vec!["vfs.read".into(), "media.submit".into()],
            report_redaction: "astra.emu.redaction.v1".into(),
            license: "MPL-2.0".into(),
        }
    }

    fn probe(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, LegacyProviderError> {
        ctx.validate()?;
        if request.max_entries == 0 || request.max_metadata_bytes < 64 {
            return Err(invalid(
                "ASTRA_FVP_PROBE_BUDGET",
                "probe budget is too small",
            ));
        }
        let (script, fingerprint, detected_nls) = if let Ok(image) =
            self.case_for_mount(&request.root_mount_id)
        {
            (
                FvpHcbScript::parse(image.script_bytes.clone(), image.nls).map_err(format_error)?,
                image.case_fingerprint,
                image.nls,
            )
        } else {
            let host = self.host_vfs.as_ref().ok_or_else(|| {
                invalid(
                    "ASTRA_FVP_PROBE_SOURCE",
                    "probe root mount is not registered and no host VFS is bound",
                )
            })?;
            let mut matches = Vec::new();
            for uri in request
                .candidate_uris
                .iter()
                .take(request.max_entries as usize)
            {
                if !uri.to_ascii_lowercase().ends_with(".hcb") {
                    continue;
                }
                let bytes = host.read_file(
                    &request.root_mount_id,
                    uri,
                    request.max_metadata_bytes.min(MAX_FILE_BYTES as u64),
                )?;
                for nls in [FvpNls::ShiftJis, FvpNls::Gbk, FvpNls::Utf8] {
                    if let Ok(script) = FvpHcbScript::parse(bytes.clone(), nls) {
                        matches.push((script, Hash256::from_sha256(&bytes), nls));
                        break;
                    }
                }
            }
            if matches.len() != 1 {
                return Err(invalid(
                    "ASTRA_FVP_PROBE_AMBIGUOUS",
                    "host VFS must expose exactly one valid bounded FVP HCB candidate",
                ));
            }
            matches.pop().unwrap()
        };
        let marker_match =
            request.marker_hashes.is_empty() || request.marker_hashes.contains(&fingerprint);
        Ok(LegacyProbeReport {
            family_id: FamilyId(FVP_FAMILY_ID.into()),
            confidence_permyriad: if marker_match { 10_000 } else { 0 },
            markers: if marker_match {
                vec![
                    "fvp.hcb.descriptor".into(),
                    format!("fvp.game_mode.{}", script.header.game_mode),
                    format!("fvp.stage_width.{}", script.header.width),
                    format!("fvp.stage_height.{}", script.header.height),
                    format!(
                        "fvp.nls.{}",
                        match detected_nls {
                            FvpNls::ShiftJis => "shift_jis",
                            FvpNls::Gbk => "gbk",
                            FvpNls::Utf8 => "utf8",
                        }
                    ),
                ]
            } else {
                Vec::new()
            },
            blockers: Vec::new(),
            content_identity: fingerprint,
        })
    }

    fn open(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyOpenRequest,
    ) -> Result<LegacyRuntimeSessionId, LegacyProviderError> {
        ctx.validate()?;
        validate_symbol("session_id", &request.requested_session_id.0)?;
        validate_symbol("compatibility_profile", &request.compatibility_profile)?;
        if request.fixed_delta_ns == 0 || request.fixed_delta_ns > 1_000_000_000 {
            return Err(invalid(
                "ASTRA_FVP_FIXED_DELTA",
                "fixed delta is outside 1ns..=1s",
            ));
        }
        if self.sessions.contains_key(&request.requested_session_id.0) {
            return Err(invalid(
                "ASTRA_FVP_SESSION_DUPLICATE",
                "session id is already active",
            ));
        }
        let (script_bytes, nls, vfs): (Vec<u8>, FvpNls, Arc<dyn RuntimeVfs>) =
            if let Some(image) = self.cases.get(&request.case_fingerprint) {
                if image.root_mount_id != ctx.mount_set_id {
                    return Err(invalid(
                        "ASTRA_FVP_MOUNT_BINDING",
                        "host mount does not match the registered case",
                    ));
                }
                (
                    image.script_bytes.clone(),
                    image.nls,
                    Arc::new(CaseVfs {
                        files: Some(image.files.clone()),
                        host: None,
                        nls: image.nls,
                        archives: Mutex::new(BTreeMap::new()),
                    }),
                )
            } else {
                let host = self
                    .host_vfs
                    .as_ref()
                    .ok_or_else(|| {
                        invalid(
                            "ASTRA_FVP_CASE_MISSING",
                            "case is not registered and no host VFS is bound",
                        )
                    })?
                    .clone();
                let script_uri = normalize_vfs_path(&request.script_uri)
                    .map_err(|message| invalid("ASTRA_FVP_SCRIPT_URI", message))?;
                let script_bytes =
                    host.read_file(&ctx.mount_set_id, &script_uri, MAX_FILE_BYTES as u64)?;
                if Hash256::from_sha256(&script_bytes) != request.case_fingerprint {
                    return Err(invalid(
                        "ASTRA_FVP_CASE_FINGERPRINT",
                        "host VFS script hash does not match case fingerprint",
                    ));
                }
                let nls = parse_nls_option(&request.family_options)?;
                (
                    script_bytes,
                    nls,
                    Arc::new(CaseVfs {
                        files: None,
                        host: Some((host, ctx.mount_set_id.clone())),
                        nls,
                        archives: Mutex::new(BTreeMap::new()),
                    }),
                )
            };
        let (stage_width, stage_height) = parse_stage_dimensions(&request.family_options)?;
        let runtime =
            RuntimeSession::new(script_bytes, map_nls(nls), vfs, (stage_width, stage_height))
                .map_err(|error| invalid("ASTRA_FVP_OPEN", error.to_string()))?;
        let session = FvpSession {
            case_fingerprint: request.case_fingerprint,
            runtime,
            last_step: 0,
            seed: request.session_seed,
            fixed_delta_ns: request.fixed_delta_ns,
            compatibility_profile: request.compatibility_profile,
            instruction_count: 0,
            syscall_count: 0,
            pointer_x: 0,
            pointer_y: 0,
            pointer_in_screen: false,
            stage_width,
            stage_height,
            poisoned: false,
            ephemeral_text: BTreeMap::new(),
            pending_movie: None,
        };
        self.sessions
            .insert(request.requested_session_id.0.clone(), session);
        Ok(request.requested_session_id)
    }

    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        ctx.validate()?;
        input.validate()?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_FVP_SESSION_MISSING", "session is not active"))?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_FVP_SESSION_POISONED",
                "session previously failed and must be shut down",
            ));
        }
        for result in &input.await_results {
            if result.token_id.starts_with("fvp.movie.") {
                let pending = session.pending_movie.as_ref().ok_or_else(|| {
                    invalid(
                        "ASTRA_FVP_MOVIE_COMPLETION_UNSOLICITED",
                        "movie completion has no matching pending playback",
                    )
                })?;
                if result.token_id != pending.token_id {
                    session.poisoned = true;
                    return Err(invalid(
                        "ASTRA_FVP_MOVIE_COMPLETION_IDENTITY",
                        "movie completion token does not match pending playback",
                    ));
                }
                if result.status != "completed" {
                    session.poisoned = true;
                    return Err(invalid(
                        "ASTRA_FVP_MOVIE_COMPLETION_STATUS",
                        "movie completion returned a non-completed status",
                    ));
                }
                session.runtime.complete_movie();
                session.pending_movie = None;
            }
        }
        if input.tick_index != session.last_step + 1 {
            return Err(invalid(
                "ASTRA_FVP_STEP_SEQUENCE",
                "step must be strictly consecutive",
            ));
        }
        if input.session_seed != session.seed || input.delta_ns != session.fixed_delta_ns {
            return Err(invalid(
                "ASTRA_FVP_STEP_IDENTITY",
                "step seed or delta drifted",
            ));
        }
        if !matches!(
            input.mode,
            LegacyReplayMode::Live | LegacyReplayMode::RestoreContinuation
        ) {
            return Err(invalid("ASTRA_FVP_STEP_MODE", "unsupported step mode"));
        }
        // The VM must run to the same semantic yield point as RFVP. Effects are
        // emitted only by syscalls and host media drains, so using the effect
        // capacity as an instruction quota splits one RFVP frame across many
        // Astra ticks and changes timer/dissolve behavior. Bound instruction
        // tracing here and validate the actual effect count after collection.
        let instruction_budget = input
            .budget
            .max_instructions
            .min(input.budget.max_trace_entries);
        apply_inputs(session, &input.input_edges)?;
        let frame_ms = input.delta_ns.div_ceil(1_000_000);
        let tick_result = catch_unwind(AssertUnwindSafe(|| {
            session
                .runtime
                .tick_bounded(frame_ms, u64::from(instruction_budget))
        }));
        match tick_result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                session.poisoned = true;
                return Err(invalid("ASTRA_FVP_STEP_FAILED", error.to_string()));
            }
            Err(_) => {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_FVP_STEP_PANIC",
                    "rfvp runtime panicked; session is poisoned",
                ));
            }
        }
        let raw_trace = session.runtime.take_trace();
        let journal = session.runtime.take_syscall_journal();
        let text_wait_active = session.runtime.has_text_wait();
        let text_print_observed = journal.iter().any(|entry| entry.name == "TextPrint");
        session.instruction_count = session
            .instruction_count
            .saturating_add(raw_trace.len() as u64);
        session.syscall_count = session.syscall_count.saturating_add(journal.len() as u64);
        session.last_step = input.tick_index;

        let mut effects = Vec::new();
        let mut waits = Vec::new();
        let mut coverage = LegacyCoverageDelta {
            instructions: raw_trace.len() as u64,
            syscalls: journal.len() as u64,
            ..Default::default()
        };
        for entry in journal {
            map_syscall(
                input.tick_index,
                effects.len() as u64,
                entry,
                &mut effects,
                &mut waits,
                &mut coverage,
                &mut session.ephemeral_text,
            )?;
        }
        if text_wait_active && text_print_observed {
            waits.push(LegacyWaitRequest::Input {
                token_id: format!("fvp.text_wait.{}", input.tick_index),
                mask: (1 << 0) | (1 << 6) | (1 << 7),
            });
        }
        for command in session.runtime.take_audio_commands() {
            let command = map_audio_command(command)?;
            command.validate()?;
            let payload = postcard::to_allocvec(&command)
                .map_err(|error| invalid("ASTRA_FVP_AUDIO_ENCODE", error.to_string()))?;
            effects.push(LegacyEffect::Audio {
                sequence: effects.len() as u64,
                command: "astra.emu.audio_command.v1".into(),
                payload,
            });
            coverage.audio_commands = coverage.audio_commands.saturating_add(1);
        }
        for command in session.runtime.take_movie_commands() {
            let (command, wait) = match command {
                HostMovieCommand::Play {
                    resource_uri,
                    mode,
                    screen_w,
                    screen_h,
                } => {
                    if session.pending_movie.is_some() {
                        session.poisoned = true;
                        return Err(invalid(
                            "ASTRA_FVP_MOVIE_PLAYBACK_CONFLICT",
                            "a second movie started before the pending movie completed",
                        ));
                    }
                    let playback_id = format!("movie.{}", input.tick_index);
                    let token_id = format!("fvp.movie.{}", input.tick_index);
                    let resource_uri = normalize_vfs_path(&resource_uri)
                        .map_err(|message| invalid("ASTRA_FVP_MOVIE_URI", message))?;
                    let legacy_mode = match mode {
                        MovieMode::ModalWithAudio => LegacyVideoMode::ModalWithAudio,
                        MovieMode::LayerNoAudio => LegacyVideoMode::LayerNoAudio,
                    };
                    session.pending_movie = Some(PendingMovieV1 {
                        playback_id: playback_id.clone(),
                        token_id: token_id.clone(),
                        resource_uri: resource_uri.clone(),
                        mode: legacy_mode,
                        stage_width: screen_w,
                        stage_height: screen_h,
                    });
                    (
                        LegacyVideoCommandV1::Play {
                            playback_id: playback_id.clone(),
                            resource_uri,
                            mode: legacy_mode,
                            stage_width: screen_w,
                            stage_height: screen_h,
                        },
                        Some(LegacyWaitRequest::MediaFence {
                            token_id,
                            media_id: playback_id,
                        }),
                    )
                }
                HostMovieCommand::Stop => {
                    let playback_id = session
                        .pending_movie
                        .as_ref()
                        .map(|pending| pending.playback_id.clone())
                        .ok_or_else(|| {
                            invalid(
                                "ASTRA_FVP_MOVIE_STOP_UNSOLICITED",
                                "movie stop has no matching pending playback",
                            )
                        })?;
                    (LegacyVideoCommandV1::Stop { playback_id }, None)
                }
            };
            command.validate()?;
            let payload = postcard::to_allocvec(&command)
                .map_err(|error| invalid("ASTRA_FVP_VIDEO_ENCODE", error.to_string()))?;
            effects.push(LegacyEffect::Presentation {
                sequence: effects.len() as u64,
                command: "astra.emu.video_command.v1".into(),
                payload,
            });
            if let Some(wait) = wait {
                waits.push(wait);
            }
            coverage.presentation_commands = coverage.presentation_commands.saturating_add(1);
        }
        let render_frame = session.runtime.record_render_frame().map_err(|error| {
            session.poisoned = true;
            invalid("ASTRA_FVP_RENDER_RECORD", error.to_string())
        })?;
        let render_frame = map_render_frame(render_frame)?;
        render_frame.validate()?;
        let render_payload = postcard::to_allocvec(&render_frame)
            .map_err(|error| invalid("ASTRA_FVP_RENDER_ENCODE", error.to_string()))?;
        if tracing::enabled!(tracing::Level::TRACE) {
            let mut translucent_draws = 0u32;
            let mut minimum_translucent_alpha = u32::MAX;
            let mut maximum_translucent_alpha = 0u32;
            for draw in &render_frame.draws {
                let alpha = draw.vertices[0].color[3].clamp(0.0, 1.0);
                if alpha > 0.0 && alpha < 1.0 {
                    translucent_draws = translucent_draws.saturating_add(1);
                    let quantized = (alpha * 1_000_000.0).round() as u32;
                    minimum_translucent_alpha = minimum_translucent_alpha.min(quantized);
                    maximum_translucent_alpha = maximum_translucent_alpha.max(quantized);
                }
            }
            tracing::trace!(
                event = "astra.emu.fvp.render_frame",
                fixed_step = input.tick_index,
                draw_count = render_frame.draws.len(),
                texture_update_count = render_frame.texture_updates.len(),
                translucent_draw_count = translucent_draws,
                minimum_translucent_alpha_ppm = if translucent_draws == 0 {
                    0
                } else {
                    minimum_translucent_alpha
                },
                maximum_translucent_alpha_ppm = maximum_translucent_alpha,
                render_payload_hash = %Hash256::from_sha256(&render_payload),
            );
        }
        effects.push(LegacyEffect::Presentation {
            sequence: effects.len() as u64,
            command: "astra.emu.render_frame.v1".into(),
            payload: render_payload,
        });
        coverage.presentation_commands = coverage.presentation_commands.saturating_add(1);
        if effects.len() > input.budget.max_effects as usize {
            session.poisoned = true;
            return Err(invalid(
                "ASTRA_FVP_EFFECT_BUDGET",
                format!(
                    "rfvp emitted {} effects; negotiated maximum is {}",
                    effects.len(),
                    input.budget.max_effects
                ),
            ));
        }
        let mut contexts = BTreeSet::new();
        let trace = raw_trace
            .into_iter()
            .enumerate()
            .map(|(sequence, entry)| {
                contexts.insert(entry.context_id);
                LegacyTraceEntry {
                    sequence: sequence as u64,
                    context_id: entry.context_id,
                    pc: entry.pc,
                    opcode: format!("0x{:02x}", entry.opcode),
                    action: None,
                    yield_reason: None,
                }
            })
            .collect();
        coverage.contexts = contexts.into_iter().collect();
        let state_bytes = session.runtime.canonical_state_bytes().map_err(|error| {
            session.poisoned = true;
            invalid("ASTRA_FVP_STATE", error.to_string())
        })?;
        let output = LegacyStepOutput {
            status: if session.runtime.is_terminal() {
                LegacyRuntimeStatus::Terminal
            } else if !waits.is_empty() || session.runtime.has_pending_wait() {
                LegacyRuntimeStatus::Awaiting
            } else {
                LegacyRuntimeStatus::Active
            },
            effects,
            waits,
            trace,
            diagnostics: Vec::new(),
            coverage,
            state_hash: Hash256::from_sha256(&state_bytes),
        };
        output.validate(&input.budget)?;
        Ok(output)
    }

    fn save(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<LegacySnapshotEnvelope, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_FVP_SESSION_MISSING", "session is not active"))?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_FVP_SESSION_POISONED",
                "poisoned sessions cannot be saved",
            ));
        }
        let payload = FvpSessionSnapshotV1 {
            case_fingerprint: session.case_fingerprint,
            runtime_bytes: session
                .runtime
                .snapshot()
                .map_err(|error| invalid("ASTRA_FVP_SNAPSHOT_CAPTURE", error.to_string()))?,
            last_step: session.last_step,
            seed: session.seed,
            fixed_delta_ns: session.fixed_delta_ns,
            compatibility_profile: session.compatibility_profile.clone(),
            instruction_count: session.instruction_count,
            syscall_count: session.syscall_count,
            pointer_x: session.pointer_x,
            pointer_y: session.pointer_y,
            pointer_in_screen: session.pointer_in_screen,
            stage_width: session.stage_width,
            stage_height: session.stage_height,
            pending_movie: session.pending_movie.clone(),
        };
        let bytes = postcard::to_allocvec(&payload)
            .map_err(|error| invalid("ASTRA_FVP_SNAPSHOT_ENCODE", error.to_string()))?;
        let state_hash = Hash256::from_sha256(&payload.runtime_bytes);
        let envelope = LegacySnapshotEnvelope {
            family_id: FamilyId(FVP_FAMILY_ID.into()),
            session_id: session_id.clone(),
            schema_version: SchemaVersion::new(2, 0, 0),
            case_fingerprint: session.case_fingerprint,
            fixed_step: session.last_step,
            session_seed: session.seed,
            runtime_cursor: session.instruction_count,
            family_sections: vec![LegacySnapshotSection {
                section_id: "fvp.runtime".into(),
                schema: "astra.emu.fvp.runtime.v2".into(),
                version: SchemaVersion::new(2, 0, 0),
                hash: Hash256::from_sha256(&bytes),
                bytes,
            }],
            redaction_status: "passed".into(),
        };
        envelope.validate()?;
        tracing::debug!(event = "astra.emu.fvp.snapshot_captured", state_hash = %state_hash, fixed_step = session.last_step);
        Ok(envelope)
    }

    fn restore(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        snapshot: &LegacySnapshotEnvelope,
    ) -> Result<LegacyRestoreReport, LegacyProviderError> {
        ctx.validate()?;
        snapshot.validate()?;
        if snapshot.family_id.0 != FVP_FAMILY_ID || snapshot.session_id != *session_id {
            return Err(invalid(
                "ASTRA_FVP_SNAPSHOT_IDENTITY",
                "snapshot family or session identity does not match",
            ));
        }
        if snapshot.family_sections.len() != 1 {
            return Err(invalid(
                "ASTRA_FVP_SNAPSHOT_SECTION",
                "FVP snapshot must contain exactly one runtime section",
            ));
        }
        let section = &snapshot.family_sections[0];
        if section.section_id != "fvp.runtime"
            || section.schema != "astra.emu.fvp.runtime.v2"
            || section.version != SchemaVersion::new(2, 0, 0)
            || section.hash != Hash256::from_sha256(&section.bytes)
        {
            return Err(invalid(
                "ASTRA_FVP_SNAPSHOT_SECTION",
                "FVP runtime section identity or hash is invalid",
            ));
        }
        let payload: FvpSessionSnapshotV1 = postcard::from_bytes(&section.bytes)
            .map_err(|error| invalid("ASTRA_FVP_SNAPSHOT_DECODE", error.to_string()))?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_FVP_SESSION_MISSING", "session is not active"))?;
        if payload.case_fingerprint != session.case_fingerprint
            || payload.seed != session.seed
            || payload.fixed_delta_ns != session.fixed_delta_ns
            || payload.stage_width != session.stage_width
            || payload.stage_height != session.stage_height
        {
            return Err(invalid(
                "ASTRA_FVP_SNAPSHOT_BINDING",
                "snapshot payload binding does not match the open session",
            ));
        }
        session
            .runtime
            .restore(&payload.runtime_bytes)
            .map_err(|error| {
                invalid(
                    "ASTRA_FVP_SNAPSHOT_RESTORE",
                    snapshot_restore_diagnostic(&error),
                )
            })?;
        session.last_step = payload.last_step;
        session.instruction_count = payload.instruction_count;
        session.syscall_count = payload.syscall_count;
        session.pointer_x = payload.pointer_x;
        session.pointer_y = payload.pointer_y;
        session.pointer_in_screen = payload.pointer_in_screen;
        session.stage_width = payload.stage_width;
        session.stage_height = payload.stage_height;
        session.pending_movie = payload.pending_movie.clone();
        if let Some(pending) = payload.pending_movie {
            session
                .runtime
                .restore_pending_movie(
                    pending.resource_uri,
                    match pending.mode {
                        LegacyVideoMode::ModalWithAudio => MovieMode::ModalWithAudio,
                        LegacyVideoMode::LayerNoAudio => MovieMode::LayerNoAudio,
                    },
                    pending.stage_width,
                    pending.stage_height,
                )
                .map_err(|error| invalid("ASTRA_FVP_MOVIE_RESTORE", error.to_string()))?;
        }
        session.poisoned = false;
        session.ephemeral_text.clear();
        let state_hash = Hash256::from_sha256(
            &session
                .runtime
                .canonical_state_bytes()
                .map_err(|error| invalid("ASTRA_FVP_STATE", error.to_string()))?,
        );
        Ok(LegacyRestoreReport {
            restored_fixed_step: session.last_step,
            session_seed: session.seed,
            state_hash,
            diagnostics: Vec::new(),
        })
    }

    fn shutdown(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
    ) -> Result<LegacyShutdownReport, LegacyProviderError> {
        ctx.validate()?;
        let mut session = self
            .sessions
            .remove(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_FVP_SESSION_MISSING", "session is not active"))?;
        let state_hash = Hash256::from_sha256(
            &session
                .runtime
                .canonical_state_bytes()
                .map_err(|error| invalid("ASTRA_FVP_STATE", error.to_string()))?,
        );
        Ok(LegacyShutdownReport {
            final_state_hash: state_hash,
            instruction_count: session.instruction_count,
            syscall_count: session.syscall_count,
            diagnostics: Vec::new(),
        })
    }

    fn take_ephemeral_text(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<LegacyEphemeralText>, LegacyProviderError> {
        ctx.validate()?;
        validate_symbol("text_lease_id", lease_id)?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_FVP_SESSION_MISSING", "session is not active"))?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_FVP_SESSION_POISONED",
                "poisoned session cannot expose ephemeral text",
            ));
        }
        Ok(session.ephemeral_text.remove(lease_id))
    }

    fn read_session_resource(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, LegacyProviderError> {
        ctx.validate()?;
        if max_bytes == 0 || max_bytes > MAX_FILE_BYTES as u64 {
            return Err(invalid(
                "ASTRA_FVP_RESOURCE_READ_BOUNDS",
                "session resource read limit is outside supported bounds",
            ));
        }
        let resource_uri = normalize_vfs_path(resource_uri)
            .map_err(|_| invalid("ASTRA_FVP_RESOURCE_URI", "resource URI is invalid"))?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_FVP_SESSION_MISSING", "session is not active"))?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_FVP_SESSION_POISONED",
                "poisoned session cannot expose resources",
            ));
        }
        let bytes = session
            .runtime
            .read_vfs_file(&resource_uri)
            .map_err(|_| invalid("ASTRA_FVP_RESOURCE_READ", "session resource is unavailable"))?;
        if bytes.len() as u64 > max_bytes {
            return Err(invalid(
                "ASTRA_FVP_RESOURCE_READ_BOUNDS",
                "session resource exceeds the requested byte limit",
            ));
        }
        Ok(bytes)
    }
}

fn map_render_frame(
    frame: RecordedRenderFrame,
) -> Result<LegacyRenderFrameV1, LegacyProviderError> {
    if !(320..=8192).contains(&frame.width) || !(240..=8192).contains(&frame.height) {
        return Err(invalid(
            "ASTRA_FVP_RENDER_DIMENSIONS",
            "recorded render dimensions are outside negotiated bounds",
        ));
    }
    let mut uploaded_bytes = 0usize;
    let texture_updates = frame
        .texture_updates
        .into_iter()
        .map(|update| {
            uploaded_bytes = uploaded_bytes
                .checked_add(update.pixels.len())
                .ok_or_else(|| invalid("ASTRA_FVP_RENDER_UPLOAD_BOUNDS", "upload size overflow"))?;
            let format = match update.desc.format {
                PixelFormat::Rgba8 => LegacyTextureFormat::Rgba8,
                PixelFormat::LumaA8 => LegacyTextureFormat::LumaAlpha8,
                _ => {
                    return Err(invalid(
                        "ASTRA_FVP_RENDER_TEXTURE_FORMAT",
                        "recorded texture uses an unsupported portable format",
                    ))
                }
            };
            let channels = match format {
                LegacyTextureFormat::Rgba8 => 4usize,
                LegacyTextureFormat::LumaAlpha8 => 2usize,
            };
            let expected = usize::try_from(update.desc.width)
                .ok()
                .and_then(|width| {
                    usize::try_from(update.desc.height)
                        .ok()
                        .and_then(|height| width.checked_mul(height))
                })
                .and_then(|pixels| pixels.checked_mul(channels))
                .ok_or_else(|| {
                    invalid("ASTRA_FVP_RENDER_TEXTURE_BOUNDS", "texture size overflow")
                })?;
            if update.pixels.len() != expected {
                return Err(invalid(
                    "ASTRA_FVP_RENDER_TEXTURE_LENGTH",
                    "texture byte length does not match descriptor",
                ));
            }
            Ok(LegacyTextureUpdateV1 {
                texture_id: update.texture_id,
                width: update.desc.width,
                height: update.desc.height,
                format,
                content_hash: Hash256::from_sha256(&update.pixels),
                pixels: update.pixels,
            })
        })
        .collect::<Result<Vec<_>, LegacyProviderError>>()?;
    if uploaded_bytes > MAX_RENDER_UPLOAD_BYTES_PER_STEP {
        return Err(invalid(
            "ASTRA_FVP_RENDER_UPLOAD_BOUNDS",
            "render texture uploads exceed the per-step budget",
        ));
    }
    let draws = frame
        .draws
        .into_iter()
        .map(|draw| LegacyDrawV1 {
            texture_id: draw.texture_id,
            vertices: draw.vertices.map(|vertex| LegacyVertexV1 {
                position: vertex.position,
                tex_coord: vertex.tex_coord,
                color: [
                    vertex.color.r,
                    vertex.color.g,
                    vertex.color.b,
                    vertex.color.a,
                ],
            }),
            blend: match draw.blend {
                BlendMode::Alpha | BlendMode::Opaque => LegacyBlendMode::Alpha,
                BlendMode::Add => LegacyBlendMode::Add,
                BlendMode::Multiply => LegacyBlendMode::Multiply,
                BlendMode::Screen => LegacyBlendMode::Alpha,
            },
            scissor: draw.scissor.map(|scissor| LegacyScissorV1 {
                x: scissor.x,
                y: scissor.y,
                width: scissor.width,
                height: scissor.height,
            }),
        })
        .collect();
    Ok(LegacyRenderFrameV1 {
        width: frame.width,
        height: frame.height,
        texture_updates,
        draws,
    })
}

fn apply_inputs(
    session: &mut FvpSession,
    edges: &[LegacyInputEdge],
) -> Result<(), LegacyProviderError> {
    let mut previous = None;
    for edge in edges {
        if previous.is_some_and(|sequence| edge.sequence <= sequence) {
            return Err(invalid(
                "ASTRA_FVP_INPUT_ORDER",
                "input edge sequence must be strictly increasing",
            ));
        }
        previous = Some(edge.sequence);
        match edge.control.as_str() {
            "pointer.x" => {
                session.pointer_x = input_i32(edge.value, "pointer x")?;
                session.pointer_in_screen = edge.pressed;
                session.runtime.inject_pointer(
                    session.pointer_x,
                    session.pointer_y,
                    session.pointer_in_screen,
                );
            }
            "pointer.y" => {
                session.pointer_y = input_i32(edge.value, "pointer y")?;
                session.pointer_in_screen = edge.pressed;
                session.runtime.inject_pointer(
                    session.pointer_x,
                    session.pointer_y,
                    session.pointer_in_screen,
                );
            }
            "wheel" => session
                .runtime
                .inject_wheel(input_i32(edge.value, "wheel")?),
            control => {
                let key = match control {
                    "confirm" => KeyCode::Enter,
                    "cancel" => KeyCode::Esc,
                    "pointer.primary" => KeyCode::MouseLeft,
                    "pointer.secondary" => KeyCode::MouseRight,
                    "up" => KeyCode::UpArrow,
                    "down" => KeyCode::DownArrow,
                    "left" => KeyCode::LeftArrow,
                    "right" => KeyCode::RightArrow,
                    "space" => KeyCode::Space,
                    _ => {
                        return Err(invalid(
                            "ASTRA_FVP_INPUT_CONTROL",
                            format!("unsupported input control {control}"),
                        ))
                    }
                };
                session.runtime.inject_key(key, edge.pressed, false);
            }
        }
    }
    Ok(())
}

fn input_i32(value: f32, subject: &'static str) -> Result<i32, LegacyProviderError> {
    if !value.is_finite()
        || value.fract() != 0.0
        || value < i32::MIN as f32
        || value >= i32::MAX as f32
    {
        return Err(invalid(
            "ASTRA_FVP_INPUT_VALUE",
            format!("{subject} must be a finite integer inside i32 bounds"),
        ));
    }
    Ok(value as i32)
}

fn map_syscall(
    tick_index: u64,
    sequence: u64,
    entry: SyscallJournalEntry,
    effects: &mut Vec<LegacyEffect>,
    waits: &mut Vec<LegacyWaitRequest>,
    coverage: &mut LegacyCoverageDelta,
    ephemeral_text: &mut BTreeMap<String, LegacyEphemeralText>,
) -> Result<(), LegacyProviderError> {
    let payload = postcard::to_allocvec(&entry.args)
        .map_err(|error| invalid("ASTRA_FVP_EFFECT_ENCODE", error.to_string()))?;
    if matches!(entry.name.as_str(), "ThreadWait" | "ThreadSleep") {
        let milliseconds = entry
            .args
            .first()
            .and_then(Variant::as_int)
            .unwrap_or(0)
            .max(0) as u32;
        waits.push(LegacyWaitRequest::Time {
            token_id: format!("fvp.wait.{tick_index}.{sequence}"),
            milliseconds,
        });
    }
    let effect = if entry.name.starts_with("Audio") || entry.name.starts_with("Sound") {
        LegacyEffect::RuntimeEvent {
            sequence,
            event: format!("fvp.syscall.{}", entry.name),
            payload,
        }
    } else if entry.name.starts_with("Text") {
        coverage.text_events += 1;
        let text = entry
            .args
            .iter()
            .find_map(Variant::as_string)
            .map(String::as_str)
            .unwrap_or("");
        if text.len() > MAX_EPHEMERAL_TEXT_BYTES {
            return Err(invalid(
                "ASTRA_FVP_TEXT_CAPTURE_BOUNDS",
                "captured text exceeds the ephemeral channel bound",
            ));
        }
        let lease_id = format!("fvp.text.{tick_index}.{sequence}");
        if ephemeral_text
            .insert(
                lease_id.clone(),
                LegacyEphemeralText {
                    lease_id: lease_id.clone(),
                    text: text.to_owned(),
                    speaker: None,
                },
            )
            .is_some()
        {
            return Err(invalid(
                "ASTRA_FVP_TEXT_LEASE_DUPLICATE",
                "ephemeral text lease id is duplicated",
            ));
        }
        LegacyEffect::TextCapture {
            sequence,
            lease_id,
            text_hash: Hash256::from_sha256(text.as_bytes()),
            byte_len: text.len().try_into().unwrap_or(u32::MAX),
            speaker_hash: None,
            source_ref: "fvp.runtime.text".into(),
        }
    } else if entry.name.starts_with("Graph")
        || entry.name.starts_with("Prim")
        || entry.name.starts_with("Dissolve")
        || entry.name.starts_with("Movie")
        || entry.name.starts_with("Motion")
    {
        coverage.presentation_commands += 1;
        LegacyEffect::Presentation {
            sequence,
            command: entry.name,
            payload,
        }
    } else {
        LegacyEffect::RuntimeEvent {
            sequence,
            event: format!("fvp.syscall.{}", entry.name),
            payload,
        }
    };
    effects.push(effect);
    Ok(())
}

fn map_audio_command(command: AudioCommand) -> Result<LegacyAudioCommandV1, LegacyProviderError> {
    let mapped = match command {
        AudioCommand::LoadResource { id, kind, uri } => LegacyAudioCommandV1::LoadResource {
            stream_id: id.0,
            encoding: map_audio_encoding(kind),
            resource_uri: normalize_vfs_path(&uri)
                .map_err(|message| invalid("ASTRA_FVP_AUDIO_RESOURCE_URI", message))?,
        },
        AudioCommand::LoadEncoded { .. } => {
            return Err(invalid(
                "ASTRA_FVP_AUDIO_INLINE_PAYLOAD_FORBIDDEN",
                "inline encoded audio cannot enter deterministic provider output",
            ));
        }
        AudioCommand::CreateStream { id, desc } => LegacyAudioCommandV1::CreateStream {
            stream_id: id.0,
            sample_rate: desc.sample_rate,
            channels: desc.channels,
            sample_format: match desc.sample_format {
                AudioSampleFormat::I16 => LegacyAudioSampleFormat::I16,
                AudioSampleFormat::F32 => LegacyAudioSampleFormat::F32,
            },
        },
        AudioCommand::SubmitI16 { id, samples } => LegacyAudioCommandV1::SubmitI16 {
            stream_id: id.0,
            samples,
        },
        AudioCommand::SubmitF32 { id, samples } => LegacyAudioCommandV1::SubmitF32 {
            stream_id: id.0,
            samples,
        },
        AudioCommand::Play {
            id,
            params,
            fade_in_ms,
        } => LegacyAudioCommandV1::Play {
            stream_id: id.0,
            volume: params.volume,
            pan: params.pan * 2.0 - 1.0,
            repeat: params.repeat,
            fade_in_ms,
        },
        AudioCommand::Stop { id, fade_ms } => LegacyAudioCommandV1::Stop {
            stream_id: id.0,
            fade_ms,
        },
        AudioCommand::Pause { id } => LegacyAudioCommandV1::Pause { stream_id: id.0 },
        AudioCommand::Resume { id } => LegacyAudioCommandV1::Resume { stream_id: id.0 },
        AudioCommand::SetParams { id, params } => LegacyAudioCommandV1::SetParams {
            stream_id: id.0,
            volume: params.volume,
            pan: params.pan * 2.0 - 1.0,
            repeat: params.repeat,
        },
        AudioCommand::DestroyStream { id } => {
            LegacyAudioCommandV1::DestroyStream { stream_id: id.0 }
        }
        AudioCommand::MasterVolume { volume } => LegacyAudioCommandV1::MasterVolume { volume },
    };
    Ok(mapped)
}

fn map_audio_encoding(kind: EncodedAudioKind) -> LegacyAudioEncoding {
    match kind {
        EncodedAudioKind::Unknown => LegacyAudioEncoding::Unknown,
        EncodedAudioKind::Wav => LegacyAudioEncoding::Wav,
        EncodedAudioKind::Ogg => LegacyAudioEncoding::Ogg,
        EncodedAudioKind::Mp3 => LegacyAudioEncoding::Mp3,
        EncodedAudioKind::Flac => LegacyAudioEncoding::Flac,
    }
}

fn normalize_vfs_path(path: &str) -> Result<String, String> {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    if normalized.is_empty()
        || normalized.len() > 4096
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("RFVP_VFS_PATH_TRAVERSAL".into());
    }
    Ok(normalized)
}

fn map_nls(nls: FvpNls) -> Nls {
    match nls {
        FvpNls::ShiftJis => Nls::ShiftJIS,
        FvpNls::Gbk => Nls::GBK,
        FvpNls::Utf8 => Nls::UTF8,
    }
}
fn parse_nls_option(options: &BTreeMap<String, String>) -> Result<FvpNls, LegacyProviderError> {
    match options.get("fvp.nls").map(String::as_str) {
        Some("shift_jis") => Ok(FvpNls::ShiftJis),
        Some("gbk") => Ok(FvpNls::Gbk),
        Some("utf8") => Ok(FvpNls::Utf8),
        Some(_) => Err(invalid(
            "ASTRA_FVP_NLS",
            "fvp.nls must be shift_jis, gbk, or utf8",
        )),
        None => Err(invalid(
            "ASTRA_FVP_NLS",
            "host VFS cases must explicitly declare fvp.nls",
        )),
    }
}

fn parse_stage_dimensions(
    options: &BTreeMap<String, String>,
) -> Result<(u32, u32), LegacyProviderError> {
    let width = options
        .get("fvp.stage_width")
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| invalid("ASTRA_FVP_STAGE_DIMENSIONS", "stage width is not a u32"))?
        .unwrap_or(1024);
    let height = options
        .get("fvp.stage_height")
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| invalid("ASTRA_FVP_STAGE_DIMENSIONS", "stage height is not a u32"))?
        .unwrap_or(768);
    if !(320..=8192).contains(&width) || !(240..=8192).contains(&height) {
        return Err(invalid(
            "ASTRA_FVP_STAGE_DIMENSIONS",
            "stage dimensions are outside the supported bounds",
        ));
    }
    Ok((width, height))
}
fn invalid(code: &'static str, message: impl Into<String>) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

fn snapshot_restore_diagnostic(error: &anyhow::Error) -> &'static str {
    const CODES: [&str; 10] = [
        "RFVP_SNAPSHOT_TEXTURE_RESTORE_TEXTURE",
        "RFVP_SNAPSHOT_TEXTURE_RESTORE_MASK",
        "RFVP_SNAPSHOT_TEXTURE_RESTORE_GAIJI",
        "RFVP_SNAPSHOT_TEXTURE_RESTORE_RAW_RGBA",
        "RFVP_SNAPSHOT_TEXTURE_RESTORE_UNKNOWN",
        "RFVP_SNAPSHOT_PARTS_RESTORE",
        "RFVP_SNAPSHOT_GAIJI_RESTORE",
        "RFVP_SNAPSHOT_DISSOLVE_MASK_RESTORE",
        "RFVP_SNAPSHOT_BGM_RESTORE",
        "RFVP_SNAPSHOT_SE_RESTORE",
    ];
    for cause in error.chain().map(ToString::to_string) {
        if let Some(code) = CODES.into_iter().find(|code| cause == *code) {
            return code;
        }
    }
    "RFVP_SNAPSHOT_RESTORE_FAILED"
}
fn format_error(error: crate::FvpFormatError) -> LegacyProviderError {
    invalid(error.code(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rfvp::integration::{decode_runtime_snapshot as decode_rfvp_snapshot, RuntimeSnapshotV1};

    struct MemoryReader {
        script: Vec<u8>,
    }

    impl LegacyVfsReader for MemoryReader {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError> {
            if mount_set_id != "mount.test" || uri != "script.hcb" {
                return Err(invalid("TEST_VFS_NOT_FOUND", "fixture URI is not present"));
            }
            Ok(astra_byte_source::ByteSourceStat {
                len: self.script.len() as u64,
                revision: astra_byte_source::SourceRevision(Hash256::from_sha256(&self.script)),
            })
        }

        fn read_file_range(
            &self,
            mount_set_id: &str,
            uri: &str,
            expected_revision: astra_byte_source::SourceRevision,
            range: astra_byte_source::ByteRange,
            max_bytes: u64,
        ) -> Result<astra_byte_source::RangeReadResult, LegacyProviderError> {
            let stat = self.stat_file(mount_set_id, uri)?;
            range
                .validate(stat.len, max_bytes)
                .map_err(|error| invalid("TEST_VFS_BOUNDS", error.to_string()))?;
            if stat.revision != expected_revision {
                return Err(invalid("TEST_VFS_REVISION", "fixture revision changed"));
            }
            let bytes =
                self.script[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(astra_byte_source::RangeReadResult {
                range,
                revision: stat.revision,
                content_hash: Hash256::from_sha256(&bytes),
                bytes,
            })
        }
    }

    #[test]
    fn host_vfs_lifecycle_is_deterministic_across_snapshot_restore() {
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let mut provider = FvpRuntimeProvider::with_vfs(Arc::new(MemoryReader { script }));
        let ctx = host_ctx();
        let probe = provider
            .probe(
                &ctx,
                LegacyProbeRequest {
                    root_mount_id: "mount.test".into(),
                    candidate_uris: vec!["script.hcb".into()],
                    marker_hashes: vec![fingerprint],
                    max_entries: 8,
                    max_metadata_bytes: 4096,
                },
            )
            .unwrap();
        assert_eq!(probe.confidence_permyriad, 10_000);
        assert_eq!(probe.content_identity, fingerprint);
        assert!(probe
            .markers
            .iter()
            .any(|marker| marker == "fvp.nls.shift_jis"));
        assert!(probe
            .markers
            .iter()
            .any(|marker| marker == "fvp.stage_width.1280"));
        assert!(probe
            .markers
            .iter()
            .any(|marker| marker == "fvp.stage_height.720"));

        let session_id = LegacyRuntimeSessionId("session.test".into());
        provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: session_id.clone(),
                    case_fingerprint: fingerprint,
                    script_uri: "script.hcb".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "rfvp.reference".into(),
                    family_options: [("fvp.nls".into(), "utf8".into())].into_iter().collect(),
                },
            )
            .unwrap();
        let output = provider
            .step(
                &ctx,
                &session_id,
                LegacyStepInput {
                    tick_index: 1,
                    delta_ns: 16_666_667,
                    session_seed: 7,
                    mode: LegacyReplayMode::Live,
                    input_edges: vec![],
                    await_results: vec![],
                    provider_results: vec![],
                    budget: LegacyStepBudget {
                        max_instructions: 16,
                        max_effects: 16,
                        max_trace_entries: 16,
                    },
                },
            )
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Terminal);
        let snapshot = provider.save(&ctx, &session_id).unwrap();
        let before = output.state_hash;
        let saved: FvpSessionSnapshotV1 =
            postcard::from_bytes(&snapshot.family_sections[0].bytes).unwrap();
        let canonical_before = provider
            .sessions
            .get_mut(&session_id.0)
            .unwrap()
            .runtime
            .canonical_state_bytes()
            .unwrap();
        assert_eq!(Hash256::from_sha256(&canonical_before), before);
        let restore = provider.restore(&ctx, &session_id, &snapshot).unwrap();
        let after_bytes = provider
            .sessions
            .get_mut(&session_id.0)
            .unwrap()
            .runtime
            .state_bytes()
            .unwrap();
        let before_snapshot = decode_runtime_snapshot(&saved.runtime_bytes);
        let after_snapshot = decode_runtime_snapshot(&after_bytes);
        let before_motion = &before_snapshot.save_state.motion;
        let after_motion = &after_snapshot.save_state.motion;
        assert_eq!(
            component_hash(&before_motion.color_manager),
            component_hash(&after_motion.color_manager),
            "color manager drifted"
        );
        assert_eq!(
            component_hash(&before_motion.prim_manager),
            component_hash(&after_motion.prim_manager),
            "prim manager drifted"
        );
        assert_eq!(
            component_hash(&before_motion.textures),
            component_hash(&after_motion.textures),
            "textures drifted"
        );
        assert_eq!(
            component_hash(&before_motion.text_manager),
            component_hash(&after_motion.text_manager),
            "text manager drifted"
        );
        assert_eq!(
            component_hash(&before_motion.parts_manager),
            component_hash(&after_motion.parts_manager),
            "parts manager drifted"
        );
        assert_eq!(
            component_hash(&before_motion.gaiji_manager),
            component_hash(&after_motion.gaiji_manager),
            "gaiji manager drifted"
        );
        assert_eq!(
            component_hash(&before_motion.dissolve1),
            component_hash(&after_motion.dissolve1),
            "dissolve1 drifted"
        );
        assert_eq!(
            component_hash(&before_motion.dissolve2),
            component_hash(&after_motion.dissolve2),
            "dissolve2 drifted"
        );
        assert_eq!(
            component_hash(&before_snapshot.save_state.audio),
            component_hash(&after_snapshot.save_state.audio),
            "audio snapshot drifted"
        );
        assert_eq!(
            component_hash(&before_snapshot.save_state.vm),
            component_hash(&after_snapshot.save_state.vm),
            "VM snapshot drifted"
        );
        assert_eq!(
            component_hash(&before_snapshot.save_state.globals_non_volatile),
            component_hash(&after_snapshot.save_state.globals_non_volatile),
            "non-volatile globals drifted"
        );
        assert_eq!(
            component_hash(&before_snapshot.globals_volatile),
            component_hash(&after_snapshot.globals_volatile),
            "volatile globals drifted"
        );
        assert_eq!(restore.state_hash, before);
        let shutdown = provider.shutdown(&ctx, &session_id).unwrap();
        assert_eq!(shutdown.final_state_hash, before);
        assert!(!provider.has_active_sessions());
    }

    #[test]
    fn session_resource_channel_resolves_virtual_files_and_enforces_bounds() {
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let mut provider = FvpRuntimeProvider::default();
        provider
            .register_case(FvpCaseImage {
                case_fingerprint: fingerprint,
                root_mount_id: "mount.test".into(),
                script_bytes: script,
                nls: FvpNls::Utf8,
                files: [("audio/theme.ogg".into(), vec![1, 2, 3, 4])]
                    .into_iter()
                    .collect(),
            })
            .unwrap();
        let ctx = host_ctx();
        let session_id = LegacyRuntimeSessionId("session.resource".into());
        open_fixture(&mut provider, &ctx, &session_id, fingerprint);

        assert_eq!(
            provider
                .read_session_resource(&ctx, &session_id, "Audio/Theme.ogg", 4)
                .unwrap(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            provider
                .read_session_resource(&ctx, &session_id, "audio/theme.ogg", 3)
                .unwrap_err()
                .code(),
            "ASTRA_FVP_RESOURCE_READ_BOUNDS"
        );
        assert_eq!(
            provider
                .read_session_resource(&ctx, &session_id, "../theme.ogg", 4)
                .unwrap_err()
                .code(),
            "ASTRA_FVP_RESOURCE_URI"
        );
    }

    #[test]
    fn audio_commands_are_resource_referenced_bounded_and_redacted() {
        let mapped = map_audio_command(AudioCommand::LoadResource {
            id: rfvp::host_api::AudioStreamId::bgm(2),
            kind: EncodedAudioKind::Ogg,
            uri: "Audio/Bgm/Theme.ogg".into(),
        })
        .unwrap();
        assert_eq!(
            mapped,
            LegacyAudioCommandV1::LoadResource {
                stream_id: 2,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: "audio/bgm/theme.ogg".into(),
            }
        );
        mapped.validate().unwrap();

        let inline = map_audio_command(AudioCommand::LoadEncoded {
            id: rfvp::host_api::AudioStreamId::se(0),
            kind: EncodedAudioKind::Wav,
            bytes: vec![1, 2, 3],
        })
        .unwrap_err();
        assert_eq!(inline.code(), "ASTRA_FVP_AUDIO_INLINE_PAYLOAD_FORBIDDEN");

        let play = map_audio_command(AudioCommand::Play {
            id: rfvp::host_api::AudioStreamId::se(3),
            params: rfvp::host_api::AudioParams {
                volume: 0.75,
                pan: 0.0,
                repeat: false,
            },
            fade_in_ms: 250,
        })
        .unwrap();
        assert!(matches!(play, LegacyAudioCommandV1::Play { pan: -1.0, .. }));
        play.validate().unwrap();
    }

    #[test]
    fn text_capture_uses_single_use_out_of_band_lease_without_payload_text() {
        let secret_text = "commercial dialogue fixture";
        let mut effects = Vec::new();
        let mut waits = Vec::new();
        let mut coverage = LegacyCoverageDelta::default();
        let mut leases = BTreeMap::new();
        map_syscall(
            7,
            3,
            SyscallJournalEntry {
                name: "TextPrint".into(),
                args: vec![Variant::String(secret_text.into())],
                result: Variant::Nil,
            },
            &mut effects,
            &mut waits,
            &mut coverage,
            &mut leases,
        )
        .unwrap();
        let LegacyEffect::TextCapture {
            lease_id,
            text_hash,
            byte_len,
            ..
        } = &effects[0]
        else {
            panic!("expected text capture");
        };
        assert_eq!(*text_hash, Hash256::from_sha256(secret_text.as_bytes()));
        assert_eq!(*byte_len as usize, secret_text.len());
        assert_eq!(leases.remove(lease_id).unwrap().text, secret_text);
        assert!(leases.remove(lease_id).is_none());
        let serialized = postcard::to_allocvec(&effects[0]).unwrap();
        assert!(!serialized
            .windows(secret_text.len())
            .any(|window| window == secret_text.as_bytes()));
    }

    #[test]
    fn sanitized_text_flow_covers_wait_input_snapshot_replay_and_shutdown() {
        let script = text_flow_hcb("Synthetic line");
        let fingerprint = Hash256::from_sha256(&script);
        let ctx = host_ctx();
        let mut first = FvpRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            script: script.clone(),
        }));
        let first_id = LegacyRuntimeSessionId("session.full_flow.first".into());
        open_fixture(&mut first, &ctx, &first_id, fingerprint);

        let first_step = first
            .step(&ctx, &first_id, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(first_step.status, LegacyRuntimeStatus::Awaiting);
        assert_eq!(first_step.coverage.syscalls, 2);
        assert!(first_step.waits.iter().any(|wait| matches!(
            wait,
            LegacyWaitRequest::Time {
                milliseconds: 40,
                ..
            }
        )));
        let lease_id = first_step
            .effects
            .iter()
            .find_map(|effect| match effect {
                LegacyEffect::TextCapture { lease_id, .. } => Some(lease_id.clone()),
                _ => None,
            })
            .expect("TextPrint must publish a redacted capture lease");
        let text = first
            .take_ephemeral_text(&ctx, &first_id, &lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(text.text, "Synthetic line");
        assert!(first
            .take_ephemeral_text(&ctx, &first_id, &lease_id)
            .unwrap()
            .is_none());

        let waiting_snapshot = first.save(&ctx, &first_id).unwrap();
        let physical_input = vec![LegacyInputEdge {
            control: "confirm".into(),
            pressed: true,
            value: 1.0,
            sequence: 1,
        }];
        let first_step_2 = first
            .step(&ctx, &first_id, step_input(2, physical_input.clone()))
            .unwrap();
        assert_eq!(first_step_2.status, LegacyRuntimeStatus::Awaiting);
        let first_step_3 = first
            .step(&ctx, &first_id, step_input(3, Vec::new()))
            .unwrap();
        assert_eq!(first_step_3.status, LegacyRuntimeStatus::Awaiting);
        let first_terminal = first
            .step(&ctx, &first_id, step_input(4, Vec::new()))
            .unwrap();
        assert_eq!(first_terminal.status, LegacyRuntimeStatus::Terminal);

        first.restore(&ctx, &first_id, &waiting_snapshot).unwrap();
        first
            .step(
                &ctx,
                &first_id,
                LegacyStepInput {
                    mode: LegacyReplayMode::RestoreContinuation,
                    ..step_input(2, physical_input)
                },
            )
            .unwrap();
        first
            .step(
                &ctx,
                &first_id,
                LegacyStepInput {
                    mode: LegacyReplayMode::RestoreContinuation,
                    ..step_input(3, Vec::new())
                },
            )
            .unwrap();
        let replay_terminal = first
            .step(
                &ctx,
                &first_id,
                LegacyStepInput {
                    mode: LegacyReplayMode::RestoreContinuation,
                    ..step_input(4, Vec::new())
                },
            )
            .unwrap();
        assert_eq!(replay_terminal.status, LegacyRuntimeStatus::Terminal);
        assert_eq!(replay_terminal.state_hash, first_terminal.state_hash);
        assert_eq!(
            postcard::to_allocvec(&replay_terminal.trace).unwrap(),
            postcard::to_allocvec(&first_terminal.trace).unwrap()
        );
        let shutdown = first.shutdown(&ctx, &first_id).unwrap();
        assert_eq!(shutdown.final_state_hash, replay_terminal.state_hash);
    }

    #[test]
    fn negotiated_effect_budget_does_not_throttle_non_effect_instructions() {
        let script = text_flow_hcb("Budgeted line");
        let fingerprint = Hash256::from_sha256(&script);
        let ctx = host_ctx();
        let mut provider = FvpRuntimeProvider::with_vfs(Arc::new(MemoryReader { script }));
        let session_id = LegacyRuntimeSessionId("session.effect_budget".into());
        open_fixture(&mut provider, &ctx, &session_id, fingerprint);

        let bounded = provider
            .step(
                &ctx,
                &session_id,
                LegacyStepInput {
                    budget: LegacyStepBudget {
                        max_instructions: 100_000,
                        max_effects: 4,
                        max_trace_entries: 65_536,
                    },
                    ..step_input(1, Vec::new())
                },
            )
            .unwrap();
        assert!(bounded.effects.len() <= 4);
        assert!(
            bounded.trace.len() > 1,
            "effect capacity must not become an instruction quota"
        );
    }

    fn open_fixture(
        provider: &mut FvpRuntimeProvider,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        fingerprint: Hash256,
    ) {
        provider
            .open(
                ctx,
                LegacyOpenRequest {
                    requested_session_id: session_id.clone(),
                    case_fingerprint: fingerprint,
                    script_uri: "script.hcb".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "rfvp.reference".into(),
                    family_options: [("fvp.nls".into(), "utf8".into())].into_iter().collect(),
                },
            )
            .unwrap();
    }

    fn step_input(tick_index: u64, input_edges: Vec<LegacyInputEdge>) -> LegacyStepInput {
        LegacyStepInput {
            tick_index,
            delta_ns: 16_666_667,
            session_seed: 7,
            mode: LegacyReplayMode::Live,
            input_edges,
            await_results: Vec::new(),
            provider_results: Vec::new(),
            budget: LegacyStepBudget {
                max_instructions: 64,
                max_effects: 64,
                max_trace_entries: 64,
            },
        }
    }

    fn text_flow_hcb(text: &str) -> Vec<u8> {
        assert!(text.len() < u8::MAX as usize);
        let mut code = vec![
            0x01,
            0,
            0, // init_stack(0 args, 0 locals)
            0x0c,
            0, // push_i8 text slot
            0x0e,
            (text.len() + 1) as u8,
        ];
        code.extend_from_slice(text.as_bytes());
        code.push(0);
        code.extend_from_slice(&[0x03, 0, 0]); // TextPrint
        code.extend_from_slice(&[0x0b, 40, 0, 0x03, 1, 0]); // ThreadWait(40ms)
        code.push(0x04); // ret

        let descriptor_offset = 4 + code.len();
        let mut bytes = (descriptor_offset as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(&code);
        bytes.extend_from_slice(&4u32.to_le_bytes()); // entry point
        bytes.extend_from_slice(&0u16.to_le_bytes()); // non-volatile globals
        bytes.extend_from_slice(&0u16.to_le_bytes()); // volatile globals
        bytes.extend_from_slice(&[8, 0, 2, b'X', 0]); // 1280x720 and title
        bytes.extend_from_slice(&2u16.to_le_bytes()); // syscall count
        bytes.extend_from_slice(&[2, 10]); // TextPrint argc and NUL-inclusive name size
        bytes.extend_from_slice(b"TextPrint\0");
        bytes.extend_from_slice(&[1, 11]); // ThreadWait argc and NUL-inclusive name size
        bytes.extend_from_slice(b"ThreadWait\0");
        bytes.extend_from_slice(&0u16.to_le_bytes()); // custom syscall count
        bytes
    }

    fn terminal_hcb() -> Vec<u8> {
        let mut bytes = 8u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0x04, 0, 0, 0]);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&[8, 0, 2, b'X', 0]);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    fn host_ctx() -> LegacyRuntimeHostCtx {
        LegacyRuntimeHostCtx {
            case_id: "case.test".into(),
            package_id: "package.test".into(),
            package_hash: Hash256::from_sha256(b"package"),
            mount_set_id: "mount.test".into(),
            media_service_ids: vec!["astra.media".into()],
            permission_policy_id: "permission.test".into(),
            report_sink_id: "report.test".into(),
            target: "windows".into(),
            profile: "test".into(),
        }
    }

    fn decode_runtime_snapshot(bytes: &[u8]) -> RuntimeSnapshotV1 {
        decode_rfvp_snapshot(bytes).unwrap()
    }

    fn component_hash(value: &impl serde::Serialize) -> Hash256 {
        Hash256::from_sha256(&bincode::serialize(value).unwrap())
    }
}
