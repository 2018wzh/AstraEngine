mod allocation;
mod config;
mod crash;
mod perfetto;
mod pipeline;
mod process_memory;
mod record;
mod ring;
mod windows_crash;

pub use allocation::{
    allocation_snapshot, thread_allocation_snapshot, AllocationSnapshot, ThreadAllocationSnapshot,
    TrackingAllocator,
};
pub use config::{
    ConsoleFormat, CrashReportingMode, HostObservabilityConfig, HostRole, DEFAULT_MAX_ARCHIVES,
    DEFAULT_MAX_CRASH_BUNDLES, DEFAULT_MAX_FILE_BYTES, DEFAULT_RING_MAX_BYTES,
    DEFAULT_RING_MAX_RECORDS,
};
pub use crash::{CrashArtifactRef, CrashBundleManifestV1, CRASH_BUNDLE_SCHEMA};
pub use perfetto::{
    PerfettoFlowPhase, PerfettoTraceConfig, PerfettoTraceError, PerfettoTraceSummary,
    PerfettoTraceWriter, DEFAULT_PERFETTO_MAX_BYTES, DEFAULT_PERFETTO_MAX_EVENTS,
};
pub use pipeline::{init_host, ObservabilityError, ObservabilityGuard};
pub use process_memory::{sample_process_memory, ProcessMemoryError, ProcessMemorySample};
pub use record::{LogEventV1, SpanContextV1, LOG_EVENT_SCHEMA};
pub use windows_crash::{
    install_windows_crash_reporter, WindowsCrashReporterConfig, WindowsCrashReporterGuard,
};
