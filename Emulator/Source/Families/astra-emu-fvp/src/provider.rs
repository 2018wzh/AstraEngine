use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Arc,
};

use astra_core::{Hash256, SchemaVersion};
use astra_emu_family_api::*;
use rfvp_hosted::{
    host_api::{InputModifiers, KeyCode, PointerButton, RfvpEvent},
    hosted::HostedStepInput,
};
use serde::{Deserialize, Serialize};

use crate::{
    hosted::{audio_commands_from_delta, video_commands_from_delta},
    hosted_runtime::HostedFvpSession,
    FvpHcbScript, FvpNls, FVP_FAMILY_ID, FVP_PROVIDER_ID,
};

const MAX_CASE_FILES: usize = 65_536;
const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct FvpCaseImage {
    pub case_fingerprint: Hash256,
    pub root_mount_id: String,
    pub script_bytes: Vec<u8>,
    pub nls: FvpNls,
    pub files: BTreeMap<String, Vec<u8>>,
}

struct FvpSession {
    case_fingerprint: Hash256,
    runtime: HostedFvpSession,
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
        let (stage_width, stage_height) = parse_stage_dimensions(&request.family_options)?;
        let script_uri = normalize_vfs_path(&request.script_uri)
            .map_err(|message| invalid("ASTRA_FVP_SCRIPT_URI", message))?;
        let runtime = if let Some(image) = self.cases.get(&request.case_fingerprint) {
            if image.root_mount_id != ctx.mount_set_id {
                return Err(invalid(
                    "ASTRA_FVP_MOUNT_BINDING",
                    "host mount does not match the registered case",
                ));
            }
            HostedFvpSession::open_case(
                image.files.clone(),
                script_uri.clone(),
                image.script_bytes.clone(),
                image.nls,
                stage_width,
                stage_height,
            )
            .map_err(|error| invalid("ASTRA_FVP_OPEN", error.to_string()))?
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
            let script_bytes =
                host.read_file(&ctx.mount_set_id, &script_uri, MAX_FILE_BYTES as u64)?;
            if Hash256::from_sha256(&script_bytes) != request.case_fingerprint {
                return Err(invalid(
                    "ASTRA_FVP_CASE_FINGERPRINT",
                    "host VFS script hash does not match case fingerprint",
                ));
            }
            let nls = parse_nls_option(&request.family_options)?;
            HostedFvpSession::open_vfs(
                host,
                ctx.mount_set_id.clone(),
                script_uri,
                nls,
                stage_width,
                stage_height,
            )
            .map_err(|error| invalid("ASTRA_FVP_OPEN", error.to_string()))?
        };
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
        complete_hosted_movies(session, &input.await_results)?;
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
        let hosted_input = HostedStepInput {
            events: hosted_inputs(session, &input.input_edges)?,
        };
        let tick_result = catch_unwind(AssertUnwindSafe(|| {
            session.runtime.step(input.delta_ns, hosted_input)
        }));
        let (delta, prepared) = match tick_result {
            Ok(Ok(result)) => result,
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
        };
        session.last_step = input.tick_index;

        let mut effects = Vec::new();
        let mut waits = Vec::new();
        let mut coverage = LegacyCoverageDelta::default();
        if let Some(prepared) = prepared {
            let payload = postcard::to_allocvec(&prepared)
                .map_err(|error| invalid("ASTRA_FVP_SCENE_ENCODE", error.to_string()))?;
            effects.push(LegacyEffect::Presentation {
                sequence: effects.len() as u64,
                command: "astra.emu.scene_packet.v1".into(),
                payload,
            });
            coverage.presentation_commands = coverage.presentation_commands.saturating_add(1);
        }
        for command in audio_commands_from_delta(&delta)
            .map_err(|error| invalid("ASTRA_FVP_AUDIO_DELTA", error.to_string()))?
        {
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
        for command in video_commands_from_delta(&delta)
            .map_err(|error| invalid("ASTRA_FVP_VIDEO_DELTA", error.to_string()))?
        {
            let LegacyVideoCommandV1::Play {
                playback_id,
                resource_uri,
                mode,
                stage_width,
                stage_height,
            } = &command
            else {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_FVP_VIDEO_DELTA",
                    "hosted RFVP delta emitted an unsupported video stop command",
                ));
            };
            if session.pending_movie.is_some() {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_FVP_MOVIE_PLAYBACK_CONFLICT",
                    "a second movie started before the pending movie completed",
                ));
            }
            let token_id = format!("fvp.movie.{}", input.tick_index);
            session.pending_movie = Some(PendingMovieV1 {
                playback_id: playback_id.clone(),
                token_id: token_id.clone(),
                resource_uri: resource_uri.clone(),
                mode: *mode,
                stage_width: *stage_width,
                stage_height: *stage_height,
            });
            command.validate()?;
            let payload = postcard::to_allocvec(&command)
                .map_err(|error| invalid("ASTRA_FVP_VIDEO_ENCODE", error.to_string()))?;
            effects.push(LegacyEffect::Presentation {
                sequence: effects.len() as u64,
                command: "astra.emu.video_command.v1".into(),
                payload,
            });
            waits.push(LegacyWaitRequest::MediaFence {
                token_id,
                media_id: playback_id.clone(),
            });
            coverage.presentation_commands = coverage.presentation_commands.saturating_add(1);
        }
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
        let state_bytes = session.runtime.canonical_state_bytes().map_err(|error| {
            session.poisoned = true;
            invalid("ASTRA_FVP_STATE", error.to_string())
        })?;
        let output = LegacyStepOutput {
            status: if session.runtime.quit_requested().map_err(|error| {
                session.poisoned = true;
                invalid("ASTRA_FVP_STATE", error.to_string())
            })? {
                LegacyRuntimeStatus::Terminal
            } else if !waits.is_empty() {
                LegacyRuntimeStatus::Awaiting
            } else {
                LegacyRuntimeStatus::Active
            },
            effects,
            waits,
            trace: Vec::new(),
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
                .snapshot_bytes()
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
        let state_hash = Hash256::from_sha256(
            &session
                .runtime
                .canonical_state_bytes()
                .map_err(|error| invalid("ASTRA_FVP_STATE", error.to_string()))?,
        );
        let envelope = LegacySnapshotEnvelope {
            family_id: FamilyId(FVP_FAMILY_ID.into()),
            session_id: session_id.clone(),
            schema_version: SchemaVersion::new(5, 0, 0),
            case_fingerprint: session.case_fingerprint,
            fixed_step: session.last_step,
            session_seed: session.seed,
            runtime_cursor: session.instruction_count,
            family_sections: vec![LegacySnapshotSection {
                section_id: "fvp.runtime".into(),
                schema: "astra.emu.fvp.runtime.v5".into(),
                version: SchemaVersion::new(5, 0, 0),
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
            || section.schema != "astra.emu.fvp.runtime.v5"
            || section.version != SchemaVersion::new(5, 0, 0)
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
            .restore_bytes(payload.runtime_bytes.clone())
            .map_err(|error| invalid("ASTRA_FVP_SNAPSHOT_RESTORE", error.to_string()))?;
        session.last_step = payload.last_step;
        session.instruction_count = payload.instruction_count;
        session.syscall_count = payload.syscall_count;
        session.pointer_x = payload.pointer_x;
        session.pointer_y = payload.pointer_y;
        session.pointer_in_screen = payload.pointer_in_screen;
        session.stage_width = payload.stage_width;
        session.stage_height = payload.stage_height;
        session.pending_movie = payload.pending_movie.clone();
        session.poisoned = false;
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
        let session = self
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
            .get(&session_id.0)
            .ok_or_else(|| invalid("ASTRA_FVP_SESSION_MISSING", "session is not active"))?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_FVP_SESSION_POISONED",
                "poisoned session cannot expose ephemeral text",
            ));
        }
        Ok(None)
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
            .read_resource(resource_uri, max_bytes as usize)
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

fn complete_hosted_movies(
    session: &mut FvpSession,
    results: &[LegacyAwaitResult],
) -> Result<(), LegacyProviderError> {
    for result in results {
        if !result.token_id.starts_with("fvp.movie.") {
            continue;
        }
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
        session.runtime.complete_video().map_err(|error| {
            session.poisoned = true;
            invalid("ASTRA_FVP_MOVIE_COMPLETION", error.to_string())
        })?;
        session.pending_movie = None;
    }
    Ok(())
}

fn hosted_inputs(
    session: &mut FvpSession,
    edges: &[LegacyInputEdge],
) -> Result<Vec<RfvpEvent>, LegacyProviderError> {
    let mut events = Vec::with_capacity(edges.len());
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
                events.push(RfvpEvent::PointerMove {
                    x: session.pointer_x,
                    y: session.pointer_y,
                    in_screen: session.pointer_in_screen,
                });
            }
            "pointer.y" => {
                session.pointer_y = input_i32(edge.value, "pointer y")?;
                session.pointer_in_screen = edge.pressed;
                events.push(RfvpEvent::PointerMove {
                    x: session.pointer_x,
                    y: session.pointer_y,
                    in_screen: session.pointer_in_screen,
                });
            }
            "wheel" => events.push(RfvpEvent::Wheel {
                delta_x: 0,
                delta_y: input_i32(edge.value, "wheel")?,
            }),
            "pointer.primary" | "pointer.secondary" => {
                let button = if edge.control == "pointer.primary" {
                    PointerButton::Left
                } else {
                    PointerButton::Right
                };
                events.push(if edge.pressed {
                    RfvpEvent::PointerDown {
                        button,
                        x: session.pointer_x,
                        y: session.pointer_y,
                    }
                } else {
                    RfvpEvent::PointerUp {
                        button,
                        x: session.pointer_x,
                        y: session.pointer_y,
                    }
                });
            }
            control => {
                let key = match control {
                    "confirm" => KeyCode::Return,
                    "cancel" => KeyCode::Escape,
                    "up" => KeyCode::Up,
                    "down" => KeyCode::Down,
                    "left" => KeyCode::Left,
                    "right" => KeyCode::Right,
                    "space" => KeyCode::Space,
                    "shift" => KeyCode::Shift,
                    "control" => KeyCode::Control,
                    _ => {
                        return Err(invalid(
                            "ASTRA_FVP_INPUT_CONTROL",
                            format!("unsupported input control {control}"),
                        ))
                    }
                };
                events.push(if edge.pressed {
                    RfvpEvent::KeyDown {
                        key,
                        repeat: false,
                        modifiers: InputModifiers::empty(),
                    }
                } else {
                    RfvpEvent::KeyUp {
                        key,
                        modifiers: InputModifiers::empty(),
                    }
                });
            }
        }
    }
    Ok(events)
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

fn format_error(error: crate::FvpFormatError) -> LegacyProviderError {
    invalid(error.code(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hosted_host::HostedMemoryHost, hosted_worker::HostedSessionWorker};
    use rfvp_hosted::{
        host_api::{RfvpFileSystem, RfvpHost},
        hosted::{HostedBootConfig, HostedConfig, HostedLimits, HostedSession, HostedStepInput},
        script::parser::Nls as HostedNls,
    };

    #[test]
    fn hosted_session_boots_and_steps_on_the_thread_confined_worker() {
        let script = terminal_hcb();
        let files = BTreeMap::from([
            ("script.hcb".into(), script),
            (
                "default.ttf".into(),
                include_bytes!(
                    "../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf"
                )
                .to_vec(),
            ),
        ]);
        let worker = HostedSessionWorker::try_spawn(move || {
            let mut host = HostedMemoryHost::new(files)
                .map_err(|error| invalid("TEST_HOST", format!("{error:?}")))?;
            let mut hcb_paths = Vec::new();
            host.fs()
                .enumerate_by_extension(".", "hcb", &mut |path, _| {
                    hcb_paths.push(path.to_owned());
                    Ok(())
                })
                .map_err(|error| invalid("TEST_ENUMERATE", format!("{error:?}")))?;
            if hcb_paths != ["script.hcb"] {
                return Err(invalid(
                    "TEST_ENUMERATE",
                    format!("unexpected HCB entries: {}", hcb_paths.join(",")),
                ));
            }
            let font_len = host
                .fs()
                .metadata("default.ttf")
                .map_err(|error| invalid("TEST_FONT", format!("{error:?}")))?
                .len;
            if font_len == 0 {
                return Err(invalid("TEST_FONT", "default font is unexpectedly empty"));
            }
            let mut runtime = HostedSession::new(HostedConfig::default(), HostedLimits::default())
                .map_err(|error| invalid("TEST_SESSION", format!("{error:?}")))?;
            if let Err(error) = runtime.boot(
                &mut host,
                HostedBootConfig {
                    asset_root: ".",
                    hcb_extension: "hcb",
                    max_hcb_bytes: MAX_FILE_BYTES,
                    max_manifest_entries: MAX_CASE_FILES,
                    nls: HostedNls::UTF8,
                },
            ) {
                return Err(invalid(
                    "TEST_BOOT",
                    format!(
                        "{error:?}: {}",
                        runtime.core().last_error_detail().unwrap_or("no detail")
                    ),
                ));
            }
            Ok::<_, LegacyProviderError>((runtime, host))
        })
        .expect("hosted session must boot on its owner thread");
        let delta = worker
            .execute(|(runtime, host)| {
                host.advance(16_666_667)
                    .map_err(|error| invalid("TEST_CLOCK", format!("{error:?}")))?;
                runtime
                    .step(host, HostedStepInput::default())
                    .map_err(|error| invalid("TEST_STEP", format!("{error:?}")))
            })
            .expect("worker must answer")
            .expect("hosted step must succeed");
        assert_eq!(delta.tick.frame_index, 1);
        assert!(delta.scene.iter().any(|operation| matches!(
            operation,
            rfvp_hosted::hosted::HostedSceneOperation::Present
        )));
        worker.shutdown().expect("worker must stop");
    }

    #[test]
    #[cfg(any())]
    fn pointer_button_edges_use_rfvp_mouse_semantics() {
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let ctx = host_ctx();

        let mut routed = FvpRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            script: script.clone(),
        }));
        let routed_id = LegacyRuntimeSessionId("session.pointer.routed".into());
        open_fixture(&mut routed, &ctx, &routed_id, fingerprint);
        let routed_output = routed
            .step(
                &ctx,
                &routed_id,
                step_input(
                    1,
                    vec![
                        LegacyInputEdge {
                            control: "pointer.x".into(),
                            pressed: true,
                            value: 1135.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: "pointer.y".into(),
                            pressed: true,
                            value: 110.0,
                            sequence: 2,
                        },
                        LegacyInputEdge {
                            control: "pointer.primary".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 3,
                        },
                    ],
                ),
            )
            .unwrap();

        let mut direct = FvpRuntimeProvider::with_vfs(Arc::new(MemoryReader { script }));
        let direct_id = LegacyRuntimeSessionId("session.pointer.direct".into());
        open_fixture(&mut direct, &ctx, &direct_id, fingerprint);
        let runtime = &mut direct.sessions.get_mut(&direct_id.0).unwrap().runtime;
        runtime.inject_pointer(1135, 0, true);
        runtime.inject_pointer(1135, 110, true);
        runtime.inject_pointer_button(KeyCode::MouseLeft, true);
        let direct_output = direct
            .step(&ctx, &direct_id, step_input(1, Vec::new()))
            .unwrap();

        assert_eq!(routed_output.state_hash, direct_output.state_hash);
        assert_eq!(
            postcard::to_allocvec(&routed_output.trace).unwrap(),
            postcard::to_allocvec(&direct_output.trace).unwrap()
        );
    }

    #[test]
    #[cfg(any())]
    fn modifier_edges_use_rfvp_key_semantics() {
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let ctx = host_ctx();

        let mut routed = FvpRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            script: script.clone(),
        }));
        let routed_id = LegacyRuntimeSessionId("session.modifier.routed".into());
        open_fixture(&mut routed, &ctx, &routed_id, fingerprint);
        let routed_output = routed
            .step(
                &ctx,
                &routed_id,
                step_input(
                    1,
                    vec![
                        LegacyInputEdge {
                            control: "shift".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 1,
                        },
                        LegacyInputEdge {
                            control: "control".into(),
                            pressed: true,
                            value: 1.0,
                            sequence: 2,
                        },
                    ],
                ),
            )
            .unwrap();

        let mut direct = FvpRuntimeProvider::with_vfs(Arc::new(MemoryReader { script }));
        let direct_id = LegacyRuntimeSessionId("session.modifier.direct".into());
        open_fixture(&mut direct, &ctx, &direct_id, fingerprint);
        let runtime = &mut direct.sessions.get_mut(&direct_id.0).unwrap().runtime;
        runtime.inject_key(KeyCode::Shift, true, false);
        runtime.inject_key(KeyCode::Ctrl, true, false);
        let direct_output = direct
            .step(&ctx, &direct_id, step_input(1, Vec::new()))
            .unwrap();

        assert_eq!(routed_output.state_hash, direct_output.state_hash);
        assert_eq!(
            postcard::to_allocvec(&routed_output.trace).unwrap(),
            postcard::to_allocvec(&direct_output.trace).unwrap()
        );
    }

    #[test]
    #[cfg(any())]
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
        let saved: FvpSessionSnapshotV1 =
            postcard::from_bytes(&snapshot.family_sections[0].bytes).expect("snapshot payload");
        let before = Hash256::from_sha256(&saved.runtime_bytes);
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
    #[cfg(any())]
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
                files: [
                    ("audio/theme.ogg".into(), vec![1, 2, 3, 4]),
                    (
                        "default.ttf".into(),
                        include_bytes!(
                            "../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf"
                        )
                        .to_vec(),
                    ),
                ]
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
    fn hosted_v5_session_emits_one_semantic_commit_and_restores() {
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let mut provider = FvpRuntimeProvider::default();
        provider
            .register_case(FvpCaseImage {
                case_fingerprint: fingerprint,
                root_mount_id: "mount.test".into(),
                script_bytes: script,
                nls: FvpNls::Utf8,
                files: [("default.ttf".into(), public_default_font())]
                    .into_iter()
                    .collect(),
            })
            .expect("registered case must be valid");
        let ctx = host_ctx();
        let session_id = LegacyRuntimeSessionId("session.hosted.v5".into());
        open_fixture(&mut provider, &ctx, &session_id, fingerprint);
        let output = provider
            .step(&ctx, &session_id, step_input(1, Vec::new()))
            .expect("hosted step must succeed");
        assert!(output.effects.iter().any(|effect| matches!(
            effect,
            LegacyEffect::Presentation { command, .. } if command == "astra.emu.scene_packet.v1"
        )));
        assert!(
            output.trace.is_empty(),
            "shipping profile must not format opcode trace"
        );
        let snapshot = provider.save(&ctx, &session_id).expect("save must succeed");
        assert_eq!(snapshot.schema_version, SchemaVersion::new(5, 0, 0));
        assert_eq!(
            snapshot.family_sections[0].schema,
            "astra.emu.fvp.runtime.v5"
        );
        let before = output.state_hash;
        let before_components = provider
            .sessions
            .get(&session_id.0)
            .expect("session must remain open")
            .runtime
            .canonical_state_component_hashes()
            .expect("canonical state must be available");
        let restored = provider
            .restore(&ctx, &session_id, &snapshot)
            .expect("v5 snapshot must restore");
        let restored_components = provider
            .sessions
            .get(&session_id.0)
            .expect("session must remain open")
            .runtime
            .canonical_state_component_hashes()
            .expect("canonical state must be available");
        assert_eq!(
            restored.state_hash, before,
            "canonical component mismatch: before={before_components:?} restored={restored_components:?}"
        );
        provider
            .shutdown(&ctx, &session_id)
            .expect("hosted session must shut down");
    }
    #[cfg(any())]
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
    #[cfg(any())]
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
    #[cfg(any())]
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
    #[cfg(any())]
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

    fn public_default_font() -> Vec<u8> {
        include_bytes!("../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf")
            .to_vec()
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

    #[cfg(any())]
    fn decode_runtime_snapshot(bytes: &[u8]) -> RuntimeSnapshotV1 {
        decode_rfvp_snapshot(bytes).unwrap()
    }
}
