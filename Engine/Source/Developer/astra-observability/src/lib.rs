mod config;
mod crash;
mod pipeline;
mod record;
mod ring;
mod windows_crash;

pub use config::{
    ConsoleFormat, CrashReportingMode, HostObservabilityConfig, HostRole, DEFAULT_MAX_ARCHIVES,
    DEFAULT_MAX_CRASH_BUNDLES, DEFAULT_MAX_FILE_BYTES, DEFAULT_RING_MAX_BYTES,
    DEFAULT_RING_MAX_RECORDS,
};
pub use crash::{CrashArtifactRef, CrashBundleManifestV1, CRASH_BUNDLE_SCHEMA};
pub use pipeline::{init_host, ObservabilityError, ObservabilityGuard};
pub use record::{LogEventV1, SpanContextV1, LOG_EVENT_SCHEMA};
pub use windows_crash::{
    install_windows_crash_reporter, WindowsCrashReporterConfig, WindowsCrashReporterGuard,
};
