use std::{path::Path, sync::Arc};

use astra_emu_family_api::{
    AdvanceResponse, FamilyCapability, FamilyDescriptor, FamilyError, FamilyEvent, FamilyOpen,
    FamilyProvider, FamilyResult, FamilySession, FamilyStatus, FrameAlpha, FrameFormat, FrameInfo,
    FrameView, FrameVisitor, OpenRequest, OpenResponse, ProbeReport, ProbeRequest,
};
use rfvp::{
    host_api::{AudioStreamId, RfvpFileSystem, RfvpHost},
    hosted::{HostedBootConfig, HostedConfig, HostedLimits, HostedSession, HostedStepInput},
    script::parser::Nls,
    soft_render::PixelFormat,
};

use crate::{
    audio::AudioBridge,
    error, events,
    filesystem::NativeFileSystem,
    font_bindings,
    renderer::{NullAudio, NullRenderer, SessionClock},
    video::VideoPlayback,
};

pub fn fvp_descriptor() -> FamilyDescriptor {
    FamilyDescriptor {
        family_id: "fvp".into(),
        plugin_id: "astra.emu.fvp".into(),
        abi_fingerprint: astra_emu_family_api::FAMILY_ABI_FINGERPRINT.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec![
            FamilyCapability::CpuFrame,
            FamilyCapability::PcmAudio,
            FamilyCapability::NativeSave,
        ]
        .into(),
        supported_formats: vec!["fvp.hcb".into(), "fvp.bin".into()].into(),
    }
}

pub fn create_fvp_provider() -> Box<dyn FamilyProvider> {
    Box::new(FvpProvider::default())
}

#[derive(Default)]
pub struct FvpProvider {
    next_session_id: u64,
}

impl FamilyProvider for FvpProvider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        let descriptor = fvp_descriptor();
        descriptor.validate()?;
        Ok(descriptor)
    }
    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        request
            .validate()
            .map_err(|_| error::invalid("ASTRA_EMU_FVP_PROBE_PATH", "invalid game directory"))?;
        let mut fs = NativeFileSystem::new(&request.game_path).map_err(error::rfvp)?;
        let paths = hcb_paths(&mut fs).map_err(error::rfvp)?;
        if paths.is_empty() {
            return Ok(None);
        }
        if paths.len() != 1 {
            return Err(error::invalid(
                "ASTRA_EMU_FVP_PROBE_AMBIGUOUS",
                "FVP requires exactly one HCB entry in the game directory",
            ));
        }
        let mut paths = paths;
        paths.sort();
        let game_id = Path::new(&paths[0])
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                error::invalid("ASTRA_EMU_FVP_GAME_ID", "FVP game ID is not valid UTF-8")
            })?;
        let report = ProbeReport {
            family_id: "fvp".into(),
            game_id: game_id.into(),
            format: "fvp.hcb".into(),
            confidence_permyriad: 10_000,
        };
        report.validate().map_err(|_| {
            error::invalid(
                "ASTRA_EMU_FVP_PROBE_REPORT",
                "FVP probe produced an invalid report",
            )
        })?;
        Ok(Some(report))
    }
    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        let (response, session) = self.open_session(request)?;
        Ok(FamilyOpen {
            response,
            session: Box::new(session),
        })
    }
}

impl FvpProvider {
    pub(crate) fn open_session(
        &mut self,
        request: OpenRequest,
    ) -> FamilyResult<(OpenResponse, FvpSession)> {
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let mut fs = NativeFileSystem::new(&request.game_path).map_err(error::rfvp)?;
        let font_bindings = font_bindings::load_system_font_bindings()?;
        let hcb_paths = hcb_paths(&mut fs).map_err(error::rfvp)?;
        if hcb_paths.len() > 1 {
            return Err(error::invalid(
                "ASTRA_EMU_FVP_PROBE_AMBIGUOUS",
                "FVP requires exactly one HCB entry in the game directory",
            ));
        }
        let sink = match request.host.audio_sink {
            abi_stable::std_types::ROption::RSome(sink) => sink,
            abi_stable::std_types::ROption::RNone => {
                return Err(error::invalid(
                    "ASTRA_EMU_FVP_AUDIO_SINK",
                    "FVP requires the host audio sink",
                ))
            }
        };
        let mut clock = SessionClock::default();
        let mut hosted = HostedSession::new(
            HostedConfig {
                virtual_width: 800,
                virtual_height: 600,
                max_pending_events: 4096,
            },
            HostedLimits::default(),
        )
        .map_err(error::rfvp)?;
        hosted
            .set_system_font_bindings(font_bindings)
            .map_err(error::rfvp)?;
        {
            let mut host = SessionHost {
                fs: &mut fs,
                renderer: NullRenderer,
                audio: NullAudio,
                clock: &mut clock,
            };
            hosted
                .boot(
                    &mut host,
                    HostedBootConfig {
                        asset_root: ".",
                        hcb_extension: "hcb",
                        max_hcb_bytes: 64 * 1024 * 1024,
                        max_manifest_entries: 4096,
                        nls: Nls::ShiftJIS,
                    },
                )
                .map_err(error::rfvp)?;
        }
        let config = hosted.core().config();
        let frame_info = FrameInfo {
            width: config.virtual_width,
            height: config.virtual_height,
            stride: config.virtual_width.checked_mul(4).ok_or_else(|| {
                error::invalid("ASTRA_EMU_FVP_FRAME_SIZE", "frame stride overflows")
            })?,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Opaque,
            },
        };
        let frame_len = frame_info.required_bytes().ok_or_else(|| {
            error::invalid("ASTRA_EMU_FVP_FRAME_SIZE", "frame dimensions overflow")
        })?;
        let frame = vec![0_u8; frame_len];
        let audio = AudioBridge::new(sink)?;
        let id = self.next_session_id;
        self.next_session_id = self
            .next_session_id
            .checked_add(1)
            .ok_or_else(|| error::invalid("ASTRA_EMU_FVP_SESSION_ID", "session ID exhausted"))?;
        let session_id = format!("fvp-{id}");
        let mut session = FvpSession {
            hosted,
            fs,
            audio,
            clock,
            input_state: events::InputState::new(request.initial_window),
            video: None,
            video_audio_id: None,
            frame,
            frame_info,
            fatal: None,
        };
        session.render_frame()?;
        Ok((
            OpenResponse {
                session_id: session_id.into(),
                frame: frame_info,
                audio_format: abi_stable::std_types::ROption::RSome(astro_audio_format()),
            },
            session,
        ))
    }
}

fn hcb_paths(fs: &mut NativeFileSystem) -> rfvp::host_api::RfvpResult<Vec<String>> {
    let mut paths = Vec::new();
    fs.enumerate_by_extension(".", "hcb", &mut |path, _| {
        paths.push(path.to_owned());
        Ok(())
    })?;
    paths.sort();
    Ok(paths)
}

fn astro_audio_format() -> astra_emu_family_api::PcmFormatSpec {
    astra_emu_family_api::PcmFormatSpec {
        sample_rate: 48_000,
        channels: 2,
        format: astra_emu_family_api::PcmFormat::I16,
    }
}

struct SessionHost<'a> {
    fs: &'a mut NativeFileSystem,
    renderer: NullRenderer,
    audio: NullAudio,
    clock: &'a mut SessionClock,
}
impl RfvpHost for SessionHost<'_> {
    type FileSystem = NativeFileSystem;
    type Renderer = NullRenderer;
    type Audio = NullAudio;
    type Clock = SessionClock;
    fn fs(&mut self) -> &mut Self::FileSystem {
        self.fs
    }
    fn renderer(&mut self) -> &mut Self::Renderer {
        &mut self.renderer
    }
    fn audio(&mut self) -> &mut Self::Audio {
        &mut self.audio
    }
    fn clock(&mut self) -> &mut Self::Clock {
        self.clock
    }
    fn log(&mut self, _level: rfvp::host_api::RfvpLogLevel, _message: &str) {}
}

pub(crate) struct FvpSession {
    hosted: HostedSession,
    fs: NativeFileSystem,
    audio: AudioBridge,
    clock: SessionClock,
    input_state: events::InputState,
    video: Option<VideoPlayback>,
    video_audio_id: Option<AudioStreamId>,
    frame: Vec<u8>,
    frame_info: FrameInfo,
    fatal: Option<astra_emu_family_api::FamilyError>,
}

impl FvpSession {
    fn fail<T>(&mut self, value: FamilyError) -> FamilyResult<T> {
        self.fatal = Some(value.clone());
        Err(value)
    }
    fn ensure_live(&self) -> FamilyResult<()> {
        self.fatal.clone().map_or(Ok(()), Err)
    }
    fn render_frame(&mut self) -> FamilyResult<()> {
        let pixels = std::mem::take(&mut self.frame);
        let rendered = self
            .hosted
            .render_direct_surface(
                self.frame_info.width,
                self.frame_info.height,
                PixelFormat::Rgba8,
                pixels,
            )
            .map_err(error::rfvp)?;
        if rendered.len()
            != self.frame_info.required_bytes().ok_or_else(|| {
                error::invalid("ASTRA_EMU_FVP_FRAME_SIZE", "frame dimensions overflow")
            })?
        {
            return self.fail(error::invalid(
                "ASTRA_EMU_FVP_FRAME_BYTES",
                "RFVP returned an invalid frame allocation",
            ));
        }
        self.frame = rendered;
        if let Some(video) = self.video.as_ref() {
            if video.frame().len() != self.frame.len() {
                return self.fail(error::invalid(
                    "ASTRA_EMU_FVP_VIDEO_FRAME_BYTES",
                    "decoded WMV frame does not match the game viewport",
                ));
            }
            self.frame.copy_from_slice(video.frame());
        }
        Ok(())
    }

    fn start_video(&mut self, command: rfvp::hosted::HostedVideoOperation) -> FamilyResult<()> {
        if self.video.is_some() {
            return Err(error::invalid(
                "ASTRA_EMU_FVP_VIDEO_CONCURRENT",
                "RFVP requested a second video while one is active",
            ));
        }
        let rfvp::hosted::HostedVideoOperation::Play {
            resource_uri,
            byte_len,
            modal_with_audio,
            stage_width,
            stage_height,
        } = command
        else {
            return Err(error::invalid(
                "ASTRA_EMU_FVP_VIDEO_OPERATION",
                "RFVP sent a non-play operation to the movie starter",
            ));
        };
        if stage_width != self.frame_info.width || stage_height != self.frame_info.height {
            return Err(error::invalid(
                "ASTRA_EMU_FVP_VIDEO_VIEWPORT",
                "WMV stage dimensions do not match the configured game viewport",
            ));
        }
        let resource_name = resource_uri
            .strip_prefix("rfvp://")
            .unwrap_or(&resource_uri);
        let byte_len = usize::try_from(byte_len).map_err(|_| {
            error::invalid(
                "ASTRA_EMU_FVP_VIDEO_BYTES",
                "WMV resource length overflows usize",
            )
        })?;
        let mut host = SessionHost {
            fs: &mut self.fs,
            renderer: NullRenderer,
            audio: NullAudio,
            clock: &mut self.clock,
        };
        let bytes = self
            .hosted
            .read_resource(&mut host, resource_name, byte_len)
            .map_err(error::rfvp)?;
        if bytes.len() != byte_len {
            return Err(error::invalid(
                "ASTRA_EMU_FVP_VIDEO_BYTES",
                "WMV resource length changed during playback setup",
            ));
        }

        let bytes: Arc<[u8]> = bytes.into();
        let playback = VideoPlayback::open(
            Arc::clone(&bytes),
            stage_width,
            stage_height,
            modal_with_audio,
        )?;

        let video_audio_id = if modal_with_audio {
            let id = self.audio.allocate_video_stream_id()?;
            match self.audio.start_video_audio(id, Arc::clone(&bytes)) {
                Ok(true) => Some(id),
                Ok(false) => {
                    self.audio.release_stream_id(id);
                    None
                }
                Err(error) => {
                    self.audio.release_stream_id(id);
                    return Err(error);
                }
            }
        } else {
            None
        };
        self.video_audio_id = video_audio_id;
        self.video = Some(playback);
        Ok(())
    }

    fn finish_video(&mut self, complete_core: bool) -> FamilyResult<()> {
        let audio_id = self.video_audio_id.take();
        let audio_result = audio_id.map(|id| {
            let stop = self.audio.stop_video_audio(id);
            self.audio.release_stream_id(id);
            stop
        });
        let core_result = if complete_core {
            self.hosted.complete_video().map_err(error::rfvp)
        } else {
            Ok(())
        };
        self.video = None;
        if let Some(Err(error)) = audio_result {
            return Err(error);
        }
        core_result
    }
    fn send_audio(&mut self, operation: rfvp::hosted::HostedAudioOperation) -> FamilyResult<()> {
        let operation = match operation {
            rfvp::hosted::HostedAudioOperation::LoadResource {
                id,
                kind,
                resource_uri,
            } => {
                let resource_name = resource_uri
                    .strip_prefix("rfvp://")
                    .unwrap_or(&resource_uri);
                if resource_name.is_empty() {
                    return Err(error::invalid(
                        "ASTRA_EMU_FVP_AUDIO_RESOURCE_URI",
                        "audio resource URI is empty",
                    ));
                }
                let mut host = SessionHost {
                    fs: &mut self.fs,
                    renderer: NullRenderer,
                    audio: NullAudio,
                    clock: &mut self.clock,
                };
                let bytes = self
                    .hosted
                    .read_resource(&mut host, resource_name, usize::MAX)
                    .map_err(error::rfvp)?;
                rfvp::hosted::HostedAudioOperation::LoadEncoded { id, kind, bytes }
            }
            other => other,
        };
        self.audio.submit(operation)
    }

    fn sync_audio_playback_state(&mut self) -> FamilyResult<()> {
        let states = self.audio.playback_states()?;
        self.hosted
            .set_audio_playback_states(&states)
            .map_err(error::rfvp)
    }
}

impl FamilySession for FvpSession {
    fn advance(
        &mut self,
        elapsed_ns: u64,
        events: &[FamilyEvent],
    ) -> FamilyResult<AdvanceResponse> {
        self.ensure_live()?;
        let input = events::convert(events, &mut self.input_state)?;
        self.clock.advance_ns(elapsed_ns);
        self.sync_audio_playback_state()?;
        let delta = {
            let mut host = SessionHost {
                fs: &mut self.fs,
                renderer: NullRenderer,
                audio: NullAudio,
                clock: &mut self.clock,
            };
            self.hosted
                .step(&mut host, HostedStepInput { events: input })
                .map_err(error::rfvp)?
        };
        for operation in delta.audio {
            if let Err(value) = self.send_audio(operation) {
                return self.fail(value);
            }
        }
        let mut started_video = false;
        for operation in delta.video {
            match operation {
                rfvp::hosted::HostedVideoOperation::Play { .. } => {
                    if let Err(value) = self.start_video(operation) {
                        return self.fail(value);
                    }
                    started_video = true;
                }
                rfvp::hosted::HostedVideoOperation::Stop => {
                    if self.video.is_none() && self.video_audio_id.is_none() {
                        return self.fail(error::invalid(
                            "ASTRA_EMU_FVP_VIDEO_STOP",
                            "RFVP requested a movie stop without an active movie",
                        ));
                    }
                    if let Err(value) = self.finish_video(false) {
                        return self.fail(value);
                    }
                }
            }
        }
        let video_eof = if let Some(video) = self.video.as_mut() {
            // The elapsed time belongs to the hosted tick that emitted the
            // operation. A newly opened movie starts at its first frame and
            // consumes time beginning with the following family tick.
            match video.advance(if started_video { 0 } else { elapsed_ns }) {
                Ok(eof) => eof,
                Err(value) => return self.fail(value),
            }
        } else {
            false
        };
        if let Err(value) = self.render_frame() {
            return self.fail(value);
        }
        let video_complete = if video_eof {
            match self.video_audio_id {
                Some(id) => match self.audio.video_audio_playing(id) {
                    Ok(playing) => !playing,
                    Err(value) => return self.fail(value),
                },
                None => true,
            }
        } else {
            false
        };
        if video_complete {
            if let Err(value) = self.finish_video(true) {
                return self.fail(value);
            }
        }
        let status = if self.hosted.is_terminal() {
            FamilyStatus::Finished
        } else if self
            .video
            .as_ref()
            .is_some_and(VideoPlayback::modal_with_audio)
        {
            FamilyStatus::Waiting
        } else {
            FamilyStatus::Running
        };
        Ok(AdvanceResponse { status })
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        let view = FrameView::from_slice(&self.frame, self.frame_info)?;
        visitor.accept(view)
    }
    fn close(mut self: Box<Self>) -> FamilyResult<()> {
        let video_result = if self.video.is_some() || self.video_audio_id.is_some() {
            self.finish_video(true)
        } else {
            Ok(())
        };
        let audio_result = self.audio.close();
        video_result?;
        audio_result
    }
}
