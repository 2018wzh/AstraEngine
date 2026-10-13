use std::{env, fs};

use astra_observability::{
    init_host, ConsoleFormat, CrashReportingMode, HostObservabilityConfig, ObservabilityGuard,
};
use astra_platform::{PlatformCapabilityReport, PlatformHostConformanceReport, PlatformId};
use astra_player_core::PlayerAutomationReport;
use astra_release::ReleaseReport;
use astra_target::{TargetManifest, TargetValidationReport};

use super::*;

pub(crate) fn init_logging(cli: &Cli) -> Result<ObservabilityGuard, CliError> {
    let filter = cli
        .log_filter
        .clone()
        .or_else(|| env::var("ASTRA_LOG").ok())
        .unwrap_or_else(|| "info".to_string());
    let mut config = HostObservabilityConfig::for_cli(filter);
    config.console_format = match cli.log_format {
        LogFormat::Compact => ConsoleFormat::Compact,
        LogFormat::Json => ConsoleFormat::Json,
    };
    config.log_dir = cli.log_dir.clone();
    config.max_file_bytes = cli.log_max_file_bytes;
    config.max_archives = cli.log_max_archives;
    config.crash_dir = cli.crash_dir.clone();
    config.crash_reporting = if cli.crash_dir.is_some() {
        CrashReportingMode::Required
    } else {
        CrashReportingMode::Disabled
    };
    Ok(init_host(config)?)
}

pub(crate) fn encode_release_report(
    report: &ReleaseReport,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}

pub(crate) fn encode_target_manifest(
    manifest: &TargetManifest,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(manifest)?,
        ReportFormat::Yaml => serde_yaml::to_string(manifest)?,
    })
}

pub(crate) fn encode_target_report(
    report: &TargetValidationReport,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}

pub(crate) fn encode_platform_report(
    report: &PlatformCapabilityReport,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}

pub(crate) fn encode_bundle_manifest(
    manifest: &StandaloneBundleManifest,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(manifest)?,
        ReportFormat::Yaml => serde_yaml::to_string(manifest)?,
    })
}

pub(crate) fn explain_report(text: &str) -> Result<String, CliError> {
    let report: ReleaseReport = serde_yaml::from_str(text)?;
    if report.schema != "astra.release_report.v1" {
        return Err("unsupported release report schema".into());
    }
    Ok(report.explain())
}

pub(crate) fn read_platform_report(
    path: Option<&std::path::Path>,
) -> Result<Option<PlatformCapabilityReport>, CliError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)?;
    Ok(Some(serde_yaml::from_str(&text)?))
}

pub(crate) fn read_platform_conformance_report(
    path: Option<&std::path::Path>,
) -> Result<Option<PlatformHostConformanceReport>, CliError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)?;
    Ok(Some(serde_yaml::from_str(&text)?))
}

pub(crate) fn read_player_automation_report(
    path: Option<&std::path::Path>,
) -> Result<Option<PlayerAutomationReport>, CliError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)?;
    let report = if path.extension().and_then(|ext| ext.to_str()) == Some("yaml")
        || path.extension().and_then(|ext| ext.to_str()) == Some("yml")
    {
        serde_yaml::from_str(&text)?
    } else {
        serde_json::from_str(&text)?
    };
    Ok(Some(report))
}

pub(crate) fn release_profile_requires_platform_report(profile: &str) -> bool {
    matches!(profile, "desktop-release" | "web-release")
}

pub(crate) fn probe_platform(
    platform: PlatformId,
    target: Option<&str>,
) -> PlatformCapabilityReport {
    match platform {
        PlatformId::Windows => astra_platform_windows::probe(target),
        PlatformId::Linux => astra_platform_linux::probe(target),
        PlatformId::Macos => astra_platform_macos::probe(target),
        PlatformId::Ios => astra_platform::probe_ios(target),
        PlatformId::Android => astra_platform_android::probe(target),
        PlatformId::Web => astra_platform_web::probe(target),
    }
}
