use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use astra_core::{Diagnostic, Hash256, StableId};
use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV7, LegacyAudioSampleFormat,
    LegacyAwaitResult, LegacyBlackboardMutation, LegacyControlTransaction, LegacyDiagnostic,
    LegacyEvent, LegacyInputEdge, LegacyLiveOutput, LegacyOpenRequest, LegacyPcmBufferV7,
    LegacyProbeReport, LegacyProbeRequest, LegacyProviderResult, LegacyReplayMode,
    LegacyRuntimeHostCtx, LegacyRuntimeProvider, LegacyRuntimeSessionId, LegacyShutdownReport,
    LegacyStepInput, LegacyVideoCommandV1, LegacyVideoMode, LegacyWaitRequest,
};
use astra_plugin::{ProductRuntimeProvider, ProductRuntimeProviderFactory, ProductRuntimeSession};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ProviderInstanceId, RuntimeLiveAudioCommand,
    RuntimeLiveAudioEncoding, RuntimeLiveAudioPacket, RuntimeLiveAudioSampleFormat,
    RuntimeLiveBlackboardMutation, RuntimeLiveCoverage, RuntimeLiveDirtySection, RuntimeLiveEvent,
    RuntimeLiveOutput, RuntimeLivePcmBuffer, RuntimeLiveVideoCommand, RuntimeLiveVideoCommandKind,
    RuntimeLiveVideoMode, RuntimeLiveWait, RuntimeLiveWaitKind, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimePrepareReport, RuntimePrepareRequest, RuntimeProbeReport,
    RuntimeProbeRequest, RuntimeProviderInstanceReport, RuntimeRestoreReport,
    RuntimeRestoreRequest, RuntimeSaveRequest, RuntimeSaveSections, RuntimeSectionPayload,
    RuntimeShutdownReport, RuntimeStepInput, RuntimeStepMode, RuntimeStepOutput,
    RuntimeTickIntegrityMode,
};
use astra_runtime::{
    ActionAccess, ActionDescriptor, ActionExecutionClass, ActionInvocation, ActionResourceKey,
    ActionTrace, AwaitResult, AwaitTokenId, BlackboardValue, DeterministicActionContext,
    EventPayload, GuardExpr, OrderedTickIngress, PackageHandle, PlayerInput, PresentationCommand,
    RuntimeAction, RuntimeConfig, RuntimeError, RuntimeWorld, StateDefinition,
    StateMachineDefinition, TickIngress, TickInput, TickIntegrityMode, TickRequest,
    TransitionDefinition,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub fn evidence_terminal_hash(
    family_id: &str,
    seed: u64,
    fixed_step: u64,
    state_revision: u64,
) -> Hash256 {
    Hash256::from_sha256(
        format!("{family_id}\0terminal\0{fixed_step}\0{state_revision}\0{seed}").as_bytes(),
    )
}

pub fn evidence_vm_coverage_ids(
    family_id: &str,
    trace: &[astra_emu_family_api::LegacyVmTraceRecord],
) -> Vec<String> {
    trace
        .iter()
        .map(|record| {
            format!(
                "{}.vm.c{}.pc{:08x}.op{:02x}",
                family_id, record.context_id, record.program_counter, record.opcode
            )
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn evidence_vm_coverage_hash(ids: &[String]) -> Hash256 {
    Hash256::from_sha256(format!("{}\n", ids.join("\n")).as_bytes())
}

fn move_layer_damage(
    damage: astra_emu_family_api::LegacySurfaceDamageV9,
) -> astra_plugin_abi::RuntimeLiveSurfaceDamage {
    match damage {
        astra_emu_family_api::LegacySurfaceDamageV9::Unchanged => {
            astra_plugin_abi::RuntimeLiveSurfaceDamage::Unchanged
        }
        astra_emu_family_api::LegacySurfaceDamageV9::Full => {
            astra_plugin_abi::RuntimeLiveSurfaceDamage::Full
        }
        astra_emu_family_api::LegacySurfaceDamageV9::Rects(rects) => {
            astra_plugin_abi::RuntimeLiveSurfaceDamage::Rects(
                rects
                    .into_iter()
                    .map(|rect| astra_plugin_abi::RuntimeLiveDamageRect {
                        x: rect.x,
                        y: rect.y,
                        width: rect.width,
                        height: rect.height,
                    })
                    .collect(),
            )
        }
    }
}

fn move_layer_state(
    layer: astra_emu_family_api::LegacyLayerStateV9,
) -> astra_plugin_abi::RuntimeLiveLayerState {
    astra_plugin_abi::RuntimeLiveLayerState {
        layer_id: layer.layer_id,
        role: layer.role,
        z_index: layer.z_index,
        surface_id: layer.surface_id,
        generation: layer.generation,
        width: layer.width,
        height: layer.height,
        stride: layer.stride,
        format: match layer.format {
            astra_emu_family_api::LegacySurfaceFormatV9::Rgba8SrgbPremultiplied => {
                astra_plugin_abi::RuntimeLiveSurfaceFormat::Rgba8SrgbPremultiplied
            }
            astra_emu_family_api::LegacySurfaceFormatV9::Bgra8SrgbPremultiplied => {
                astra_plugin_abi::RuntimeLiveSurfaceFormat::Bgra8SrgbPremultiplied
            }
        },
        damage: move_layer_damage(layer.damage),
        transform: astra_plugin_abi::RuntimeLiveLayerTransform {
            m11: layer.transform.m11,
            m12: layer.transform.m12,
            m21: layer.transform.m21,
            m22: layer.transform.m22,
            tx: layer.transform.tx,
            ty: layer.transform.ty,
        },
        clip: layer
            .clip
            .map(|rect| astra_plugin_abi::RuntimeLiveDamageRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            }),
        opacity: layer.opacity,
        texture_filter: match layer.texture_filter {
            astra_emu_family_api::LegacyLayerFilterV9::Nearest => {
                astra_plugin_abi::RuntimeLiveLayerFilter::Nearest
            }
            astra_emu_family_api::LegacyLayerFilterV9::Linear => {
                astra_plugin_abi::RuntimeLiveLayerFilter::Linear
            }
        },
        blend: match layer.blend {
            astra_emu_family_api::LegacyLayerBlendV9::Opaque => {
                astra_plugin_abi::RuntimeLiveLayerBlend::Opaque
            }
            astra_emu_family_api::LegacyLayerBlendV9::Alpha => {
                astra_plugin_abi::RuntimeLiveLayerBlend::Alpha
            }
            astra_emu_family_api::LegacyLayerBlendV9::Add => {
                astra_plugin_abi::RuntimeLiveLayerBlend::Add
            }
            astra_emu_family_api::LegacyLayerBlendV9::Multiply => {
                astra_plugin_abi::RuntimeLiveLayerBlend::Multiply
            }
            astra_emu_family_api::LegacyLayerBlendV9::Screen => {
                astra_plugin_abi::RuntimeLiveLayerBlend::Screen
            }
        },
        filter_graph_binding: layer.filter_graph_binding,
    }
}

fn move_layer_transaction(
    transaction: astra_emu_family_api::LegacyLayerTransactionV9,
) -> astra_plugin_abi::RuntimeLiveLayerTransaction {
    astra_plugin_abi::RuntimeLiveLayerTransaction {
        sequence: transaction.sequence,
        viewport_width: transaction.viewport_width,
        viewport_height: transaction.viewport_height,
        operations: transaction
            .operations
            .into_iter()
            .map(|operation| match operation {
                astra_emu_family_api::LegacyLayerOperationV9::Create(layer) => {
                    astra_plugin_abi::RuntimeLiveLayerOperation::Create(move_layer_state(layer))
                }
                astra_emu_family_api::LegacyLayerOperationV9::Update(layer) => {
                    astra_plugin_abi::RuntimeLiveLayerOperation::Update(move_layer_state(layer))
                }
                astra_emu_family_api::LegacyLayerOperationV9::Destroy { layer_id } => {
                    astra_plugin_abi::RuntimeLiveLayerOperation::Destroy { layer_id }
                }
            })
            .collect(),
    }
}

fn move_live_audio(packet: LegacyAudioPacketV7) -> Result<RuntimeLiveAudioPacket, String> {
    packet.validate().map_err(|error| error.to_string())?;
    let pcm = match packet.pcm {
        LegacyPcmBufferV7::I16(samples) => RuntimeLivePcmBuffer::I16(samples),
        LegacyPcmBufferV7::F32(samples) => RuntimeLivePcmBuffer::F32(samples),
    };
    if pcm.is_empty() {
        return Err("ASTRA_EMU_LIVE_PCM_EMPTY".into());
    }
    Ok(RuntimeLiveAudioPacket {
        sequence: packet.sequence,
        stream_id: packet.stream_id,
        sample_rate: packet.sample_rate,
        channels: packet.channels,
        pcm,
    })
}

fn move_live_audio_command(
    sequence: u64,
    command: LegacyAudioCommandV1,
) -> Result<RuntimeLiveAudioCommand, String> {
    command.validate().map_err(|error| error.to_string())?;
    Ok(match command {
        LegacyAudioCommandV1::LoadResource {
            stream_id,
            encoding,
            resource_uri,
        } => RuntimeLiveAudioCommand::LoadResource {
            sequence,
            stream_id,
            encoding: match encoding {
                LegacyAudioEncoding::Unknown => RuntimeLiveAudioEncoding::Unknown,
                LegacyAudioEncoding::Wav => RuntimeLiveAudioEncoding::Wav,
                LegacyAudioEncoding::Ogg => RuntimeLiveAudioEncoding::Ogg,
                LegacyAudioEncoding::Mp3 => RuntimeLiveAudioEncoding::Mp3,
                LegacyAudioEncoding::Flac => RuntimeLiveAudioEncoding::Flac,
            },
            resource_uri,
        },
        LegacyAudioCommandV1::CreateStream {
            stream_id,
            sample_rate,
            channels,
            sample_format,
        } => RuntimeLiveAudioCommand::CreateStream {
            sequence,
            stream_id,
            sample_rate,
            channels,
            sample_format: match sample_format {
                LegacyAudioSampleFormat::I16 => RuntimeLiveAudioSampleFormat::I16,
                LegacyAudioSampleFormat::F32 => RuntimeLiveAudioSampleFormat::F32,
            },
        },
        LegacyAudioCommandV1::SubmitI16 { stream_id, samples } => {
            RuntimeLiveAudioCommand::SubmitI16 {
                sequence,
                stream_id,
                samples,
            }
        }
        LegacyAudioCommandV1::SubmitF32 { stream_id, samples } => {
            RuntimeLiveAudioCommand::SubmitF32 {
                sequence,
                stream_id,
                samples,
            }
        }
        LegacyAudioCommandV1::Play {
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        } => RuntimeLiveAudioCommand::Play {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
            fade_in_ms,
        },
        LegacyAudioCommandV1::Stop { stream_id, fade_ms } => RuntimeLiveAudioCommand::Stop {
            sequence,
            stream_id,
            fade_ms,
        },
        LegacyAudioCommandV1::Pause { stream_id } => RuntimeLiveAudioCommand::Pause {
            sequence,
            stream_id,
        },
        LegacyAudioCommandV1::Resume { stream_id } => RuntimeLiveAudioCommand::Resume {
            sequence,
            stream_id,
        },
        LegacyAudioCommandV1::SetParams {
            stream_id,
            volume,
            pan,
            repeat,
        } => RuntimeLiveAudioCommand::SetParams {
            sequence,
            stream_id,
            volume,
            pan,
            repeat,
        },
        LegacyAudioCommandV1::DestroyStream { stream_id } => {
            RuntimeLiveAudioCommand::DestroyStream {
                sequence,
                stream_id,
            }
        }
        LegacyAudioCommandV1::MasterVolume { volume } => {
            RuntimeLiveAudioCommand::MasterVolume { sequence, volume }
        }
    })
}

fn move_live_video(
    sequence: u64,
    command: LegacyVideoCommandV1,
) -> Result<RuntimeLiveVideoCommand, String> {
    command.validate().map_err(|error| error.to_string())?;
    let command = match command {
        LegacyVideoCommandV1::Play {
            playback_id,
            resource_uri,
            mode,
            stage_width,
            stage_height,
        } => RuntimeLiveVideoCommandKind::Play {
            playback_id,
            resource_uri,
            mode: match mode {
                LegacyVideoMode::ModalWithAudio => RuntimeLiveVideoMode::ModalWithAudio,
                LegacyVideoMode::LayerNoAudio => RuntimeLiveVideoMode::LayerNoAudio,
            },
            stage_width,
            stage_height,
        },
        LegacyVideoCommandV1::Stop { playback_id } => {
            RuntimeLiveVideoCommandKind::Stop { playback_id }
        }
    };
    Ok(RuntimeLiveVideoCommand { sequence, command })
}

fn move_live_wait(sequence: u64, wait: LegacyWaitRequest) -> Result<RuntimeLiveWait, String> {
    let (token_id, kind) = match wait {
        LegacyWaitRequest::Frame { token_id, frames } => {
            (token_id, RuntimeLiveWaitKind::Frame { frames })
        }
        LegacyWaitRequest::Time {
            token_id,
            milliseconds,
        } => (token_id, RuntimeLiveWaitKind::Time { milliseconds }),
        LegacyWaitRequest::Input { token_id, keys } => {
            (token_id, RuntimeLiveWaitKind::Input { keys })
        }
        LegacyWaitRequest::MediaFence { token_id, media_id } => {
            (token_id, RuntimeLiveWaitKind::MediaFence { media_id })
        }
        LegacyWaitRequest::PresentationFence { token_id, fence_id } => (
            token_id,
            RuntimeLiveWaitKind::PresentationFence { fence_id },
        ),
        LegacyWaitRequest::ProviderCompletion { .. } => {
            return Err(
                "ASTRA_EMU_PROVIDER_COMPLETION_REMOVED: ABI v9 families use synchronous Hooks"
                    .into(),
            );
        }
    };
    Ok(RuntimeLiveWait {
        sequence,
        token_id,
        kind,
    })
}

fn move_live_output(live: LegacyLiveOutput) -> Result<RuntimeLiveOutput, String> {
    let mut output = RuntimeLiveOutput {
        layers: live
            .layers
            .into_iter()
            .map(move_layer_transaction)
            .collect(),
        ..RuntimeLiveOutput::default()
    };
    output.audio.reserve(live.audio.len());
    for packet in live.audio {
        output.audio.push(move_live_audio(packet)?);
    }
    output.audio_commands.reserve(live.audio_commands.len());
    for command in live.audio_commands {
        output
            .audio_commands
            .push(move_live_audio_command(command.sequence, command.value)?);
    }
    output.video.reserve(live.video.len());
    for video in live.video {
        output
            .video
            .push(move_live_video(video.sequence, video.value)?);
    }
    Ok(output)
}

fn move_control_output(
    control: LegacyControlTransaction,
    wait_sequence_start: u64,
) -> Result<RuntimeLiveOutput, String> {
    let mut output = RuntimeLiveOutput::default();
    output.events.reserve(control.events.len());
    for event in control.events {
        output.events.push(RuntimeLiveEvent {
            sequence: event.sequence,
            event: event.event,
            value: event.value,
        });
    }
    output.blackboard.reserve(control.blackboard.len());
    for mutation in control.blackboard {
        output.blackboard.push(RuntimeLiveBlackboardMutation {
            sequence: mutation.sequence,
            key: mutation.key,
            value: mutation.value,
        });
    }
    output.dirty_sections.reserve(control.dirty_sections.len());
    for dirty in control.dirty_sections {
        output.dirty_sections.push(RuntimeLiveDirtySection {
            sequence: dirty.sequence,
            section_id: dirty.section_id,
        });
    }
    output.waits.reserve(control.waits.len());
    let mut next_sequence = wait_sequence_start;
    for wait in control.waits {
        output.waits.push(move_live_wait(next_sequence, wait)?);
        next_sequence = next_sequence.saturating_add(1);
    }
    Ok(output)
}

fn emit_family_diagnostics(
    fixed_step: u64,
    diagnostics: &[LegacyDiagnostic],
) -> Result<(), String> {
    for diagnostic in diagnostics {
        let event = diagnostic.code.as_str();
        let subject = diagnostic.subject.as_str();
        match diagnostic.severity.as_str() {
            "error" => tracing::error!(
                target: "astra_emu_manager_core::family",
                event,
                fixed_step,
                diagnostic_subject = subject,
                "family runtime diagnostic"
            ),
            "warn" => tracing::warn!(
                target: "astra_emu_manager_core::family",
                event,
                fixed_step,
                diagnostic_subject = subject,
                "family runtime diagnostic"
            ),
            "info" => tracing::info!(
                target: "astra_emu_manager_core::family",
                event,
                fixed_step,
                diagnostic_subject = subject,
                "family runtime diagnostic"
            ),
            "debug" => tracing::debug!(
                target: "astra_emu_manager_core::family",
                event,
                fixed_step,
                diagnostic_subject = subject,
                "family runtime diagnostic"
            ),
            "trace" => tracing::trace!(
                target: "astra_emu_manager_core::family",
                event,
                fixed_step,
                diagnostic_subject = subject,
                "family runtime diagnostic"
            ),
            _ => return Err("ASTRA_EMU_FAMILY_DIAGNOSTIC_SEVERITY".to_owned()),
        }
    }
    Ok(())
}

const RUNTIME_ID: &str = "astra.emu.runtime";
const PROVIDER_ID: &str = "astra.emu.runtime_provider";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmuCaseProfile {
    pub schema: String,
    pub family_id: String,
    pub case_fingerprint: Hash256,
    pub script_uri: String,
    pub fixed_delta_ns: u64,
    pub compatibility_profile: String,
    pub mount_set_id: String,
    pub permission_policy_id: String,
    #[serde(default)]
    pub family_options: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueuedPatchEffect {
    RuntimeEvent { event: String, value: String },
    SetBlackboard { key: String, value: String },
}

struct EmuSession {
    world: RuntimeWorld,
    family_session_id: LegacyRuntimeSessionId,
    host_ctx: LegacyRuntimeHostCtx,
    pending_control: Arc<Mutex<Option<PendingControlStep>>>,
    await_tokens: Arc<Mutex<BTreeMap<String, AwaitTokenId>>>,
    pending_patch_effects: Vec<QueuedPatchEffect>,
    poisoned: bool,
}

struct PendingControlStep {
    state_revision: u64,
    control: LegacyControlTransaction,
    live_effect_count: usize,
}

struct ApplyLegacyControlAction {
    pending: Arc<Mutex<Option<PendingControlStep>>>,
    await_tokens: Arc<Mutex<BTreeMap<String, AwaitTokenId>>>,
}

impl RuntimeAction for ApplyLegacyControlAction {
    fn descriptor(&self) -> ActionDescriptor {
        ActionDescriptor::declared(
            "astra.emu.apply_legacy_control",
            "astra.emu.legacy_control_input.v1",
            "astra.emu.legacy_control_trace.v1",
            ActionExecutionClass::Serial,
            ActionAccess::new(
                [ActionResourceKey::ActorStore],
                [
                    ActionResourceKey::ActorStore,
                    ActionResourceKey::AwaitQueue,
                    ActionResourceKey::EventQueue,
                    ActionResourceKey::Presentation,
                    ActionResourceKey::MutationLog,
                    ActionResourceKey::StableIdSource,
                    ActionResourceKey::Blackboard,
                ],
            ),
            256,
        )
    }

    fn run(
        &self,
        ctx: &mut DeterministicActionContext<'_>,
        _input: &BTreeMap<String, BlackboardValue>,
    ) -> Result<ActionTrace, RuntimeError> {
        let pending = self
            .pending
            .lock()
            .map_err(|_| RuntimeError::message("ASTRA_EMU_CONTROL_LOCK_POISONED"))?;
        let pending = pending.as_ref().ok_or_else(|| {
            RuntimeError::diagnostic(Diagnostic::blocking(
                "ASTRA_EMU_CONTROL_MISSING",
                "family provider did not publish a control transaction",
            ))
        })?;
        for event in &pending.control.events {
            ctx.emit_event(
                astra_runtime::EventSource::StateMachine,
                EventPayload {
                    kind: event.event.clone(),
                    data: [("value".into(), BlackboardValue::String(event.value.clone()))]
                        .into_iter()
                        .collect(),
                },
            );
        }
        for mutation in &pending.control.blackboard {
            ctx.set_blackboard(
                mutation.key.clone(),
                BlackboardValue::String(mutation.value.clone()),
            );
        }
        for dirty in &pending.control.dirty_sections {
            ctx.emit_presentation(PresentationCommand::Custom {
                kind: "astra.emu.snapshot_dirty.v1".into(),
                data: BTreeMap::from([(
                    "section_id".into(),
                    BlackboardValue::String(dirty.section_id.clone()),
                )]),
            });
        }

        for wait in &pending.control.waits {
            let family_token_id = wait_token_id(wait);
            let mut tokens = self
                .await_tokens
                .lock()
                .map_err(|_| RuntimeError::message("ASTRA_EMU_AWAIT_LOCK_POISONED"))?;
            if tokens.contains_key(&family_token_id) {
                // The RuntimeWorld token is the stable mirror of the family
                // token. A host-validated Input/Time modality rebind keeps the
                // same identity and therefore must not enqueue a second token.
                continue;
            }
            let token = ctx.create_await(astra_runtime::AwaitKind::Custom(wait_kind(wait)));
            tokens.insert(family_token_id, token.token_id);
            drop(tokens);
            ctx.push_await(token)?;
        }

        Ok(ActionTrace {
            action_id: self.descriptor().id,
            payload: [
                (
                    "family_state_revision".into(),
                    BlackboardValue::I64(pending.state_revision as i64),
                ),
                (
                    "control_effect_count".into(),
                    BlackboardValue::I64(pending.control.len() as i64),
                ),
                (
                    "live_effect_count".into(),
                    BlackboardValue::I64(pending.live_effect_count as i64),
                ),
            ]
            .into_iter()
            .collect(),
        })
    }
}

pub struct AstraEmuRuntimeProvider {
    instance_id: Option<ProviderInstanceId>,
    family: Box<dyn LegacyRuntimeProvider>,
    sessions: BTreeMap<String, EmuSession>,
}

type FamilyProviderBuilder =
    dyn Fn() -> Result<Box<dyn LegacyRuntimeProvider>, String> + Send + Sync + 'static;

pub struct AstraEmuRuntimeProviderFactory {
    instance_id: Mutex<Option<ProviderInstanceId>>,
    active_sessions: Arc<AtomicUsize>,
    family_builder: Arc<FamilyProviderBuilder>,
}

struct AstraEmuProviderSession {
    provider: AstraEmuRuntimeProvider,
    session_id: GameRuntimeSessionId,
    instance_id: ProviderInstanceId,
    active_sessions: Arc<AtomicUsize>,
    active: bool,
}

impl AstraEmuRuntimeProviderFactory {
    pub fn new(
        family_builder: impl Fn() -> Result<Box<dyn LegacyRuntimeProvider>, String>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            instance_id: Mutex::new(None),
            active_sessions: Arc::new(AtomicUsize::new(0)),
            family_builder: Arc::new(family_builder),
        }
    }
}

impl ProductRuntimeProviderFactory for AstraEmuRuntimeProviderFactory {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Ok(AstraEmuRuntimeProvider::descriptor_value())
    }

    fn create_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        let mut current = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_EMU_FACTORY_LOCK_POISONED".to_string())?;
        if current.is_some() {
            return Err("ASTRA_EMU_INSTANCE_DUPLICATE".into());
        }
        *current = Some(instance_id.clone());
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".into(),
            diagnostics: vec![],
        })
    }

    fn destroy_instance(
        &self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if self.active_sessions.load(Ordering::Acquire) != 0 {
            return Err("ASTRA_EMU_INSTANCE_ACTIVE_SESSIONS".into());
        }
        let mut current = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_EMU_FACTORY_LOCK_POISONED".to_string())?;
        if current.as_ref() != Some(&instance_id) {
            return Err("ASTRA_EMU_INSTANCE_MISMATCH".into());
        }
        *current = None;
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        let mut provider = AstraEmuRuntimeProvider::new((self.family_builder)()?)?;
        ProductRuntimeProvider::prepare(&mut provider, request)
    }

    fn probe(&self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        let mut provider = AstraEmuRuntimeProvider::new((self.family_builder)()?)?;
        ProductRuntimeProvider::probe(&mut provider, request)
    }

    fn open(
        &self,
        request: RuntimeOpenRequest,
    ) -> Result<(RuntimeOpenReport, Box<dyn ProductRuntimeSession>), String> {
        let instance_id = self
            .instance_id
            .lock()
            .map_err(|_| "ASTRA_EMU_FACTORY_LOCK_POISONED".to_string())?
            .clone()
            .ok_or_else(|| "ASTRA_EMU_INSTANCE_NOT_CREATED".to_string())?;
        let mut provider = AstraEmuRuntimeProvider::new((self.family_builder)()?)?;
        ProductRuntimeProvider::create_instance(&mut provider, instance_id.clone())?;
        let report = match ProductRuntimeProvider::open(&mut provider, request) {
            Ok(report) => report,
            Err(error) => {
                let _ = ProductRuntimeProvider::destroy_instance(&mut provider, instance_id);
                return Err(error);
            }
        };
        self.active_sessions.fetch_add(1, Ordering::AcqRel);
        Ok((
            report.clone(),
            Box::new(AstraEmuProviderSession {
                provider,
                session_id: report.session_id,
                instance_id,
                active_sessions: Arc::clone(&self.active_sessions),
                active: true,
            }),
        ))
    }
}

impl Drop for AstraEmuProviderSession {
    fn drop(&mut self) {
        if self.active {
            self.active_sessions.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

impl ProductRuntimeSession for AstraEmuProviderSession {
    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        if input.session_id != self.session_id {
            return Err("ASTRA_EMU_SESSION_MISMATCH".into());
        }
        ProductRuntimeProvider::step(&mut self.provider, input)
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        ProductRuntimeProvider::save(&mut self.provider, request)
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        ProductRuntimeProvider::restore(&mut self.provider, request)
    }

    fn shutdown(
        mut self: Box<Self>,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        if session_id != self.session_id {
            return Err("ASTRA_EMU_SESSION_MISMATCH".into());
        }
        let report = ProductRuntimeProvider::shutdown(&mut self.provider, session_id)?;
        ProductRuntimeProvider::destroy_instance(&mut self.provider, self.instance_id.clone())?;
        self.active_sessions.fetch_sub(1, Ordering::AcqRel);
        self.active = false;
        Ok(report)
    }
}

impl AstraEmuRuntimeProvider {
    pub fn new(family: Box<dyn LegacyRuntimeProvider>) -> Result<Self, String> {
        family
            .descriptor()
            .validate()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            instance_id: None,
            family,
            sessions: BTreeMap::new(),
        })
    }

    /// Shuts down one concrete AstraEMU session and returns the family-owned
    /// cold-path evidence alongside the generic provider lifecycle report.
    pub fn shutdown_with_family_report(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<(RuntimeShutdownReport, LegacyShutdownReport), String> {
        let session = self
            .sessions
            .remove(&session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        let family_report = self
            .family
            .shutdown(&session.host_ctx, &session.family_session_id)
            .map_err(|error| error.to_string())?;
        let report = RuntimeShutdownReport {
            session_id,
            status: "shutdown".into(),
            diagnostics: family_report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect(),
        };
        Ok((report, family_report))
    }

    pub fn descriptor_value() -> ProductRuntimeDescriptor {
        ProductRuntimeDescriptor {
            runtime_id: RUNTIME_ID.into(),
            product_kind: "legacy_visual_novel".into(),
            provider_id: PROVIDER_ID.into(),
            presentation_lane: astra_plugin_abi::RuntimePresentationLane::Layer2D,
            supported_targets: vec!["game".into()],
            capabilities: vec!["runtime.astra_emu".into()],
            package_sections: vec!["emu.case_profile".into()],
            release_checks: vec![
                "emu.provider_binding".into(),
                "emu.family_binding".into(),
                "emu.payload_redaction".into(),
            ],
        }
    }

    pub fn probe_family(
        &self,
        ctx: &LegacyRuntimeHostCtx,
        request: LegacyProbeRequest,
    ) -> Result<LegacyProbeReport, String> {
        if self.instance_id.is_none() {
            return Err("ASTRA_EMU_INSTANCE_MISSING".into());
        }
        if !self.sessions.is_empty() {
            return Err("ASTRA_EMU_PROBE_ACTIVE_SESSION".into());
        }
        ctx.validate().map_err(|error| error.to_string())?;
        let report = self
            .family
            .probe(ctx, request)
            .map_err(|error| error.to_string())?;
        report.validate().map_err(|error| error.to_string())?;
        if report.family_id != self.family.descriptor().family_id {
            return Err("ASTRA_EMU_PROBE_FAMILY_ID_MISMATCH".into());
        }
        Ok(report)
    }

    pub fn queue_patch_effect(
        &mut self,
        session_id: &GameRuntimeSessionId,
        effect: QueuedPatchEffect,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        if session.pending_patch_effects.len() >= 4096 {
            return Err("ASTRA_EMU_PATCH_EFFECT_COUNT".into());
        }
        session.pending_patch_effects.push(effect);
        Ok(())
    }
}

impl ProductRuntimeProvider for AstraEmuRuntimeProvider {
    fn descriptor(&self) -> Result<ProductRuntimeDescriptor, String> {
        Ok(Self::descriptor_value())
    }

    fn create_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if self.instance_id.is_some() {
            return Err("ASTRA_EMU_INSTANCE_DUPLICATE".into());
        }
        self.instance_id = Some(instance_id.clone());
        tracing::info!(
            event = "astra.emu.runtime.instance_created",
            instance_id = %instance_id.0,
            provider_id = PROVIDER_ID
        );
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "created".into(),
            diagnostics: vec![],
        })
    }

    fn destroy_instance(
        &mut self,
        instance_id: ProviderInstanceId,
    ) -> Result<RuntimeProviderInstanceReport, String> {
        if !self.sessions.is_empty() {
            return Err("ASTRA_EMU_INSTANCE_ACTIVE_SESSIONS".into());
        }
        if self.instance_id.as_ref() != Some(&instance_id) {
            return Err("ASTRA_EMU_INSTANCE_MISMATCH".into());
        }
        self.instance_id = None;
        tracing::info!(
            event = "astra.emu.runtime.instance_destroyed",
            instance_id = %instance_id.0,
            provider_id = PROVIDER_ID
        );
        Ok(RuntimeProviderInstanceReport {
            instance_id,
            status: "destroyed".into(),
            diagnostics: vec![],
        })
    }

    fn prepare(&mut self, request: RuntimePrepareRequest) -> Result<RuntimePrepareReport, String> {
        let pass = request
            .section_ids
            .iter()
            .any(|id| id == "emu.case_profile");
        Ok(RuntimePrepareReport {
            runtime_id: RUNTIME_ID.into(),
            provider_id: PROVIDER_ID.into(),
            status: if pass { "pass" } else { "blocked" }.into(),
            diagnostics: if pass {
                vec![]
            } else {
                vec!["ASTRA_EMU_CASE_PROFILE_MISSING".into()]
            },
        })
    }

    fn probe(&mut self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        let supported = request.platform.as_deref() != Some("web")
            && request
                .section_ids
                .iter()
                .any(|id| id == "emu.case_profile");
        Ok(RuntimeProbeReport {
            runtime_id: RUNTIME_ID.into(),
            provider_id: PROVIDER_ID.into(),
            status: if supported { "supported" } else { "blocked" }.into(),
            diagnostics: if supported {
                vec![]
            } else {
                vec![if request.platform.as_deref() == Some("web") {
                    "PLATFORM_NOT_IMPLEMENTED:native-family-plugin".into()
                } else {
                    "ASTRA_EMU_CASE_PROFILE_MISSING".into()
                }]
            },
        })
    }

    fn open(&mut self, request: RuntimeOpenRequest) -> Result<RuntimeOpenReport, String> {
        if self.instance_id.is_none() {
            return Err("ASTRA_EMU_INSTANCE_NOT_CREATED".into());
        }
        let section = required_section(
            &request.sections,
            "emu.case_profile",
            "astra.emu.case_profile.v1",
        )?;
        let profile: EmuCaseProfile = postcard::from_bytes(&section.bytes)
            .map_err(|error| format!("ASTRA_EMU_CASE_PROFILE_DECODE:{error}"))?;
        if profile.schema != "astra.emu.case_profile.v1"
            || profile.family_id != self.family.descriptor().family_id.0
        {
            return Err("ASTRA_EMU_FAMILY_BINDING_MISMATCH".into());
        }
        let session_id = GameRuntimeSessionId(format!(
            "{RUNTIME_ID}:{}:{}",
            request.target_id, request.seed
        ));
        if self.sessions.contains_key(&session_id.0) {
            return Err("ASTRA_EMU_SESSION_DUPLICATE".into());
        }
        tracing::info!(
            event = "astra.emu.runtime.session_opening",
            session_id = %session_id.0,
            family_id = %self.family.descriptor().family_id.0,
            provider_id = PROVIDER_ID
        );
        let family_session_id = LegacyRuntimeSessionId(session_id.0.clone());
        let host_ctx = LegacyRuntimeHostCtx {
            case_id: format!("case-{}", &profile.case_fingerprint.to_string()[..16]),
            package_id: request.package_hash.clone(),
            package_hash: parse_package_hash(&request.package_hash)?,
            mount_set_id: profile.mount_set_id.clone(),
            media_service_ids: vec!["astra.media".into()],
            permission_policy_id: profile.permission_policy_id.clone(),
            report_sink_id: "astra.emu.report".into(),
            target: request.target_id.clone(),
            profile: request.profile.clone(),
        };
        self.family
            .open(
                &host_ctx,
                LegacyOpenRequest {
                    requested_session_id: family_session_id.clone(),
                    case_fingerprint: profile.case_fingerprint,
                    script_uri: profile.script_uri,
                    fixed_delta_ns: profile.fixed_delta_ns,
                    session_seed: request.seed,
                    compatibility_profile: profile.compatibility_profile,
                    family_options: {
                        let mut options = profile.family_options;
                        options.insert(
                            "astra.hosted_trace_profile".into(),
                            match request.integrity_mode {
                                RuntimeTickIntegrityMode::Shipping => "shipping",
                                RuntimeTickIntegrityMode::Evidence => "evidence",
                            }
                            .into(),
                        );
                        options
                    },
                },
            )
            .map_err(|error| error.to_string())?;

        let world_setup = (|| -> Result<_, String> {
            let integrity_mode = match request.integrity_mode {
                RuntimeTickIntegrityMode::Shipping => TickIntegrityMode::Shipping,
                RuntimeTickIntegrityMode::Evidence => TickIntegrityMode::Evidence,
            };
            let mut world = RuntimeWorld::create_with_integrity(
                RuntimeConfig {
                    seed: request.seed,
                    required_slots: vec![],
                },
                PackageHandle {
                    package_id: request.package_hash.clone(),
                    target: request.target_id.clone(),
                    profile: request.profile.clone(),
                    ..PackageHandle::default()
                },
                integrity_mode,
            )
            .map_err(|error| error.to_string())?;
            let owner = world.create_actor(
                "astra.emu.runtime",
                vec!["gameplay_runtime".into(), "legacy_runtime".into()],
            );
            let pending_control = Arc::new(Mutex::new(None));
            let await_tokens = Arc::new(Mutex::new(BTreeMap::new()));
            world
                .register_action(
                    PROVIDER_ID,
                    ApplyLegacyControlAction {
                        pending: pending_control.clone(),
                        await_tokens: await_tokens.clone(),
                    },
                )
                .map_err(|error| error.to_string())?;
            let running = StableId::deterministic_v7(0, 1, request.seed);
            world
                .add_state_machine(StateMachineDefinition {
                    id: StableId::deterministic_v7(0, 2, request.seed),
                    owner,
                    states: vec![StateDefinition {
                        id: running,
                        name: "emu.running".into(),
                        terminal: false,
                    }],
                    transitions: vec![TransitionDefinition {
                        from: running,
                        to: running,
                        guard: GuardExpr::EventIs {
                            kind: "emu.step".into(),
                        },
                        actions: vec![ActionInvocation {
                            action_id: "astra.emu.apply_legacy_control".into(),
                            input: BTreeMap::new(),
                        }],
                        priority: 0,
                        source_ref: None,
                    }],
                    initial_state: running,
                })
                .map_err(|error| error.to_string())?;
            Ok((world, pending_control, await_tokens))
        })();
        let (world, pending_control, await_tokens) = match world_setup {
            Ok(world) => world,
            Err(setup_error) => {
                let cleanup = self.family.shutdown(&host_ctx, &family_session_id);
                return match cleanup {
                    Ok(_) => Err(setup_error),
                    Err(cleanup_error) => Err(format!(
                        "ASTRA_EMU_OPEN_SETUP_AND_CLEANUP_FAILED:{setup_error};{}",
                        cleanup_error.code()
                    )),
                };
            }
        };
        self.sessions.insert(
            session_id.0.clone(),
            EmuSession {
                world,
                family_session_id,
                host_ctx,
                pending_control,
                await_tokens,
                pending_patch_effects: Vec::new(),
                poisoned: false,
            },
        );
        Ok(RuntimeOpenReport {
            session_id,
            runtime_id: RUNTIME_ID.into(),
            provider_id: PROVIDER_ID.into(),
            diagnostics: vec![],
        })
    }

    fn step(&mut self, input: RuntimeStepInput) -> Result<RuntimeStepOutput, String> {
        if input.action != "emu.step" {
            return Err("ASTRA_EMU_STEP_ACTION".into());
        }
        let input_edges = input
            .input_edges
            .into_iter()
            .map(|edge| LegacyInputEdge {
                control: edge.control,
                pressed: edge.pressed,
                value: edge.value,
                sequence: edge.sequence,
            })
            .collect::<Vec<_>>();
        let await_results = input
            .await_results
            .into_iter()
            .map(|result| LegacyAwaitResult {
                token_id: result.token_id,
                status: result.status,
                payload_len: result.payload_len,
                sequence: result.sequence,
            })
            .collect::<Vec<_>>();
        let provider_results = input
            .provider_results
            .into_iter()
            .map(|result| {
                if !result.payload.is_empty() {
                    return Err("ASTRA_EMU_PROVIDER_RESULT_PAYLOAD_REMOVED".to_owned());
                }
                Ok(LegacyProviderResult {
                    request_id: result.request_id,
                    provider_id: result.provider_id,
                    status: result.status,
                    payload_len: 0,
                    sequence: result.sequence,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let session = self
            .sessions
            .get_mut(&input.session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        let await_results = await_results.clone();
        let mut family_output = self
            .family
            .step(
                &session.host_ctx,
                &session.family_session_id,
                LegacyStepInput {
                    tick_index: input.fixed_step,
                    delta_ns: input.delta_ns,
                    session_seed: input.session_seed,
                    mode: match input.mode {
                        RuntimeStepMode::Live => LegacyReplayMode::Live,
                        RuntimeStepMode::RestoreContinuation => {
                            LegacyReplayMode::RestoreContinuation
                        }
                    },
                    input_edges,
                    await_results: await_results.clone(),
                    provider_results,
                },
            )
            .map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        if !session.pending_patch_effects.is_empty() {
            let mut next_sequence = family_output
                .live
                .max_sequence()
                .into_iter()
                .chain(family_output.control.max_sequence())
                .max()
                .map_or(0, |sequence| sequence.saturating_add(1));
            for effect in std::mem::take(&mut session.pending_patch_effects) {
                match effect {
                    QueuedPatchEffect::RuntimeEvent { event, value } => {
                        family_output.control.events.push(LegacyEvent {
                            sequence: next_sequence,
                            event,
                            value,
                        })
                    }
                    QueuedPatchEffect::SetBlackboard { key, value } => family_output
                        .control
                        .blackboard
                        .push(LegacyBlackboardMutation {
                            sequence: next_sequence,
                            key,
                            value,
                        }),
                }
                next_sequence = next_sequence.saturating_add(1);
            }
            family_output.validate().map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        }
        let live = std::mem::take(&mut family_output.live);
        let control_transaction = std::mem::take(&mut family_output.control);
        let wait_sequence_start = live
            .max_sequence()
            .into_iter()
            .chain(control_transaction.max_sequence())
            .max()
            .map_or(0, |sequence| sequence.saturating_add(1));
        *session
            .pending_control
            .lock()
            .map_err(|_| "ASTRA_EMU_CONTROL_LOCK_POISONED")? = Some(PendingControlStep {
            state_revision: family_output.state_revision,
            control: control_transaction,
            live_effect_count: live.len(),
        });
        let mut ingress = Vec::with_capacity(await_results.len() + 1);
        for result in await_results {
            let token_id = session
                .await_tokens
                .lock()
                .map_err(|_| "ASTRA_EMU_AWAIT_LOCK_POISONED")?
                .remove(&result.token_id)
                .ok_or_else(|| {
                    session.poisoned = true;
                    "ASTRA_EMU_AWAIT_TOKEN_UNKNOWN".to_string()
                })?;
            let mut event = EventPayload::new("await.completed");
            event
                .data
                .insert("status".into(), BlackboardValue::String(result.status));
            event.data.insert(
                "payload_len".into(),
                BlackboardValue::I64(result.payload_len as i64),
            );
            ingress.push(OrderedTickIngress {
                sequence: result.sequence,
                payload: TickIngress::AwaitCompletion(AwaitResult {
                    token_id,
                    sequence: result.sequence,
                    completed_at_step: input.fixed_step,
                    payload: event,
                }),
            });
        }
        let event_sequence = ingress
            .last()
            .map_or(1, |item| item.sequence.saturating_add(1));
        ingress.push(OrderedTickIngress {
            sequence: event_sequence,
            payload: TickIngress::PlayerInput(PlayerInput {
                kind: "emu.step".into(),
                payload: EventPayload::new("emu.step"),
            }),
        });
        let timing = TickInput {
            fixed_step: input.fixed_step,
            delta_ns: input.delta_ns,
            seed: input.session_seed,
        };
        let tick = match session.world.tick(match input.mode {
            RuntimeStepMode::Live => TickRequest::live(timing, ingress),
            RuntimeStepMode::RestoreContinuation => {
                TickRequest::restore_continuation(timing, ingress)
            }
        }) {
            Ok(tick) => tick,
            Err(error) => {
                let _ = session
                    .pending_control
                    .lock()
                    .map_err(|_| "ASTRA_EMU_CONTROL_LOCK_POISONED")?
                    .take();
                session.poisoned = true;
                return Err(error.to_string());
            }
        };
        let mut control = session
            .pending_control
            .lock()
            .map_err(|_| "ASTRA_EMU_CONTROL_LOCK_POISONED")?
            .take()
            .ok_or_else(|| {
                session.poisoned = true;
                "ASTRA_EMU_CONTROL_MISSING_AFTER_TICK".to_owned()
            })?;
        if let Some(diagnostic) = tick.diagnostics.first() {
            session.poisoned = true;
            return Err(format!("{}:{}", diagnostic.code, diagnostic.message));
        }
        let family_diagnostics = std::mem::take(&mut family_output.diagnostics);
        emit_family_diagnostics(input.fixed_step, &family_diagnostics)?;
        let status = format!("{:?}", family_output.status).to_ascii_lowercase();
        let state_revision = family_output.state_revision;
        let coverage = family_output.coverage.clone();
        let mut live = move_live_output(live)?;
        let control_live =
            move_control_output(std::mem::take(&mut control.control), wait_sequence_start)?;
        live.events.extend(control_live.events);
        live.blackboard.extend(control_live.blackboard);
        live.dirty_sections.extend(control_live.dirty_sections);
        live.waits.extend(control_live.waits);
        live.state_revision = state_revision;
        live.coverage = RuntimeLiveCoverage {
            instructions: coverage.instructions,
            syscalls: coverage.syscalls,
            presentation_commands: coverage.presentation_commands,
            audio_commands: coverage.audio_commands,
            text_events: coverage.text_events,
            capture_bytes: coverage.capture_bytes,
            operation_bytes: coverage.operation_bytes,
            scene_moved_bytes: coverage.scene_moved_bytes,
            scene_copied_bytes: coverage.scene_copied_bytes,
            pcm_moved_bytes: coverage.pcm_moved_bytes,
            pcm_copied_bytes: coverage.pcm_copied_bytes,
        };
        let diagnostics = family_diagnostics
            .into_iter()
            .map(|diagnostic| format!("{}:{}", diagnostic.code, diagnostic.message))
            .collect();
        live.diagnostics = diagnostics;
        Ok(RuntimeStepOutput {
            session_id: input.session_id,
            status,
            live,
            diagnostics: vec![],
        })
    }

    fn save(&mut self, request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        let session = self
            .sessions
            .get(&request.session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        Err(
            "ASTRA_EMU_FAMILY_SNAPSHOT_REMOVED: game saves use the ABI v9 writable-file port"
                .into(),
        )
    }

    fn restore(&mut self, request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        let session = self
            .sessions
            .get(&request.session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        Err(
            "ASTRA_EMU_FAMILY_SNAPSHOT_REMOVED: game loads use the ABI v9 writable-file port"
                .into(),
        )
    }

    fn shutdown(
        &mut self,
        session_id: GameRuntimeSessionId,
    ) -> Result<RuntimeShutdownReport, String> {
        self.shutdown_with_family_report(session_id)
            .map(|(report, _)| report)
    }
}

fn required_section<'a>(
    sections: &'a [RuntimeSectionPayload],
    id: &str,
    schema: &str,
) -> Result<&'a RuntimeSectionPayload, String> {
    let mut matches = sections.iter().filter(|section| section.section_id == id);
    let section = matches
        .next()
        .ok_or_else(|| format!("ASTRA_EMU_SECTION_MISSING:{id}"))?;
    if matches.next().is_some() || section.schema != schema {
        return Err(format!("ASTRA_EMU_SECTION_INVALID:{id}"));
    }
    Ok(section)
}
fn wait_kind(wait: &LegacyWaitRequest) -> String {
    match wait {
        LegacyWaitRequest::Frame { .. } => "fvp.frame",
        LegacyWaitRequest::Time { .. } => "fvp.time",
        LegacyWaitRequest::Input { .. } => "fvp.input",
        LegacyWaitRequest::MediaFence { .. } => "fvp.media",
        LegacyWaitRequest::PresentationFence { .. } => "fvp.presentation",
        LegacyWaitRequest::ProviderCompletion { .. } => "fvp.provider",
    }
    .into()
}
fn wait_token_id(wait: &LegacyWaitRequest) -> String {
    match wait {
        LegacyWaitRequest::Frame { token_id, .. }
        | LegacyWaitRequest::Time { token_id, .. }
        | LegacyWaitRequest::Input { token_id, .. }
        | LegacyWaitRequest::MediaFence { token_id, .. }
        | LegacyWaitRequest::PresentationFence { token_id, .. }
        | LegacyWaitRequest::ProviderCompletion { token_id, .. } => token_id.clone(),
    }
}
fn parse_package_hash(value: &str) -> Result<Hash256, String> {
    value
        .parse()
        .map_err(|error| format!("ASTRA_EMU_PACKAGE_HASH:{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use astra_emu_family_api::LegacyVmTraceRecord;

    #[test]
    fn evidence_identity_is_family_scoped_and_deduplicated() {
        let trace = vec![
            LegacyVmTraceRecord {
                context_id: 7,
                program_counter: 9,
                opcode: 11,
            },
            LegacyVmTraceRecord {
                context_id: 7,
                program_counter: 9,
                opcode: 11,
            },
        ];
        assert_eq!(
            evidence_vm_coverage_ids("minori", &trace),
            ["minori.vm.c7.pc00000009.op0b"]
        );
        assert_ne!(
            evidence_terminal_hash("minori", 1, 2, 3),
            evidence_terminal_hash("fvp", 1, 2, 3)
        );
    }

    #[test]
    fn typed_live_output_keeps_layer_surface_references_outside_runtime_world_control() {
        let live = LegacyLiveOutput {
            layers: vec![astra_emu_family_api::LegacyLayerTransactionV9 {
                sequence: 0,
                viewport_width: 1,
                viewport_height: 1,
                operations: vec![astra_emu_family_api::LegacyLayerOperationV9::Create(
                    astra_emu_family_api::LegacyLayerStateV9 {
                        layer_id: "layer.stage".into(),
                        role: "stage".into(),
                        z_index: 0,
                        surface_id: "surface.stage".into(),
                        generation: 1,
                        width: 1,
                        height: 1,
                        stride: 4,
                        format: astra_emu_family_api::LegacySurfaceFormatV9::Rgba8SrgbPremultiplied,
                        damage: astra_emu_family_api::LegacySurfaceDamageV9::Full,
                        transform: astra_emu_family_api::LegacyLayerTransformV9 {
                            m11: 1.0,
                            m12: 0.0,
                            m21: 0.0,
                            m22: 1.0,
                            tx: 0.0,
                            ty: 0.0,
                        },
                        clip: None,
                        opacity: 1.0,
                        texture_filter: astra_emu_family_api::LegacyLayerFilterV9::Linear,
                        blend: astra_emu_family_api::LegacyLayerBlendV9::Alpha,
                        filter_graph_binding: None,
                    },
                )],
            }],
            ..LegacyLiveOutput::default()
        };
        let control = LegacyControlTransaction {
            events: vec![LegacyEvent {
                sequence: 1,
                event: "test.event".into(),
                value: String::new(),
            }],
            ..LegacyControlTransaction::default()
        };
        assert_eq!(control.len(), 1);
        assert_eq!(live.len(), 1);
        assert_eq!(live.layers[0].operations.len(), 1);
    }
}
