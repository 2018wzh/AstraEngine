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
    use std::{ffi::c_void, sync::OnceLock};

    use astra_platform::{PlatformSmokeCheck, PlatformSmokeStatus};
    use cpal::traits::HostTrait;
    use windows::Win32::{
        Foundation::ERROR_DEVICE_NOT_CONNECTED,
        Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION},
        System::Com::CoTaskMemFree,
        UI::{
            Input::XboxController::{XInputGetState, XINPUT_STATE},
            Shell::{FOLDERID_RoamingAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
        },
    };

    static WINDOW_SMOKE: OnceLock<Result<String, String>> = OnceLock::new();
    use winit::{
        application::ApplicationHandler,
        dpi::{LogicalPosition, LogicalSize},
        event::WindowEvent,
        event_loop::{ActiveEventLoop, EventLoop},
        platform::windows::EventLoopBuilderExtWindows,
        window::{Window, WindowAttributes, WindowId},
    };

    pub fn smoke_checks() -> Vec<PlatformSmokeCheck> {
        vec![
            smoke(
                "sdk.windows",
                PlatformSmokeStatus::Pass,
                "Windows target SDK is linked",
            ),
            match windowed_smoke() {
                Ok(summary) => smoke("windowed_smoke", PlatformSmokeStatus::Pass, summary),
                Err(err) => smoke("windowed_smoke", PlatformSmokeStatus::Blocked, err),
            },
            match wmf_smoke() {
                Ok(summary) => smoke("decode.wmf", PlatformSmokeStatus::Pass, summary),
                Err(err) => smoke("decode.wmf", PlatformSmokeStatus::Blocked, err),
            },
            match wasapi_smoke() {
                Ok(summary) => smoke("audio.wasapi", PlatformSmokeStatus::Pass, summary),
                Err(err) => smoke("audio.wasapi", PlatformSmokeStatus::Warning, err),
            },
            match known_folder_smoke() {
                Ok(summary) => smoke("save.known_folder", PlatformSmokeStatus::Pass, summary),
                Err(err) => smoke("save.known_folder", PlatformSmokeStatus::Blocked, err),
            },
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
        PlatformSmokeCheck {
            id: id.into(),
            status,
            summary: summary.into(),
        }
    }

    fn windowed_smoke() -> Result<String, String> {
        WINDOW_SMOKE.get_or_init(run_windowed_smoke).clone()
    }

    fn run_windowed_smoke() -> Result<String, String> {
        let event_loop = EventLoop::builder()
            .with_any_thread(true)
            .build()
            .map_err(|err| format!("create event loop: {err}"))?;
        let mut app = WindowSmokeApp::default();
        event_loop
            .run_app(&mut app)
            .map_err(|err| format!("run event loop: {err}"))?;
        app.result
            .ok_or_else(|| "windowed smoke did not produce evidence".to_string())?
    }

    #[derive(Default)]
    struct WindowSmokeApp {
        window: Option<Window>,
        result: Option<Result<String, String>>,
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
                    self.window = Some(window);
                    if size.width == 0 || size.height == 0 || scale_factor <= 0.0 {
                        self.result =
                            Some(Err("window smoke produced invalid size or DPI".to_string()));
                    } else {
                        self.result = Some(Ok(format!(
                            "winit hidden window created; dpi_scale={scale_factor:.2}; ime enabled; input event loop active"
                        )));
                    }
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

    fn wmf_smoke() -> Result<String, String> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)
                .map_err(|err| format!("Media Foundation startup: {err}"))?;
            MFShutdown().map_err(|err| format!("Media Foundation shutdown: {err}"))?;
        }
        Ok("Media Foundation startup/shutdown completed".to_string())
    }

    fn wasapi_smoke() -> Result<String, String> {
        let host = cpal::default_host();
        if host.default_output_device().is_some() {
            Ok("WASAPI default output device is available".to_string())
        } else {
            Err("WASAPI backend loaded, but no default output device was reported".to_string())
        }
    }

    fn known_folder_smoke() -> Result<String, String> {
        unsafe {
            let path = SHGetKnownFolderPath(&FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, None)
                .map_err(|err| format!("known folder lookup: {err}"))?;
            let present = !path.as_ptr().is_null();
            CoTaskMemFree(Some(path.as_ptr() as *const c_void));
            if present {
                Ok("RoamingAppData known-folder save store is available".to_string())
            } else {
                Err("RoamingAppData known-folder lookup returned an empty pointer".to_string())
            }
        }
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
            for required in ["windowed_smoke", "decode.wmf", "save.known_folder"] {
                assert!(
                    report
                        .smoke
                        .iter()
                        .any(|check| check.id == required
                            && check.status == PlatformSmokeStatus::Pass)
                );
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
