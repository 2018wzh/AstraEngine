//! Standard-library entry point for RFVP's host-neutral core.
//!
//! The hosted surface is intentionally a small layer on top of the upstream
//! portable core.  It does not know about any embedding product, ABI, serializer
//! or native handles.  Its responsibility is to turn one core tick into one
//! bounded, typed delta that an embedding can validate and commit atomically.

use alloc::vec::Vec;

#[cfg(feature = "hosted")]
use crate::host_api::{
    AudioParams, AudioStreamDesc, AudioStreamId, ColorRgba, DrawSolidCommand, DrawSpriteCommand,
    EncodedAudioKind, HostedPixelBuffer, PixelBuffer, PixelFormat, PlatformCallbacks, RectI32,
    RfvpAudio, RfvpError, RfvpEvent, RfvpFile, RfvpFileSystem, RfvpHost, RfvpLogLevel,
    RfvpRenderer, RfvpResult, TextureDesc, TextureId, TextureRect,
};
#[cfg(feature = "hosted")]
const MAX_HOSTED_AUDIO_STATES: usize =
    crate::host_api::BGM_LOGICAL_SLOT_COUNT + crate::host_api::SE_LOGICAL_SLOT_COUNT;
pub use crate::no_std_core::{
    RfvpBootConfig as HostedBootConfig, RfvpCore, RfvpCoreConfig as HostedConfig,
    RfvpCoreRunState as HostedRunState, RfvpLoadedGame as HostedLoadedGame,
    RfvpResourceEntry as HostedResourceEntry, RfvpTickResult as HostedTickResult,
};
#[cfg(feature = "hosted")]
use crate::subsystem::resources::text_manager::SystemFontBindings;
#[cfg(feature = "hosted")]
pub use crate::vm_runner::HostedVmTraceRecord;

/// Increment only for a deliberately incompatible hosted-core wire contract.
pub const HOSTED_ABI_VERSION: u16 = 3;

/// Hard caps for one hosted transaction.  Every cap is fail-closed: a core
/// tick that exceeds it returns `CapacityExceeded`, and no partial delta is
/// returned to an embedding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostedLimits {
    pub max_input_events: usize,
    pub max_scene_operations: usize,
    pub max_texture_bytes: usize,
    pub max_audio_operations: usize,
    pub max_audio_bytes: usize,
    pub max_text_operations: usize,
    pub max_text_bytes: usize,
    /// Bounded structured hosted-core diagnostics retained in an Evidence
    /// transaction.  Shipping never records these entries.
    pub max_log_records: usize,
    /// Accounting bound for structured diagnostic records.  Hosted records do
    /// not contain host paths, payload bytes, or formatted engine messages.
    pub max_log_bytes: usize,
}

impl Default for HostedLimits {
    fn default() -> Self {
        Self {
            max_input_events: 256,
            max_scene_operations: 65_536,
            max_texture_bytes: 64 * 1024 * 1024,
            max_audio_operations: 4_096,
            max_audio_bytes: 16 * 1024 * 1024,
            max_text_operations: 256,
            max_text_bytes: 128 * 1024,
            max_log_records: 256,
            max_log_bytes: 16 * 1024,
        }
    }
}

/// Input accepted by exactly one hosted step.  The embedding owns event
/// collection; RFVP never reaches into a platform event queue.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostedStepInput {
    pub events: Vec<RfvpEvent>,
}

/// Texture payload retained only for the current transaction.  The host must
/// validate dimensions, byte count and resource policy before it allocates or
/// uploads a backend texture.
#[derive(Debug)]
pub struct HostedTextureData {
    pub id: TextureId,
    pub desc: TextureDesc,
    pub pixels: Option<HostedPixelBuffer>,
}

#[derive(Debug)]
pub struct HostedTextureUpdate {
    pub id: TextureId,
    pub rect: TextureRect,
    pub format: PixelFormat,
    pub pixels: HostedPixelBuffer,
}

/// Semantic scene operations, preserving upstream renderer data rather than
/// producing a software-rasterized frame or a product-specific DTO.
#[derive(Debug)]
pub enum HostedSceneOperation {
    CreateTexture(HostedTextureData),
    UpdateTexture(HostedTextureUpdate),
    DestroyTexture(TextureId),
    BeginFrame {
        width: u32,
        height: u32,
        clear: Option<ColorRgba>,
    },
    DrawSprite(DrawSpriteCommand),
    DrawSolid(DrawSolidCommand),
    EndFrame,
    Present,
}

/// Semantic audio operations.  Encoded and PCM payloads are bounded and are
/// copied only when the RFVP core actually emits a command.
#[derive(Debug, Clone, PartialEq)]
pub enum HostedAudioOperation {
    LoadResource {
        id: AudioStreamId,
        kind: EncodedAudioKind,
        resource_uri: String,
    },
    LoadEncoded {
        id: AudioStreamId,
        kind: EncodedAudioKind,
        bytes: Vec<u8>,
    },
    CreateStream {
        id: AudioStreamId,
        desc: AudioStreamDesc,
    },
    SubmitI16 {
        id: AudioStreamId,
        samples: Vec<i16>,
    },
    SubmitF32 {
        id: AudioStreamId,
        samples: Vec<f32>,
    },
    Play {
        id: AudioStreamId,
        params: AudioParams,
        fade_in_ms: u32,
    },
    Stop {
        id: AudioStreamId,
        fade_ms: u32,
    },
    Pause(AudioStreamId),
    Resume(AudioStreamId),
    SetParams {
        id: AudioStreamId,
        params: AudioParams,
    },
    SetMasterVolume(f32),
    DestroyStream(AudioStreamId),
    Tick {
        elapsed_us: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedVideoOperation {
    Play {
        resource_uri: String,
        byte_len: u64,
        modal_with_audio: bool,
        stage_width: u32,
        stage_height: u32,
    },
    Stop,
}

/// Semantic text emitted by script printing. Text remains in the hosted
/// session until the embedding explicitly turns it into a local-only lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedTextOperation {
    pub slot: u8,
    pub text: String,
}

/// Stable hosted-core diagnostic identity.  It deliberately carries no
/// formatted RFVP text so an embedding cannot accidentally persist a local
/// path, game payload, or platform handle in its observability pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedLogEvent {
    QuitRequested,
    CoreFailure,
    ResourceWarning,
    HostMessage,
}

impl HostedLogEvent {
    pub const fn code(self) -> &'static str {
        match self {
            Self::QuitRequested => "rfvp.host.quit_requested",
            Self::CoreFailure => "rfvp.core.failure",
            Self::ResourceWarning => "rfvp.resource.warning",
            Self::HostMessage => "rfvp.host.message",
        }
    }
}

/// A bounded diagnostic emitted by the hosted-core boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostedLogRecord {
    pub level: RfvpLogLevel,
    pub event: HostedLogEvent,
}

/// Per-step payload movement accounting. These counters are observational:
/// they are excluded from snapshots and all deterministic state hashes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostedCopyTelemetry {
    /// Required copies from RFVP-owned graphic storage into hosted capture.
    pub capture_bytes: u64,
    /// Payload bytes retained by typed hosted operations.
    pub operation_bytes: u64,
    /// Scene bytes transferred by moving an allocation into hosted capture.
    pub scene_moved_bytes: u64,
    /// Scene bytes copied because the renderer received borrowed storage.
    pub scene_copied_bytes: u64,
    /// PCM bytes transferred by moving their existing allocation.
    pub pcm_moved_bytes: u64,
    /// PCM bytes copied because a caller supplied only a borrowed slice.
    pub pcm_copied_bytes: u64,
}

/// Bounded visual transition state exposed only for Evidence runs. It contains
/// no scene payload and is not part of snapshot or replay state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostedVisualState {
    pub dissolve_kind: HostedDissolveKind,
    pub dissolve_color_id: u32,
    pub dissolve_alpha: f32,
    pub dissolve2_mode: u8,
    pub dissolve2_color_id: u32,
    pub dissolve2_alpha: f32,
    pub dissolve2_transitioning: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedDissolveKind {
    None,
    Static,
    ColoredFadeIn,
    ColoredFadeOut,
    MaskFadeIn,
    MaskFadeInOut,
    MaskFadeOut,
}

/// Phase markers are generic observer hooks.  RFVP never timestamps them;
/// the embedding is responsible for measuring its own execution environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostedPhase {
    Input,
    CoreTick,
    DeltaFinalize,
}

/// The only presentation and audio result produced by one hosted step.
///
/// An embedding may convert this into its own ABI payload, but must not commit
/// any part of it until all resource, profile and binding checks have passed.
#[derive(Debug)]
pub struct HostedStepDelta {
    pub tick: HostedTickResult,
    pub scene: Vec<HostedSceneOperation>,
    pub audio: Vec<HostedAudioOperation>,
    pub video: Vec<HostedVideoOperation>,
    pub text: Vec<HostedTextOperation>,
    pub logs: Vec<HostedLogRecord>,
    pub log_dropped_count: u32,
    pub copy_telemetry: HostedCopyTelemetry,
    pub visual_state: Option<HostedVisualState>,
    /// True when the direct Astra presentation state differs from the last
    /// presented frame. This is derived from typed renderer state, never a
    /// pixel or content hash.
    pub visual_changed: bool,
    /// Conservative pixel-space damage derived from typed draw commands and
    /// texture identities. It never scans or hashes framebuffer bytes.
    pub visual_damage: HostedVisualDamage,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostedVisualDamage {
    #[default]
    Unchanged,
    Full,
    Rect(HostedDamageRect),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostedDamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The production profile leaves instruction evidence disabled. Evidence is
/// opt-in and bounded to a fixed crash ring; it never formats or serializes an
/// opcode on the dispatch path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostedTraceProfile {
    #[default]
    Shipping,
    Evidence {
        crash_trace_capacity: usize,
    },
}

/// Optional embedding hook invoked only after a complete hosted delta has
/// been captured. Observers cannot mutate RFVP state or make a failed step
/// appear successful.
pub trait HostedStepObserver {
    fn on_step(&mut self, delta: &HostedStepDelta);
}

/// Optional, session-scoped observer for embedding-side profiling.  It has no
/// access to RFVP state and is not part of snapshots, replay hashes, or the
/// hosted wire format.
pub trait HostedExecutionObserver: HostedStepObserver {
    fn on_phase_begin(&mut self, _phase: HostedPhase) {}

    fn on_phase_end(&mut self, _phase: HostedPhase) {}
}

/// A session owns its pending input, core state and capture limits.  It is not
/// `Sync` by construction: concurrent games must use separate sessions and
/// separate hosts rather than a process-wide lock.
pub struct HostedSession {
    core: RfvpCore,
    limits: HostedLimits,
    // Renderer identity must outlive one captured delta. The core legitimately
    // reuses texture ids across ticks; recreating this recorder per tick turns
    // those updates into duplicate creates at every embedding boundary.
    renderer: RecordingRenderer,
    direct_renderer: Option<crate::soft_render::SoftRenderer>,
    capture_logs: bool,
    audio_playback_states: Vec<(AudioStreamId, bool)>,
}

impl HostedSession {
    pub fn new(config: HostedConfig, limits: HostedLimits) -> RfvpResult<Self> {
        if limits.max_input_events == 0
            || limits.max_scene_operations == 0
            || limits.max_texture_bytes == 0
            || limits.max_audio_operations == 0
            || limits.max_audio_bytes == 0
            || limits.max_text_operations == 0
            || limits.max_text_bytes == 0
            || limits.max_log_records == 0
            || limits.max_log_bytes == 0
        {
            return Err(RfvpError::InvalidArgument);
        }
        let mut core = RfvpCore::new(config);
        core.set_hosted_text_limits(limits.max_text_operations, limits.max_text_bytes);
        Ok(Self {
            core,
            limits,
            renderer: RecordingRenderer::new(limits),
            direct_renderer: None,
            capture_logs: false,
            audio_playback_states: Vec::new(),
        })
    }

    /// Selects the Astra hosted path where RFVP rasterizes into the Host surface directly.
    pub fn use_direct_surface(&mut self) {
        self.renderer.set_capture(false);
        self.core.invalidate_host_render_cache();
    }

    /// Applies successful synchronous Hook results before acquiring a presentation surface.
    pub fn replace_text(&mut self, slot: u8, text: &str) -> RfvpResult<()> {
        self.core.replace_hosted_text(slot, text)
    }

    /// Moves a Host lease allocation into RFVP, rasterizes in place, and returns that allocation.
    pub fn render_direct_surface(
        &mut self,
        width: u32,
        height: u32,
        format: crate::soft_render::PixelFormat,
        pixels: Vec<u8>,
    ) -> RfvpResult<Vec<u8>> {
        let renderer = match self.direct_renderer.as_mut() {
            Some(renderer) => renderer,
            None => self.direct_renderer.insert(
                crate::soft_render::SoftRenderer::new(width, height, format)
                    .map_err(|_| RfvpError::CapacityExceeded)?,
            ),
        };
        let _old = renderer
            .replace_framebuffer_pixels(width, height, format, pixels)
            .map_err(|_| RfvpError::InvalidData)?;
        self.core.render_hosted_software(renderer)?;
        renderer
            .take_framebuffer_pixels()
            .map_err(|_| RfvpError::InvalidData)
    }

    pub fn core(&self) -> &RfvpCore {
        &self.core
    }

    /// Supplies the host-owned faces for RFVP's original system font slots.
    /// This must be called before `boot`; no family is substituted for another.
    pub fn set_system_font_bindings(&mut self, bindings: SystemFontBindings) -> RfvpResult<()> {
        self.core.set_hosted_system_font_bindings(bindings)
    }

    pub fn limits(&self) -> HostedLimits {
        self.limits
    }

    pub fn set_trace_profile(&mut self, profile: HostedTraceProfile) -> RfvpResult<()> {
        let capacity = match profile {
            HostedTraceProfile::Shipping => 0,
            HostedTraceProfile::Evidence {
                crash_trace_capacity,
            } if crash_trace_capacity > 0 => crash_trace_capacity,
            HostedTraceProfile::Evidence { .. } => return Err(RfvpError::InvalidArgument),
        };
        self.core.set_hosted_trace_capacity(capacity)?;
        self.capture_logs = matches!(profile, HostedTraceProfile::Evidence { .. });
        Ok(())
    }

    pub fn crash_trace(&self) -> RfvpResult<Vec<HostedVmTraceRecord>> {
        self.core.hosted_trace()
    }

    pub fn quit_requested(&self) -> bool {
        self.core.quit_requested()
    }

    /// Returns the terminal state for the hosted lifecycle, including normal
    /// script completion rather than only an explicit host quit event.
    pub fn is_terminal(&self) -> bool {
        self.core.hosted_terminal()
    }

    /// Supplies the latest host mixer state for the next hosted tick.  The
    /// adapter owns the backend and calls this only after a complete command
    /// transaction; RFVP then exposes the result to its audio wait syscalls
    /// through the typed `RfvpAudio::is_playing` query.
    pub fn set_audio_playback_states(
        &mut self,
        states: &[(AudioStreamId, bool)],
    ) -> RfvpResult<()> {
        if states.len() > MAX_HOSTED_AUDIO_STATES {
            return Err(RfvpError::CapacityExceeded);
        }
        for (id, _) in states {
            if id.slot_kind().is_none() {
                return Err(RfvpError::InvalidArgument);
            }
        }
        self.audio_playback_states.clear();
        self.audio_playback_states.extend_from_slice(states);
        Ok(())
    }

    /// Acknowledge completion of the one outstanding hosted video operation.
    /// The embedding owns request-token matching; this method deliberately
    /// carries no product-specific identifier.
    pub fn complete_video(&mut self) -> RfvpResult<()> {
        self.core.complete_hosted_video()
    }

    /// Resolves a resource through the embedding-owned host port. The RFVP
    /// core never accesses an ambient or process-owned filesystem.
    pub fn read_resource<H: RfvpHost>(
        &mut self,
        host: &mut H,
        resource_uri: &str,
        max_bytes: usize,
    ) -> RfvpResult<Vec<u8>> {
        if resource_uri.is_empty() || resource_uri.contains('\0') || max_bytes == 0 {
            return Err(RfvpError::InvalidArgument);
        }
        match host.fs().open(resource_uri) {
            Ok(mut file) => file.read_to_vec(max_bytes),
            Err(RfvpError::NotFound) => self.core.read_hosted_resource(resource_uri, max_bytes),
            Err(error) => Err(error),
        }
    }

    /// Boot through the same constrained ports used by `step`.  A boot that
    /// attempts to present or emit audio is rejected rather than silently
    /// dropping output, because there is no commit delta at this lifecycle
    /// boundary.
    pub fn boot<H: RfvpHost>(
        &mut self,
        host: &mut H,
        config: HostedBootConfig<'_>,
    ) -> RfvpResult<()>
    where
        <H::FileSystem as RfvpFileSystem>::File: 'static,
    {
        let mut recording = RecordingHost::new(host, self.limits, self.capture_logs);
        self.core.boot(&mut recording, config)?;
        recording.finish()?;
        if !recording.renderer.operations.is_empty() || !recording.audio.operations.is_empty() {
            return Err(RfvpError::InvalidData);
        }
        self.core.invalidate_host_render_cache();
        self.renderer.reset_resources();
        Ok(())
    }

    /// Runs one bounded RFVP tick and captures all host-facing work in one
    /// typed delta.  Errors intentionally prevent a partial delta from
    /// escaping the session.
    pub fn step<H: RfvpHost>(
        &mut self,
        host: &mut H,
        input: HostedStepInput,
    ) -> RfvpResult<HostedStepDelta> {
        if input.events.len() > self.limits.max_input_events {
            return Err(RfvpError::CapacityExceeded);
        }
        for event in input.events {
            self.core.push_event(event)?;
        }

        let capture = self.renderer.capture;
        let renderer = std::mem::replace(
            &mut self.renderer,
            RecordingRenderer::with_capture(self.limits, capture),
        );
        let mut recording =
            RecordingHost::with_renderer(host, renderer, self.limits, self.capture_logs);
        recording
            .audio
            .set_playback_states(&self.audio_playback_states);
        let tick_result = self.core.tick(&mut recording);
        let finish_result = tick_result.and_then(|tick| recording.finish().map(|()| tick));
        self.renderer = recording.renderer;
        let tick = finish_result?;
        let text = self
            .core
            .take_hosted_text_events()
            .into_iter()
            .map(|event| HostedTextOperation {
                slot: event.slot,
                text: event.text,
            })
            .collect::<Vec<_>>();
        let logs = recording.logs.take()?;
        let scene_telemetry = self.renderer.take_copy_telemetry();
        let visual_changed = self.renderer.take_visual_changed();
        let visual_damage = self.renderer.take_visual_damage();
        let audio_telemetry = recording.audio.copy_telemetry;
        Ok(HostedStepDelta {
            tick,
            scene: self.renderer.take_operations(),
            audio: recording.audio.operations,
            video: {
                let mut operations = self
                    .core
                    .take_hosted_video_commands()
                    .into_iter()
                    .map(|command| HostedVideoOperation::Play {
                        resource_uri: command.resource_uri,
                        byte_len: command.byte_len,
                        modal_with_audio: matches!(
                            command.mode,
                            crate::subsystem::resources::videoplayer::MovieMode::ModalWithAudio
                        ),
                        stage_width: command.screen_w,
                        stage_height: command.screen_h,
                    })
                    .collect::<Vec<_>>();
                if self.core.take_hosted_video_stop() {
                    operations.push(HostedVideoOperation::Stop);
                }
                operations
            },
            text,
            log_dropped_count: recording.logs.dropped_count,
            logs,
            copy_telemetry: HostedCopyTelemetry {
                capture_bytes: scene_telemetry.capture_bytes,
                operation_bytes: scene_telemetry
                    .operation_bytes
                    .saturating_add(audio_telemetry.operation_bytes),
                scene_moved_bytes: scene_telemetry.scene_moved_bytes,
                scene_copied_bytes: scene_telemetry.scene_copied_bytes,
                pcm_moved_bytes: audio_telemetry.pcm_moved_bytes,
                pcm_copied_bytes: audio_telemetry.pcm_copied_bytes,
            },
            visual_state: self.capture_logs.then(|| self.core.hosted_visual_state()),
            visual_changed,
            visual_damage,
        })
    }

    /// Runs a step and publishes its complete delta to an observer. A failed
    /// core step never invokes the observer, preserving fail-stop commit
    /// semantics for recorders and evidence collectors.
    pub fn step_observed<H: RfvpHost, O: HostedStepObserver>(
        &mut self,
        host: &mut H,
        input: HostedStepInput,
        observer: &mut O,
    ) -> RfvpResult<HostedStepDelta>
    where
        <H::FileSystem as RfvpFileSystem>::File: 'static,
    {
        let delta = self.step(host, input)?;
        observer.on_step(&delta);
        Ok(delta)
    }

    /// Runs a hosted step with generic phase callbacks for an embedding-owned
    /// profiler.  The callbacks intentionally do not include timestamps or
    /// product-specific data.
    pub fn step_profiled<H: RfvpHost, O: HostedExecutionObserver>(
        &mut self,
        host: &mut H,
        input: HostedStepInput,
        observer: &mut O,
    ) -> RfvpResult<HostedStepDelta>
    where
        <H::FileSystem as RfvpFileSystem>::File: 'static,
    {
        observer.on_phase_begin(HostedPhase::Input);
        observer.on_phase_end(HostedPhase::Input);
        observer.on_phase_begin(HostedPhase::CoreTick);
        let delta = self.step(host, input);
        observer.on_phase_end(HostedPhase::CoreTick);
        let delta = delta?;
        observer.on_phase_begin(HostedPhase::DeltaFinalize);
        observer.on_step(&delta);
        observer.on_phase_end(HostedPhase::DeltaFinalize);
        Ok(delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_profile_requires_a_fixed_positive_ring_capacity() {
        let mut session = HostedSession::new(HostedConfig::default(), HostedLimits::default())
            .expect("default hosted session is valid");
        assert_eq!(
            session.set_trace_profile(HostedTraceProfile::Evidence {
                crash_trace_capacity: 0,
            }),
            Err(RfvpError::InvalidArgument)
        );
        session
            .set_trace_profile(HostedTraceProfile::Evidence {
                crash_trace_capacity: 32,
            })
            .expect("bounded evidence profile is valid");
        session
            .set_trace_profile(HostedTraceProfile::Shipping)
            .expect("shipping profile disables trace capture");
    }

    #[test]
    fn recording_audio_preserves_resource_identity_without_payload_bytes() {
        let mut audio = RecordingAudio::new(HostedLimits::default());
        audio
            .load_resource(AudioStreamId(3), EncodedAudioKind::Ogg, "audio/theme.ogg")
            .expect("resource audio is accepted");
        assert!(matches!(
            audio.operations.as_slice(),
            [HostedAudioOperation::LoadResource { resource_uri, .. }]
                if resource_uri == "audio/theme.ogg"
        ));
    }

    #[test]
    fn recording_audio_moves_owned_pcm_without_copying() {
        let mut audio = RecordingAudio::new(HostedLimits::default());
        let samples = vec![1_i16, -2, 3, -4];
        let allocation = samples.as_ptr();

        audio
            .submit_i16_owned(AudioStreamId(5), samples)
            .expect("owned PCM is accepted");

        let HostedAudioOperation::SubmitI16 { samples, .. } = &audio.operations[0] else {
            panic!("owned PCM must remain a typed submit operation");
        };
        assert_eq!(samples.as_ptr(), allocation);
        assert_eq!(audio.copy_telemetry.pcm_moved_bytes, 8);
        assert_eq!(audio.copy_telemetry.pcm_copied_bytes, 0);
        assert_eq!(audio.copy_telemetry.operation_bytes, 8);
    }

    #[test]
    fn recording_audio_accounts_borrowed_pcm_as_a_copy() {
        let mut audio = RecordingAudio::new(HostedLimits::default());
        audio
            .submit_f32(AudioStreamId(6), &[0.25, -0.25])
            .expect("borrowed PCM is accepted");

        assert_eq!(audio.copy_telemetry.pcm_moved_bytes, 0);
        assert_eq!(audio.copy_telemetry.pcm_copied_bytes, 8);
        assert_eq!(audio.copy_telemetry.operation_bytes, 8);
    }

    #[test]
    fn recording_renderer_preserves_texture_identity_across_deltas() {
        let mut renderer = RecordingRenderer::new(HostedLimits::default());
        let desc = TextureDesc {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            mip_count: 1,
        };
        let pixels = [1, 2, 3, 4];

        renderer
            .create_texture(TextureId(7), desc, Some(PixelBuffer::Borrowed(&pixels)))
            .expect("the first texture write is accepted");
        assert!(matches!(
            renderer.take_operations().as_slice(),
            [HostedSceneOperation::CreateTexture(texture)] if texture.id == TextureId(7)
        ));

        renderer
            .create_texture(TextureId(7), desc, Some(PixelBuffer::Borrowed(&pixels)))
            .expect("a same-id texture write is an update");
        assert!(matches!(
            renderer.take_operations().as_slice(),
            [HostedSceneOperation::UpdateTexture(update)] if update.id == TextureId(7)
        ));
    }

    #[test]
    fn recording_renderer_recreates_same_id_when_texture_shape_changes() {
        let mut renderer = RecordingRenderer::new(HostedLimits::default());
        let initial = TextureDesc {
            width: 1,
            height: 1,
            format: PixelFormat::Rgba8,
            mip_count: 1,
        };
        renderer
            .create_texture(
                TextureId(9),
                initial,
                Some(PixelBuffer::Owned(HostedPixelBuffer::from_bytes(vec![
                    1, 2, 3, 4,
                ]))),
            )
            .expect("initial texture is accepted");
        renderer.take_operations();

        let replacement = TextureDesc {
            width: 2,
            height: 1,
            format: PixelFormat::Rgba8,
            mip_count: 1,
        };
        renderer
            .create_texture(
                TextureId(9),
                replacement,
                Some(PixelBuffer::Owned(HostedPixelBuffer::from_bytes(vec![
                    1, 2, 3, 4, 5, 6, 7, 8,
                ]))),
            )
            .expect("shape changes recreate the retained texture generation");

        assert!(matches!(
            renderer.take_operations().as_slice(),
            [
                HostedSceneOperation::DestroyTexture(TextureId(9)),
                HostedSceneOperation::CreateTexture(texture)
            ] if texture.id == TextureId(9) && texture.desc == replacement
        ));
    }

    #[test]
    fn shipping_log_recorder_does_not_allocate_records() {
        let mut logs = RecordingLogs::new(HostedLimits::default(), false);
        logs.record(RfvpLogLevel::Error, "core failure");
        assert!(logs.take().expect("shipping logs are valid").is_empty());
        assert_eq!(logs.dropped_count, 0);
    }

    #[test]
    fn evidence_log_recorder_keeps_only_structured_identity() {
        let mut logs = RecordingLogs::new(HostedLimits::default(), true);
        logs.record(RfvpLogLevel::Info, "a local path must not escape");
        assert_eq!(
            logs.take().expect("evidence log is bounded"),
            vec![HostedLogRecord {
                level: RfvpLogLevel::Info,
                event: HostedLogEvent::HostMessage,
            }]
        );
    }

    #[test]
    fn evidence_log_overflow_fails_closed() {
        let limits = HostedLimits {
            max_log_records: 1,
            max_log_bytes: RecordingLogs::ACCOUNTED_BYTES,
            ..HostedLimits::default()
        };
        let mut logs = RecordingLogs::new(limits, true);
        logs.record(RfvpLogLevel::Info, "first");
        logs.record(RfvpLogLevel::Info, "second");
        assert_eq!(logs.take(), Err(RfvpError::CapacityExceeded));
        assert_eq!(logs.dropped_count, 1);
    }

    #[test]
    fn direct_frame_damage_is_unchanged_without_semantic_changes() {
        let frame = solid_frame(RectI32 {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        });
        assert_eq!(
            direct_frame_damage(&frame, &frame, &[]),
            HostedVisualDamage::Unchanged
        );
    }

    #[test]
    fn direct_frame_damage_unions_old_and_new_draw_bounds() {
        let previous = solid_frame(RectI32 {
            x: 10,
            y: 20,
            width: 30,
            height: 40,
        });
        let current = solid_frame(RectI32 {
            x: 25,
            y: 30,
            width: 30,
            height: 10,
        });
        assert_eq!(
            direct_frame_damage(&previous, &current, &[]),
            HostedVisualDamage::Rect(HostedDamageRect {
                x: 10,
                y: 20,
                width: 45,
                height: 40,
            })
        );
    }

    fn solid_frame(rect: RectI32) -> Vec<DirectFrameCommand> {
        vec![
            DirectFrameCommand::BeginFrame {
                width: 100,
                height: 100,
                clear: Some(ColorRgba::BLACK),
            },
            DirectFrameCommand::DrawSolid(DrawSolidCommand {
                rect,
                color: ColorRgba {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
                blend: crate::host_api::BlendMode::Opaque,
                scissor: None,
            }),
            DirectFrameCommand::EndFrame,
            DirectFrameCommand::Present,
        ]
    }
}

struct RecordingHost<'a, H: RfvpHost> {
    inner: &'a mut H,
    renderer: RecordingRenderer,
    audio: RecordingAudio,
    logs: RecordingLogs,
}

impl<'a, H: RfvpHost> RecordingHost<'a, H> {
    fn new(inner: &'a mut H, limits: HostedLimits, capture_logs: bool) -> Self {
        Self {
            inner,
            renderer: RecordingRenderer::new(limits),
            audio: RecordingAudio::new(limits),
            logs: RecordingLogs::new(limits, capture_logs),
        }
    }

    fn with_renderer(
        inner: &'a mut H,
        renderer: RecordingRenderer,
        limits: HostedLimits,
        capture_logs: bool,
    ) -> Self {
        Self {
            inner,
            renderer,
            audio: RecordingAudio::new(limits),
            logs: RecordingLogs::new(limits, capture_logs),
        }
    }

    fn finish(&self) -> RfvpResult<()> {
        self.renderer.finish()?;
        self.audio.finish()
    }
}

impl<H: RfvpHost> RfvpHost for RecordingHost<'_, H> {
    type FileSystem = H::FileSystem;
    type Renderer = RecordingRenderer;
    type Audio = RecordingAudio;
    type Clock = H::Clock;

    fn fs(&mut self) -> &mut Self::FileSystem {
        self.inner.fs()
    }

    fn renderer(&mut self) -> &mut Self::Renderer {
        &mut self.renderer
    }

    fn audio(&mut self) -> &mut Self::Audio {
        &mut self.audio
    }

    fn clock(&mut self) -> &mut Self::Clock {
        self.inner.clock()
    }

    fn log(&mut self, level: RfvpLogLevel, message: &str) {
        self.inner.log(level, message);
        self.logs.record(level, message);
    }

    fn platform_callbacks(&mut self) -> PlatformCallbacks {
        self.inner.platform_callbacks()
    }
}

struct RecordingLogs {
    limits: HostedLimits,
    enabled: bool,
    bytes: usize,
    records: Vec<HostedLogRecord>,
    dropped_count: u32,
}

impl RecordingLogs {
    const ACCOUNTED_BYTES: usize = 16;

    fn new(limits: HostedLimits, enabled: bool) -> Self {
        Self {
            limits,
            enabled,
            bytes: 0,
            records: Vec::new(),
            dropped_count: 0,
        }
    }

    fn record(&mut self, level: RfvpLogLevel, message: &str) {
        if !self.enabled {
            return;
        }
        let event = classify_hosted_log(level, message);
        let next_bytes = self.bytes.saturating_add(Self::ACCOUNTED_BYTES);
        if self.records.len() >= self.limits.max_log_records
            || next_bytes > self.limits.max_log_bytes
        {
            self.dropped_count = self.dropped_count.saturating_add(1);
            return;
        }
        self.bytes = next_bytes;
        self.records.push(HostedLogRecord { level, event });
    }

    fn take(&mut self) -> RfvpResult<Vec<HostedLogRecord>> {
        if self.dropped_count != 0 {
            return Err(RfvpError::CapacityExceeded);
        }
        Ok(std::mem::take(&mut self.records))
    }
}

fn classify_hosted_log(level: RfvpLogLevel, message: &str) -> HostedLogEvent {
    if message == "quit requested by host event" {
        HostedLogEvent::QuitRequested
    } else if matches!(level, RfvpLogLevel::Error) {
        HostedLogEvent::CoreFailure
    } else if matches!(level, RfvpLogLevel::Warn) {
        HostedLogEvent::ResourceWarning
    } else {
        HostedLogEvent::HostMessage
    }
}

#[derive(Debug, Clone, PartialEq)]
enum DirectFrameCommand {
    BeginFrame {
        width: u32,
        height: u32,
        clear: Option<ColorRgba>,
    },
    DrawSprite(DrawSpriteCommand),
    DrawSolid(DrawSolidCommand),
    EndFrame,
    Present,
}

struct RecordingRenderer {
    limits: HostedLimits,
    texture_bytes: usize,
    operations: Vec<HostedSceneOperation>,
    textures: Vec<(TextureId, TextureDesc)>,
    failure: Option<RfvpError>,
    copy_telemetry: HostedCopyTelemetry,
    capture: bool,
    direct_frame: Vec<DirectFrameCommand>,
    previous_direct_frame: Vec<DirectFrameCommand>,
    direct_resource_changed: bool,
    direct_changed_resources: Vec<TextureId>,
    visual_changed: bool,
    visual_damage: HostedVisualDamage,
}

impl RecordingRenderer {
    fn new(limits: HostedLimits) -> Self {
        Self::with_capture(limits, true)
    }

    fn with_capture(limits: HostedLimits, capture: bool) -> Self {
        Self {
            limits,
            texture_bytes: 0,
            operations: Vec::new(),
            textures: Vec::new(),
            failure: None,
            copy_telemetry: HostedCopyTelemetry::default(),
            capture,
            direct_frame: Vec::new(),
            previous_direct_frame: Vec::new(),
            direct_resource_changed: true,
            direct_changed_resources: Vec::new(),
            visual_changed: true,
            visual_damage: HostedVisualDamage::Full,
        }
    }

    fn set_capture(&mut self, capture: bool) {
        self.capture = capture;
        self.operations.clear();
        self.copy_telemetry = HostedCopyTelemetry::default();
        self.direct_frame.clear();
        self.previous_direct_frame.clear();
        self.direct_resource_changed = true;
        self.direct_changed_resources.clear();
        self.visual_changed = true;
        self.visual_damage = HostedVisualDamage::Full;
    }

    fn push(&mut self, operation: HostedSceneOperation) -> RfvpResult<()> {
        self.finish()?;
        if !self.capture {
            return Ok(());
        }
        if self.operations.len() >= self.limits.max_scene_operations {
            return Err(RfvpError::CapacityExceeded);
        }
        self.operations.push(operation);
        Ok(())
    }

    fn finish(&self) -> RfvpResult<()> {
        self.failure.map_or(Ok(()), Err)
    }

    fn take_operations(&mut self) -> Vec<HostedSceneOperation> {
        self.texture_bytes = 0;
        std::mem::take(&mut self.operations)
    }

    fn take_copy_telemetry(&mut self) -> HostedCopyTelemetry {
        core::mem::take(&mut self.copy_telemetry)
    }

    fn take_visual_changed(&mut self) -> bool {
        core::mem::take(&mut self.visual_changed)
    }

    fn take_visual_damage(&mut self) -> HostedVisualDamage {
        core::mem::take(&mut self.visual_damage)
    }

    fn push_direct(&mut self, command: DirectFrameCommand) -> RfvpResult<()> {
        self.finish()?;
        self.direct_frame.push(command);
        Ok(())
    }

    fn finish_direct_frame(&mut self) {
        let changed = self.direct_resource_changed
            || self.direct_frame.as_slice() != self.previous_direct_frame.as_slice();
        self.visual_damage = direct_frame_damage(
            &self.previous_direct_frame,
            &self.direct_frame,
            &self.direct_changed_resources,
        );
        core::mem::swap(&mut self.direct_frame, &mut self.previous_direct_frame);
        self.direct_frame.clear();
        self.direct_resource_changed = false;
        self.direct_changed_resources.clear();
        self.visual_changed = changed;
    }

    fn reset_resources(&mut self) {
        self.texture_bytes = 0;
        self.operations.clear();
        self.textures.clear();
        self.failure = None;
        self.copy_telemetry = HostedCopyTelemetry::default();
        self.direct_frame.clear();
        self.previous_direct_frame.clear();
        self.direct_resource_changed = true;
        self.direct_changed_resources.clear();
        self.visual_changed = true;
        self.visual_damage = HostedVisualDamage::Full;
    }

    fn reserve_texture_bytes(&mut self, additional: usize) -> RfvpResult<()> {
        self.finish()?;
        self.texture_bytes = self
            .texture_bytes
            .checked_add(additional)
            .ok_or(RfvpError::CapacityExceeded)?;
        if self.texture_bytes > self.limits.max_texture_bytes {
            return Err(RfvpError::CapacityExceeded);
        }
        Ok(())
    }
}

impl RfvpRenderer for RecordingRenderer {
    fn create_texture(
        &mut self,
        id: TextureId,
        desc: TextureDesc,
        pixels: Option<PixelBuffer<'_>>,
    ) -> RfvpResult<()> {
        self.finish()?;
        validate_texture_desc(desc)?;
        if !self.capture {
            if let Some(pixels) = pixels.as_ref() {
                validate_texture_pixels(desc, pixels.len())?;
            }
            if let Some((_, known_desc)) = self.textures.iter_mut().find(|(known, _)| *known == id)
            {
                *known_desc = desc;
            } else {
                self.textures.push((id, desc));
            }
            self.direct_resource_changed = true;
            remember_texture(&mut self.direct_changed_resources, id);
            return Ok(());
        }
        let pixels = pixels
            .map(|pixels| {
                let byte_len = pixels.len();
                validate_texture_pixels(desc, byte_len)?;
                self.reserve_texture_bytes(byte_len)?;
                self.copy_telemetry.capture_bytes = self
                    .copy_telemetry
                    .capture_bytes
                    .saturating_add(byte_len as u64);
                self.copy_telemetry.operation_bytes = self
                    .copy_telemetry
                    .operation_bytes
                    .saturating_add(byte_len as u64);
                let pixels = match pixels {
                    PixelBuffer::Borrowed(bytes) => {
                        self.copy_telemetry.scene_copied_bytes = self
                            .copy_telemetry
                            .scene_copied_bytes
                            .saturating_add(byte_len as u64);
                        HostedPixelBuffer::from_bytes(bytes.to_vec())
                    }
                    PixelBuffer::Owned(bytes) => {
                        self.copy_telemetry.scene_moved_bytes = self
                            .copy_telemetry
                            .scene_moved_bytes
                            .saturating_add(byte_len as u64);
                        bytes
                    }
                };
                Ok::<HostedPixelBuffer, RfvpError>(pixels)
            })
            .transpose()?;
        if let Some(index) = self.textures.iter().position(|(known, _)| *known == id) {
            if self.textures[index].1 != desc {
                let pixels = pixels.ok_or(RfvpError::InvalidData)?;
                self.push(HostedSceneOperation::DestroyTexture(id))?;
                self.textures[index].1 = desc;
                return self.push(HostedSceneOperation::CreateTexture(HostedTextureData {
                    id,
                    desc,
                    pixels: Some(pixels),
                }));
            }
            let pixels = pixels.ok_or(RfvpError::InvalidData)?;
            return self.push(HostedSceneOperation::UpdateTexture(HostedTextureUpdate {
                id,
                rect: TextureRect {
                    x: 0,
                    y: 0,
                    width: desc.width,
                    height: desc.height,
                },
                format: desc.format,
                pixels,
            }));
        }
        self.textures.push((id, desc));
        self.push(HostedSceneOperation::CreateTexture(HostedTextureData {
            id,
            desc,
            pixels,
        }))
    }

    fn update_texture(
        &mut self,
        id: TextureId,
        rect: TextureRect,
        pixels: PixelBuffer<'_>,
    ) -> RfvpResult<()> {
        self.finish()?;
        let Some((_, desc)) = self.textures.iter().find(|(known, _)| *known == id) else {
            return Err(RfvpError::NotFound);
        };
        let desc = *desc;
        if rect.width == 0
            || rect.height == 0
            || rect
                .x
                .checked_add(rect.width)
                .ok_or(RfvpError::CapacityExceeded)?
                > desc.width
            || rect
                .y
                .checked_add(rect.height)
                .ok_or(RfvpError::CapacityExceeded)?
                > desc.height
        {
            return Err(RfvpError::InvalidArgument);
        }
        if !self.capture {
            validate_texture_region(desc, rect, pixels.len())?;
            self.direct_resource_changed = true;
            remember_texture(&mut self.direct_changed_resources, id);
            return Ok(());
        }
        let byte_len = pixels.len();
        validate_texture_region(desc, rect, byte_len)?;
        self.reserve_texture_bytes(byte_len)?;
        self.copy_telemetry.capture_bytes = self
            .copy_telemetry
            .capture_bytes
            .saturating_add(byte_len as u64);
        self.copy_telemetry.operation_bytes = self
            .copy_telemetry
            .operation_bytes
            .saturating_add(byte_len as u64);
        let pixels = match pixels {
            PixelBuffer::Borrowed(bytes) => {
                self.copy_telemetry.scene_copied_bytes = self
                    .copy_telemetry
                    .scene_copied_bytes
                    .saturating_add(byte_len as u64);
                HostedPixelBuffer::from_bytes(bytes.to_vec())
            }
            PixelBuffer::Owned(bytes) => {
                self.copy_telemetry.scene_moved_bytes = self
                    .copy_telemetry
                    .scene_moved_bytes
                    .saturating_add(byte_len as u64);
                bytes
            }
        };
        self.push(HostedSceneOperation::UpdateTexture(HostedTextureUpdate {
            id,
            rect,
            format: desc.format,
            pixels,
        }))
    }

    fn destroy_texture(&mut self, id: TextureId) {
        if self.failure.is_some() {
            return;
        }
        if !self.textures.iter().any(|(known, _)| *known == id) {
            self.failure = Some(RfvpError::NotFound);
            return;
        }
        if let Err(error) = self.push(HostedSceneOperation::DestroyTexture(id)) {
            self.failure = Some(error);
            return;
        }
        self.textures.retain(|(known, _)| *known != id);
        if !self.capture {
            self.direct_resource_changed = true;
            remember_texture(&mut self.direct_changed_resources, id);
        }
    }

    fn begin_frame(&mut self, width: u32, height: u32, clear: Option<ColorRgba>) -> RfvpResult<()> {
        if width == 0 || height == 0 {
            return Err(RfvpError::InvalidArgument);
        }
        if self.capture {
            self.push(HostedSceneOperation::BeginFrame {
                width,
                height,
                clear,
            })
        } else {
            self.push_direct(DirectFrameCommand::BeginFrame {
                width,
                height,
                clear,
            })
        }
    }

    fn draw_sprite(&mut self, command: &DrawSpriteCommand) -> RfvpResult<()> {
        if self.capture {
            self.push(HostedSceneOperation::DrawSprite(*command))
        } else {
            self.push_direct(DirectFrameCommand::DrawSprite(*command))
        }
    }

    fn draw_solid(&mut self, command: &DrawSolidCommand) -> RfvpResult<()> {
        if self.capture {
            self.push(HostedSceneOperation::DrawSolid(*command))
        } else {
            self.push_direct(DirectFrameCommand::DrawSolid(*command))
        }
    }

    fn end_frame(&mut self) -> RfvpResult<()> {
        if self.capture {
            self.push(HostedSceneOperation::EndFrame)
        } else {
            self.push_direct(DirectFrameCommand::EndFrame)
        }
    }

    fn present(&mut self) -> RfvpResult<()> {
        if self.capture {
            self.push(HostedSceneOperation::Present)
        } else {
            self.push_direct(DirectFrameCommand::Present)?;
            self.finish_direct_frame();
            Ok(())
        }
    }
}

fn remember_texture(changed: &mut Vec<TextureId>, id: TextureId) {
    if !changed.contains(&id) {
        changed.push(id);
    }
}

fn direct_frame_damage(
    previous: &[DirectFrameCommand],
    current: &[DirectFrameCommand],
    changed_resources: &[TextureId],
) -> HostedVisualDamage {
    let Some((width, height, clear)) = direct_frame_header(current) else {
        return HostedVisualDamage::Full;
    };
    let Some((previous_width, previous_height, previous_clear)) = direct_frame_header(previous)
    else {
        return HostedVisualDamage::Full;
    };
    if width != previous_width || height != previous_height || clear != previous_clear {
        return HostedVisualDamage::Full;
    }

    let mut prefix = 0usize;
    while prefix < previous.len() && prefix < current.len() && previous[prefix] == current[prefix] {
        prefix += 1;
    }
    let mut previous_suffix = previous.len();
    let mut current_suffix = current.len();
    while previous_suffix > prefix
        && current_suffix > prefix
        && previous[previous_suffix - 1] == current[current_suffix - 1]
    {
        previous_suffix -= 1;
        current_suffix -= 1;
    }

    let mut bounds = None;
    for command in previous[prefix..previous_suffix]
        .iter()
        .chain(current[prefix..current_suffix].iter())
        .chain(previous.iter().filter(|command| {
            command_texture(command).is_some_and(|id| changed_resources.contains(&id))
        }))
        .chain(current.iter().filter(|command| {
            command_texture(command).is_some_and(|id| changed_resources.contains(&id))
        }))
    {
        let Some(rect) = command_bounds(command, width, height) else {
            continue;
        };
        bounds = Some(union_rect(bounds, rect));
    }

    match bounds {
        None => HostedVisualDamage::Unchanged,
        Some(rect)
            if rect.x == 0 && rect.y == 0 && rect.width == width && rect.height == height =>
        {
            HostedVisualDamage::Full
        }
        Some(rect) => HostedVisualDamage::Rect(rect),
    }
}

fn direct_frame_header(frame: &[DirectFrameCommand]) -> Option<(u32, u32, Option<ColorRgba>)> {
    frame.iter().find_map(|command| match command {
        DirectFrameCommand::BeginFrame {
            width,
            height,
            clear,
        } => Some((*width, *height, *clear)),
        _ => None,
    })
}

fn command_texture(command: &DirectFrameCommand) -> Option<TextureId> {
    match command {
        DirectFrameCommand::DrawSprite(command) => Some(command.texture),
        _ => None,
    }
}

fn command_bounds(
    command: &DirectFrameCommand,
    width: u32,
    height: u32,
) -> Option<HostedDamageRect> {
    let (min_x, min_y, max_x, max_y, scissor) = match command {
        DirectFrameCommand::DrawSprite(command) => {
            let mut min_x = f32::INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            for vertex in command.vertices {
                if !vertex.position[0].is_finite() || !vertex.position[1].is_finite() {
                    return Some(HostedDamageRect {
                        x: 0,
                        y: 0,
                        width,
                        height,
                    });
                }
                min_x = min_x.min(vertex.position[0]);
                min_y = min_y.min(vertex.position[1]);
                max_x = max_x.max(vertex.position[0]);
                max_y = max_y.max(vertex.position[1]);
            }
            (min_x, min_y, max_x, max_y, command.scissor)
        }
        DirectFrameCommand::DrawSolid(command) => (
            command.rect.x as f32,
            command.rect.y as f32,
            command.rect.x.saturating_add(command.rect.width) as f32,
            command.rect.y.saturating_add(command.rect.height) as f32,
            command.scissor,
        ),
        _ => return None,
    };
    let mut left = min_x.floor().max(0.0) as i64;
    let mut top = min_y.floor().max(0.0) as i64;
    let mut right = max_x.ceil().max(0.0) as i64;
    let mut bottom = max_y.ceil().max(0.0) as i64;
    if let Some(RectI32 {
        x,
        y,
        width: clip_width,
        height: clip_height,
    }) = scissor
    {
        left = left.max(i64::from(x));
        top = top.max(i64::from(y));
        right = right.min(i64::from(x).saturating_add(i64::from(clip_width)));
        bottom = bottom.min(i64::from(y).saturating_add(i64::from(clip_height)));
    }
    left = left.clamp(0, i64::from(width));
    top = top.clamp(0, i64::from(height));
    right = right.clamp(0, i64::from(width));
    bottom = bottom.clamp(0, i64::from(height));
    if right <= left || bottom <= top {
        return None;
    }
    Some(HostedDamageRect {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

fn union_rect(current: Option<HostedDamageRect>, next: HostedDamageRect) -> HostedDamageRect {
    let Some(current) = current else {
        return next;
    };
    let left = current.x.min(next.x);
    let top = current.y.min(next.y);
    let right = current
        .x
        .saturating_add(current.width)
        .max(next.x.saturating_add(next.width));
    let bottom = current
        .y
        .saturating_add(current.height)
        .max(next.y.saturating_add(next.height));
    HostedDamageRect {
        x: left,
        y: top,
        width: right.saturating_sub(left),
        height: bottom.saturating_sub(top),
    }
}

fn validate_texture_desc(desc: TextureDesc) -> RfvpResult<()> {
    if desc.width == 0 || desc.height == 0 || desc.mip_count == 0 {
        return Err(RfvpError::InvalidArgument);
    }
    match desc.format {
        PixelFormat::Rgba8
        | PixelFormat::Bgra8
        | PixelFormat::Rgb8
        | PixelFormat::Luma8
        | PixelFormat::LumaA8
        | PixelFormat::Alpha8 => Ok(()),
    }
}

fn texture_bytes_per_pixel(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Rgba8 | PixelFormat::Bgra8 => 4,
        PixelFormat::Rgb8 => 3,
        PixelFormat::LumaA8 => 2,
        PixelFormat::Luma8 | PixelFormat::Alpha8 => 1,
    }
}

fn validate_texture_pixels(desc: TextureDesc, bytes: usize) -> RfvpResult<()> {
    let expected = (desc.width as usize)
        .checked_mul(desc.height as usize)
        .and_then(|pixels| pixels.checked_mul(texture_bytes_per_pixel(desc.format)))
        .ok_or(RfvpError::CapacityExceeded)?;
    if bytes != expected {
        return Err(RfvpError::InvalidData);
    }
    Ok(())
}

fn validate_texture_region(desc: TextureDesc, rect: TextureRect, bytes: usize) -> RfvpResult<()> {
    validate_texture_pixels(
        TextureDesc {
            width: rect.width,
            height: rect.height,
            format: desc.format,
            mip_count: 1,
        },
        bytes,
    )
}

struct RecordingAudio {
    limits: HostedLimits,
    audio_bytes: usize,
    operations: Vec<HostedAudioOperation>,
    failure: Option<RfvpError>,
    copy_telemetry: HostedCopyTelemetry,
    playback_states: Vec<(AudioStreamId, bool)>,
}

impl RecordingAudio {
    fn new(limits: HostedLimits) -> Self {
        Self {
            limits,
            audio_bytes: 0,
            operations: Vec::new(),
            failure: None,
            copy_telemetry: HostedCopyTelemetry::default(),
            playback_states: Vec::new(),
        }
    }

    fn set_playback_states(&mut self, states: &[(AudioStreamId, bool)]) {
        self.playback_states.clear();
        self.playback_states.extend_from_slice(states);
    }

    fn push(&mut self, operation: HostedAudioOperation) -> RfvpResult<()> {
        self.finish()?;
        if self.operations.len() >= self.limits.max_audio_operations {
            return Err(RfvpError::CapacityExceeded);
        }
        self.operations.push(operation);
        Ok(())
    }

    fn finish(&self) -> RfvpResult<()> {
        self.failure.map_or(Ok(()), Err)
    }

    fn reserve_audio_bytes(&mut self, additional: usize) -> RfvpResult<()> {
        self.finish()?;
        self.audio_bytes = self
            .audio_bytes
            .checked_add(additional)
            .ok_or(RfvpError::CapacityExceeded)?;
        if self.audio_bytes > self.limits.max_audio_bytes {
            return Err(RfvpError::CapacityExceeded);
        }
        Ok(())
    }
}

impl RfvpAudio for RecordingAudio {
    fn load_resource(
        &mut self,
        id: AudioStreamId,
        kind: EncodedAudioKind,
        resource_uri: &str,
    ) -> RfvpResult<()> {
        if resource_uri.is_empty() {
            return Err(RfvpError::InvalidArgument);
        }
        self.push(HostedAudioOperation::LoadResource {
            id,
            kind,
            resource_uri: resource_uri.into(),
        })
    }

    fn load_encoded(
        &mut self,
        id: AudioStreamId,
        kind: EncodedAudioKind,
        bytes: &[u8],
    ) -> RfvpResult<()> {
        self.reserve_audio_bytes(bytes.len())?;
        self.copy_telemetry.operation_bytes = self
            .copy_telemetry
            .operation_bytes
            .saturating_add(bytes.len() as u64);
        self.push(HostedAudioOperation::LoadEncoded {
            id,
            kind,
            bytes: bytes.to_vec(),
        })
    }

    fn load_encoded_owned(
        &mut self,
        id: AudioStreamId,
        kind: EncodedAudioKind,
        bytes: Vec<u8>,
    ) -> RfvpResult<()> {
        self.reserve_audio_bytes(bytes.len())?;
        self.copy_telemetry.operation_bytes = self
            .copy_telemetry
            .operation_bytes
            .saturating_add(bytes.len() as u64);
        self.push(HostedAudioOperation::LoadEncoded { id, kind, bytes })
    }

    fn create_stream(&mut self, id: AudioStreamId, desc: AudioStreamDesc) -> RfvpResult<()> {
        self.push(HostedAudioOperation::CreateStream { id, desc })
    }

    fn submit_i16(&mut self, id: AudioStreamId, samples: &[i16]) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<i16>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.copy_telemetry.operation_bytes = self
            .copy_telemetry
            .operation_bytes
            .saturating_add(bytes as u64);
        self.copy_telemetry.pcm_copied_bytes = self
            .copy_telemetry
            .pcm_copied_bytes
            .saturating_add(bytes as u64);
        self.push(HostedAudioOperation::SubmitI16 {
            id,
            samples: samples.to_vec(),
        })
    }

    fn submit_i16_owned(&mut self, id: AudioStreamId, samples: Vec<i16>) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<i16>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.copy_telemetry.operation_bytes = self
            .copy_telemetry
            .operation_bytes
            .saturating_add(bytes as u64);
        self.copy_telemetry.pcm_moved_bytes = self
            .copy_telemetry
            .pcm_moved_bytes
            .saturating_add(bytes as u64);
        self.push(HostedAudioOperation::SubmitI16 { id, samples })
    }

    fn submit_f32(&mut self, id: AudioStreamId, samples: &[f32]) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<f32>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.copy_telemetry.operation_bytes = self
            .copy_telemetry
            .operation_bytes
            .saturating_add(bytes as u64);
        self.copy_telemetry.pcm_copied_bytes = self
            .copy_telemetry
            .pcm_copied_bytes
            .saturating_add(bytes as u64);
        self.push(HostedAudioOperation::SubmitF32 {
            id,
            samples: samples.to_vec(),
        })
    }

    fn submit_f32_owned(&mut self, id: AudioStreamId, samples: Vec<f32>) -> RfvpResult<()> {
        let bytes = samples
            .len()
            .checked_mul(core::mem::size_of::<f32>())
            .ok_or(RfvpError::CapacityExceeded)?;
        self.reserve_audio_bytes(bytes)?;
        self.copy_telemetry.operation_bytes = self
            .copy_telemetry
            .operation_bytes
            .saturating_add(bytes as u64);
        self.copy_telemetry.pcm_moved_bytes = self
            .copy_telemetry
            .pcm_moved_bytes
            .saturating_add(bytes as u64);
        self.push(HostedAudioOperation::SubmitF32 { id, samples })
    }

    fn play(&mut self, id: AudioStreamId, params: AudioParams, fade_in_ms: u32) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Play {
            id,
            params,
            fade_in_ms,
        })
    }

    fn stop(&mut self, id: AudioStreamId, fade_ms: u32) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Stop { id, fade_ms })
    }

    fn pause(&mut self, id: AudioStreamId) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Pause(id))
    }

    fn resume(&mut self, id: AudioStreamId) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Resume(id))
    }

    fn set_params(&mut self, id: AudioStreamId, params: AudioParams) -> RfvpResult<()> {
        self.push(HostedAudioOperation::SetParams { id, params })
    }

    fn set_master_volume(&mut self, volume: f32) -> RfvpResult<()> {
        if !volume.is_finite() {
            return Err(RfvpError::InvalidArgument);
        }
        self.push(HostedAudioOperation::SetMasterVolume(volume))
    }

    fn destroy_stream(&mut self, id: AudioStreamId) {
        if self.failure.is_some() {
            return;
        }
        if let Err(error) = self.push(HostedAudioOperation::DestroyStream(id)) {
            self.failure = Some(error);
        }
    }

    fn tick(&mut self, elapsed_us: u64) -> RfvpResult<()> {
        self.push(HostedAudioOperation::Tick { elapsed_us })
    }

    fn is_playing(&mut self, id: AudioStreamId) -> RfvpResult<bool> {
        Ok(self
            .playback_states
            .iter()
            .find(|(state_id, _)| *state_id == id)
            .map(|(_, playing)| *playing)
            .unwrap_or(false))
    }
}
