use std::{fmt, str::FromStr};

use astra_core::{Diagnostic, DiagnosticSeverity};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PLATFORM_REPORT_SCHEMA: &str = "astra.platform_capability_report.v1";

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),
    #[error("{0}")]
    Message(String),
}

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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
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
            other => Err(PlatformError::UnsupportedPlatform(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SdkStatus {
    Present,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlatformCapabilityReport {
    pub schema: String,
    pub platform: PlatformId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub sdk_status: SdkStatus,
    pub renderer: Vec<String>,
    pub decode: Vec<String>,
    pub audio: Vec<String>,
    pub filesystem: Vec<String>,
    pub input: Vec<String>,
    pub lifecycle: Vec<String>,
    pub permissions: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
}

impl PlatformCapabilityReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        platform: PlatformId,
        target: Option<String>,
        sdk_status: SdkStatus,
        renderer: Vec<String>,
        decode: Vec<String>,
        audio: Vec<String>,
        filesystem: Vec<String>,
        input: Vec<String>,
        lifecycle: Vec<String>,
        permissions: Vec<String>,
    ) -> Self {
        let mut diagnostics = Vec::new();
        if sdk_status == SdkStatus::Missing {
            diagnostics.push(
                Diagnostic::warning(
                    "ASTRA_PLATFORM_SDK_MISSING",
                    "platform SDK is not available in this environment",
                )
                .with_field("platform", platform.as_str()),
            );
        }
        Self {
            schema: PLATFORM_REPORT_SCHEMA.to_string(),
            platform,
            target,
            sdk_status,
            renderer,
            decode,
            audio,
            filesystem,
            input,
            lifecycle,
            permissions,
            diagnostics,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceToken {
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AudioOutputToken {
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveStoreToken {
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceRequest {
    pub width: u32,
    pub height: u32,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInput {
    pub kind: String,
}

pub trait PlatformHost {
    fn platform(&self) -> PlatformId;
    fn create_surface(&mut self, request: SurfaceRequest) -> Result<SurfaceToken, PlatformError>;
    fn poll_input(&mut self) -> Result<Vec<PlayerInput>, PlatformError>;
    fn audio_output(&mut self) -> Result<AudioOutputToken, PlatformError>;
    fn decode_provider(&mut self) -> Result<String, PlatformError>;
    fn save_store(&mut self) -> Result<SaveStoreToken, PlatformError>;
    fn capability_report(&self) -> PlatformCapabilityReport;
}

#[derive(Debug, Clone)]
pub struct ReportBackedPlatformHost {
    report: PlatformCapabilityReport,
}

impl ReportBackedPlatformHost {
    pub fn new(report: PlatformCapabilityReport) -> Self {
        Self { report }
    }
}

impl PlatformHost for ReportBackedPlatformHost {
    fn platform(&self) -> PlatformId {
        self.report.platform
    }

    fn create_surface(&mut self, request: SurfaceRequest) -> Result<SurfaceToken, PlatformError> {
        if request.width == 0 || request.height == 0 {
            return Err(PlatformError::Message(
                "surface dimensions must be non-zero".to_string(),
            ));
        }
        Ok(SurfaceToken {
            provider: format!("{}.surface", self.report.platform),
        })
    }

    fn poll_input(&mut self) -> Result<Vec<PlayerInput>, PlatformError> {
        Ok(Vec::new())
    }

    fn audio_output(&mut self) -> Result<AudioOutputToken, PlatformError> {
        Ok(AudioOutputToken {
            provider: self
                .report
                .audio
                .first()
                .cloned()
                .unwrap_or_else(|| "none".to_string()),
        })
    }

    fn decode_provider(&mut self) -> Result<String, PlatformError> {
        Ok(self
            .report
            .decode
            .first()
            .cloned()
            .unwrap_or_else(|| "none".to_string()))
    }

    fn save_store(&mut self) -> Result<SaveStoreToken, PlatformError> {
        Ok(SaveStoreToken {
            provider: self
                .report
                .filesystem
                .first()
                .cloned()
                .unwrap_or_else(|| "none".to_string()),
        })
    }

    fn capability_report(&self) -> PlatformCapabilityReport {
        self.report.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlatformValidationStatus {
    Pass,
    Warning,
    Blocked,
}

pub fn validate_capability_report(
    report: &PlatformCapabilityReport,
) -> (PlatformValidationStatus, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if report.schema != PLATFORM_REPORT_SCHEMA {
        diagnostics.push(Diagnostic::blocking(
            "ASTRA_PLATFORM_SCHEMA",
            "platform report schema must be astra.platform_capability_report.v1",
        ));
    }
    if report.sdk_status == SdkStatus::Missing {
        diagnostics.push(
            Diagnostic::blocking(
                "ASTRA_PLATFORM_SDK_MISSING",
                "platform SDK is required for native platform completion",
            )
            .with_field("platform", report.platform.as_str()),
        );
    }
    for (field, values) in [
        ("renderer", &report.renderer),
        ("decode", &report.decode),
        ("audio", &report.audio),
        ("filesystem", &report.filesystem),
        ("input", &report.input),
        ("lifecycle", &report.lifecycle),
    ] {
        if values.is_empty() {
            diagnostics.push(
                Diagnostic::blocking("ASTRA_PLATFORM_CAPABILITY", "capability list is empty")
                    .with_field("platform", report.platform.as_str())
                    .with_field("field", field),
            );
        }
    }

    let status = if diagnostics.iter().any(|diag| {
        matches!(
            diag.severity,
            DiagnosticSeverity::Blocking | DiagnosticSeverity::Error
        )
    }) {
        PlatformValidationStatus::Blocked
    } else if diagnostics
        .iter()
        .any(|diag| diag.severity == DiagnosticSeverity::Warning)
    {
        PlatformValidationStatus::Warning
    } else {
        PlatformValidationStatus::Pass
    };
    (status, diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_missing_sdk_as_blocking_for_native_completion() {
        let report = PlatformCapabilityReport::new(
            PlatformId::Android,
            Some("nativevn-game".to_string()),
            SdkStatus::Missing,
            vec!["vulkan".to_string()],
            vec!["mediacodec".to_string()],
            vec!["aaudio".to_string()],
            vec!["app_storage".to_string()],
            vec!["touch".to_string()],
            vec!["resume".to_string()],
            vec!["network_profile_gated".to_string()],
        );
        let (status, diagnostics) = validate_capability_report(&report);
        assert_eq!(status, PlatformValidationStatus::Blocked);
        assert!(diagnostics
            .iter()
            .any(|diag| diag.code == "ASTRA_PLATFORM_SDK_MISSING"));
    }

    #[test]
    fn report_backed_host_exposes_token_facades() {
        let mut host = ReportBackedPlatformHost::new(PlatformCapabilityReport::new(
            PlatformId::Windows,
            None,
            SdkStatus::Present,
            vec!["wgpu".to_string()],
            vec!["wmf".to_string()],
            vec!["wasapi".to_string()],
            vec!["user_data".to_string()],
            vec!["keyboard".to_string()],
            vec!["window".to_string()],
            vec!["network_profile_gated".to_string()],
        ));
        assert_eq!(host.platform(), PlatformId::Windows);
        assert_eq!(host.decode_provider().unwrap(), "wmf");
        assert_eq!(host.audio_output().unwrap().provider, "wasapi");
        assert!(host
            .create_surface(SurfaceRequest {
                width: 0,
                height: 720,
                title: "invalid".to_string(),
            })
            .is_err());
    }
}
