use std::{collections::BTreeMap, fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PlatformId {
    Windows,
    Linux,
    Macos,
    Ios,
    Android,
    Web,
}

impl PlatformId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Ios => "ios",
            Self::Android => "android",
            Self::Web => "web",
        }
    }

    pub fn all() -> [Self; 6] {
        [
            Self::Windows,
            Self::Linux,
            Self::Macos,
            Self::Ios,
            Self::Android,
            Self::Web,
        ]
    }
}

impl fmt::Display for PlatformId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PlatformId {
    type Err = PlatformError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Macos),
            "ios" => Ok(Self::Ios),
            "android" => Ok(Self::Android),
            "web" => Ok(Self::Web),
            _ => Err(PlatformError::new(
                PlatformErrorCode::UnsupportedPlatform,
                "platform.parse",
                "platform id is unsupported",
            )
            .with_field("platform", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlatformErrorCode {
    UnsupportedPlatform,
    PlatformNotImplemented,
    InvalidProfile,
    InvalidHandle,
    StaleHandle,
    InvalidState,
    QueueClosed,
    QueueOverflow,
    PermissionDenied,
    ProviderUnavailable,
    DeviceLost,
    ContextLost,
    IntegrityMismatch,
    ResourceLeak,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetryClass {
    Never,
    AfterPermission,
    AfterDeviceRecovery,
    Transient,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformError {
    pub code: PlatformErrorCode,
    pub operation: String,
    pub message: String,
    pub retry: RetryClass,
    pub fields: BTreeMap<String, String>,
}

impl PlatformError {
    pub fn new(
        code: PlatformErrorCode,
        operation: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            operation: operation.into(),
            message: message.into(),
            retry: RetryClass::Never,
            fields: BTreeMap::new(),
        }
    }

    pub fn with_retry(mut self, retry: RetryClass) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} during {}: {}",
            self.code, self.operation, self.message
        )
    }
}

impl std::error::Error for PlatformError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SdkStatus {
    Present,
    Missing,
    Unknown,
}
