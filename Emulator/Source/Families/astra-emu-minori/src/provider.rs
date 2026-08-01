use std::{collections::BTreeMap, io::Cursor, sync::Arc};

use astra_core::{Hash256, SchemaVersion};
use astra_emu_family_api::{
    validate_symbol, FamilyId, LegacyAudioCommandV1, LegacyAudioEncoding, LegacyBlendMode,
    LegacyCoverageDelta, LegacyDrawV1, LegacyEffect, LegacyEphemeralText,
    LegacyFamilyPluginDescriptor, LegacyOpenRequest, LegacyProbeReport, LegacyProbeRequest,
    LegacyProviderError, LegacyRenderResourceFrameV1, LegacyRestoreReport, LegacyRuntimeHostCtx,
    LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacyRuntimeStatus, LegacyShutdownReport,
    LegacySnapshotEnvelope, LegacySnapshotSection, LegacyStepInput, LegacyStepOutput,
    LegacyTextPresentationLeaseV1, LegacyTextPresentationV1, LegacyTextRegionV1,
    LegacyTextureFormat, LegacyTextureResourceV1, LegacyTraceEntry, LegacyVertexV1,
    LegacyVfsReader, LegacyWaitRequest, LEGACY_FAMILY_ABI_FINGERPRINT,
};

use crate::{
    parse_sc, MinoriAudioCommand, MinoriEffectFrame, MinoriRuntimeError, MinoriRuntimeState,
    MinoriStageCommand, MinoriStageLayer, MinoriVm, MinoriVmEvent, MinoriWaitState,
    ScOpcodeCatalog, MINORI_RUNTIME_STATE_SCHEMA,
};

pub const MINORI_FAMILY_ID: &str = "minori";
pub const MINORI_RUNTIME_PROVIDER_ID: &str = "astra.emu.family.minori";
const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESOURCE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_EPHEMERAL_TEXT_BYTES: usize = 64 * 1024;
const MESSAGE_INPUT_MASK: u64 = (1 << 0) | (1 << 6) | (1 << 7);

fn minori_message_presentation(
    stage_size: Option<(u32, u32)>,
) -> Result<LegacyTextPresentationV1, LegacyProviderError> {
    if stage_size != Some((1280, 720)) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY",
            "the verified Minori message layout requires the 1280x720 reference stage",
        ));
    }
    let presentation = LegacyTextPresentationV1 {
        layout_id: "minori.message".into(),
        language: "ja-JP".into(),
        font_families: vec!["Noto Sans JP".into()],
        body: LegacyTextRegionV1 {
            x: 160,
            y: 568,
            width: 960,
            height: 112,
            font_size: 26.0,
            line_height: 32.0,
            max_lines: 3,
        },
        speaker: Some(LegacyTextRegionV1 {
            x: 160,
            y: 528,
            width: 960,
            height: 32,
            font_size: 26.0,
            line_height: 32.0,
            max_lines: 1,
        }),
        rgba: [255, 255, 255, 255],
    };
    presentation.validate()?;
    Ok(presentation)
}

struct MinoriSession {
    case_fingerprint: Hash256,
    mount_set_id: String,
    fixed_delta_ns: u64,
    session_seed: u64,
    stage_size: Option<(u32, u32)>,
    vm: MinoriVm,
    ephemeral_text: BTreeMap<String, LegacyEphemeralText>,
    poisoned: bool,
}

#[derive(Default)]
pub struct MinoriRuntimeProvider {
    vfs: Option<Arc<dyn LegacyVfsReader>>,
    sessions: BTreeMap<String, MinoriSession>,
}

impl MinoriRuntimeProvider {
    pub fn with_vfs(vfs: Arc<dyn LegacyVfsReader>) -> Self {
        Self {
            vfs: Some(vfs),
            sessions: BTreeMap::new(),
        }
    }

    pub fn has_active_sessions(&self) -> bool {
        !self.sessions.is_empty()
    }

    fn vfs(&self) -> Result<&Arc<dyn LegacyVfsReader>, LegacyProviderError> {
        self.vfs.as_ref().ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_RUNTIME_VFS",
                "Minori runtime has no explicitly bound VFS reader",
            )
        })
    }
}

pub fn create_static_minori_provider(
    vfs: Arc<dyn LegacyVfsReader>,
) -> Result<Box<dyn LegacyRuntimeProvider>, LegacyProviderError> {
    let provider = MinoriRuntimeProvider::with_vfs(vfs);
    provider.descriptor().validate()?;
    Ok(Box::new(provider))
}

impl LegacyRuntimeProvider for MinoriRuntimeProvider {
    fn descriptor(&self) -> LegacyFamilyPluginDescriptor {
        LegacyFamilyPluginDescriptor {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            plugin_id: "astra.emu.minori".into(),
            provider_id: MINORI_RUNTIME_PROVIDER_ID.into(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            rustc_fingerprint: env!("ASTRA_MINORI_RUSTC_FINGERPRINT").into(),
            feature_fingerprint: env!("ASTRA_MINORI_FEATURE_FINGERPRINT").into(),
            abi_fingerprint: LEGACY_FAMILY_ABI_FINGERPRINT.into(),
            supported_formats: vec![
                "minori.sc".into(),
                "minori.paz".into(),
                "minori.ani".into(),
                "minori.sqz".into(),
            ],
            permissions: vec![
                "vfs.read".into(),
                "media.submit".into(),
                "storage.request".into(),
            ],
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
        if request.max_entries == 0 || request.max_metadata_bytes == 0 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PROBE_BUDGET",
                "Minori probe budget is empty",
            ));
        }
        let candidates = request
            .candidate_uris
            .iter()
            .take(request.max_entries as usize)
            .filter(|uri| uri.starts_with("minori:/scr/") && uri.ends_with(".sc"))
            .collect::<Vec<_>>();
        let candidate = candidates
            .iter()
            .find(|uri| uri.eq_ignore_ascii_case("minori:/scr/test.sc"))
            .copied()
            .or_else(|| (candidates.len() == 1).then(|| candidates[0]))
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_PROBE_ENTRY",
                    "probe requires one unambiguous Minori entry script",
                )
            })?;
        let bytes = self.vfs()?.read_file(
            &request.root_mount_id,
            candidate,
            request.max_metadata_bytes.min(MAX_SCRIPT_BYTES),
        )?;
        parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        let identity = Hash256::from_sha256(&bytes);
        let marker_match =
            request.marker_hashes.is_empty() || request.marker_hashes.contains(&identity);
        Ok(LegacyProbeReport {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            confidence_permyriad: if marker_match { 10_000 } else { 0 },
            markers: if marker_match {
                vec!["minori.sc.cp932".into(), "minori.sc.command_stream".into()]
            } else {
                Vec::new()
            },
            blockers: Vec::new(),
            content_identity: identity,
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
                "ASTRA_EMU_MINORI_FIXED_DELTA",
                "fixed delta is outside 1ns..=1s",
            ));
        }
        if self.sessions.contains_key(&request.requested_session_id.0) {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_DUPLICATE",
                "session id is already active",
            ));
        }
        validate_script_uri(&request.script_uri)?;
        let bytes =
            self.vfs()?
                .read_file(&ctx.mount_set_id, &request.script_uri, MAX_SCRIPT_BYTES)?;
        let script_hash = Hash256::from_sha256(&bytes);
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        let vm = MinoriVm::new(
            request.script_uri,
            script_hash,
            script,
            request.session_seed,
        )
        .map_err(runtime_error)?;
        let stage_size = match (
            request.family_options.get("astra.stage_width"),
            request.family_options.get("astra.stage_height"),
        ) {
            (None, None) => None,
            (Some(width), Some(height)) => {
                let width = width.parse::<u32>().map_err(|_| {
                    invalid("ASTRA_EMU_MINORI_STAGE_SIZE", "stage width is invalid")
                })?;
                let height = height.parse::<u32>().map_err(|_| {
                    invalid("ASTRA_EMU_MINORI_STAGE_SIZE", "stage height is invalid")
                })?;
                if !(320..=8192).contains(&width) || !(240..=8192).contains(&height) {
                    return Err(invalid(
                        "ASTRA_EMU_MINORI_STAGE_SIZE",
                        "stage dimensions are outside the supported bound",
                    ));
                }
                Some((width, height))
            }
            _ => {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_STAGE_SIZE",
                    "stage dimensions must be supplied together",
                ));
            }
        };
        let id = request.requested_session_id;
        self.sessions.insert(
            id.0.clone(),
            MinoriSession {
                case_fingerprint: request.case_fingerprint,
                mount_set_id: ctx.mount_set_id.clone(),
                fixed_delta_ns: request.fixed_delta_ns,
                session_seed: request.session_seed,
                stage_size,
                vm,
                ephemeral_text: BTreeMap::new(),
                poisoned: false,
            },
        );
        Ok(id)
    }

    fn step(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        input: LegacyStepInput,
    ) -> Result<LegacyStepOutput, LegacyProviderError> {
        ctx.validate()?;
        input.validate()?;
        let vfs = Arc::clone(self.vfs()?);
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_POISONED",
                "poisoned session cannot continue",
            ));
        }
        if input.delta_ns != session.fixed_delta_ns || input.session_seed != session.session_seed {
            return Err(invalid(
                "ASTRA_EMU_MINORI_STEP_IDENTITY",
                "step timing or seed does not match the open session",
            ));
        }
        if session
            .vm
            .state()
            .fixed_tick
            .checked_add(1)
            .is_none_or(|expected| input.tick_index != expected)
        {
            session.poisoned = true;
            return Err(runtime_error(MinoriRuntimeError::State));
        }
        if !input.input_edges.is_empty() || !input.provider_results.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_STEP_CHANNEL",
                "input or provider result semantics are not yet verified",
            ));
        }
        let animated_effect = session
            .vm
            .advance_effect_clock(input.delta_ns)
            .map_err(runtime_error)?;
        if let Some(wait) = session.vm.state().wait.clone() {
            if input.await_results.is_empty() {
                session
                    .vm
                    .advance_waiting_tick(input.tick_index)
                    .map_err(runtime_error)?;
                let effects = animated_effect
                    .as_ref()
                    .map(|frame| {
                        effect_presentation(
                            &vfs,
                            &session.mount_set_id,
                            session.stage_size,
                            session.vm.state(),
                            frame,
                        )
                    })
                    .transpose()?
                    .into_iter()
                    .collect();
                return waiting_output(&session.vm, wait, effects, &input);
            }
            let expected = wait_token(&wait);
            if input.await_results.len() != 1
                || input.await_results[0].token_id != expected
                || input.await_results[0].status != "completed"
            {
                return Err(invalid(
                    "ASTRA_EMU_MINORI_AWAIT_RESULT",
                    "await result does not match the active wait token",
                ));
            }
            session.vm.resolve_wait(expected).map_err(runtime_error)?;
        } else if !input.await_results.is_empty() {
            return Err(invalid(
                "ASTRA_EMU_MINORI_AWAIT_UNEXPECTED",
                "step supplied an await result without an active wait",
            ));
        }
        let before = session.vm.state().instruction_count;
        let event = match session
            .vm
            .step(input.tick_index, input.budget.max_instructions)
        {
            Ok(event) => event,
            Err(error) => {
                session.poisoned = true;
                return Err(runtime_error(error));
            }
        };
        let chain_target = match &event {
            Some(MinoriVmEvent::Chain { target }) => Some(target.clone()),
            _ => None,
        };
        if let Some(target) = chain_target.as_deref() {
            let switch_result = load_script(&vfs, &session.mount_set_id, target).and_then(
                |(script_uri, script_hash, script)| {
                    session
                        .vm
                        .replace_script(script_uri, script_hash, script)
                        .map_err(runtime_error)
                },
            );
            if let Err(error) = switch_result {
                session.poisoned = true;
                return Err(error);
            }
        }
        let after = session.vm.state().instruction_count;
        let mut effects = Vec::new();
        if let Some(frame) = &animated_effect {
            if !matches!(
                event,
                Some(
                    MinoriVmEvent::Stage(_)
                        | MinoriVmEvent::Effect(_)
                        | MinoriVmEvent::Panel { .. }
                )
            ) {
                effects.push(effect_presentation(
                    &vfs,
                    &session.mount_set_id,
                    session.stage_size,
                    session.vm.state(),
                    frame,
                )?);
            }
        }
        if let Some(MinoriVmEvent::Message {
            presentation_sequence,
            capture_sequence,
            text,
            speaker,
            wait: _,
        }) = &event
        {
            if text.len() > MAX_EPHEMERAL_TEXT_BYTES
                || speaker
                    .as_ref()
                    .is_some_and(|value| value.len() > MAX_EPHEMERAL_TEXT_BYTES)
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                    "message or speaker exceeds the ephemeral text channel bound",
                ));
            }
            let lease_id = format!("minori.text.{}.{}", input.tick_index, capture_sequence);
            let presentation = LegacyTextPresentationLeaseV1 {
                lease_id: lease_id.clone(),
                presentation: match minori_message_presentation(session.stage_size) {
                    Ok(presentation) => presentation,
                    Err(error) => {
                        session.poisoned = true;
                        return Err(error);
                    }
                },
            };
            presentation.validate().inspect_err(|_| {
                session.poisoned = true;
            })?;
            let presentation_payload = postcard::to_allocvec(&presentation).map_err(|_| {
                session.poisoned = true;
                invalid(
                    "ASTRA_EMU_MINORI_TEXT_PRESENTATION_ENCODE",
                    "message presentation could not be encoded",
                )
            })?;
            if session
                .ephemeral_text
                .insert(
                    lease_id.clone(),
                    LegacyEphemeralText {
                        lease_id: lease_id.clone(),
                        text: text.clone(),
                        speaker: speaker.clone(),
                    },
                )
                .is_some()
            {
                session.poisoned = true;
                return Err(invalid(
                    "ASTRA_EMU_MINORI_TEXT_LEASE_DUPLICATE",
                    "ephemeral text lease id is duplicated",
                ));
            }
            effects.push(LegacyEffect::Presentation {
                sequence: *presentation_sequence,
                command: "astra.emu.text_presentation.v1".into(),
                payload: presentation_payload,
            });
            effects.push(LegacyEffect::TextCapture {
                sequence: *capture_sequence,
                lease_id,
                text_hash: Hash256::from_sha256(text.as_bytes()),
                byte_len: text.len().try_into().map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_TEXT_CAPTURE_BOUNDS",
                        "message length cannot be represented by the ABI",
                    )
                })?,
                speaker_hash: speaker
                    .as_ref()
                    .map(|value| Hash256::from_sha256(value.as_bytes())),
                source_ref: "minori.sc.message".into(),
            });
        }
        if let Some(MinoriVmEvent::Stage(stage)) = &event {
            let stage_size = session.stage_size.ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_SIZE",
                    "stage presentation requires explicit host dimensions",
                )
            })?;
            let sequence = session.vm.state().effect_sequence;
            let frame = match describe_stage_frame(&vfs, &session.mount_set_id, stage, stage_size) {
                Ok(frame) => frame,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            let payload = postcard::to_allocvec(&frame).map_err(|_| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_ENCODE",
                    "stage render frame could not be encoded",
                )
            })?;
            effects.push(LegacyEffect::Presentation {
                sequence,
                command: "astra.emu.render_resource_frame.v1".into(),
                payload,
            });
        }
        if let Some(MinoriVmEvent::Effect(frame)) = &event {
            let effect = match effect_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                frame,
            ) {
                Ok(effect) => effect,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            effects.push(effect);
        }
        if let Some(MinoriVmEvent::Panel { sequence }) = &event {
            let panel = match panel_presentation(
                &vfs,
                &session.mount_set_id,
                session.stage_size,
                session.vm.state(),
                *sequence,
            ) {
                Ok(effect) => effect,
                Err(error) => {
                    session.poisoned = true;
                    return Err(error);
                }
            };
            effects.push(panel);
        }
        let mut audio_command_count = 0u64;
        if let Some(MinoriVmEvent::Audio { commands }) = &event {
            for command in commands {
                let (sequence, command) = map_audio_command(command);
                if let LegacyAudioCommandV1::LoadResource { resource_uri, .. } = &command {
                    let stat = match vfs.stat_file(&session.mount_set_id, resource_uri) {
                        Ok(stat) if stat.len > 0 && stat.len <= MAX_RESOURCE_BYTES => stat,
                        Ok(_) => {
                            session.poisoned = true;
                            return Err(invalid(
                                "ASTRA_EMU_MINORI_AUDIO_RESOURCE_BOUNDS",
                                "audio resource is empty or exceeds the session bound",
                            ));
                        }
                        Err(error) => {
                            session.poisoned = true;
                            return Err(error);
                        }
                    };
                    let _ = stat;
                }
                if let Err(error) = command.validate() {
                    session.poisoned = true;
                    return Err(error);
                }
                let payload = postcard::to_allocvec(&command).map_err(|_| {
                    invalid(
                        "ASTRA_EMU_MINORI_AUDIO_ENCODE",
                        "audio command could not be encoded",
                    )
                })?;
                effects.push(LegacyEffect::Audio {
                    sequence,
                    command: "astra.emu.audio_command.v1".into(),
                    payload,
                });
                audio_command_count += 1;
            }
        }
        let waits = match &event {
            Some(MinoriVmEvent::Wait(wait)) | Some(MinoriVmEvent::Message { wait, .. }) => {
                vec![legacy_wait(wait)]
            }
            _ => Vec::new(),
        };
        let status = match &event {
            Some(MinoriVmEvent::Wait(_)) | Some(MinoriVmEvent::Message { .. }) => {
                LegacyRuntimeStatus::Awaiting
            }
            Some(MinoriVmEvent::Chain { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Audio { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Stage(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Effect(_)) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Panel { .. }) => LegacyRuntimeStatus::Active,
            Some(MinoriVmEvent::Terminal) => LegacyRuntimeStatus::Terminal,
            None => LegacyRuntimeStatus::Active,
        };
        let trace = (after > before)
            .then(|| LegacyTraceEntry {
                sequence: after,
                context_id: 0,
                pc: session.vm.state().pc_line as u64,
                opcode: "minori.sc".into(),
                action: match &event {
                    Some(MinoriVmEvent::Chain { .. }) => Some("chain".into()),
                    Some(MinoriVmEvent::Audio { .. }) => Some("audio".into()),
                    Some(MinoriVmEvent::Stage(_)) => Some("stage".into()),
                    Some(MinoriVmEvent::Effect(_)) => Some("effect".into()),
                    Some(MinoriVmEvent::Panel { .. }) => Some("panel".into()),
                    _ => None,
                },
                yield_reason: waits.first().map(|_| "wait".into()),
            })
            .into_iter()
            .collect::<Vec<_>>();
        let output = LegacyStepOutput {
            status,
            effects,
            waits,
            trace,
            diagnostics: Vec::new(),
            coverage: LegacyCoverageDelta {
                instructions: after - before,
                contexts: vec![0],
                audio_commands: audio_command_count,
                ..LegacyCoverageDelta::default()
            },
            state_hash: session.vm.state_hash().map_err(runtime_error)?,
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
            .get(session_id.0.as_str())
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if session.poisoned {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SESSION_POISONED",
                "poisoned session cannot be saved",
            ));
        }
        let bytes = session.vm.snapshot_bytes().map_err(runtime_error)?;
        let envelope = LegacySnapshotEnvelope {
            family_id: FamilyId(MINORI_FAMILY_ID.into()),
            session_id: session_id.clone(),
            schema_version: SchemaVersion::new(6, 0, 0),
            case_fingerprint: session.case_fingerprint,
            fixed_step: session.vm.state().fixed_tick,
            session_seed: session.session_seed,
            runtime_cursor: session.vm.state().instruction_count,
            family_sections: vec![LegacySnapshotSection {
                section_id: "minori.runtime".into(),
                schema: MINORI_RUNTIME_STATE_SCHEMA.into(),
                version: SchemaVersion::new(6, 0, 0),
                hash: Hash256::from_sha256(&bytes),
                bytes,
            }],
            redaction_status: "passed".into(),
        };
        envelope.validate()?;
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
        let vfs = Arc::clone(self.vfs()?);
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if snapshot.family_id.0 != MINORI_FAMILY_ID
            || snapshot.session_id != *session_id
            || snapshot.case_fingerprint != session.case_fingerprint
            || snapshot.session_seed != session.session_seed
            || snapshot.family_sections.len() != 1
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SNAPSHOT_IDENTITY",
                "snapshot identity does not match the open session",
            ));
        }
        let section = &snapshot.family_sections[0];
        if section.section_id != "minori.runtime"
            || section.schema != MINORI_RUNTIME_STATE_SCHEMA
            || section.version != SchemaVersion::new(6, 0, 0)
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SNAPSHOT_SECTION",
                "snapshot runtime section identity is invalid",
            ));
        }
        let restored = MinoriVm::decode_snapshot(&section.bytes).map_err(runtime_error)?;
        validate_script_uri(&restored.script_uri)?;
        let bytes = vfs.read_file(&ctx.mount_set_id, &restored.script_uri, MAX_SCRIPT_BYTES)?;
        let script_hash = Hash256::from_sha256(&bytes);
        if script_hash != restored.script_hash {
            return Err(invalid(
                "ASTRA_EMU_MINORI_SNAPSHOT_SCRIPT_IDENTITY",
                "snapshot script hash does not match the mounted VFS",
            ));
        }
        let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
        session
            .vm
            .replace_script(restored.script_uri, script_hash, script)
            .and_then(|_| session.vm.restore_state(&section.bytes))
            .map_err(runtime_error)?;
        session.poisoned = false;
        Ok(LegacyRestoreReport {
            restored_fixed_step: session.vm.state().fixed_tick,
            session_seed: session.session_seed,
            state_hash: session.vm.state_hash().map_err(runtime_error)?,
            diagnostics: Vec::new(),
        })
    }

    fn take_ephemeral_text(
        &mut self,
        ctx: &LegacyRuntimeHostCtx,
        session_id: &LegacyRuntimeSessionId,
        lease_id: &str,
    ) -> Result<Option<astra_emu_family_api::LegacyEphemeralText>, LegacyProviderError> {
        ctx.validate()?;
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
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
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, session)?;
        if max_bytes == 0 || max_bytes > MAX_RESOURCE_BYTES || !resource_uri.starts_with("minori:/")
        {
            return Err(invalid(
                "ASTRA_EMU_MINORI_RESOURCE_BOUNDS",
                "resource request is outside the session VFS or byte budget",
            ));
        }
        self.vfs()?
            .read_file(&ctx.mount_set_id, resource_uri, max_bytes)
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
            .ok_or_else(session_missing)?;
        validate_session_binding(ctx, &session)?;
        Ok(LegacyShutdownReport {
            final_state_hash: session.vm.state_hash().map_err(runtime_error)?,
            instruction_count: session.vm.state().instruction_count,
            syscall_count: 0,
            diagnostics: Vec::new(),
        })
    }
}

fn map_audio_command(command: &MinoriAudioCommand) -> (u64, LegacyAudioCommandV1) {
    match command {
        MinoriAudioCommand::LoadResource {
            sequence,
            stream_id,
            resource_uri,
        } => (
            *sequence,
            LegacyAudioCommandV1::LoadResource {
                stream_id: *stream_id,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: resource_uri.clone(),
            },
        ),
        MinoriAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => (
            *sequence,
            LegacyAudioCommandV1::Play {
                stream_id: *stream_id,
                volume: *volume,
                pan: *pan,
                repeat: *repeat,
                fade_in_ms: *fade_in_ms,
            },
        ),
        MinoriAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        } => (
            *sequence,
            LegacyAudioCommandV1::Stop {
                stream_id: *stream_id,
                fade_ms: *fade_ms,
            },
        ),
        MinoriAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        } => (
            *sequence,
            LegacyAudioCommandV1::SetParams {
                stream_id: *stream_id,
                volume: *volume,
                pan: *pan,
                repeat: *repeat,
            },
        ),
    }
}

fn describe_stage_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage: &MinoriStageCommand,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    if !stage.stands.is_empty() {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
            "stand position semantics are not yet verified",
        ));
    }
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    if let Some(background) = &stage.background {
        append_stage_layer(
            vfs,
            mount_set_id,
            background,
            1,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(foreground) = &stage.foreground {
        append_stage_layer(
            vfs,
            mount_set_id,
            foreground,
            2,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn describe_effect_frame(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
    (width, height): (u32, u32),
) -> Result<LegacyRenderResourceFrameV1, LegacyProviderError> {
    let mut texture_resources = Vec::new();
    let mut draws = Vec::new();
    for (layer_id, layer) in &state.layers {
        if *layer_id >= 16 || layer.x_milli % 1000 != 0 || layer.y_milli % 1000 != 0 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_STAGE_STAND_POSITION",
                "effect composition encountered unverified stage positioning",
            ));
        }
        append_resource_layer(
            vfs,
            mount_set_id,
            &layer.resource_uri,
            layer.x_milli / 1000,
            layer.y_milli / 1000,
            f32::from(layer.opacity_milli) / 1000.0,
            layer_id.checked_add(1).ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_STAGE_LAYER_ID",
                    "stage layer id overflowed the render resource namespace",
                )
            })?,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    let alpha = f32::from(effect.alpha_255) / 255.0;
    if let Some(resource_uri) = &effect.current_resource_uri {
        append_resource_layer(
            vfs,
            mount_set_id,
            resource_uri,
            0,
            0,
            if effect.next_resource_uri.is_some() {
                1.0
            } else {
                1.0 - alpha
            },
            100,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(resource_uri) = &effect.next_resource_uri {
        append_resource_layer(
            vfs,
            mount_set_id,
            resource_uri,
            0,
            0,
            alpha,
            101,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    if let Some(panel) = &state.panel {
        if panel.mode != 1 {
            return Err(invalid(
                "ASTRA_EMU_MINORI_PANEL_MODE",
                "panel state contains an unverified mode",
            ));
        }
        append_panel_layer(
            vfs,
            mount_set_id,
            &panel.resource_uri,
            height,
            200,
            &mut texture_resources,
            &mut draws,
        )?;
    }
    let frame = LegacyRenderResourceFrameV1 {
        width,
        height,
        texture_resources,
        draws,
    };
    frame.validate()?;
    Ok(frame)
}

fn append_panel_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    stage_height: u32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    append_resource_layer(
        vfs,
        mount_set_id,
        resource_uri,
        0,
        0,
        1.0,
        texture_id,
        texture_resources,
        draws,
    )?;
    let image_height = texture_resources
        .last()
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_PANEL_RESOURCE",
                "panel texture metadata was not appended",
            )
        })?
        .decoded_height;
    let top = i64::from(stage_height)
        .checked_sub(i64::from(image_height))
        .and_then(|value| value.checked_add(64))
        .ok_or_else(|| {
            invalid(
                "ASTRA_EMU_MINORI_PANEL_POSITION",
                "panel position overflowed the verified coordinate range",
            )
        })?;
    let top = i32::try_from(top).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_PANEL_POSITION",
            "panel position cannot be represented by the render contract",
        )
    })? as f32;
    let draw = draws.last_mut().ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_PANEL_RESOURCE",
            "panel draw was not appended",
        )
    })?;
    for vertex in &mut draw.vertices {
        vertex.position[1] += top;
    }
    Ok(())
}

fn panel_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<LegacyEffect, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "panel presentation requires explicit host dimensions",
        )
    })?;
    let effect = visible_effect_frame(state, sequence)?;
    let frame = describe_effect_frame(vfs, mount_set_id, state, &effect, stage_size)?;
    let payload = postcard::to_allocvec(&frame).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_PANEL_ENCODE",
            "panel render frame could not be encoded",
        )
    })?;
    Ok(LegacyEffect::Presentation {
        sequence,
        command: "astra.emu.render_resource_frame.v1".into(),
        payload,
    })
}

fn visible_effect_frame(
    state: &MinoriRuntimeState,
    sequence: u64,
) -> Result<MinoriEffectFrame, LegacyProviderError> {
    let Some(effect) = &state.effect else {
        return Ok(MinoriEffectFrame {
            sequence,
            current_resource_uri: None,
            next_resource_uri: None,
            alpha_255: 0,
        });
    };
    let current = usize::try_from(effect.visible_current_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_EFFECT_STATE",
            "visible effect resource index cannot be represented",
        )
    })?;
    let next = usize::try_from(effect.visible_next_index).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_EFFECT_STATE",
            "visible effect resource index cannot be represented",
        )
    })?;
    Ok(MinoriEffectFrame {
        sequence,
        current_resource_uri: effect
            .resources
            .get(current)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_EFFECT_STATE",
                    "visible current effect resource is outside the sequence",
                )
            })?
            .clone(),
        next_resource_uri: effect
            .resources
            .get(next)
            .ok_or_else(|| {
                invalid(
                    "ASTRA_EMU_MINORI_EFFECT_STATE",
                    "visible next effect resource is outside the sequence",
                )
            })?
            .clone(),
        alpha_255: effect.visible_alpha_255,
    })
}

fn effect_presentation(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    stage_size: Option<(u32, u32)>,
    state: &MinoriRuntimeState,
    effect: &MinoriEffectFrame,
) -> Result<LegacyEffect, LegacyProviderError> {
    let stage_size = stage_size.ok_or_else(|| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_SIZE",
            "effect presentation requires explicit host dimensions",
        )
    })?;
    let frame = describe_effect_frame(vfs, mount_set_id, state, effect, stage_size)?;
    let payload = postcard::to_allocvec(&frame).map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_EFFECT_ENCODE",
            "effect render frame could not be encoded",
        )
    })?;
    Ok(LegacyEffect::Presentation {
        sequence: effect.sequence,
        command: "astra.emu.render_resource_frame.v1".into(),
        payload,
    })
}

fn append_stage_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    layer: &MinoriStageLayer,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    append_resource_layer(
        vfs,
        mount_set_id,
        &layer.resource_uri,
        layer.x,
        layer.y,
        1.0,
        texture_id,
        texture_resources,
        draws,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_resource_layer(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    resource_uri: &str,
    x: i32,
    y: i32,
    opacity: f32,
    texture_id: u32,
    texture_resources: &mut Vec<LegacyTextureResourceV1>,
    draws: &mut Vec<LegacyDrawV1>,
) -> Result<(), LegacyProviderError> {
    if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
        return Err(invalid(
            "ASTRA_EMU_MINORI_EFFECT_ALPHA",
            "effect alpha is outside the normalized bound",
        ));
    }
    let bytes = vfs.read_file(mount_set_id, resource_uri, MAX_RESOURCE_BYTES)?;
    let image_reader = image::ImageReader::new(Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|_| {
            invalid(
                "ASTRA_EMU_MINORI_STAGE_IMAGE_FORMAT",
                "stage image format could not be determined",
            )
        })?;
    let (image_width, image_height) = image_reader.into_dimensions().map_err(|_| {
        invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_METADATA",
            "stage image dimensions could not be read safely",
        )
    })?;
    if image_width == 0 || image_height == 0 || image_width > 16_384 || image_height > 16_384 {
        return Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_BOUNDS",
            "stage image dimensions are outside the supported bound",
        ));
    }
    let codec = image_codec(resource_uri)?;
    texture_resources.push(LegacyTextureResourceV1 {
        texture_id,
        resource_uri: resource_uri.to_owned(),
        codec: codec.into(),
        encoded_hash: Hash256::from_sha256(&bytes),
        decoded_width: image_width,
        decoded_height: image_height,
        decoded_format: LegacyTextureFormat::Rgba8,
    });
    let left = x as f32;
    let top = y as f32;
    let right = left + image_width as f32;
    let bottom = top + image_height as f32;
    let vertex = |x, y, u, v| LegacyVertexV1 {
        position: [x, y],
        tex_coord: [u, v],
        color: [1.0, 1.0, 1.0, opacity],
    };
    draws.push(LegacyDrawV1 {
        texture_id,
        vertices: [
            vertex(left, top, 0.0, 0.0),
            vertex(right, top, 1.0, 0.0),
            vertex(left, bottom, 0.0, 1.0),
            vertex(right, bottom, 1.0, 1.0),
        ],
        blend: LegacyBlendMode::Alpha,
        scissor: None,
    });
    Ok(())
}

fn image_codec(resource_uri: &str) -> Result<&'static str, LegacyProviderError> {
    let extension = resource_uri
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("png") {
        Ok("png")
    } else if extension.eq_ignore_ascii_case("bmp") {
        Ok("bmp")
    } else if extension.eq_ignore_ascii_case("jpg") {
        Ok("jpg")
    } else if extension.eq_ignore_ascii_case("jpeg") {
        Ok("jpeg")
    } else if extension.eq_ignore_ascii_case("webp") {
        Ok("webp")
    } else {
        Err(invalid(
            "ASTRA_EMU_MINORI_STAGE_IMAGE_CODEC",
            "stage image extension has no explicitly bound decode codec",
        ))
    }
}

fn load_script(
    vfs: &Arc<dyn LegacyVfsReader>,
    mount_set_id: &str,
    target: &str,
) -> Result<(String, Hash256, crate::ScScript), LegacyProviderError> {
    let script_uri = format!("minori:/scr/{target}");
    validate_script_uri(&script_uri)?;
    let bytes = vfs.read_file(mount_set_id, &script_uri, MAX_SCRIPT_BYTES)?;
    let script_hash = Hash256::from_sha256(&bytes);
    let script = parse_sc(&bytes, &ScOpcodeCatalog::observed_minori()).map_err(script_error)?;
    Ok((script_uri, script_hash, script))
}

fn validate_script_uri(script_uri: &str) -> Result<(), LegacyProviderError> {
    let Some(target) = script_uri.strip_prefix("minori:/scr/") else {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCRIPT_URI",
            "script URI is outside the Minori script mount",
        ));
    };
    if target.is_empty()
        || target.len() > 256
        || !target.to_ascii_lowercase().ends_with(".sc")
        || target.contains('/')
        || target.contains('\\')
        || target.contains("..")
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid(
            "ASTRA_EMU_MINORI_SCRIPT_URI",
            "script URI contains an invalid direct-entry name",
        ));
    }
    Ok(())
}

fn waiting_output(
    vm: &MinoriVm,
    _wait: MinoriWaitState,
    effects: Vec<LegacyEffect>,
    input: &LegacyStepInput,
) -> Result<LegacyStepOutput, LegacyProviderError> {
    let output = LegacyStepOutput {
        status: LegacyRuntimeStatus::Awaiting,
        effects,
        // A wait request is edge-triggered: it is published only by the
        // command that creates the token. Re-emitting the same pending token
        // on later ticks would violate RuntimeWorld AwaitQueue uniqueness.
        waits: Vec::new(),
        trace: Vec::new(),
        diagnostics: Vec::new(),
        coverage: LegacyCoverageDelta::default(),
        state_hash: vm.state_hash().map_err(runtime_error)?,
    };
    output.validate(&input.budget)?;
    Ok(output)
}

fn legacy_wait(wait: &MinoriWaitState) -> LegacyWaitRequest {
    match wait {
        MinoriWaitState::Time {
            token_id,
            timer_ticks: _,
            milliseconds,
        } => LegacyWaitRequest::Time {
            token_id: token_id.clone(),
            milliseconds: *milliseconds,
        },
        MinoriWaitState::Input { token_id } => LegacyWaitRequest::Input {
            token_id: token_id.clone(),
            mask: MESSAGE_INPUT_MASK,
        },
        MinoriWaitState::Media { token_id, media_id } => LegacyWaitRequest::MediaFence {
            token_id: token_id.clone(),
            media_id: media_id.clone(),
        },
        MinoriWaitState::Presentation { token_id, fence_id } => {
            LegacyWaitRequest::PresentationFence {
                token_id: token_id.clone(),
                fence_id: fence_id.clone(),
            }
        }
        MinoriWaitState::Provider {
            token_id,
            request_id,
        } => LegacyWaitRequest::ProviderCompletion {
            token_id: token_id.clone(),
            request_id: request_id.clone(),
        },
    }
}

fn wait_token(wait: &MinoriWaitState) -> &str {
    match wait {
        MinoriWaitState::Time { token_id, .. }
        | MinoriWaitState::Input { token_id }
        | MinoriWaitState::Media { token_id, .. }
        | MinoriWaitState::Presentation { token_id, .. }
        | MinoriWaitState::Provider { token_id, .. } => token_id,
    }
}

fn validate_session_binding(
    ctx: &LegacyRuntimeHostCtx,
    session: &MinoriSession,
) -> Result<(), LegacyProviderError> {
    if ctx.mount_set_id != session.mount_set_id {
        return Err(invalid(
            "ASTRA_EMU_MINORI_MOUNT_BINDING",
            "host mount does not match the open session",
        ));
    }
    Ok(())
}

fn script_error(_error: crate::ScParseError) -> LegacyProviderError {
    invalid(
        "ASTRA_EMU_MINORI_SCRIPT_PARSE",
        "Minori script failed strict parsing",
    )
}

fn runtime_error(error: MinoriRuntimeError) -> LegacyProviderError {
    let code = match &error {
        MinoriRuntimeError::State => "ASTRA_EMU_MINORI_RUNTIME_STATE",
        MinoriRuntimeError::ProgramCounter => "ASTRA_EMU_MINORI_RUNTIME_PC",
        MinoriRuntimeError::Label => "ASTRA_EMU_MINORI_RUNTIME_LABEL",
        MinoriRuntimeError::Operand => "ASTRA_EMU_MINORI_RUNTIME_OPERAND",
        MinoriRuntimeError::UnsupportedOpcode { .. } => "ASTRA_EMU_MINORI_RUNTIME_OPCODE",
        MinoriRuntimeError::Budget => "ASTRA_EMU_MINORI_RUNTIME_BUDGET",
        MinoriRuntimeError::Waiting => "ASTRA_EMU_MINORI_RUNTIME_WAIT",
        MinoriRuntimeError::Overflow => "ASTRA_EMU_MINORI_RUNTIME_OVERFLOW",
        MinoriRuntimeError::Snapshot => "ASTRA_EMU_MINORI_RUNTIME_SNAPSHOT",
        MinoriRuntimeError::ChainTarget => "ASTRA_EMU_MINORI_RUNTIME_CHAIN",
        MinoriRuntimeError::AudioResource => "ASTRA_EMU_MINORI_RUNTIME_AUDIO_RESOURCE",
        MinoriRuntimeError::Effect => "ASTRA_EMU_MINORI_RUNTIME_EFFECT",
        MinoriRuntimeError::Panel => "ASTRA_EMU_MINORI_RUNTIME_PANEL",
    };
    LegacyProviderError::invalid(code, error.to_string())
}

fn session_missing() -> LegacyProviderError {
    invalid("ASTRA_EMU_MINORI_SESSION_MISSING", "session is not active")
}

fn invalid(code: &'static str, message: &'static str) -> LegacyProviderError {
    LegacyProviderError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use astra_byte_source::{ByteRange, ByteSourceStat, RangeReadResult, SourceRevision};
    use astra_emu_family_api::{LegacyAwaitResult, LegacyReplayMode, LegacyStepBudget};
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};

    use super::*;

    struct MemoryReader {
        scripts: BTreeMap<String, Vec<u8>>,
    }

    impl LegacyVfsReader for MemoryReader {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<ByteSourceStat, LegacyProviderError> {
            if mount_set_id != "mount.test" {
                return Err(invalid("TEST_VFS_NOT_FOUND", "fixture entry is missing"));
            }
            let script = self
                .scripts
                .get(uri)
                .ok_or_else(|| invalid("TEST_VFS_NOT_FOUND", "fixture entry is missing"))?;
            Ok(ByteSourceStat {
                len: script.len() as u64,
                revision: SourceRevision(Hash256::from_sha256(script)),
            })
        }

        fn read_file_range(
            &self,
            mount_set_id: &str,
            uri: &str,
            expected_revision: SourceRevision,
            range: ByteRange,
            max_bytes: u64,
        ) -> Result<RangeReadResult, LegacyProviderError> {
            let stat = self.stat_file(mount_set_id, uri)?;
            range
                .validate(stat.len, max_bytes)
                .map_err(|_| invalid("TEST_VFS_BOUNDS", "fixture range is invalid"))?;
            if expected_revision != stat.revision {
                return Err(invalid("TEST_VFS_REVISION", "fixture revision changed"));
            }
            let script = self
                .scripts
                .get(uri)
                .ok_or_else(|| invalid("TEST_VFS_NOT_FOUND", "fixture entry is missing"))?;
            let bytes = script[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(RangeReadResult {
                range,
                revision: stat.revision,
                content_hash: Hash256::from_sha256(&bytes),
                bytes,
            })
        }
    }

    #[test]
    fn provider_lifecycle_wait_snapshot_restore_and_shutdown() {
        let script = b".setglobal route = 1\r\n.wait 20\r\n.end\r\n".to_vec();
        let case_fingerprint = Hash256::from_sha256(b"case");
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.test".into()),
                    case_fingerprint,
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let first = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(first.status, LegacyRuntimeStatus::Awaiting);
        let token = match &first.waits[0] {
            LegacyWaitRequest::Time {
                token_id,
                milliseconds,
            } => {
                assert_eq!(*milliseconds, 200);
                token_id.clone()
            }
            _ => panic!("expected time wait"),
        };
        let snapshot = provider.save(&ctx, &session).unwrap();
        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        provider.restore(&ctx, &session, &snapshot).unwrap();
        let completed = provider
            .step(
                &ctx,
                &session,
                step_input(
                    2,
                    vec![LegacyAwaitResult {
                        token_id: token,
                        status: "completed".into(),
                        payload_hash: Hash256::from_sha256(b"completed"),
                        sequence: 1,
                    }],
                ),
            )
            .unwrap();
        assert_eq!(completed.status, LegacyRuntimeStatus::Terminal);
        let shutdown = provider.shutdown(&ctx, &session).unwrap();
        assert_eq!(shutdown.instruction_count, 3);
        assert!(!provider.has_active_sessions());
    }

    #[test]
    fn provider_tail_chains_and_restores_the_active_script_identity() {
        let entry = b".set local = 1\r\n.chain K01.sc\r\n".to_vec();
        let next = b".wait 20\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), entry),
                ("minori:/scr/K01.sc".into(), next),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.chain".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let chained = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(chained.status, LegacyRuntimeStatus::Active);
        assert_eq!(chained.trace[0].action.as_deref(), Some("chain"));

        let waiting = provider
            .step(&ctx, &session, step_input(2, Vec::new()))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        let snapshot = provider.save(&ctx, &session).unwrap();
        provider.restore(&ctx, &session, &snapshot).unwrap();
        assert_eq!(
            snapshot.family_sections[0].version,
            SchemaVersion::new(6, 0, 0)
        );
    }

    #[test]
    fn provider_exposes_message_plaintext_only_through_a_one_shot_lease() {
        let script = b".message 42 voice speaker hello world\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.message".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Awaiting);
        let LegacyEffect::Presentation {
            sequence,
            command,
            payload,
        } = &output.effects[0]
        else {
            panic!("expected text presentation effect")
        };
        assert_eq!(*sequence, 1);
        assert_eq!(command, "astra.emu.text_presentation.v1");
        let presentation: LegacyTextPresentationLeaseV1 =
            postcard::from_bytes(payload).expect("presentation payload must decode");
        let LegacyEffect::TextCapture {
            sequence,
            lease_id,
            text_hash,
            speaker_hash,
            ..
        } = &output.effects[1]
        else {
            panic!("expected text capture effect")
        };
        assert_eq!(*sequence, 2);
        assert_eq!(presentation.lease_id, *lease_id);
        assert_eq!(*text_hash, Hash256::from_sha256(b"hello world"));
        assert_eq!(*speaker_hash, Some(Hash256::from_sha256(b"speaker")));
        let presentation = &presentation.presentation;
        assert_eq!(presentation.layout_id, "minori.message");
        assert_eq!(presentation.language, "ja-JP");
        assert_eq!(presentation.font_families, ["Noto Sans JP"]);
        assert_eq!(presentation.body.font_size, 26.0);
        assert_eq!(presentation.body.max_lines, 3);
        let text = provider
            .take_ephemeral_text(&ctx, &session, lease_id)
            .unwrap()
            .unwrap();
        assert_eq!(text.text, "hello world");
        assert_eq!(text.speaker.as_deref(), Some("speaker"));
        assert!(provider
            .take_ephemeral_text(&ctx, &session, lease_id)
            .unwrap()
            .is_none());
        assert!(matches!(
            output.waits.as_slice(),
            [LegacyWaitRequest::Input { mask, .. }] if *mask == MESSAGE_INPUT_MASK
        ));
    }

    #[test]
    fn provider_blocks_message_without_the_verified_reference_stage() {
        let script = b".message 42 voice speaker body\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([("minori:/scr/test.sc".into(), script)]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId(
                        "session.message.invalid-stage".into(),
                    ),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        assert_eq!(
            provider
                .step(&ctx, &session, step_input(1, Vec::new()))
                .unwrap_err()
                .code(),
            "ASTRA_EMU_MINORI_TEXT_STAGE_IDENTITY"
        );
    }

    #[test]
    fn provider_validates_and_emits_bgm_through_the_shared_audio_contract() {
        let script = b".playBGM theme.ogg * * 80\r\n.end\r\n".to_vec();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bgm/theme.ogg".into(), b"OggSfixture".to_vec()),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.bgm".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::new(),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        assert_eq!(output.status, LegacyRuntimeStatus::Active);
        assert_eq!(output.coverage.audio_commands, 2);
        assert_eq!(output.effects.len(), 2);
        let commands = output
            .effects
            .iter()
            .map(|effect| match effect {
                LegacyEffect::Audio { payload, .. } => {
                    postcard::from_bytes::<LegacyAudioCommandV1>(payload).unwrap()
                }
                _ => panic!("expected audio command"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            commands[0],
            LegacyAudioCommandV1::LoadResource {
                stream_id: 0,
                encoding: LegacyAudioEncoding::Ogg,
                resource_uri: "minori:/bgm/theme.ogg".into(),
            }
        );
        assert_eq!(
            commands[1],
            LegacyAudioCommandV1::Play {
                stream_id: 0,
                volume: 0.8,
                pan: 0.0,
                repeat: true,
                fade_in_ms: 2,
            }
        );
    }

    #[test]
    fn provider_emits_a_resource_bound_stage_frame_without_decoded_pixels() {
        let script = b".transition 0 * 10\r\n.stage * BLACK.png 0 0\r\n.end\r\n".to_vec();
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&[0, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/BLACK.png".into(), png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.stage".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 16_666_667,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let output = provider
            .step(&ctx, &session, step_input(1, Vec::new()))
            .unwrap();
        let LegacyEffect::Presentation {
            command, payload, ..
        } = &output.effects[0]
        else {
            panic!("expected presentation effect")
        };
        assert_eq!(command, "astra.emu.render_resource_frame.v1");
        let frame: LegacyRenderResourceFrameV1 = postcard::from_bytes(payload).unwrap();
        assert_eq!((frame.width, frame.height), (1280, 720));
        assert_eq!(frame.texture_resources.len(), 1);
        assert_eq!(frame.draws.len(), 1);
        assert_eq!(
            frame.texture_resources[0].resource_uri,
            "minori:/bg/BLACK.png"
        );
        assert_eq!(frame.texture_resources[0].decoded_width, 1);
        assert_eq!(frame.texture_resources[0].decoded_height, 1);
    }

    #[test]
    fn provider_emits_crossfade2_frames_from_vfs_resources_while_waiting() {
        let script =
            b".effect CrossFade2 first.png:second.png:*:* 320 100\r\n.wait 20\r\n.end\r\n".to_vec();
        let mut first = Vec::new();
        PngEncoder::new(&mut first)
            .write_image(&[255, 0, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut second = Vec::new();
        PngEncoder::new(&mut second)
            .write_image(&[0, 255, 0, 255], 1, 1, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/first.png".into(), first),
                ("minori:/bg/second.png".into(), second),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.effect".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 20_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        let created = provider
            .step(&ctx, &session, step_input_with_delta(1, 20_000_000))
            .unwrap();
        let LegacyEffect::Presentation { payload, .. } = &created.effects[0] else {
            panic!("expected initial effect frame")
        };
        let initial: LegacyRenderResourceFrameV1 = postcard::from_bytes(payload).unwrap();
        assert_eq!(initial.texture_resources.len(), 2);
        assert_eq!(initial.draws[1].vertices[0].color[3], 0.0);

        let waiting = provider
            .step(&ctx, &session, step_input_with_delta(2, 20_000_000))
            .unwrap();
        assert_eq!(waiting.status, LegacyRuntimeStatus::Awaiting);
        assert_eq!(waiting.waits.len(), 1);
        for tick in 3..6 {
            let unchanged = provider
                .step(&ctx, &session, step_input_with_delta(tick, 20_000_000))
                .unwrap();
            assert!(unchanged.effects.is_empty());
            assert!(unchanged.waits.is_empty());
        }
        let advanced = provider
            .step(&ctx, &session, step_input_with_delta(6, 20_000_000))
            .unwrap();
        let LegacyEffect::Presentation { payload, .. } = &advanced.effects[0] else {
            panic!("expected advanced effect frame")
        };
        let frame: LegacyRenderResourceFrameV1 = postcard::from_bytes(payload).unwrap();
        assert_eq!(frame.texture_resources.len(), 1);
        assert_eq!(
            frame.texture_resources[0].resource_uri,
            "minori:/bg/second.png"
        );
    }

    #[test]
    fn provider_composes_verified_message_panel_over_the_visible_effect_frame() {
        let script =
            b".effect CrossFade2 first.png:second.png 320 100\r\n.panel 1\r\n.end\r\n".to_vec();
        let encode = |rgba: [u8; 4]| {
            let mut png = Vec::new();
            PngEncoder::new(&mut png)
                .write_image(&rgba, 1, 1, ExtendedColorType::Rgba8)
                .unwrap();
            png
        };
        let mut panel_png = Vec::new();
        PngEncoder::new(&mut panel_png)
            .write_image(&vec![255; 4 * 263], 1, 263, ExtendedColorType::Rgba8)
            .unwrap();
        let mut provider = MinoriRuntimeProvider::with_vfs(Arc::new(MemoryReader {
            scripts: BTreeMap::from([
                ("minori:/scr/test.sc".into(), script),
                ("minori:/bg/first.png".into(), encode([255, 0, 0, 255])),
                ("minori:/bg/second.png".into(), encode([0, 255, 0, 255])),
                ("minori:/sys/msgPanel.png".into(), panel_png),
            ]),
        }));
        let ctx = context();
        let session = provider
            .open(
                &ctx,
                LegacyOpenRequest {
                    requested_session_id: LegacyRuntimeSessionId("session.panel".into()),
                    case_fingerprint: Hash256::from_sha256(b"case"),
                    script_uri: "minori:/scr/test.sc".into(),
                    fixed_delta_ns: 20_000_000,
                    session_seed: 7,
                    compatibility_profile: "minori.reference".into(),
                    family_options: BTreeMap::from([
                        ("astra.stage_width".into(), "1280".into()),
                        ("astra.stage_height".into(), "720".into()),
                    ]),
                },
            )
            .unwrap();
        provider
            .step(&ctx, &session, step_input_with_delta(1, 20_000_000))
            .unwrap();
        let panel = provider
            .step(&ctx, &session, step_input_with_delta(2, 20_000_000))
            .unwrap();
        let LegacyEffect::Presentation { payload, .. } = &panel.effects[0] else {
            panic!("expected panel presentation")
        };
        let frame: LegacyRenderResourceFrameV1 = postcard::from_bytes(payload).unwrap();
        assert_eq!(frame.texture_resources.len(), 3);
        assert_eq!(frame.draws.len(), 3);
        assert_eq!(frame.draws[1].vertices[0].color[3], 0.0);
        assert_eq!(
            frame.texture_resources[2].resource_uri,
            "minori:/sys/msgPanel.png"
        );
        assert_eq!(frame.texture_resources[2].texture_id, 200);
        assert_eq!(frame.texture_resources[2].decoded_height, 263);
        assert_eq!(frame.draws[2].vertices[0].position[1], 521.0);
        assert_eq!(frame.draws[2].vertices[2].position[1], 784.0);
    }

    fn context() -> LegacyRuntimeHostCtx {
        LegacyRuntimeHostCtx {
            case_id: "case.test".into(),
            package_id: "package.test".into(),
            package_hash: Hash256::from_sha256(b"package"),
            mount_set_id: "mount.test".into(),
            media_service_ids: vec!["media.test".into()],
            permission_policy_id: "policy.test".into(),
            report_sink_id: "report.test".into(),
            target: "headless".into(),
            profile: "test".into(),
        }
    }

    fn step_input(tick_index: u64, await_results: Vec<LegacyAwaitResult>) -> LegacyStepInput {
        step_input_with_delta_and_await(tick_index, 16_666_667, await_results)
    }

    fn step_input_with_delta(tick_index: u64, delta_ns: u64) -> LegacyStepInput {
        step_input_with_delta_and_await(tick_index, delta_ns, Vec::new())
    }

    fn step_input_with_delta_and_await(
        tick_index: u64,
        delta_ns: u64,
        await_results: Vec<LegacyAwaitResult>,
    ) -> LegacyStepInput {
        LegacyStepInput {
            tick_index,
            delta_ns,
            session_seed: 7,
            mode: LegacyReplayMode::Live,
            input_edges: Vec::new(),
            await_results,
            provider_results: Vec::new(),
            budget: LegacyStepBudget {
                max_instructions: 64,
                max_effects: 64,
                max_trace_entries: 64,
            },
        }
    }
}
