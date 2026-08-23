use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

#[cfg(test)]
use astra_core::SchemaVersion;
use astra_core::{Diagnostic, Hash256, StableId};
use astra_emu_family_api::{
    LegacyAudioCommandV1, LegacyAudioEncoding, LegacyAudioPacketV7, LegacyAudioSampleFormat,
    LegacyAwaitResult, LegacyBlackboardMutation, LegacyControlTransaction, LegacyDiagnostic,
    LegacyEvent, LegacyInputEdge, LegacyLiveOutput, LegacyOpenRequest, LegacyPcmBufferV7,
    LegacyProbeReport, LegacyProbeRequest, LegacyProviderError, LegacyProviderResult,
    LegacyReplayMode, LegacyRuntimeHostCtx, LegacyRuntimeProvider, LegacyRuntimeSessionId,
    LegacyShutdownReport, LegacyStepInput, LegacyVideoCommandV1, LegacyVideoMode,
    LegacyWaitRequest,
};
use astra_plugin::{ProductRuntimeProvider, ProductRuntimeProviderFactory, ProductRuntimeSession};
#[cfg(test)]
use astra_plugin_abi::RuntimeSectionCodec;
use astra_plugin_abi::{
    GameRuntimeSessionId, ProductRuntimeDescriptor, ProviderInstanceId, RuntimeLiveAudioCommand,
    RuntimeLiveAudioEncoding, RuntimeLiveAudioPacket, RuntimeLiveAudioSampleFormat,
    RuntimeLiveBlackboardMutation, RuntimeLiveCoverage, RuntimeLiveEvent, RuntimeLiveOutput,
    RuntimeLivePcmBuffer, RuntimeLiveVideoCommand, RuntimeLiveVideoCommandKind,
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
    EventPayload, GuardExpr, OrderedTickIngress, PackageHandle, PlayerInput, RuntimeAction,
    RuntimeConfig, RuntimeError, RuntimeWorld, StateDefinition, StateMachineDefinition,
    TickIngress, TickInput, TickIntegrityMode, TickRequest, TransitionDefinition,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    AstraEmuFamilyHost, FamilySurfaceHost, PublishedFamilySurface, SynchronousFamilyHookProvider,
};

pub fn evidence_vm_coverage_ids(
    trace: &[astra_emu_family_api::LegacyVmTraceRecord],
) -> Vec<String> {
    trace
        .iter()
        .map(|record| {
            format!(
                "fvp.vm.c{}.pc{:08x}.op{:02x}",
                record.context_id, record.program_counter, record.opcode
            )
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
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

fn move_live_wait(sequence: u64, wait: LegacyWaitRequest) -> RuntimeLiveWait {
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
        LegacyWaitRequest::ProviderCompletion {
            token_id,
            request_id,
        } => (
            token_id,
            RuntimeLiveWaitKind::ProviderCompletion { request_id },
        ),
    };
    RuntimeLiveWait {
        sequence,
        token_id,
        kind,
    }
}

fn move_live_output(live: LegacyLiveOutput) -> Result<RuntimeLiveOutput, String> {
    let mut output = RuntimeLiveOutput::default();
    output.layers.reserve(live.layers.len());
    for transaction in live.layers {
        output.layers.push(move_live_layers(transaction)?);
    }
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

fn move_live_layers(
    transaction: astra_emu_family_api::LegacyLayerTransactionV9,
) -> Result<astra_media_core::Layer2DTransaction, String> {
    use astra_media_core::{Layer2DId, Layer2DOperation};

    let operations = transaction
        .operations
        .into_iter()
        .map(|operation| match operation {
            astra_emu_family_api::LegacyLayerOperationV9::Create(layer) => {
                move_live_layer(layer).map(Layer2DOperation::Create)
            }
            astra_emu_family_api::LegacyLayerOperationV9::Update(layer) => {
                move_live_layer(layer).map(Layer2DOperation::Update)
            }
            astra_emu_family_api::LegacyLayerOperationV9::Destroy { layer_id } => {
                Ok(Layer2DOperation::Destroy(Layer2DId(layer_id)))
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    let transaction = astra_media_core::Layer2DTransaction {
        sequence: transaction.sequence,
        viewport_width: transaction.viewport_width,
        viewport_height: transaction.viewport_height,
        operations,
    };
    Ok(transaction)
}

fn move_live_layer(
    layer: astra_emu_family_api::LegacyLayerStateV9,
) -> Result<astra_media_core::Layer2DState, String> {
    use astra_media_core::*;
    let rect = |rect: astra_emu_family_api::LegacyDamageRectV9| -> Result<RectI, String> {
        Ok(RectI {
            x: i32::try_from(rect.x).map_err(|_| "ASTRA_EMU_LAYER_RECT")?,
            y: i32::try_from(rect.y).map_err(|_| "ASTRA_EMU_LAYER_RECT")?,
            width: rect.width,
            height: rect.height,
        })
    };
    let damage = match layer.damage {
        astra_emu_family_api::LegacySurfaceDamageV9::Unchanged => Layer2DDamage::Unchanged,
        astra_emu_family_api::LegacySurfaceDamageV9::Full => Layer2DDamage::Full,
        astra_emu_family_api::LegacySurfaceDamageV9::Rects(rects) => {
            Layer2DDamage::Rects(rects.into_iter().map(rect).collect::<Result<Vec<_>, _>>()?)
        }
    };
    Ok(Layer2DState {
        id: Layer2DId(layer.layer_id),
        role: Layer2DRole(layer.role),
        z_index: layer.z_index,
        content: Layer2DContent::WritableSurface(WritableSurface2DRef {
            surface_id: Surface2DId(layer.surface_id),
            generation: layer.generation,
            width: layer.width,
            height: layer.height,
            stride: layer.stride,
            format: match layer.format {
                astra_emu_family_api::LegacySurfaceFormatV9::Rgba8SrgbPremultiplied => {
                    Surface2DFormat::Rgba8SrgbPremultiplied
                }
                astra_emu_family_api::LegacySurfaceFormatV9::Bgra8SrgbPremultiplied => {
                    Surface2DFormat::Bgra8SrgbPremultiplied
                }
            },
            damage,
        }),
        transform: Transform2D {
            m11: layer.transform.m11,
            m12: layer.transform.m12,
            m21: layer.transform.m21,
            m22: layer.transform.m22,
            tx: layer.transform.tx,
            ty: layer.transform.ty,
        },
        clip: layer.clip.map(rect).transpose()?,
        opacity: layer.opacity,
        texture_filter: match layer.texture_filter {
            astra_emu_family_api::LegacyLayerFilterV9::Nearest => TextureFilter2D::Nearest,
            astra_emu_family_api::LegacyLayerFilterV9::Linear => TextureFilter2D::Linear,
        },
        blend: match layer.blend {
            astra_emu_family_api::LegacyLayerBlendV9::Opaque => BlendMode::Opaque,
            astra_emu_family_api::LegacyLayerBlendV9::Alpha => BlendMode::Alpha,
            astra_emu_family_api::LegacyLayerBlendV9::Add => BlendMode::Add,
            astra_emu_family_api::LegacyLayerBlendV9::Multiply => BlendMode::Multiply,
            astra_emu_family_api::LegacyLayerBlendV9::Screen => BlendMode::Screen,
        },
        filter_graph: layer.filter_graph.map(move_filter_graph),
    })
}

fn move_filter_graph(
    graph: astra_emu_family_api::LegacyFilterGraphV9,
) -> astra_media_core::FilterGraph {
    use astra_emu_family_api::{LegacyFilterParamV9 as Param, LegacyFilterTargetV9 as Target};
    use astra_media_core::{FilterGraph, FilterNode, FilterParam, FilterTarget};
    let target = |value| match value {
        Target::Background => FilterTarget::Background,
        Target::Character => FilterTarget::Character,
        Target::Ui => FilterTarget::Ui,
        Target::Text => FilterTarget::Text,
        Target::Video => FilterTarget::Video,
        Target::Final => FilterTarget::Final,
    };
    FilterGraph {
        schema: graph.schema,
        nodes: graph
            .nodes
            .into_iter()
            .map(|node| FilterNode {
                id: node.id,
                kind: node.kind,
                input: target(node.input),
                output: target(node.output),
                params: node
                    .params
                    .into_iter()
                    .map(|entry| {
                        let value = match entry.value {
                            Param::Float(value) => FilterParam::Float(value),
                            Param::Int(value) => FilterParam::Int(value),
                            Param::Bool(value) => FilterParam::Bool(value),
                            Param::Text(value) => FilterParam::Text(value),
                        };
                        (entry.key, value)
                    })
                    .collect(),
                deterministic: node.deterministic,
                allow_cpu_fallback: node.allow_cpu_fallback,
            })
            .collect(),
    }
}

fn referenced_surface_generations(
    transactions: &[astra_media_core::Layer2DTransaction],
) -> Result<BTreeMap<String, u64>, String> {
    use astra_media_core::{Layer2DContent, Layer2DOperation};

    let mut generations = BTreeMap::new();
    for transaction in transactions {
        for operation in &transaction.operations {
            let layer = match operation {
                Layer2DOperation::Create(layer) | Layer2DOperation::Update(layer) => layer,
                Layer2DOperation::Destroy(_) => continue,
            };
            let Layer2DContent::WritableSurface(surface) = &layer.content else {
                return Err("ASTRA_EMU_LAYER_TEXTURE_RESOURCE_FORBIDDEN".into());
            };
            match generations.insert(surface.surface_id.0.clone(), surface.generation) {
                Some(existing) if existing != surface.generation => {
                    return Err("ASTRA_EMU_SURFACE_GENERATION_CONFLICT".into());
                }
                _ => {}
            }
        }
    }
    Ok(generations)
}

struct SurfaceStepGuard {
    host: Arc<FamilySurfaceHost>,
    session_id: String,
    fixed_step: u64,
    published: bool,
}

impl SurfaceStepGuard {
    fn new(host: Arc<FamilySurfaceHost>, session_id: String, fixed_step: u64) -> Self {
        Self {
            host,
            session_id,
            fixed_step,
            published: false,
        }
    }

    fn publish(&mut self, generations: &BTreeMap<String, u64>) -> Result<(), LegacyProviderError> {
        self.host
            .publish_step(&self.session_id, self.fixed_step, generations)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for SurfaceStepGuard {
    fn drop(&mut self) {
        if !self.published {
            self.host.rollback_step(&self.session_id, self.fixed_step);
        }
    }
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
    output.waits.reserve(control.waits.len());
    let mut next_sequence = wait_sequence_start;
    for wait in control.waits {
        output.waits.push(move_live_wait(next_sequence, wait));
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
    layer_state: astra_media_core::RetainedLayer2DState,
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
        for wait in &pending.control.waits {
            let family_token_id = wait_token_id(wait);
            let token = ctx.create_await(astra_runtime::AwaitKind::Custom(wait_kind(wait)));
            let mut tokens = self
                .await_tokens
                .lock()
                .map_err(|_| RuntimeError::message("ASTRA_EMU_AWAIT_LOCK_POISONED"))?;
            if tokens.insert(family_token_id, token.token_id).is_some() {
                return Err(RuntimeError::diagnostic(Diagnostic::blocking(
                    "ASTRA_EMU_AWAIT_TOKEN_DUPLICATE",
                    "family provider emitted a duplicate pending wait token",
                )));
            }
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
    host: AstraEmuFamilyHost,
    sessions: BTreeMap<String, EmuSession>,
}

type FamilyProviderBuilder = dyn Fn() -> Result<(Box<dyn LegacyRuntimeProvider>, AstraEmuFamilyHost), String>
    + Send
    + Sync
    + 'static;

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
        family_builder: impl Fn() -> Result<(Box<dyn LegacyRuntimeProvider>, AstraEmuFamilyHost), String>
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
        let (family, host) = (self.family_builder)()?;
        let mut provider = AstraEmuRuntimeProvider::new(family, host)?;
        ProductRuntimeProvider::prepare(&mut provider, request)
    }

    fn probe(&self, request: RuntimeProbeRequest) -> Result<RuntimeProbeReport, String> {
        let (family, host) = (self.family_builder)()?;
        let mut provider = AstraEmuRuntimeProvider::new(family, host)?;
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
        let (family, host) = (self.family_builder)()?;
        let mut provider = AstraEmuRuntimeProvider::new(family, host)?;
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
    pub fn new(
        family: Box<dyn LegacyRuntimeProvider>,
        host: AstraEmuFamilyHost,
    ) -> Result<Self, String> {
        family
            .descriptor()
            .validate()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            instance_id: None,
            family,
            host,
            sessions: BTreeMap::new(),
        })
    }

    pub fn bind_writable_root_for_open(
        &self,
        target_id: &str,
        seed: u64,
        case_fingerprint: Hash256,
        root: impl AsRef<std::path::Path>,
    ) -> Result<(), String> {
        let session_id = format!("{RUNTIME_ID}:{target_id}:{seed}");
        let family_id = self.family.descriptor().family_id.0;
        let family_game_id = format!("case-{}", &case_fingerprint.to_string()[..16]);
        self.host
            .writable_files
            .bind_session(session_id, family_id, family_game_id, root)
            .map_err(|error| error.to_string())
    }

    pub fn bind_hook_provider(
        &self,
        case_fingerprint: Hash256,
        timeout_ms: u32,
        provider: Arc<dyn SynchronousFamilyHookProvider>,
    ) -> Result<(), String> {
        let family_id = self.family.descriptor().family_id.0;
        let family_game_id = format!("case-{}", &case_fingerprint.to_string()[..16]);
        self.host
            .hooks
            .bind(family_id, family_game_id, timeout_ms, provider)
            .map_err(|error| error.to_string())
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
        self.host.release_session(&session.family_session_id.0);
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

    pub fn read_vfs_resource(
        &self,
        session_id: &GameRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<astra_byte_source::OwnedByteBuffer, String> {
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        self.host
            .vfs()
            .read_file(&session.host_ctx.mount_set_id, resource_uri, max_bytes)
            .map_err(|error| error.to_string())
    }

    pub fn begin_vfs_resource_read(
        &self,
        session_id: &GameRuntimeSessionId,
        resource_uri: &str,
        max_bytes: u64,
    ) -> Result<astra_emu_family_api::LegacyResourceRead, String> {
        let session = self
            .sessions
            .get(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        let vfs = self.host.vfs();
        let mount_set_id = session.host_ctx.mount_set_id.clone();
        let resource_uri = resource_uri.to_owned();
        astra_emu_family_api::LegacyResourceRead::spawn(move || {
            vfs.read_file(&mount_set_id, &resource_uri, max_bytes)
        })
        .map_err(|error| error.to_string())
    }

    pub fn take_published_surface(
        &self,
        session_id: &GameRuntimeSessionId,
        surface_id: &str,
        generation: u64,
    ) -> Result<PublishedFamilySurface, String> {
        self.sessions
            .get(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        self.host
            .surfaces
            .take_published(&session_id.0, surface_id, generation)
            .map_err(|error| error.to_string())
    }

    pub fn return_published_surface(&self, surface: PublishedFamilySurface) -> Result<(), String> {
        self.host
            .surfaces
            .return_published(surface)
            .map_err(|error| error.to_string())
    }

    pub fn with_published_surface<R>(
        &self,
        session_id: &GameRuntimeSessionId,
        surface_id: &str,
        generation: u64,
        reader: impl FnOnce(&[u8], u32, u32, u32, astra_emu_family_api::LegacySurfaceFormatV9) -> R,
    ) -> Result<R, String> {
        self.sessions
            .get(&session_id.0)
            .ok_or_else(|| "ASTRA_EMU_SESSION_MISSING".to_owned())?;
        self.host
            .surfaces
            .with_published(&session_id.0, surface_id, generation, reader)
            .map_err(|error| error.to_string())
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
        if let Err(error) = self.family.open(
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
        ) {
            self.host.release_session(&family_session_id.0);
            return Err(error.to_string());
        }

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
                self.host.release_session(&family_session_id.0);
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
                layer_state: astra_media_core::RetainedLayer2DState::default(),
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
        if input.mode != RuntimeStepMode::Live {
            return Err(
                "ASTRA_EMU_SHARED_RESTORE_UNSUPPORTED:native writable-file storage is authoritative"
                    .into(),
            );
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
            .map(|result| LegacyProviderResult {
                request_id: result.request_id,
                provider_id: result.provider_id,
                status: result.status,
                payload_len: result.payload_len,
                sequence: result.sequence,
            })
            .collect::<Vec<_>>();
        let surface_host = self.host.surfaces.clone();
        let session = self
            .sessions
            .get_mut(&input.session_id.0)
            .ok_or("ASTRA_EMU_SESSION_MISSING")?;
        if session.poisoned {
            return Err("ASTRA_EMU_SESSION_POISONED".into());
        }
        let await_results = await_results.clone();
        let mut surface_guard = SurfaceStepGuard::new(
            surface_host,
            session.family_session_id.0.clone(),
            input.fixed_step,
        );
        let mut family_output = match self.family.step(
            &session.host_ctx,
            &session.family_session_id,
            LegacyStepInput {
                tick_index: input.fixed_step,
                delta_ns: input.delta_ns,
                session_seed: input.session_seed,
                mode: LegacyReplayMode::Live,
                input_edges,
                await_results: await_results.clone(),
                provider_results,
            },
        ) {
            Ok(output) => output,
            Err(error) => {
                session.poisoned = true;
                return Err(error.to_string());
            }
        };
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
        }
        family_output.validate().map_err(|error| {
            session.poisoned = true;
            error.to_string()
        })?;
        let live = std::mem::take(&mut family_output.live);
        let control_transaction = std::mem::take(&mut family_output.control);
        let wait_sequence_start = live
            .max_sequence()
            .into_iter()
            .chain(control_transaction.max_sequence())
            .max()
            .map_or(0, |sequence| sequence.saturating_add(1));
        let live_effect_count = live.len();
        let mut live = move_live_output(live).inspect_err(|_| {
            session.poisoned = true;
        })?;
        let mut next_layer_state = session.layer_state.clone();
        for transaction in &live.layers {
            next_layer_state.apply(transaction).map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        }
        let surface_generations =
            referenced_surface_generations(&live.layers).inspect_err(|_| {
                session.poisoned = true;
            })?;
        *session
            .pending_control
            .lock()
            .map_err(|_| "ASTRA_EMU_CONTROL_LOCK_POISONED")? = Some(PendingControlStep {
            state_revision: family_output.state_revision,
            control: control_transaction,
            live_effect_count,
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
        let tick = match session.world.tick(TickRequest::live(timing, ingress)) {
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
        let control_live =
            move_control_output(std::mem::take(&mut control.control), wait_sequence_start)?;
        live.events.extend(control_live.events);
        live.blackboard.extend(control_live.blackboard);
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
        surface_guard
            .publish(&surface_generations)
            .map_err(|error| {
                session.poisoned = true;
                error.to_string()
            })?;
        session.layer_state = next_layer_state;
        Ok(RuntimeStepOutput {
            session_id: input.session_id,
            status,
            live,
            diagnostics: vec![],
        })
    }

    fn save(&mut self, _request: RuntimeSaveRequest) -> Result<RuntimeSaveSections, String> {
        Err(
            "ASTRA_EMU_SHARED_SAVE_UNSUPPORTED:native writable-file storage is authoritative"
                .into(),
        )
    }

    fn restore(&mut self, _request: RuntimeRestoreRequest) -> Result<RuntimeRestoreReport, String> {
        Err(
            "ASTRA_EMU_SHARED_RESTORE_UNSUPPORTED:native writable-file storage is authoritative"
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
    use astra_emu_family_api::{LegacyProviderError, LegacyVfsListedFile, LegacyVfsReader};
    use astra_emu_fvp::create_static_fvp_provider;

    struct MemoryVfs {
        script: Vec<u8>,
        default_font: Vec<u8>,
    }

    impl MemoryVfs {
        fn file(&self, mount_set_id: &str, uri: &str) -> Result<&[u8], LegacyProviderError> {
            if mount_set_id != "mount.test" {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_NOT_FOUND",
                    "synthetic fixture mount is missing",
                ));
            }
            match uri {
                "script.hcb" => Ok(&self.script),
                "default.ttf" => Ok(&self.default_font),
                _ => Err(LegacyProviderError::invalid(
                    "TEST_VFS_NOT_FOUND",
                    "synthetic fixture path is missing",
                )),
            }
        }
    }

    impl LegacyVfsReader for MemoryVfs {
        fn stat_file(
            &self,
            mount_set_id: &str,
            uri: &str,
        ) -> Result<astra_byte_source::ByteSourceStat, LegacyProviderError> {
            let bytes = self.file(mount_set_id, uri)?;
            Ok(astra_byte_source::ByteSourceStat {
                len: bytes.len() as u64,
                revision: astra_byte_source::SourceRevision(1),
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
            range.validate(stat.len, max_bytes).map_err(|error| {
                LegacyProviderError::invalid("TEST_VFS_BOUNDS", error.to_string())
            })?;
            if stat.revision != expected_revision {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_REVISION",
                    "synthetic fixture revision changed",
                ));
            }
            let bytes = self.file(mount_set_id, uri)?;
            let bytes = bytes[range.offset as usize..(range.offset + range.len) as usize].to_vec();
            Ok(astra_byte_source::RangeReadResult {
                range,
                revision: stat.revision,
                bytes: bytes.into(),
            })
        }

        fn enumerate_by_extension(
            &self,
            mount_set_id: &str,
            root: &str,
            extension_without_dot: &str,
            max_entries: u32,
        ) -> Result<Vec<LegacyVfsListedFile>, LegacyProviderError> {
            if !root.is_empty() || max_entries == 0 {
                return Err(LegacyProviderError::invalid(
                    "TEST_VFS_ENUMERATE",
                    "synthetic fixture enumeration is invalid",
                ));
            }
            match extension_without_dot {
                "hcb" => Ok(vec![LegacyVfsListedFile {
                    uri: "script.hcb".into(),
                    stat: self.stat_file(mount_set_id, "script.hcb")?,
                }]),
                "bin" => Ok(Vec::new()),
                _ => Err(LegacyProviderError::invalid(
                    "TEST_VFS_ENUMERATE",
                    "synthetic fixture extension is unsupported",
                )),
            }
        }
    }

    #[test]
    fn fvp_product_provider_full_lifecycle_and_repeated_run_are_deterministic() {
        let script = terminal_hcb();
        let fingerprint = Hash256::from_sha256(&script);
        let vfs: Arc<dyn LegacyVfsReader> = Arc::new(MemoryVfs {
            script,
            default_font: include_bytes!(
                "../../../../../Engine/Fixtures/PublicDomainFonts/NotoSansSC-Variable.ttf"
            )
            .to_vec(),
        });
        let host = AstraEmuFamilyHost::new(vfs);
        let family = create_static_fvp_provider(host.services()).unwrap();
        let mut provider = AstraEmuRuntimeProvider::new(family, host).unwrap();
        let instance = ProviderInstanceId("emu.test.instance".into());
        provider.create_instance(instance.clone()).unwrap();

        let first = run_once(&mut provider, fingerprint);
        let second = run_once(&mut provider, fingerprint);
        assert_eq!(
            first, second,
            "same package/input identity must replay identically"
        );

        provider.destroy_instance(instance).unwrap();
    }

    fn run_once(
        provider: &mut AstraEmuRuntimeProvider,
        fingerprint: Hash256,
    ) -> Vec<(u64, Vec<u64>)> {
        let writable_root = tempfile::tempdir().unwrap();
        provider
            .bind_writable_root_for_open("windows", 17, fingerprint, writable_root.path())
            .unwrap();
        let profile = EmuCaseProfile {
            schema: "astra.emu.case_profile.v1".into(),
            family_id: "fvp".into(),
            case_fingerprint: fingerprint,
            script_uri: "script.hcb".into(),
            fixed_delta_ns: 16_666_667,
            compatibility_profile: "rfvp-v1".into(),
            mount_set_id: "mount.test".into(),
            permission_policy_id: "permission.test".into(),
            family_options: [
                ("fvp.nls".into(), "utf8".into()),
                ("fvp.pack_paths".into(), "[]".into()),
            ]
            .into_iter()
            .collect(),
        };
        let bytes = postcard::to_allocvec(&profile).unwrap();
        let package_hash = Hash256::from_sha256(b"package.test").to_string();
        let open = provider
            .open(RuntimeOpenRequest {
                target_id: "windows".into(),
                profile: "fvp-v1".into(),
                locale: "und".into(),
                seed: 17,
                integrity_mode: RuntimeTickIntegrityMode::Evidence,
                executor: astra_plugin_abi::RuntimeExecutorConfig::serial(),
                package_hash,
                sections: vec![RuntimeSectionPayload {
                    section_id: "emu.case_profile".into(),
                    schema: "astra.emu.case_profile.v1".into(),
                    version: SchemaVersion::new(1, 0, 0),
                    codec: RuntimeSectionCodec::Postcard,
                    hash: Hash256::from_sha256(&bytes),
                    bytes,
                }],
            })
            .unwrap();
        provider
            .queue_patch_effect(
                &open.session_id,
                QueuedPatchEffect::RuntimeEvent {
                    event: "patch.synthetic".into(),
                    value: "typed-value".into(),
                },
            )
            .unwrap();
        let output = provider
            .step(RuntimeStepInput {
                session_id: open.session_id.clone(),
                fixed_step: 1,
                delta_ns: 16_666_667,
                session_seed: 17,
                mode: RuntimeStepMode::Live,
                action: "emu.step".into(),
                budget: astra_plugin_abi::RuntimeStepBudget {
                    max_instructions: 32,
                    max_effects: 32,
                    max_trace_entries: 32,
                },
                ..RuntimeStepInput::default()
            })
            .unwrap();
        assert_eq!(output.status, "active");
        let live = output.live;
        assert!(live
            .events
            .iter()
            .any(|event| event.event == "patch.synthetic" && event.value == "typed-value"));
        let mut output_observations = vec![(
            live.state_revision,
            live.events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
        )];
        let second = provider
            .step(RuntimeStepInput {
                session_id: open.session_id.clone(),
                fixed_step: 2,
                delta_ns: 16_666_667,
                session_seed: 17,
                mode: RuntimeStepMode::Live,
                action: "emu.step".into(),
                budget: astra_plugin_abi::RuntimeStepBudget {
                    max_instructions: 32,
                    max_effects: 32,
                    max_trace_entries: 32,
                },
                ..RuntimeStepInput::default()
            })
            .unwrap();
        assert_eq!(second.status, "active");
        let live = second.live;
        output_observations.push((
            live.state_revision,
            live.events
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
        ));
        assert!(provider
            .save(RuntimeSaveRequest {
                session_id: open.session_id.clone(),
                slot: "test".into(),
            })
            .unwrap_err()
            .starts_with("ASTRA_EMU_SHARED_SAVE_UNSUPPORTED"));
        provider.shutdown(open.session_id).unwrap();
        output_observations
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
}
