//! Family-owned adaptation over RFVP's native core; no alternate VM or renderer.

mod audio;
mod renderer;

pub(crate) use audio::HostedAudioOperation;
use audio::RecordingAudio;
use renderer::SceneTracker;
use rfvp::host_api::{
    AudioStreamId, RfvpError, RfvpEvent, RfvpFile, RfvpFileSystem, RfvpHost, RfvpLogLevel,
    RfvpResult,
};
use rfvp::no_std_core::RfvpCore;
pub(crate) use rfvp::no_std_core::{
    RfvpBootConfig as HostedBootConfig, RfvpCoreConfig as HostedConfig,
};
use rfvp::subsystem::resources::text_manager::SystemFontBindings;

#[derive(Clone, Copy)]
pub(crate) struct HostedLimits {
    max_input_events: usize,
    max_scene_operations: usize,
    max_audio_operations: usize,
    max_audio_bytes: usize,
}
impl Default for HostedLimits {
    fn default() -> Self {
        Self {
            max_input_events: 256,
            max_scene_operations: 65_536,
            max_audio_operations: 4_096,
            max_audio_bytes: 16 * 1024 * 1024,
        }
    }
}

pub(crate) struct HostedStepInput {
    pub events: Vec<RfvpEvent>,
}
pub(crate) struct HostedStepDelta {
    pub audio: Vec<HostedAudioOperation>,
    pub visual_changed: bool,
}

pub(crate) struct HostedSession {
    core: RfvpCore,
    limits: HostedLimits,
    renderer: SceneTracker,
    audio_playback_states: Vec<(AudioStreamId, bool)>,
}
impl HostedSession {
    pub(crate) fn new(config: HostedConfig, limits: HostedLimits) -> RfvpResult<Self> {
        if limits.max_input_events == 0
            || limits.max_scene_operations == 0
            || limits.max_audio_operations == 0
            || limits.max_audio_bytes == 0
        {
            return Err(RfvpError::InvalidArgument);
        }
        Ok(Self {
            core: RfvpCore::new(config),
            limits,
            renderer: SceneTracker::new(limits.max_scene_operations),
            audio_playback_states: Vec::new(),
        })
    }
    pub(crate) fn core(&self) -> &RfvpCore {
        &self.core
    }
    pub(crate) fn core_mut(&mut self) -> &mut RfvpCore {
        &mut self.core
    }
    pub(crate) fn take_timeline_reset(&mut self) -> bool {
        self.core.take_timeline_reset()
    }
    pub(crate) fn shutdown(&mut self) -> RfvpResult<()> {
        self.core.shutdown()
    }
    pub(crate) fn is_terminal(&self) -> bool {
        self.core.hosted_terminal()
    }
    pub(crate) fn set_system_font_bindings(
        &mut self,
        bindings: SystemFontBindings,
    ) -> RfvpResult<()> {
        self.core.set_hosted_system_font_bindings(bindings)
    }
    pub(crate) fn set_audio_playback_states(
        &mut self,
        states: &[(AudioStreamId, bool)],
    ) -> RfvpResult<()> {
        let max =
            rfvp::host_api::BGM_LOGICAL_SLOT_COUNT + rfvp::host_api::SE_LOGICAL_SLOT_COUNT + 1;
        if states.len() > max {
            return Err(RfvpError::CapacityExceeded);
        }
        if states.iter().any(|(id, _)| id.slot_kind().is_none()) {
            return Err(RfvpError::InvalidArgument);
        }
        self.audio_playback_states.clear();
        self.audio_playback_states.extend_from_slice(states);
        Ok(())
    }
    pub(crate) fn read_resource<H: RfvpHost>(
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
    pub(crate) fn boot<H: RfvpHost>(
        &mut self,
        host: &mut H,
        config: HostedBootConfig<'_>,
    ) -> RfvpResult<()>
    where
        <H::FileSystem as RfvpFileSystem>::File: 'static,
    {
        let mut recording = RecordingHost {
            inner: host,
            renderer: &mut self.renderer,
            audio: RecordingAudio::new(self.limits, &[]),
        };
        self.core.boot(&mut recording, config)?;
        recording.audio.finish()?;
        recording.renderer.finish()?;
        if !recording.audio.operations.is_empty() || recording.renderer.has_frame() {
            return Err(RfvpError::InvalidData);
        }
        self.core.invalidate_host_render_cache();
        self.renderer.reset();
        Ok(())
    }
    pub(crate) fn step_with_gpu<H: RfvpHost>(
        &mut self,
        host: &mut H,
        input: HostedStepInput,
        gpu: &mut rfvp::rfvp_render::hosted::HostedGpuRenderer,
    ) -> RfvpResult<HostedStepDelta> {
        if input.events.len() > self.limits.max_input_events {
            return Err(RfvpError::CapacityExceeded);
        }
        for event in input.events {
            self.core.push_event(event)?;
        }
        let mut recording = RecordingHost {
            inner: host,
            renderer: &mut self.renderer,
            audio: RecordingAudio::new(self.limits, &self.audio_playback_states),
        };
        self.core.tick_with_gpu(&mut recording, gpu)?;
        recording.audio.finish()?;
        recording.renderer.finish()?;
        Ok(HostedStepDelta {
            audio: recording.audio.operations,
            visual_changed: recording.renderer.take_changed(),
        })
    }
}

struct RecordingHost<'host, 'states, H: RfvpHost> {
    inner: &'host mut H,
    renderer: &'host mut SceneTracker,
    audio: RecordingAudio<'states>,
}
impl<'states, H: RfvpHost> RfvpHost for RecordingHost<'_, 'states, H> {
    type FileSystem = H::FileSystem;
    type Renderer = SceneTracker;
    type Audio = RecordingAudio<'states>;
    type Clock = H::Clock;
    fn fs(&mut self) -> &mut Self::FileSystem {
        self.inner.fs()
    }
    fn renderer(&mut self) -> &mut Self::Renderer {
        self.renderer
    }
    fn audio(&mut self) -> &mut Self::Audio {
        &mut self.audio
    }
    fn clock(&mut self) -> &mut Self::Clock {
        self.inner.clock()
    }
    fn log(&mut self, level: RfvpLogLevel, message: &str) {
        self.inner.log(level, message);
    }
}
