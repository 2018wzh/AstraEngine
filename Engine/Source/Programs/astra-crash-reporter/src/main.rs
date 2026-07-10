use std::{env, fs, path::PathBuf, thread, time::Duration};

use astra_core::Hash256;
use astra_observability::{
    init_host, install_windows_crash_reporter, ConsoleFormat, CrashArtifactRef,
    CrashBundleManifestV1, CrashReportingMode, HostObservabilityConfig, HostRole,
    WindowsCrashReporterConfig, CRASH_BUNDLE_SCHEMA,
};

type ReporterError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), ReporterError> {
    let mut fixture_target = false;
    let mut write_dump = false;
    let mut watch = false;
    let mut self_test = false;
    let mut seh_fixture = false;
    let mut pid = None;
    let mut output = None;
    let mut manifest = None;
    let mut session_id = None;
    let mut crash_dir = None;
    let mut log_file = None;
    let mut mapping = None;
    let mut ready_event = None;
    let mut request_event = None;
    let mut complete_event = None;
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_string_lossy().as_ref() {
            "--fixture-target" => fixture_target = true,
            "--write-dump" => write_dump = true,
            "--watch" => watch = true,
            "--self-test" => self_test = true,
            "--seh-fixture" => seh_fixture = true,
            "--pid" => {
                pid = Some(
                    args.next()
                        .ok_or("missing --pid value")?
                        .to_string_lossy()
                        .parse::<u32>()?,
                )
            }
            "--output" => output = args.next().map(PathBuf::from),
            "--manifest" => manifest = args.next().map(PathBuf::from),
            "--session-id" => {
                session_id = args
                    .next()
                    .map(|value| value.to_string_lossy().into_owned())
            }
            "--crash-dir" => crash_dir = args.next().map(PathBuf::from),
            "--log-file" => log_file = args.next().map(PathBuf::from),
            "--mapping" => {
                mapping = args
                    .next()
                    .map(|value| value.to_string_lossy().into_owned())
            }
            "--ready-event" => {
                ready_event = args
                    .next()
                    .map(|value| value.to_string_lossy().into_owned())
            }
            "--request-event" => {
                request_event = args
                    .next()
                    .map(|value| value.to_string_lossy().into_owned())
            }
            "--complete-event" => {
                complete_event = args
                    .next()
                    .map(|value| value.to_string_lossy().into_owned())
            }
            _ => return Err("unsupported crash reporter argument".into()),
        }
    }
    if fixture_target {
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }
    if seh_fixture {
        return run_seh_fixture(crash_dir.ok_or("missing --crash-dir")?);
    }
    if self_test {
        println!(
            r#"{{"schema":"astra.crash_reporter_self_test.v1","status":"pass","platform":"{}"}}"#,
            if cfg!(target_os = "windows") {
                "windows"
            } else {
                "unsupported"
            }
        );
        return if cfg!(target_os = "windows") {
            Ok(())
        } else {
            Err("Windows crash reporter self-test is unavailable".into())
        };
    }
    if watch {
        return watch_process(WatchConfig {
            pid: pid.ok_or("missing --pid")?,
            session_id: session_id.ok_or("missing --session-id")?,
            crash_dir: crash_dir.ok_or("missing --crash-dir")?,
            log_file,
            mapping: mapping.ok_or("missing --mapping")?,
            ready_event: ready_event.ok_or("missing --ready-event")?,
            request_event: request_event.ok_or("missing --request-event")?,
            complete_event: complete_event.ok_or("missing --complete-event")?,
        });
    }
    if !write_dump {
        return Err("crash reporter mode is required".into());
    }
    let pid = pid.ok_or("missing --pid")?;
    let output = output.ok_or("missing --output")?;
    let manifest_path = manifest.ok_or("missing --manifest")?;
    let session_id = session_id.ok_or("missing --session-id")?;
    let _observability =
        init_reporter_logging(output.parent().unwrap_or_else(|| std::path::Path::new(".")))?;
    tracing::info!(
        event = "crash_reporter.dump.start",
        target_pid = pid,
        "out-of-process minidump capture started"
    );
    let result = write_dump_and_manifest(
        pid,
        output,
        manifest_path,
        session_id,
        "ASTRA_NATIVE_DUMP",
        None,
        None,
    );
    if result.is_ok() {
        tracing::info!(
            event = "crash_reporter.dump.complete",
            target_pid = pid,
            "out-of-process minidump capture completed"
        );
    } else {
        tracing::error!(
            event = "crash_reporter.dump.failed",
            diagnostic_code = "ASTRA_CRASH_WRITE_DUMP",
            "out-of-process minidump capture failed"
        );
    }
    result
}

#[cfg(target_os = "windows")]
fn run_seh_fixture(crash_dir: PathBuf) -> Result<(), ReporterError> {
    use windows::Win32::System::Diagnostics::Debug::RaiseException;

    let guard = install_windows_crash_reporter(WindowsCrashReporterConfig {
        reporter_path: env::current_exe()?,
        crash_dir,
        log_file: None,
        session_id: "seh-session".to_string(),
        mode: CrashReportingMode::Required,
        handshake_timeout: Duration::from_secs(5),
        completion_timeout: Duration::from_secs(15),
    })?
    .ok_or("required crash reporter was not installed")?;
    std::mem::forget(guard);
    unsafe { RaiseException(0xe000_0001, 1, None) };
    Err("SEH fixture unexpectedly returned".into())
}

#[cfg(not(target_os = "windows"))]
fn run_seh_fixture(_crash_dir: PathBuf) -> Result<(), ReporterError> {
    Err("SEH fixture is unavailable on this platform".into())
}

fn init_reporter_logging(
    root: &std::path::Path,
) -> Result<astra_observability::ObservabilityGuard, ReporterError> {
    let mut config = HostObservabilityConfig::for_cli("info");
    config.role = HostRole::CrashReporter;
    config.console = false;
    config.console_format = ConsoleFormat::Json;
    config.log_dir = Some(root.join("ReporterLogs"));
    Ok(init_host(config)?)
}

struct WatchConfig {
    pid: u32,
    session_id: String,
    crash_dir: PathBuf,
    log_file: Option<PathBuf>,
    mapping: String,
    ready_event: String,
    request_event: String,
    complete_event: String,
}

#[cfg(target_os = "windows")]
fn write_dump_and_manifest(
    pid: u32,
    output: PathBuf,
    manifest_path: PathBuf,
    session_id: String,
    reason_code: &str,
    exception: Option<(
        u32,
        *mut windows::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
    )>,
    log_file: Option<&std::path::Path>,
) -> Result<(), ReporterError> {
    use std::{fs::File, os::windows::io::AsRawHandle};
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            Diagnostics::Debug::{
                MiniDumpFilterMemory, MiniDumpFilterModulePaths, MiniDumpNormal,
                MiniDumpWithThreadInfo, MiniDumpWithUnloadedModules, MiniDumpWithoutOptionalData,
                MiniDumpWriteDump, MINIDUMP_EXCEPTION_INFORMATION,
            },
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = manifest_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial = output.with_extension(format!(
        "{}.partial",
        output
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("dmp")
    ));
    let process = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) }
        .map_err(|error| format!("ASTRA_CRASH_OPEN_PROCESS: {error}"))?;
    let file = File::create(&partial)?;
    let file_handle = HANDLE(file.as_raw_handle());
    let dump_type = MiniDumpNormal
        | MiniDumpFilterMemory
        | MiniDumpFilterModulePaths
        | MiniDumpWithThreadInfo
        | MiniDumpWithUnloadedModules
        | MiniDumpWithoutOptionalData;
    let exception = exception.map(|(thread_id, pointers)| MINIDUMP_EXCEPTION_INFORMATION {
        ThreadId: thread_id,
        ExceptionPointers: pointers,
        ClientPointers: true.into(),
    });
    let result = unsafe {
        MiniDumpWriteDump(
            process,
            pid,
            file_handle,
            dump_type,
            exception.as_ref().map(|value| value as *const _),
            None,
            None,
        )
    };
    drop(file);
    unsafe {
        CloseHandle(process).map_err(|error| format!("ASTRA_CRASH_CLOSE_PROCESS: {error}"))?;
    }
    result.map_err(|error| format!("ASTRA_CRASH_WRITE_DUMP: {error}"))?;
    fs::rename(&partial, &output)?;

    let dump = fs::read(&output)?;
    let tail = read_log_tail(log_file)?;
    let tail_path = manifest_path.with_file_name("log-tail.jsonl");
    fs::write(&tail_path, &tail)?;
    let minidump_name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("minidump filename is not UTF-8")?;
    let tail_name = tail_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("log tail filename is not UTF-8")?;
    let manifest = CrashBundleManifestV1 {
        schema: CRASH_BUNDLE_SCHEMA.to_string(),
        reason_code: reason_code.to_string(),
        session_id,
        process_role: "crash_reporter".to_string(),
        log_tail: CrashArtifactRef {
            path: tail_name.to_string(),
            sha256: Hash256::from_sha256(&tail).to_string(),
            byte_size: tail.len() as u64,
        },
        ring_record_count: 0,
        dropped_count: 0,
        minidump: Some(CrashArtifactRef {
            path: minidump_name.to_string(),
            sha256: Hash256::from_sha256(&dump).to_string(),
            byte_size: dump.len() as u64,
        }),
    };
    fs::write(manifest_path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn write_dump_and_manifest(
    _pid: u32,
    _output: PathBuf,
    _manifest_path: PathBuf,
    _session_id: String,
    _reason_code: &str,
    _exception: Option<(u32, *mut std::ffi::c_void)>,
    _log_file: Option<&std::path::Path>,
) -> Result<(), ReporterError> {
    Err("native minidump capture is only available on Windows".into())
}

fn read_log_tail(log_file: Option<&std::path::Path>) -> Result<Vec<u8>, std::io::Error> {
    const MAX_TAIL_BYTES: usize = 4 * 1024 * 1024;
    let Some(log_file) = log_file else {
        return Ok(Vec::new());
    };
    let bytes = match fs::read(log_file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(bytes[bytes.len().saturating_sub(MAX_TAIL_BYTES)..].to_vec())
}

#[cfg(target_os = "windows")]
fn watch_process(config: WatchConfig) -> Result<(), ReporterError> {
    use std::{mem::size_of, ptr};
    use windows::{
        core::HSTRING,
        Win32::{
            Foundation::WAIT_OBJECT_0,
            System::{
                Diagnostics::Debug::EXCEPTION_POINTERS,
                Memory::{MapViewOfFile, OpenFileMappingW, FILE_MAP_ALL_ACCESS},
                Threading::{
                    OpenEventW, SetEvent, WaitForSingleObject, INFINITE,
                    SYNCHRONIZATION_ACCESS_RIGHTS,
                },
            },
        },
    };

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CrashRequestV1 {
        version: u32,
        kind: u32,
        thread_id: u32,
        exception_code: u32,
        exception_pointers: u64,
    }

    let _observability = init_reporter_logging(&config.crash_dir)?;
    tracing::info!(
        event = "crash_reporter.watch.start",
        target_pid = config.pid,
        "crash reporter watch started"
    );
    let mapping = unsafe {
        OpenFileMappingW(
            FILE_MAP_ALL_ACCESS.0,
            false,
            &HSTRING::from(&config.mapping),
        )
    }
    .map_err(|error| format!("ASTRA_CRASH_OPEN_MAPPING: {error}"))?;
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
        return Err("ASTRA_CRASH_MAP_REQUEST".into());
    }
    let access = SYNCHRONIZATION_ACCESS_RIGHTS(0x0010_0002);
    let ready = unsafe { OpenEventW(access, false, &HSTRING::from(&config.ready_event)) }
        .map_err(|error| format!("ASTRA_CRASH_OPEN_READY: {error}"))?;
    let request = unsafe { OpenEventW(access, false, &HSTRING::from(&config.request_event)) }
        .map_err(|error| format!("ASTRA_CRASH_OPEN_REQUEST: {error}"))?;
    let complete = unsafe { OpenEventW(access, false, &HSTRING::from(&config.complete_event)) }
        .map_err(|error| format!("ASTRA_CRASH_OPEN_COMPLETE: {error}"))?;
    unsafe { SetEvent(ready) }.map_err(|error| format!("ASTRA_CRASH_READY: {error}"))?;
    tracing::info!(
        event = "crash_reporter.watch.ready",
        target_pid = config.pid,
        "crash reporter watch handshake completed"
    );
    let wait = unsafe { WaitForSingleObject(request, INFINITE) };
    if wait != WAIT_OBJECT_0 {
        return Err("ASTRA_CRASH_REQUEST_WAIT".into());
    }
    let request_value = unsafe { ptr::read_volatile(view.Value.cast::<CrashRequestV1>()) };
    if request_value.version != 1 {
        return Err("ASTRA_CRASH_REQUEST_VERSION".into());
    }
    if request_value.kind == 3 {
        close_watch_resources(mapping, view, ready, request, complete);
        return Ok(());
    }
    let timestamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let bundle = config
        .crash_dir
        .join(format!("crash-{}-{timestamp}", config.session_id));
    fs::create_dir_all(&bundle)?;
    let exception = (request_value.exception_pointers != 0).then_some((
        request_value.thread_id,
        request_value.exception_pointers as usize as *mut EXCEPTION_POINTERS,
    ));
    let reason = if request_value.kind == 2 {
        "ASTRA_PANIC"
    } else {
        "ASTRA_SEH"
    };
    let result = write_dump_and_manifest(
        config.pid,
        bundle.join("astra.dmp"),
        bundle.join("manifest.json"),
        config.session_id,
        reason,
        exception,
        config.log_file.as_deref(),
    );
    unsafe { SetEvent(complete) }.ok();
    if result.is_ok() {
        tracing::info!(
            event = "crash_reporter.watch.complete",
            target_pid = config.pid,
            "crash reporter watch completed"
        );
    } else {
        tracing::error!(
            event = "crash_reporter.watch.failed",
            diagnostic_code = "ASTRA_CRASH_WATCH_DUMP",
            "crash reporter watch failed"
        );
    }
    close_watch_resources(mapping, view, ready, request, complete);
    result
}

#[cfg(target_os = "windows")]
fn close_watch_resources(
    mapping: windows::Win32::Foundation::HANDLE,
    view: windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS,
    ready: windows::Win32::Foundation::HANDLE,
    request: windows::Win32::Foundation::HANDLE,
    complete: windows::Win32::Foundation::HANDLE,
) {
    use windows::Win32::{Foundation::CloseHandle, System::Memory::UnmapViewOfFile};
    unsafe {
        UnmapViewOfFile(view).ok();
        CloseHandle(mapping).ok();
        CloseHandle(ready).ok();
        CloseHandle(request).ok();
        CloseHandle(complete).ok();
    }
}

#[cfg(not(target_os = "windows"))]
fn watch_process(_config: WatchConfig) -> Result<(), ReporterError> {
    Err("Windows crash watch mode is unavailable on this platform".into())
}
