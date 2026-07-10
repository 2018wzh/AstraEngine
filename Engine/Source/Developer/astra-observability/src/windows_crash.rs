use std::{path::PathBuf, time::Duration};

use crate::{CrashReportingMode, ObservabilityError};

#[derive(Debug, Clone)]
pub struct WindowsCrashReporterConfig {
    pub reporter_path: PathBuf,
    pub crash_dir: PathBuf,
    pub log_file: Option<PathBuf>,
    pub session_id: String,
    pub mode: CrashReportingMode,
    pub handshake_timeout: Duration,
    pub completion_timeout: Duration,
}

#[cfg(target_os = "windows")]
mod imp {
    use std::{
        ffi::c_void,
        mem::size_of,
        process::{Child, Command, Stdio},
        ptr,
        sync::atomic::{AtomicIsize, AtomicPtr, AtomicU32, Ordering},
    };

    use windows::{
        core::HSTRING,
        Win32::{
            Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
            System::{
                Diagnostics::Debug::{
                    SetUnhandledExceptionFilter, EXCEPTION_EXECUTE_HANDLER, EXCEPTION_POINTERS,
                    LPTOP_LEVEL_EXCEPTION_FILTER,
                },
                Memory::{
                    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
                    MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READWRITE,
                },
                Threading::{CreateEventW, GetCurrentThreadId, SetEvent, WaitForSingleObject},
            },
        },
    };

    use super::*;

    const REQUEST_VERSION: u32 = 1;
    const REQUEST_PANIC: u32 = 2;
    const REQUEST_SHUTDOWN: u32 = 3;
    static REQUEST_PTR: AtomicPtr<CrashRequestV1> = AtomicPtr::new(ptr::null_mut());
    static REQUEST_EVENT: AtomicIsize = AtomicIsize::new(0);
    static COMPLETE_EVENT: AtomicIsize = AtomicIsize::new(0);
    static COMPLETION_TIMEOUT_MS: AtomicU32 = AtomicU32::new(15_000);

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CrashRequestV1 {
        version: u32,
        kind: u32,
        thread_id: u32,
        exception_code: u32,
        exception_pointers: u64,
    }

    type PanicHook = dyn for<'a> Fn(&std::panic::PanicHookInfo<'a>) + Send + Sync + 'static;

    pub struct WindowsCrashReporterGuard {
        child: Child,
        mapping: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        ready_event: HANDLE,
        request_event: HANDLE,
        complete_event: HANDLE,
        previous_filter: LPTOP_LEVEL_EXCEPTION_FILTER,
        previous_panic_hook: Option<Box<PanicHook>>,
    }

    pub fn install(
        config: WindowsCrashReporterConfig,
    ) -> Result<Option<WindowsCrashReporterGuard>, ObservabilityError> {
        if config.mode == CrashReportingMode::Disabled {
            return Ok(None);
        }
        let result = install_required(config.clone());
        match (config.mode, result) {
            (_, Ok(guard)) => Ok(Some(guard)),
            (CrashReportingMode::Optional, Err(error)) => {
                tracing::warn!(
                    event = "crash_reporter.install.optional_failed",
                    diagnostic_code = "ASTRA_CRASH_REPORTER_OPTIONAL",
                    "optional Windows crash reporter could not start"
                );
                let _ = error;
                Ok(None)
            }
            (_, Err(error)) => Err(error),
        }
    }

    fn install_required(
        config: WindowsCrashReporterConfig,
    ) -> Result<WindowsCrashReporterGuard, ObservabilityError> {
        if config.session_id.is_empty() || !config.reporter_path.is_file() {
            return Err(ObservabilityError::CrashReporter(
                "reporter path or session id is invalid".to_string(),
            ));
        }
        std::fs::create_dir_all(&config.crash_dir)?;
        let prefix = format!("Local\\AstraCrash-{}", config.session_id);
        let mapping_name = format!("{prefix}-mapping");
        let ready_name = format!("{prefix}-ready");
        let request_name = format!("{prefix}-request");
        let complete_name = format!("{prefix}-complete");
        let mapping = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                size_of::<CrashRequestV1>() as u32,
                &HSTRING::from(&mapping_name),
            )
        }
        .map_err(win_error("create shared crash request"))?;
        let view = unsafe {
            MapViewOfFile(
                mapping,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                size_of::<CrashRequestV1>(),
            )
        };
        if view.Value.is_null() {
            unsafe { CloseHandle(mapping).ok() };
            return Err(ObservabilityError::CrashReporter(
                "map shared crash request failed".to_string(),
            ));
        }
        unsafe {
            ptr::write_volatile(
                view.Value.cast::<CrashRequestV1>(),
                CrashRequestV1::default(),
            )
        };
        let ready_event = unsafe { CreateEventW(None, false, false, &HSTRING::from(&ready_name)) }
            .map_err(win_error("create ready event"))?;
        let request_event =
            unsafe { CreateEventW(None, false, false, &HSTRING::from(&request_name)) }
                .map_err(win_error("create request event"))?;
        let complete_event =
            unsafe { CreateEventW(None, false, false, &HSTRING::from(&complete_name)) }
                .map_err(win_error("create completion event"))?;
        let mut command = Command::new(&config.reporter_path);
        command
            .args(["--watch", "--pid", &std::process::id().to_string()])
            .args(["--session-id", &config.session_id])
            .arg("--crash-dir")
            .arg(&config.crash_dir)
            .args(["--mapping", &mapping_name])
            .args(["--ready-event", &ready_name])
            .args(["--request-event", &request_name])
            .args(["--complete-event", &complete_name])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(log_file) = &config.log_file {
            command.arg("--log-file").arg(log_file);
        }
        let mut child = command.spawn()?;
        let ready =
            unsafe { WaitForSingleObject(ready_event, duration_ms(config.handshake_timeout)) };
        if ready != WAIT_OBJECT_0 {
            let _ = child.kill();
            let _ = child.wait();
            close_resources(mapping, view, ready_event, request_event, complete_event);
            return Err(ObservabilityError::CrashReporter(
                "crash reporter handshake timed out".to_string(),
            ));
        }
        REQUEST_PTR.store(view.Value.cast(), Ordering::Release);
        REQUEST_EVENT.store(request_event.0 as isize, Ordering::Release);
        COMPLETE_EVENT.store(complete_event.0 as isize, Ordering::Release);
        COMPLETION_TIMEOUT_MS.store(duration_ms(config.completion_timeout), Ordering::Release);
        let previous_filter = unsafe { SetUnhandledExceptionFilter(Some(exception_filter)) };
        let previous_panic_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {
            signal_request(REQUEST_PANIC, ptr::null(), 0);
        }));
        tracing::info!(
            event = "crash_reporter.install.complete",
            mode = "required",
            "Windows crash reporter handshake completed"
        );
        Ok(WindowsCrashReporterGuard {
            child,
            mapping,
            view,
            ready_event,
            request_event,
            complete_event,
            previous_filter,
            previous_panic_hook: Some(previous_panic_hook),
        })
    }

    unsafe extern "system" fn exception_filter(info: *const EXCEPTION_POINTERS) -> i32 {
        let code = if info.is_null() || (*info).ExceptionRecord.is_null() {
            0
        } else {
            (*(*info).ExceptionRecord).ExceptionCode.0 as u32
        };
        signal_request(1, info, code);
        EXCEPTION_EXECUTE_HANDLER
    }

    fn signal_request(kind: u32, exception_pointers: *const EXCEPTION_POINTERS, code: u32) {
        let request = REQUEST_PTR.load(Ordering::Acquire);
        let request_event = REQUEST_EVENT.load(Ordering::Acquire);
        let complete_event = COMPLETE_EVENT.load(Ordering::Acquire);
        if request.is_null() || request_event == 0 || complete_event == 0 {
            return;
        }
        let value = CrashRequestV1 {
            version: REQUEST_VERSION,
            kind,
            thread_id: unsafe { GetCurrentThreadId() },
            exception_code: code,
            exception_pointers: exception_pointers as usize as u64,
        };
        unsafe {
            ptr::write_volatile(request, value);
            let request_handle = HANDLE(request_event as *mut c_void);
            let complete_handle = HANDLE(complete_event as *mut c_void);
            if SetEvent(request_handle).is_ok() {
                let _ = WaitForSingleObject(
                    complete_handle,
                    COMPLETION_TIMEOUT_MS.load(Ordering::Acquire),
                );
            }
        }
    }

    impl Drop for WindowsCrashReporterGuard {
        fn drop(&mut self) {
            unsafe {
                ptr::write_volatile(
                    self.view.Value.cast::<CrashRequestV1>(),
                    CrashRequestV1 {
                        version: REQUEST_VERSION,
                        kind: REQUEST_SHUTDOWN,
                        ..Default::default()
                    },
                );
                let _ = SetEvent(self.request_event);
            }
            let _ = self.child.wait();
            REQUEST_PTR.store(ptr::null_mut(), Ordering::Release);
            REQUEST_EVENT.store(0, Ordering::Release);
            COMPLETE_EVENT.store(0, Ordering::Release);
            unsafe {
                SetUnhandledExceptionFilter(self.previous_filter);
            }
            if let Some(previous) = self.previous_panic_hook.take() {
                std::panic::set_hook(previous);
            }
            close_resources(
                self.mapping,
                self.view,
                self.ready_event,
                self.request_event,
                self.complete_event,
            );
        }
    }

    fn close_resources(
        mapping: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        ready: HANDLE,
        request: HANDLE,
        complete: HANDLE,
    ) {
        unsafe {
            let _ = UnmapViewOfFile(view);
            let _ = CloseHandle(mapping);
            let _ = CloseHandle(ready);
            let _ = CloseHandle(request);
            let _ = CloseHandle(complete);
        }
    }

    fn duration_ms(duration: Duration) -> u32 {
        duration.as_millis().min(u128::from(u32::MAX)) as u32
    }

    fn win_error(context: &'static str) -> impl FnOnce(windows::core::Error) -> ObservabilityError {
        move |error| ObservabilityError::CrashReporter(format!("{context}: {error}"))
    }
}

#[cfg(target_os = "windows")]
pub use imp::WindowsCrashReporterGuard;

#[cfg(target_os = "windows")]
pub fn install_windows_crash_reporter(
    config: WindowsCrashReporterConfig,
) -> Result<Option<WindowsCrashReporterGuard>, ObservabilityError> {
    imp::install(config)
}

#[cfg(not(target_os = "windows"))]
pub struct WindowsCrashReporterGuard;

#[cfg(not(target_os = "windows"))]
pub fn install_windows_crash_reporter(
    config: WindowsCrashReporterConfig,
) -> Result<Option<WindowsCrashReporterGuard>, ObservabilityError> {
    match config.mode {
        CrashReportingMode::Disabled | CrashReportingMode::Optional => Ok(None),
        CrashReportingMode::Required => Err(ObservabilityError::CrashReporter(
            "Windows crash reporter is unavailable on this platform".to_string(),
        )),
    }
}
