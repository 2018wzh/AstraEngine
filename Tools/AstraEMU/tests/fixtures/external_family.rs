use abi_stable::{
    prefix_type::PrefixTypeTrait, sabi_types::Constructor, std_types::ROption,
    type_level::downcasting::TD_Opaque,
};
use astra_emu_family_api::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

#[derive(Default)]
struct Provider;
struct Session {
    pixels: [u8; 16],
    title_requested: bool,
    icon_requested: bool,
    pressed: bool,
    released: bool,
    sink: Arc<AudioSinkBox>,
    writes: Arc<AtomicUsize>,
    worker: Option<JoinHandle<FamilyResult<()>>>,
}
fn frame_info() -> FrameInfo {
    FrameInfo {
        width: 2,
        height: 2,
        logical_width: 2,
        logical_height: 2,
        stride: 8,
        format: FrameFormat::Rgba8Srgb {
            alpha: FrameAlpha::Opaque,
        },
    }
}
fn pcm() -> PcmFormatSpec {
    PcmFormatSpec {
        sample_rate: 48_000,
        channels: 2,
        format: PcmFormat::I16,
    }
}
fn error(code: &str) -> FamilyError {
    FamilyError::invalid(code, "external family development regression")
}
impl FamilyProvider for Provider {
    fn descriptor(&self) -> FamilyResult<FamilyDescriptor> {
        Ok(FamilyDescriptor {
            family_id: "external".into(),
            plugin_id: "astra.emu.external.probe".into(),
            abi_fingerprint: FAMILY_ABI_FINGERPRINT.into(),
            version: "0.1.0".into(),
            capabilities: vec![FamilyCapability::CpuFrame, FamilyCapability::PcmAudio].into(),
            supported_formats: vec!["public-fixture".into()].into(),
            configuration: vec![
                ConfigField {
                    id: "intensity".into(),
                    label: "Intensity".into(),
                    group: "".into(),
                    kind: ConfigKind::Integer { min: 1, max: 255 },
                    default: ConfigValue::Integer(32),
                },
                ConfigField {
                    id: "request_title".into(),
                    label: "Request window title".into(),
                    group: "".into(),
                    kind: ConfigKind::Bool,
                    default: ConfigValue::Bool(false),
                },
                ConfigField {
                    id: "request_icon".into(),
                    label: "Request window icon".into(),
                    group: "".into(),
                    kind: ConfigKind::Bool,
                    default: ConfigValue::Bool(false),
                },
            ]
            .into(),
        })
    }
    fn probe(&self, request: ProbeRequest) -> FamilyResult<Option<ProbeReport>> {
        Ok(std::path::Path::new(request.game_path.as_str())
            .join("public.fixture")
            .is_file()
            .then(|| ProbeReport {
                family_id: "external".into(),
                game_id: "public-fixture".into(),
                format: "public-fixture".into(),
                confidence_permyriad: 10_000,
            }))
    }
    fn open(&mut self, request: OpenRequest) -> FamilyResult<FamilyOpen> {
        let descriptor = self.descriptor()?;
        request.validate_for_descriptor(&descriptor)?;
        let config = resolve_config(&descriptor.configuration, &request.configuration)?;
        let ConfigValue::Integer(intensity) = config[0].value else {
            return Err(error("EXTERNAL_CONFIG"));
        };
        let ConfigValue::Bool(title_requested) = config[1].value else {
            return Err(error("EXTERNAL_CONFIG"));
        };
        let ConfigValue::Bool(icon_requested) = config[2].value else {
            return Err(error("EXTERNAL_CONFIG"));
        };
        let sink = Arc::new(
            request
                .host
                .audio_sink
                .into_option()
                .ok_or_else(|| error("EXTERNAL_SINK"))?,
        );
        sink.configure(pcm()).into_result()?;
        let writes = Arc::new(AtomicUsize::new(0));
        let worker_sink = sink.clone();
        let worker_writes = writes.clone();
        let worker = thread::spawn(move || {
            while !worker_sink.is_cancelled() {
                let samples = (0..960)
                    .map(|n| if n % 48 < 24 { 2048_i16 } else { -2048_i16 })
                    .collect::<Vec<_>>();
                match worker_sink
                    .write(PcmChunk::I16(samples.into()))
                    .into_result()?
                {
                    AudioWriteStatus::Accepted => {
                        worker_writes.fetch_add(1, Ordering::Release);
                    }
                    AudioWriteStatus::Cancelled => break,
                    AudioWriteStatus::Closed => return Err(error("EXTERNAL_SINK_CLOSED")),
                }
            }
            Ok(())
        });
        let mut pixels = [0; 16];
        for p in pixels.as_chunks_mut::<4>().0 {
            p.copy_from_slice(&[intensity as u8, 0, 0, 255]);
        }
        Ok(FamilyOpen {
            response: OpenResponse {
                session_id: "external-session".into(),
                frame: frame_info(),
                audio_format: ROption::RSome(pcm()),
            },
            session: Box::new(Session {
                pixels,
                title_requested,
                icon_requested,
                pressed: false,
                released: false,
                sink,
                writes,
                worker: Some(worker),
            }),
        })
    }
}
impl FamilySession for Session {
    fn advance(&mut self, _: u64, events: &[FamilyEvent]) -> FamilyResult<AdvanceResponse> {
        for event in events {
            match event {
                FamilyEvent::Key {
                    code: KeyCode::Enter,
                    state: KeyState::Pressed,
                    ..
                } => {
                    self.pressed = true;
                    for p in self.pixels.as_chunks_mut::<4>().0 {
                        p.swap(0, 1);
                    }
                }
                FamilyEvent::Key {
                    code: KeyCode::Enter,
                    state: KeyState::Released,
                    ..
                } => self.released = true,
                _ => {}
            }
        }
        Ok(AdvanceResponse {
            window_command: if self.icon_requested {
                ROption::RSome(FamilyWindowCommand::SetIcon(FamilyWindowIcon {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4].into(),
                }))
            } else if self.title_requested {
                ROption::RSome(FamilyWindowCommand::SetTitle("Public fixture".into()))
            } else {
                ROption::RNone
            },
            ..AdvanceResponse::running()
        })
    }
    fn visit_frame(&self, visitor: &mut dyn FrameVisitor) -> FamilyResult<()> {
        visitor.accept(FrameView::from_slice(&self.pixels, frame_info())?)
    }
    fn close(mut self: Box<Self>) -> FamilyResult<()> {
        let cancellation = self.sink.cancel().into_result();
        let joined = self
            .worker
            .take()
            .unwrap()
            .join()
            .map_err(|_| error("EXTERNAL_WORKER_PANIC"))?;
        cancellation?;
        joined?;
        if !self.title_requested
            && !self.icon_requested
            && (!self.pressed || !self.released || self.writes.load(Ordering::Acquire) == 0)
        {
            return Err(error("EXTERNAL_INCOMPLETE_FLOW"));
        }
        tracing::info!(
            event = "external.family.closed",
            pcm_chunks = self.writes.load(Ordering::Acquire),
            input_complete = self.pressed && self.released
        );
        Ok(())
    }
}
extern "C" fn construct() -> FamilyModuleBox {
    FamilyModule_TO::from_value(ProviderModule::<Provider>::default(), TD_Opaque)
}
#[abi_stable::export_root_module]
pub fn external_family() -> AstraFamilyModuleRef {
    AstraFamilyModule {
        service: Constructor(construct),
    }
    .leak_into_prefix()
}
