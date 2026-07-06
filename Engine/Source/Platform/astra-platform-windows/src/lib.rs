use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    let report = PlatformCapabilityReport::new(
        PlatformId::Windows,
        target.map(str::to_string),
        if cfg!(target_os = "windows") {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        },
        vec![
            "winit_window".to_string(),
            "wgpu_surface_host".to_string(),
            "headless".to_string(),
        ],
        vec!["wmf".to_string(), "ffmpeg_profile".to_string()],
        vec!["wasapi".to_string()],
        vec![
            "known_folder_roaming_app_data".to_string(),
            "file_package".to_string(),
        ],
        vec![
            "keyboard".to_string(),
            "mouse".to_string(),
            "ime".to_string(),
            "gamepad".to_string(),
        ],
        vec![
            "window".to_string(),
            "resize".to_string(),
            "crash_bundle".to_string(),
        ],
        vec!["network_runtime_ai_profile_gated".to_string()],
    );

    #[cfg(target_os = "windows")]
    {
        report.with_smoke(windows_probe::smoke_checks())
    }
    #[cfg(not(target_os = "windows"))]
    {
        report
    }
}

#[cfg(target_os = "windows")]
mod windows_probe {
    use std::{
        ffi::c_void,
        fs,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc, Mutex, OnceLock,
        },
        thread,
        time::Duration,
    };

    use astra_core::Hash256;
    use astra_media::{DecodeKind, DecodeOutput, DecodeProvider, DecodeRequest};
    use astra_platform::{PlatformSmokeCheck, PlatformSmokeEvidence, PlatformSmokeStatus};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use windows::Win32::{
        Foundation::ERROR_DEVICE_NOT_CONNECTED,
        System::Com::CoTaskMemFree,
        UI::{
            Input::XboxController::{XInputGetState, XINPUT_STATE},
            Shell::{FOLDERID_RoamingAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
        },
    };

    use winit::{
        application::ApplicationHandler,
        dpi::{LogicalPosition, LogicalSize},
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        platform::windows::EventLoopBuilderExtWindows,
        window::{Window, WindowAttributes, WindowId},
    };

    static WINDOW_PROBE: OnceLock<Result<WindowProbe, String>> = OnceLock::new();

    #[derive(Clone)]
    struct WindowProbe {
        window_summary: String,
        window_evidence: Vec<PlatformSmokeEvidence>,
        surface_summary: String,
        surface_evidence: Vec<PlatformSmokeEvidence>,
    }

    pub fn smoke_checks() -> Vec<PlatformSmokeCheck> {
        let (window_status, window_summary, window_evidence) = status_tuple(windowed_smoke());
        let (surface_status, surface_summary, surface_evidence) =
            status_tuple(wgpu_surface_smoke());
        let (wmf_audio_status, wmf_audio_summary, wmf_audio_evidence) =
            status_tuple(wmf_audio_smoke());
        let (wmf_video_status, wmf_video_summary, wmf_video_evidence) =
            status_tuple(wmf_video_smoke());
        let (wasapi_status, wasapi_summary, wasapi_evidence) = status_tuple(wasapi_smoke());
        let (save_status, save_summary, save_evidence) = status_tuple(known_folder_smoke());

        vec![
            smoke(
                "sdk.windows",
                PlatformSmokeStatus::Pass,
                "Windows target SDK is linked",
            ),
            smoke_with(
                "windowed_smoke",
                window_status,
                window_summary,
                window_evidence,
            ),
            smoke_with(
                "renderer.wgpu_surface",
                surface_status,
                surface_summary,
                surface_evidence,
            ),
            smoke_with(
                "decode.wmf.audio",
                wmf_audio_status,
                wmf_audio_summary,
                wmf_audio_evidence,
            ),
            smoke_with(
                "decode.wmf.video_first_frame",
                wmf_video_status,
                wmf_video_summary,
                wmf_video_evidence,
            ),
            smoke_with(
                "audio.wasapi",
                wasapi_status,
                wasapi_summary,
                wasapi_evidence,
            ),
            smoke_with(
                "save.known_folder_rw",
                save_status,
                save_summary,
                save_evidence,
            ),
            match gamepad_smoke() {
                Ok(summary) => smoke("input.gamepad", PlatformSmokeStatus::Pass, summary),
                Err(err) => smoke("input.gamepad", PlatformSmokeStatus::Warning, err),
            },
        ]
    }

    fn smoke(
        id: impl Into<String>,
        status: PlatformSmokeStatus,
        summary: impl Into<String>,
    ) -> PlatformSmokeCheck {
        smoke_with(id, status, summary, Vec::new())
    }

    fn smoke_with(
        id: impl Into<String>,
        status: PlatformSmokeStatus,
        summary: impl Into<String>,
        evidence: Vec<PlatformSmokeEvidence>,
    ) -> PlatformSmokeCheck {
        PlatformSmokeCheck {
            id: id.into(),
            status,
            summary: summary.into(),
            evidence,
        }
    }

    fn status_tuple(
        result: Result<(String, Vec<PlatformSmokeEvidence>), String>,
    ) -> (PlatformSmokeStatus, String, Vec<PlatformSmokeEvidence>) {
        match result {
            Ok((summary, evidence)) => (PlatformSmokeStatus::Pass, summary, evidence),
            Err(err) => (PlatformSmokeStatus::Blocked, err, Vec::new()),
        }
    }

    fn evidence(key: impl Into<String>, value: impl ToString) -> PlatformSmokeEvidence {
        PlatformSmokeEvidence {
            key: key.into(),
            value: value.to_string(),
        }
    }

    fn windowed_smoke() -> Result<(String, Vec<PlatformSmokeEvidence>), String> {
        let probe = WINDOW_PROBE.get_or_init(run_window_probe).clone()?;
        Ok((probe.window_summary, probe.window_evidence))
    }

    fn wgpu_surface_smoke() -> Result<(String, Vec<PlatformSmokeEvidence>), String> {
        let probe = WINDOW_PROBE.get_or_init(run_window_probe).clone()?;
        Ok((probe.surface_summary, probe.surface_evidence))
    }

    fn run_window_probe() -> Result<WindowProbe, String> {
        let event_loop = EventLoop::builder()
            .with_any_thread(true)
            .build()
            .map_err(|err| format!("create event loop: {err}"))?;
        let mut app = WindowSmokeApp::default();
        event_loop
            .run_app(&mut app)
            .map_err(|err| format!("run event loop: {err}"))?;
        app.result
            .ok_or_else(|| "windowed probe did not produce evidence".to_string())?
    }

    #[derive(Default)]
    struct WindowSmokeApp {
        window: Option<Window>,
        result: Option<Result<WindowProbe, String>>,
    }

    impl ApplicationHandler for WindowSmokeApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.result.is_some() {
                event_loop.exit();
                return;
            }

            let attrs = WindowAttributes::default()
                .with_title("Astra Windows Smoke")
                .with_visible(false)
                .with_inner_size(LogicalSize::new(320.0, 180.0));
            match event_loop.create_window(attrs) {
                Ok(window) => {
                    let size = window.inner_size();
                    let scale_factor = window.scale_factor();
                    window.set_ime_allowed(true);
                    window.set_ime_cursor_area(
                        LogicalPosition::new(0.0, 0.0),
                        LogicalSize::new(16.0, 16.0),
                    );
                    if size.width == 0 || size.height == 0 || scale_factor <= 0.0 {
                        self.result =
                            Some(Err("window smoke produced invalid size or DPI".to_string()));
                    } else {
                        let surface = probe_wgpu_surface(&window);
                        self.result = Some(surface.map(|(surface_summary, surface_evidence)| {
                            WindowProbe {
                                window_summary: "winit hidden window created with active event loop and IME cursor area".to_string(),
                                window_evidence: vec![
                                    evidence("width", size.width),
                                    evidence("height", size.height),
                                    evidence("dpi_scale", format!("{scale_factor:.2}")),
                                    evidence("visible", "false"),
                                    evidence("ime_cursor_area", "16x16"),
                                ],
                                surface_summary,
                                surface_evidence,
                            }
                        }));
                    }
                    self.window = Some(window);
                }
                Err(err) => {
                    self.result = Some(Err(format!("create hidden window: {err}")));
                }
            }
            event_loop.exit();
        }

        fn window_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            _event: WindowEvent,
        ) {
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if self.result.is_some() {
                event_loop.exit();
            }
        }
    }

    fn probe_wgpu_surface(window: &Window) -> Result<(String, Vec<PlatformSmokeEvidence>), String> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window)
            .map_err(|err| format!("create wgpu surface: {err}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|err| format!("request wgpu adapter: {err}"))?;
        let info = adapter.get_info();
        let caps = surface.get_capabilities(&adapter);
        if caps.formats.is_empty() {
            return Err("wgpu surface reported no supported formats".to_string());
        }
        Ok((
            "wgpu surface and compatible adapter were created for the hidden window".to_string(),
            vec![
                evidence("backend", format!("{:?}", info.backend)),
                evidence("adapter_type", format!("{:?}", info.device_type)),
                evidence("format_count", caps.formats.len()),
                evidence("present_mode_count", caps.present_modes.len()),
            ],
        ))
    }

    fn wmf_audio_smoke() -> Result<(String, Vec<PlatformSmokeEvidence>), String> {
        decode_media_fixture(
            DecodeKind::Audio,
            "mp3",
            include_bytes!("../../../../Fixtures/PublicDomainMedia/t-rex-roar.mp3"),
            16_000,
        )
        .map(|(format, bytes, hash)| {
            (
                "WMF decoded the public MP3 fixture into bounded PCM".to_string(),
                vec![
                    evidence("codec", "mp3"),
                    evidence("format", format),
                    evidence("bytes", bytes),
                    evidence("hash", hash),
                ],
            )
        })
    }

    fn wmf_video_smoke() -> Result<(String, Vec<PlatformSmokeEvidence>), String> {
        decode_media_fixture(
            DecodeKind::Video,
            "mp4",
            include_bytes!("../../../../Fixtures/PublicDomainMedia/flower.mp4"),
            320 * 180 * 4,
        )
        .map(|(format, bytes, hash)| {
            (
                "WMF decoded the public MP4 fixture into a CPU first frame".to_string(),
                vec![
                    evidence("codec", "mp4"),
                    evidence("format", format),
                    evidence("bytes", bytes),
                    evidence("hash", hash),
                ],
            )
        })
    }

    fn decode_media_fixture(
        kind: DecodeKind,
        codec: &str,
        bytes: &[u8],
        min_output_bytes: usize,
    ) -> Result<(String, usize, String), String> {
        let provider = astra_media::WindowsMediaFoundationDecodeProvider::probe()
            .map_err(|err| err.to_string())?;
        let result = provider
            .decode(&DecodeRequest {
                kind,
                codec: codec.to_string(),
                bytes: bytes.to_vec(),
                profile: "desktop-release".to_string(),
            })
            .map_err(|err| err.to_string())?;
        match result.output {
            DecodeOutput::CpuBuffer {
                bytes,
                format,
                hash,
            } => {
                if bytes.len() < min_output_bytes {
                    return Err(format!(
                        "decoded output was too small: {} bytes",
                        bytes.len()
                    ));
                }
                if hash != Hash256::from_sha256(&bytes) {
                    return Err("decoded output hash did not match bytes".to_string());
                }
                Ok((format, bytes.len(), hash.to_string()))
            }
            DecodeOutput::MediaSurfaceToken(_) => {
                Err("WMF smoke requires bounded CPU decode output".to_string())
            }
        }
    }

    fn wasapi_smoke() -> Result<(String, Vec<PlatformSmokeEvidence>), String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "WASAPI default output device was not reported".to_string())?;
        let config = device
            .default_output_config()
            .map_err(|err| format!("WASAPI default output config: {err}"))?;
        let stream_config: cpal::StreamConfig = config.clone().into();
        let frames = Arc::new(AtomicU64::new(0));
        let stream_errors = Arc::new(Mutex::new(None::<String>));
        let channels = usize::from(stream_config.channels);
        let err_sink = Arc::clone(&stream_errors);
        let err_fn = move |err: cpal::StreamError| {
            if let Ok(mut slot) = err_sink.lock() {
                *slot = Some(err.to_string());
            }
        };
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let frames = Arc::clone(&frames);
                device.build_output_stream(
                    &stream_config,
                    move |data: &mut [f32], _| fill_f32(data, channels, &frames),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let frames = Arc::clone(&frames);
                device.build_output_stream(
                    &stream_config,
                    move |data: &mut [i16], _| fill_i16(data, channels, &frames),
                    err_fn,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let frames = Arc::clone(&frames);
                device.build_output_stream(
                    &stream_config,
                    move |data: &mut [u16], _| fill_u16(data, channels, &frames),
                    err_fn,
                    None,
                )
            }
            other => return Err(format!("unsupported WASAPI sample format {other:?}")),
        }
        .map_err(|err| format!("build WASAPI output stream: {err}"))?;
        stream
            .play()
            .map_err(|err| format!("start WASAPI output stream: {err}"))?;
        thread::sleep(Duration::from_millis(120));
        drop(stream);
        if let Ok(slot) = stream_errors.lock() {
            if let Some(err) = slot.as_ref() {
                return Err(format!("WASAPI stream error: {err}"));
            }
        }
        let rendered_frames = frames.load(Ordering::SeqCst);
        if rendered_frames == 0 {
            return Err("WASAPI output callback produced no frames".to_string());
        }
        Ok((
            "WASAPI output stream initialized and rendered a silent buffer".to_string(),
            vec![
                evidence("sample_rate", config.sample_rate()),
                evidence("channels", config.channels()),
                evidence("sample_format", format!("{:?}", config.sample_format())),
                evidence("frames", rendered_frames),
            ],
        ))
    }

    fn fill_f32(data: &mut [f32], channels: usize, frames: &AtomicU64) {
        data.fill(0.0);
        record_frames(data.len(), channels, frames);
    }

    fn fill_i16(data: &mut [i16], channels: usize, frames: &AtomicU64) {
        data.fill(0);
        record_frames(data.len(), channels, frames);
    }

    fn fill_u16(data: &mut [u16], channels: usize, frames: &AtomicU64) {
        data.fill(u16::MAX / 2);
        record_frames(data.len(), channels, frames);
    }

    fn record_frames(sample_count: usize, channels: usize, frames: &AtomicU64) {
        let channels = channels.max(1);
        frames.fetch_add((sample_count / channels) as u64, Ordering::SeqCst);
    }

    fn known_folder_smoke() -> Result<(String, Vec<PlatformSmokeEvidence>), String> {
        let root = unsafe {
            let path = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None)
                .map_err(|err| format!("known folder lookup: {err}"))?;
            let root = path
                .to_string()
                .map_err(|err| format!("known folder path conversion: {err}"))?;
            CoTaskMemFree(Some(path.as_ptr() as *const c_void));
            root
        };
        if root.is_empty() {
            return Err("known folder lookup returned an empty path".to_string());
        }
        let dir = std::path::PathBuf::from(root).join("AstraEngineProbe");
        fs::create_dir_all(&dir).map_err(|err| format!("create known-folder probe dir: {err}"))?;
        let path = dir.join("stage2_known_folder_rw.bin");
        let bytes = b"astra-stage2-known-folder-rw";
        fs::write(&path, bytes).map_err(|err| format!("known-folder write: {err}"))?;
        let read = fs::read(&path).map_err(|err| format!("known-folder read: {err}"))?;
        fs::remove_file(&path).map_err(|err| format!("known-folder delete: {err}"))?;
        let _ = fs::remove_dir(&dir);
        if read != bytes {
            return Err("known-folder readback did not match written bytes".to_string());
        }
        Ok((
            "RoamingAppData known-folder save store passed write/read/delete".to_string(),
            vec![
                evidence("bytes", read.len()),
                evidence("hash", Hash256::from_sha256(&read)),
            ],
        ))
    }

    fn gamepad_smoke() -> Result<String, String> {
        unsafe {
            let mut state = XINPUT_STATE::default();
            let code = XInputGetState(0, &mut state);
            if code == 0 {
                Ok("XInput gamepad state query succeeded".to_string())
            } else if code == ERROR_DEVICE_NOT_CONNECTED.0 {
                Ok("XInput is available; no gamepad is connected".to_string())
            } else {
                Err(format!("XInput state query returned code {code}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use astra_platform::{
        validate_capability_report, PlatformSmokeStatus, PlatformValidationStatus, SdkStatus,
    };

    use super::probe;

    #[test]
    fn probe_reports_real_windows_smoke_or_missing_sdk() {
        let report = probe(Some("nativevn-game"));
        if cfg!(target_os = "windows") {
            assert_eq!(report.sdk_status, SdkStatus::Present);
            for required in [
                "windowed_smoke",
                "renderer.wgpu_surface",
                "decode.wmf.audio",
                "decode.wmf.video_first_frame",
                "audio.wasapi",
                "save.known_folder_rw",
            ] {
                assert!(report.smoke.iter().any(|check| check.id == required
                    && check.status == PlatformSmokeStatus::Pass
                    && !check.evidence.is_empty()));
            }
            let (status, diagnostics) = validate_capability_report(&report);
            assert!(
                matches!(
                    status,
                    PlatformValidationStatus::Pass | PlatformValidationStatus::Warning
                ),
                "diagnostics={diagnostics:?}"
            );
        } else {
            assert_eq!(report.sdk_status, SdkStatus::Missing);
        }
    }
}
