use std::path::Path;
use std::sync::Arc;

use astra_emu_family_api::{
    AdvanceResponse, FamilyCapability, FamilyDescriptor, FamilyError, FamilyEvent, FamilyOpen,
    FamilyProvider, FamilyResult, FamilySession, FamilyStatus, FrameAlpha, FrameFormat,
    FrameIdentity, FrameInfo, FramePoll, FramePublisher, FrameRowOrder, OpenRequest, OpenResponse,
    ProbeReport, ProbeRequest,
};
use rfvp::{
    host_api::{RfvpFileSystem, RfvpHost},
    script::parser::Nls,
};

use crate::{
    audio::AudioBridge,
    embedding::{
        HostedAudioOperation, HostedBootConfig, HostedConfig, HostedLimits, HostedSession,
        HostedStepInput,
    },
    error, events,
    filesystem::NativeFileSystem,
    font_bindings,
    renderer::{NullAudio, NullRenderer, SessionClock},
};

pub fn fvp_descriptor() -> FamilyDescriptor {
    FamilyDescriptor {
        configuration: vec![astra_emu_family_api::ConfigField {
            id: "script_encoding".into(),
            label: "Script encoding".into(),
            group: "Script".into(),
            kind: astra_emu_family_api::ConfigKind::Enum {
                choices: vec!["shift_jis".into(), "gbk".into(), "utf8".into()].into(),
            },
            default: astra_emu_family_api::ConfigValue::Enum("shift_jis".into()),
        }]
        .into(),
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
        tracing::debug!(
            event = "astra.emu.fvp.probe.complete",
            script_count = paths.len()
        );
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
        let _span =
            tracing::info_span!("fvp_session_open", event = "astra.emu.fvp.session.opening")
                .entered();
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let configuration = astra_emu_family_api::resolve_config(
            &descriptor.configuration,
            &request.configuration,
        )?;
        let nls = match &configuration[0].value {
            astra_emu_family_api::ConfigValue::Enum(value) if value == "gbk" => Nls::GBK,
            astra_emu_family_api::ConfigValue::Enum(value) if value == "utf8" => Nls::UTF8,
            astra_emu_family_api::ConfigValue::Enum(value) if value == "shift_jis" => Nls::ShiftJIS,
            _ => {
                return Err(error::invalid(
                    "ASTRA_EMU_FAMILY_CONFIG",
                    "script_encoding is invalid",
                ))
            }
        };
        let mut fs = NativeFileSystem::new(&request.game_path).map_err(error::rfvp)?;
        rfvp::utils::file::set_base_path(&request.game_path);
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
                        nls,
                        ..HostedBootConfig::default()
                    },
                )
                .map_err(|error| error::rfvp_operation(error, hosted.core().last_operation()))?;
        }
        let config = hosted.core().config();
        let frame_info = FrameInfo {
            width: config.virtual_width,
            height: config.virtual_height,
            logical_width: config.virtual_width,
            logical_height: config.virtual_height,
            stride: config.virtual_width.checked_mul(4).ok_or_else(|| {
                error::invalid("ASTRA_EMU_FVP_FRAME_SIZE", "frame stride overflows")
            })?,
            format: FrameFormat::Rgba8Srgb {
                alpha: FrameAlpha::Ignored,
            },
            rows: FrameRowOrder::TopToBottom,
        };
        let frame = FramePublisher::new(request.session_epoch)?;
        let gpu = crate::gpu::GpuRenderer::new(frame_info.width, frame_info.height)?;
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
            input_state: events::InputState::new(
                request.initial_window,
                frame_info.logical_width,
                frame_info.logical_height,
            ),
            frame,
            frame_info,
            gpu,
            fatal: None,
            game_dirty: true,
            pixels: Default::default(),
            readback_slot: None,
        };
        session.render_frame()?;
        tracing::info!(
            event = "astra.emu.fvp.session.open",
            width = frame_info.width,
            height = frame_info.height
        );
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

#[cfg(all(test, target_os = "windows"))]
#[path = "persistence_tests.rs"]
mod persistence_tests;

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
    fn log(&mut self, level: rfvp::host_api::RfvpLogLevel, message: &str) {
        use rfvp::host_api::RfvpLogLevel;
        match level {
            RfvpLogLevel::Error => tracing::error!(event = "astra.emu.fvp.core", message),
            RfvpLogLevel::Warn => tracing::warn!(event = "astra.emu.fvp.core", message),
            RfvpLogLevel::Info => tracing::info!(event = "astra.emu.fvp.core", message),
            RfvpLogLevel::Debug => tracing::debug!(event = "astra.emu.fvp.core", message),
            RfvpLogLevel::Trace => tracing::trace!(event = "astra.emu.fvp.core", message),
        }
    }
}

pub(crate) struct FvpSession {
    hosted: HostedSession,
    fs: NativeFileSystem,
    audio: AudioBridge,
    clock: SessionClock,
    input_state: events::InputState,
    frame: FramePublisher,
    frame_info: FrameInfo,
    gpu: crate::gpu::GpuRenderer,
    fatal: Option<astra_emu_family_api::FamilyError>,
    game_dirty: bool,
    pixels: [Option<Arc<Vec<u8>>>; 3],
    readback_slot: Option<usize>,
}

impl FvpSession {
    fn fail<T>(&mut self, value: FamilyError) -> FamilyResult<T> {
        self.gpu.native().cancel_pending();
        self.readback_slot = None;
        self.fatal = Some(value.clone());
        Err(value)
    }
    fn ensure_live(&self) -> FamilyResult<()> {
        self.fatal.clone().map_or(Ok(()), Err)
    }
    fn render_frame(&mut self) -> FamilyResult<()> {
        if matches!(self.frame.poll(None), FramePoll::Pending(_)) {
            let rendered = self.gpu.capture(self.hosted.core_mut())?;
            self.pixels[0] = Some(Arc::new(rendered));
            self.publish_game_frame(0)?;
            self.game_dirty = false;
            return Ok(());
        }
        if self.game_dirty && !self.gpu.native().frame_pending() {
            let Some(index) = self.pixels.iter().position(|slot| {
                slot.as_ref()
                    .is_none_or(|pixels| Arc::strong_count(pixels) == 1)
            }) else {
                return Ok(());
            };
            self.pixels[index].get_or_insert_with(|| Arc::new(Vec::new()));
            self.readback_slot = Some(index);
            self.gpu.submit(self.hosted.core_mut())?;
            self.game_dirty = false;
        }
        Ok(())
    }

    fn publish_game_frame(&mut self, index: usize) -> FamilyResult<()> {
        let rendered = self.pixels[index]
            .as_ref()
            .expect("reserved CPU slot exists");
        if self.frame_info.required_bytes() != Some(rendered.len()) {
            return self.fail(error::invalid(
                "ASTRA_EMU_FVP_FRAME_BYTES",
                "RFVP returned an invalid frame allocation",
            ));
        }
        self.frame.publish(
            Arc::clone(rendered),
            |pixels| pixels.as_slice(),
            self.frame_info,
        )?;
        Ok(())
    }

    fn send_audio(&mut self, operation: HostedAudioOperation) -> FamilyResult<()> {
        let operation = match operation {
            HostedAudioOperation::LoadResource {
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
                    .read_resource(&mut host, resource_name, isize::MAX as usize)
                    .map_err(|error| error::rfvp_operation(error, "audio resource read"))?;
                HostedAudioOperation::LoadEncoded { id, kind, bytes }
            }
            other => other,
        };
        self.audio.submit(operation)
    }

    fn sync_audio_playback_state(&mut self) -> FamilyResult<()> {
        let states = self.audio.playback_states()?;
        self.hosted
            .set_audio_playback_states(&states)
            .map_err(|cause| error::rfvp_operation(cause, "audio playback state synchronization"))
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
        let was_pending = self.gpu.native().frame_pending();
        let delta = {
            let mut host = SessionHost {
                fs: &mut self.fs,
                renderer: NullRenderer,
                audio: NullAudio,
                clock: &mut self.clock,
            };
            self.hosted
                .step_with_gpu(
                    &mut host,
                    HostedStepInput { events: input },
                    self.gpu.native(),
                )
                .map_err(|error| {
                    error::rfvp_operation(error, self.hosted.core().last_operation())
                })?
        };
        let reset_clock = self.hosted.take_timeline_reset();
        if reset_clock {
            self.gpu.native().cancel_pending();
            self.readback_slot = None;
            self.game_dirty = true;
        }
        if was_pending && !self.gpu.native().frame_pending() {
            self.readback_slot = None;
        }
        self.game_dirty |=
            delta.visual_changed || (was_pending && !self.gpu.native().frame_pending());
        for operation in delta.audio {
            if let Err(value) = self.send_audio(operation) {
                return self.fail(value);
            }
        }
        if let Err(value) = self.render_frame() {
            return self.fail(value);
        }
        let status = if self.hosted.is_terminal() {
            FamilyStatus::Finished
        } else if self.hosted.core().is_waiting_for_movie() {
            FamilyStatus::Waiting
        } else {
            FamilyStatus::Running
        };
        Ok(AdvanceResponse {
            status,
            reset_clock,
            ..AdvanceResponse::running()
        })
    }
    fn acquire_frame(&mut self, after: Option<FrameIdentity>) -> FamilyResult<FramePoll> {
        self.ensure_live()?;
        if self.game_dirty && !self.gpu.native().frame_pending() {
            if let Err(failure) = self.render_frame() {
                return self.fail(failure);
            }
        }
        if self.gpu.native().frame_pending() {
            let index = self
                .readback_slot
                .expect("map submission reserves a CPU slot");
            let pixels = Arc::get_mut(
                self.pixels[index]
                    .as_mut()
                    .expect("reserved CPU slot exists"),
            )
            .expect("reserved CPU slot has no lease");
            match self.gpu.poll_frame_into(pixels) {
                Ok(true) => {
                    self.readback_slot = None;
                    if let Err(failure) = self.publish_game_frame(index) {
                        return self.fail(failure);
                    }
                }
                Ok(false) => return Ok(self.frame.pending()),
                Err(failure) => return self.fail(failure),
            }
        } else if self.game_dirty {
            return Ok(self.frame.pending());
        }
        Ok(self.frame.poll(after))
    }
    fn close(mut self: Box<Self>) -> FamilyResult<()> {
        tracing::info!(event = "astra.emu.fvp.session.closing");
        self.gpu.native().cancel_pending();
        self.readback_slot = None;
        let save_result = self
            .hosted
            .shutdown()
            .map_err(|error| error::rfvp_operation(error, "native shutdown"));
        let audio_result = self.audio.close();
        save_result?;
        audio_result
    }
}
