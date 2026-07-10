use std::{
    alloc::{GlobalAlloc, Layout, System},
    fs,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use astra_observability::{
    init_host, ConsoleFormat, CrashReportingMode, HostObservabilityConfig, HostRole,
    LOG_EVENT_SCHEMA,
};
use tempfile::tempdir;

struct CountingAllocator;

static ALLOCATION_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATION_COUNT.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn host_pipeline_writes_stable_bounded_redacted_logs_and_crash_tail() {
    let root = tempdir().unwrap();
    let log_dir = root.path().join("logs");
    let crash_dir = root.path().join("crashes");
    let config = HostObservabilityConfig {
        role: HostRole::Test,
        filter: "trace".to_string(),
        console: false,
        console_format: ConsoleFormat::Json,
        log_dir: Some(log_dir.clone()),
        crash_dir: Some(crash_dir.clone()),
        crash_reporting: CrashReportingMode::Optional,
        max_file_bytes: 16 * 1024,
        max_archives: 2,
        ring_max_records: 8,
        ring_max_bytes: 8 * 1024,
        max_crash_bundles: 2,
    };
    let guard = init_host(config).unwrap();

    tracing::trace!(event = "test.trace", sequence = 1_u64, "trace event");
    tracing::debug!(event = "test.debug", sequence = 2_u64, "debug event");
    let span = tracing::info_span!("test.session", scenario_id = "scenario.safe");
    let _entered = span.enter();
    tracing::info!(event = "test.info", sequence = 3_u64, "info event");
    tracing::warn!(event = "test.warn", sequence = 4_u64, "warn event");
    tracing::error!(
        event = "test.error",
        secret_path = "C:\\private\\payload.txt",
        "error event"
    );
    guard.flush().unwrap();

    let main_log = fs::read_to_string(log_dir.join("astra.jsonl")).unwrap();
    let events = main_log
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 5);
    assert!(events
        .iter()
        .all(|event| event["schema"] == LOG_EVENT_SCHEMA));
    assert!(events
        .iter()
        .all(|event| event["session_id"] == guard.session_id()));
    assert!(events.iter().any(|event| event["event"] == "test.trace"));
    let info = events
        .iter()
        .find(|event| event["event"] == "test.info")
        .unwrap();
    assert_eq!(info["span_stack"][0]["name"], "test.session");
    assert_eq!(
        info["span_stack"][0]["fields"]["scenario_id"],
        "scenario.safe"
    );
    assert!(!main_log.contains("C:\\private"));
    assert!(main_log.contains("[redacted]"));

    let critical = fs::read_to_string(log_dir.join("astra-critical.jsonl")).unwrap();
    assert!(critical.contains("test.warn"));
    assert!(critical.contains("test.error"));
    assert!(!critical.contains("test.info"));

    guard.reload_filter("error").unwrap();
    tracing::info!(event = "test.filtered", "must not be written");
    tracing::error!(event = "test.after_reload", "must be written");
    guard.flush().unwrap();
    let reloaded = fs::read_to_string(log_dir.join("astra.jsonl")).unwrap();
    assert!(!reloaded.contains("test.filtered"));
    assert!(reloaded.contains("test.after_reload"));

    guard.flush().unwrap();
    tracing::trace!(event = "test.disabled_trace_warmup", sequence = 0_u64);
    let allocations_before = ALLOCATION_COUNT.load(Ordering::Relaxed);
    for sequence in 0_u64..1_000 {
        tracing::trace!(event = "test.disabled_trace", sequence);
    }
    let allocations_after = ALLOCATION_COUNT.load(Ordering::Relaxed);
    assert_eq!(allocations_before, allocations_after);

    guard.reload_filter("trace").unwrap();
    for sequence in 0_u64..50_000 {
        tracing::trace!(
            event = "test.saturation",
            sequence,
            "queue saturation event"
        );
    }
    guard.flush().unwrap();
    let critical = fs::read_to_string(log_dir.join("astra-critical.jsonl")).unwrap();
    assert!(critical.contains("observability.queue.saturated"));
    let main_files = fs::read_dir(&log_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("astra.jsonl")
        })
        .count();
    assert!((2..=3).contains(&main_files), "main_files={main_files}");

    let manifest = guard.write_fatal_bundle("ASTRA_TEST_FATAL").unwrap();
    assert_eq!(manifest.schema, "astra.crash_bundle.v1");
    assert_eq!(manifest.reason_code, "ASTRA_TEST_FATAL");
    assert!(manifest.log_tail.byte_size > 0);
    assert!(manifest.log_tail.path.starts_with("crash-"));
    assert!(manifest.ring_record_count <= 8);
    assert!(manifest.dropped_count > 0);

    guard.flush().unwrap();
    std::thread::sleep(Duration::from_millis(20));
}
