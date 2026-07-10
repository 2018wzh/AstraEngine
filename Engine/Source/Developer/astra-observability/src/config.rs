use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_MAX_ARCHIVES: usize = 8;
pub const DEFAULT_RING_MAX_RECORDS: usize = 4096;
pub const DEFAULT_RING_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_MAX_CRASH_BUNDLES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostRole {
    Cli,
    Player,
    CrashReporter,
    Test,
}

impl HostRole {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Player => "player",
            Self::CrashReporter => "crash_reporter",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleFormat {
    Compact,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashReportingMode {
    Required,
    Optional,
    Disabled,
}

#[derive(Debug, Clone)]
pub struct HostObservabilityConfig {
    pub role: HostRole,
    pub filter: String,
    pub console: bool,
    pub console_format: ConsoleFormat,
    pub log_dir: Option<PathBuf>,
    pub crash_dir: Option<PathBuf>,
    pub crash_reporting: CrashReportingMode,
    pub max_file_bytes: usize,
    pub max_archives: usize,
    pub ring_max_records: usize,
    pub ring_max_bytes: usize,
    pub max_crash_bundles: usize,
}

impl HostObservabilityConfig {
    pub fn for_cli(filter: impl Into<String>) -> Self {
        Self {
            role: HostRole::Cli,
            filter: filter.into(),
            console: true,
            console_format: ConsoleFormat::Compact,
            log_dir: None,
            crash_dir: None,
            crash_reporting: CrashReportingMode::Disabled,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            max_archives: DEFAULT_MAX_ARCHIVES,
            ring_max_records: DEFAULT_RING_MAX_RECORDS,
            ring_max_bytes: DEFAULT_RING_MAX_BYTES,
            max_crash_bundles: DEFAULT_MAX_CRASH_BUNDLES,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.filter.trim().is_empty() {
            return Err("log filter cannot be empty");
        }
        if self.max_file_bytes == 0
            || self.ring_max_records == 0
            || self.ring_max_bytes == 0
            || self.max_crash_bundles == 0
        {
            return Err("observability bounds must be non-zero");
        }
        if self.crash_reporting == CrashReportingMode::Required && self.crash_dir.is_none() {
            return Err("required crash reporting needs an explicit crash directory");
        }
        Ok(())
    }
}
