use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use astra_asset::AssetSidecar;
use astra_cook::{CookRequest, DefaultCookProcessor};
use astra_core::Hash256;
use astra_observability::{
    init_host, ConsoleFormat, CrashReportingMode, HostObservabilityConfig, ObservabilityGuard,
};
use astra_package::{
    MigrationPolicy, PackageBuildRequest, PackageBuilder, PackageManifest, PackageReader,
    SectionCodec, SectionPayload, CURRENT_CONTAINER_VERSION,
};
use astra_platform::{
    migrate_host_profile_json, validate_host_profile, PlatformCapabilityReport,
    PlatformHostConformanceReport, PlatformHostProfile, PlatformId,
};
use astra_player_core::PlayerAutomationReport;
use astra_release::{PackageValidateRequest, ReleaseReport, ReleaseValidator};
use astra_target::{
    validate_manifest, TargetKind, TargetManifest, TargetValidationReport, TargetValidationStatus,
};
use astra_test::{ScenarioReport, ScenarioRunOptions, ScenarioRunner};
use astra_vn::{
    compile_astra_sources, compile_astra_sources_with_options, format_astra_source,
    package_sections_for_story, AstraSource, CompileAstraOptions, FormatOptions,
};
use clap::{Parser, Subcommand, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use tracing::{debug, info};

type CliError = Box<dyn std::error::Error + Send + Sync>;
const WEB_PLAYER_LOADER: &[u8] =
    include_bytes!("../../astra-player-web/web/astra-player-loader.js");
const WEB_AUDIO_WORKLET: &[u8] =
    include_bytes!("../../astra-player-web/web/astra-audio-worklet.js");

#[derive(Parser)]
#[command(name = "astra")]
#[command(about = "AstraEngine Stage 1 command line")]
struct Cli {
    #[arg(long, global = true)]
    log_filter: Option<String>,
    #[arg(long, global = true, value_enum, default_value_t = LogFormat::Compact)]
    log_format: LogFormat,
    #[arg(long, global = true)]
    log_dir: Option<PathBuf>,
    #[arg(long, global = true, default_value_t = astra_observability::DEFAULT_MAX_FILE_BYTES)]
    log_max_file_bytes: usize,
    #[arg(long, global = true, default_value_t = astra_observability::DEFAULT_MAX_ARCHIVES)]
    log_max_archives: usize,
    #[arg(long, global = true)]
    crash_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Cook {
        project: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        out: PathBuf,
    },
    Package {
        #[command(subcommand)]
        command: PackageCommand,
    },
    Test {
        #[command(subcommand)]
        command: TestCommand,
    },
    Report {
        #[command(subcommand)]
        command: ReportCommand,
    },
    Target {
        #[command(subcommand)]
        command: TargetCommand,
    },
    Platform {
        #[command(subcommand)]
        command: PlatformCommand,
    },
    Script {
        #[command(subcommand)]
        command: ScriptCommand,
    },
}

#[derive(Subcommand)]
enum ScriptCommand {
    Check {
        #[arg(required = true)]
        sources: Vec<PathBuf>,
    },
    Format {
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(long, conflicts_with = "write")]
        check: bool,
        #[arg(long, conflicts_with = "check")]
        write: bool,
    },
}

#[derive(Subcommand)]
enum PackageCommand {
    Build {
        cooked: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        target: Option<String>,
    },
    Bundle {
        package: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        profile: String,
        #[arg(long, value_enum)]
        platform: PlatformArg,
        #[arg(long)]
        windows_player: Option<PathBuf>,
        #[arg(long)]
        crash_reporter: Option<PathBuf>,
        #[arg(long)]
        web_player_wasm: Option<PathBuf>,
        #[arg(long)]
        web_player_glue: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
        format: ReportFormat,
    },
    Validate {
        package: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        platform_report: Option<PathBuf>,
        #[arg(long)]
        platform_conformance_report: Option<PathBuf>,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long)]
        player_automation_report: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
        format: ReportFormat,
    },
}

#[derive(Subcommand)]
enum TestCommand {
    Run {
        scenario: PathBuf,
        #[arg(long)]
        headless: bool,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        package: Option<PathBuf>,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
        format: ReportFormat,
    },
}

#[derive(Subcommand)]
enum ReportCommand {
    Explain { report: PathBuf },
}

#[derive(Subcommand)]
enum TargetCommand {
    List {
        project: PathBuf,
        #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
        format: ReportFormat,
    },
    Validate {
        project: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
        format: ReportFormat,
    },
}

#[derive(Subcommand)]
enum PlatformCommand {
    Probe {
        #[arg(long, value_enum)]
        platform: PlatformArg,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
        format: ReportFormat,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReportFormat {
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LogFormat {
    Compact,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PlatformArg {
    Windows,
    Linux,
    Macos,
    Ios,
    Android,
    Web,
}

impl From<PlatformArg> for PlatformId {
    fn from(value: PlatformArg) -> Self {
        match value {
            PlatformArg::Windows => PlatformId::Windows,
            PlatformArg::Linux => PlatformId::Linux,
            PlatformArg::Macos => PlatformId::Macos,
            PlatformArg::Ios => PlatformId::Ios,
            PlatformArg::Android => PlatformId::Android,
            PlatformArg::Web => PlatformId::Web,
        }
    }
}

fn main() -> Result<(), CliError> {
    let cli = Cli::parse();
    let _log_guard = init_logging(&cli)?;
    match cli.command {
        Command::Cook {
            project,
            profile,
            target,
            out,
        } => {
            let manifest = cook_project(project, &profile, target.as_deref(), out)?;
            println!("{}", serde_yaml::to_string(&manifest)?);
        }
        Command::Script { command } => match command {
            ScriptCommand::Check { sources } => {
                let sources = read_astra_sources(&sources)?;
                let compiled =
                    compile_astra_sources_with_options(sources, CompileAstraOptions::default())?;
                println!(
                    "{}",
                    serde_json::json!({
                        "schema": "astra.script_check_report.v1",
                        "status": "pass",
                        "story_hash": compiled.story_hash.to_string(),
                        "command_count": compiled.command_manifest.commands.len()
                    })
                );
            }
            ScriptCommand::Format {
                sources,
                check,
                write,
            } => {
                if !check && !write {
                    return Err("astra script format requires --check or --write".into());
                }
                let mut candidates = Vec::new();
                for path in &sources {
                    let source = fs::read_to_string(path)?;
                    let formatted = format_astra_source(
                        path.to_string_lossy(),
                        &source,
                        FormatOptions::default(),
                    )?;
                    if formatted != source {
                        candidates.push((path.clone(), source, formatted));
                    }
                }
                if check && !candidates.is_empty() {
                    return Err(format!(
                        "ASTRA_SCRIPT_FORMAT_REQUIRED: {} source file(s) require formatting",
                        candidates.len()
                    )
                    .into());
                }
                if write && !candidates.is_empty() {
                    let original = sources
                        .iter()
                        .map(|path| {
                            Ok(AstraSource::new(
                                path.to_string_lossy(),
                                fs::read_to_string(path)?,
                            ))
                        })
                        .collect::<Result<Vec<_>, std::io::Error>>()?;
                    let formatted = original
                        .iter()
                        .map(|source| {
                            candidates
                                .iter()
                                .find(|(path, _, _)| path.to_string_lossy() == source.path)
                                .map_or_else(
                                    || source.clone(),
                                    |(_, _, text)| {
                                        AstraSource::new(source.path.clone(), text.clone())
                                    },
                                )
                        })
                        .collect::<Vec<_>>();
                    let before = compile_astra_sources(original)?;
                    let after = compile_astra_sources(formatted)?;
                    if before.story_hash != after.story_hash {
                        return Err("ASTRA_SCRIPT_FORMAT_SEMANTIC_CHANGE: formatter changed compiled semantics".into());
                    }
                    for (path, _, formatted) in &candidates {
                        atomic_replace(path, formatted.as_bytes())?;
                    }
                }
                println!(
                    "{}",
                    serde_json::json!({
                        "schema": "astra.script_format_report.v1",
                        "status": "pass",
                        "changed": candidates.len(),
                        "mode": if write { "write" } else { "check" }
                    })
                );
            }
        },
        Command::Package { command } => match command {
            PackageCommand::Build {
                cooked,
                out,
                target,
            } => {
                let manifest = read_cook_manifest(&cooked)?;
                let package = build_package_from_cooked(&cooked, manifest, target.as_deref())?;
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(out, package.into_bytes())?;
            }
            PackageCommand::Bundle {
                package,
                out,
                target,
                profile,
                platform,
                windows_player,
                crash_reporter,
                web_player_wasm,
                web_player_glue,
                format,
            } => {
                let artifacts = BundleArtifactInputs {
                    windows_player,
                    crash_reporter,
                    web_player_wasm,
                    web_player_glue,
                };
                let manifest = build_standalone_bundle(
                    &package,
                    &out,
                    &target,
                    &profile,
                    platform.into(),
                    &artifacts,
                )?;
                println!("{}", encode_bundle_manifest(&manifest, format)?);
            }
            PackageCommand::Validate {
                package,
                profile,
                target,
                platform_report,
                platform_conformance_report,
                report,
                player_automation_report,
                format,
            } => {
                let bytes = fs::read(package)?;
                let platform_report = read_platform_report(platform_report.as_deref())?;
                let platform_conformance_report =
                    read_platform_conformance_report(platform_conformance_report.as_deref())?;
                let player_report =
                    read_player_automation_report(player_automation_report.as_deref())?;
                let require_platform_report = release_profile_requires_platform_report(&profile);
                let release_report = ReleaseValidator.validate_package_with_platform_evidence(
                    PackageValidateRequest {
                        package_bytes: bytes,
                        profile,
                        require_ffmpeg: false,
                        target,
                        require_platform_report,
                        platform_report,
                    },
                    platform_conformance_report,
                    player_report,
                )?;
                let encoded = encode_release_report(&release_report, format)?;
                if let Some(path) = report {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, &encoded)?;
                } else {
                    println!("{encoded}");
                }
            }
        },
        Command::Test {
            command:
                TestCommand::Run {
                    scenario,
                    headless,
                    target,
                    profile,
                    platform,
                    package,
                    report,
                    format,
                },
        } => {
            if !headless {
                return Err("Stage 1 scenario runner requires --headless".into());
            }
            info!(
                headless,
                target = target.as_deref().unwrap_or(""),
                profile = profile.as_deref().unwrap_or(""),
                package = package.as_ref().map(|_| "provided").unwrap_or(""),
                format = ?format,
                has_report_path = report.is_some(),
                "cli.test.run"
            );
            let scenario_report = ScenarioRunner::run_file_with_options(
                scenario,
                ScenarioRunOptions {
                    package,
                    target,
                    platform,
                    profile,
                    headless,
                },
            )?;
            let encoded = encode_report(&scenario_report, format)?;
            if let Some(path) = report {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, &encoded)?;
                info!(
                    schema = %scenario_report.schema,
                    status = ?scenario_report.status,
                    "cli.report.write"
                );
            } else {
                debug!(
                    schema = %scenario_report.schema,
                    status = ?scenario_report.status,
                    "cli.report.stdout"
                );
                println!("{encoded}");
            }
        }
        Command::Report {
            command: ReportCommand::Explain { report },
        } => {
            info!("cli.report.explain");
            let text = fs::read_to_string(report)?;
            println!("{}", explain_report(&text)?);
        }
        Command::Target { command } => match command {
            TargetCommand::List { project, format } => {
                let manifest = read_target_manifest(&project)?;
                println!("{}", encode_target_manifest(&manifest, format)?);
            }
            TargetCommand::Validate {
                project,
                target,
                format,
            } => {
                let manifest = read_target_manifest(&project)?;
                let report = validate_manifest(&manifest, target.as_deref());
                println!("{}", encode_target_report(&report, format)?);
            }
        },
        Command::Platform { command } => match command {
            PlatformCommand::Probe {
                platform,
                target,
                report,
                format,
            } => {
                let platform_report = probe_platform(platform.into(), target.as_deref());
                let encoded = encode_platform_report(&platform_report, format)?;
                if let Some(path) = report {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, &encoded)?;
                } else {
                    println!("{encoded}");
                }
            }
        },
    }
    Ok(())
}

fn read_astra_sources(paths: &[PathBuf]) -> Result<Vec<AstraSource>, CliError> {
    paths
        .iter()
        .map(|path| {
            if path.extension().and_then(|extension| extension.to_str()) != Some("astra") {
                return Err(format!(
                    "ASTRA_SCRIPT_SOURCE_EXTENSION: source must use the .astra extension: {}",
                    path.display()
                )
                .into());
            }
            Ok(AstraSource::new(
                path.to_string_lossy(),
                fs::read_to_string(path)?,
            ))
        })
        .collect()
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), CliError> {
    let parent = path.parent().ok_or_else(|| {
        format!(
            "ASTRA_SCRIPT_FORMAT_PATH: source has no parent: {}",
            path.display()
        )
    })?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file_mut().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

fn init_logging(cli: &Cli) -> Result<ObservabilityGuard, CliError> {
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

fn encode_report(report: &ScenarioReport, format: ReportFormat) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}

fn encode_release_report(report: &ReleaseReport, format: ReportFormat) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}

fn encode_target_manifest(
    manifest: &TargetManifest,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(manifest)?,
        ReportFormat::Yaml => serde_yaml::to_string(manifest)?,
    })
}

fn encode_target_report(
    report: &TargetValidationReport,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}

fn encode_platform_report(
    report: &PlatformCapabilityReport,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(report)?,
        ReportFormat::Yaml => serde_yaml::to_string(report)?,
    })
}

fn encode_bundle_manifest(
    manifest: &StandaloneBundleManifest,
    format: ReportFormat,
) -> Result<String, CliError> {
    Ok(match format {
        ReportFormat::Json => serde_json::to_string_pretty(manifest)?,
        ReportFormat::Yaml => serde_yaml::to_string(manifest)?,
    })
}

fn explain_report(text: &str) -> Result<String, CliError> {
    let value: serde_yaml::Value = serde_yaml::from_str(text)?;
    let schema = value
        .get("schema")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or_default();
    if schema == "astra.release_report.v1" {
        let report: ReleaseReport = serde_yaml::from_str(text)?;
        Ok(report.explain())
    } else {
        let report: ScenarioReport = serde_yaml::from_str(text)?;
        Ok(report.explain())
    }
}

fn read_platform_report(
    path: Option<&std::path::Path>,
) -> Result<Option<PlatformCapabilityReport>, CliError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)?;
    Ok(Some(serde_yaml::from_str(&text)?))
}

fn read_platform_conformance_report(
    path: Option<&std::path::Path>,
) -> Result<Option<PlatformHostConformanceReport>, CliError> {
    let Some(path) = path else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)?;
    Ok(Some(serde_yaml::from_str(&text)?))
}

fn read_player_automation_report(
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

fn release_profile_requires_platform_report(profile: &str) -> bool {
    matches!(profile, "desktop-release" | "web-release")
}

fn probe_platform(platform: PlatformId, target: Option<&str>) -> PlatformCapabilityReport {
    match platform {
        PlatformId::Windows => astra_platform_windows::probe(target),
        PlatformId::Linux => astra_platform_linux::probe(target),
        PlatformId::Macos => astra_platform_macos::probe(target),
        PlatformId::Ios => astra_platform_ios::probe(target),
        PlatformId::Android => astra_platform_android::probe(target),
        PlatformId::Web => astra_platform_web::probe(target),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct CookManifest {
    schema: String,
    package_id: String,
    profile: String,
    target: String,
    project_hash: String,
    target_manifest: TargetManifest,
    #[serde(default)]
    scenario_refs: Vec<String>,
    artifacts: Vec<CookedArtifactRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct CookedArtifactRef {
    section_id: String,
    schema: String,
    path: String,
    hash: String,
    #[serde(default = "default_section_codec")]
    codec: SectionCodec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_byte_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    asset_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct ProjectPackageSection {
    id: String,
    schema: String,
    path: String,
    #[serde(default = "default_section_codec")]
    codec: SectionCodec,
    #[serde(default)]
    targets: Vec<String>,
    #[serde(default)]
    profiles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct CookedPlatformProfiles {
    schema: String,
    profiles: Vec<PlatformHostProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct StandaloneBundleManifest {
    schema: String,
    target: String,
    profile: String,
    platform: String,
    entrypoint: String,
    package_hash: String,
    package: String,
    scenario_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mount_policy: Option<String>,
    observability: BundleObservabilityEvidence,
    checks: Vec<PlayerLaunchCheck>,
    files: Vec<StandaloneBundleFile>,
}

#[derive(Debug, Clone, Default)]
struct BundleArtifactInputs {
    windows_player: Option<PathBuf>,
    crash_reporter: Option<PathBuf>,
    web_player_wasm: Option<PathBuf>,
    web_player_glue: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct BundleObservabilityEvidence {
    log_schema: String,
    crash_reporting: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    crash_reporter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct StandaloneBundleFile {
    path: String,
    role: String,
    hash: String,
    byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct PlayerLaunchCheck {
    id: String,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct PlayerDisplayConfig {
    schema: String,
    original_resolution: PlayerResolution,
    scale_filter: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    preview_layers: Vec<PlayerDisplayLayer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct PlayerResolution {
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct PlayerDisplayLayer {
    vfs_uri: String,
    x: u32,
    y: u32,
}

fn default_section_codec() -> SectionCodec {
    SectionCodec::Raw
}

fn project_player_display_config(
    project: &serde_yaml::Value,
) -> Result<Option<PlayerDisplayConfig>, CliError> {
    let Some(display) = project
        .get("nativevn")
        .and_then(|nativevn| nativevn.get("display"))
    else {
        return Ok(None);
    };
    let Some(resolution) = display.get("original_resolution") else {
        return Err(
            "nativevn.display.original_resolution is required when display is declared".into(),
        );
    };
    let width = yaml_u32(resolution.get("width"))
        .ok_or("nativevn.display.original_resolution.width must be a positive integer")?;
    let height = yaml_u32(resolution.get("height"))
        .ok_or("nativevn.display.original_resolution.height must be a positive integer")?;
    if !(1..=16_384).contains(&width) || !(1..=16_384).contains(&height) {
        return Err("nativevn.display.original_resolution dimensions are out of range".into());
    }
    let scale_filter = display
        .get("scale_filter")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("linear");
    if !matches!(scale_filter, "nearest" | "linear") {
        return Err("nativevn.display.scale_filter must be nearest or linear".into());
    }
    let preview_layers = project_display_preview_layers(display)?;
    Ok(Some(PlayerDisplayConfig {
        schema: "astra.player_display_config.v1".to_string(),
        original_resolution: PlayerResolution { width, height },
        scale_filter: scale_filter.to_string(),
        preview_layers,
    }))
}

fn project_display_preview_layers(
    display: &serde_yaml::Value,
) -> Result<Vec<PlayerDisplayLayer>, CliError> {
    let Some(raw_layers) = display.get("preview_layers") else {
        return Ok(Vec::new());
    };
    let layers = raw_layers
        .as_sequence()
        .ok_or("nativevn.display.preview_layers must be a list")?;
    let mut parsed = Vec::new();
    for layer in layers {
        let vfs_uri = layer
            .get("vfs_uri")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("nativevn.display.preview_layers entries require vfs_uri")?;
        validate_player_display_layer_uri(vfs_uri)?;
        let x = yaml_non_negative_u32(layer.get("x"))
            .ok_or("nativevn.display.preview_layers entries require non-negative x")?;
        let y = yaml_non_negative_u32(layer.get("y"))
            .ok_or("nativevn.display.preview_layers entries require non-negative y")?;
        if x > 16_384 || y > 16_384 {
            return Err("nativevn.display.preview_layers coordinates are out of range".into());
        }
        parsed.push(PlayerDisplayLayer {
            vfs_uri: vfs_uri.to_string(),
            x,
            y,
        });
    }
    Ok(parsed)
}

fn validate_player_display_layer_uri(vfs_uri: &str) -> Result<(), CliError> {
    if !vfs_uri.starts_with("package:/")
        || vfs_uri.contains('\\')
        || vfs_uri.contains("..")
        || vfs_uri.contains("://")
        || vfs_uri.trim() != vfs_uri
    {
        return Err("player display preview layer vfs_uri must be a package VFS URI".into());
    }
    Ok(())
}

fn yaml_u32(value: Option<&serde_yaml::Value>) -> Option<u32> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return u32::try_from(number).ok().filter(|number| *number > 0);
    }
    value
        .as_i64()
        .and_then(|number| u32::try_from(number).ok())
        .filter(|number| *number > 0)
}

fn yaml_non_negative_u32(value: Option<&serde_yaml::Value>) -> Option<u32> {
    let value = value?;
    if let Some(number) = value.as_u64() {
        return u32::try_from(number).ok();
    }
    value.as_i64().and_then(|number| u32::try_from(number).ok())
}

fn read_target_manifest(project: &std::path::Path) -> Result<TargetManifest, CliError> {
    let text = fs::read_to_string(project)?;
    TargetManifest::from_project_yaml(&text).map_err(|err| err.to_string().into())
}

fn cook_project(
    project: PathBuf,
    profile: &str,
    target: Option<&str>,
    out: PathBuf,
) -> Result<CookManifest, CliError> {
    tracing::info!(
        target: "astra_cook",
        event = "cook.run.start",
        profile,
        has_target = target.is_some(),
        "cook run started"
    );
    let project_text = fs::read_to_string(&project)?;
    let project_yaml: serde_yaml::Value = serde_yaml::from_str(&project_text)?;
    let target_manifest = TargetManifest::from_project_value(&project_yaml)?;
    let target = target
        .or_else(|| {
            target_manifest
                .targets
                .first()
                .map(|target| target.id.as_str())
        })
        .ok_or("project has no targets")?;
    let target_report = validate_manifest(&target_manifest, Some(target));
    if matches!(
        target_report.status,
        astra_target::TargetValidationStatus::Blocked
    ) {
        return Err(format!("target validation failed: {target}").into());
    }
    let target_descriptor = target_manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    let package_id = project_yaml
        .get("id")
        .and_then(serde_yaml::Value::as_str)
        .unwrap_or("com.example.nativevn")
        .to_string();
    let project_hash = Hash256::from_sha256(project_text.as_bytes()).to_string();
    fs::create_dir_all(&out)?;

    let project_dir = project
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let artifact = serde_json::json!({
        "schema": "astra.cooked_project.v1",
        "package_id": package_id,
        "profile": profile,
        "target": target,
        "project_hash": project_hash,
    })
    .to_string()
    .into_bytes();
    let artifact_path = "compiled_project.json";
    fs::write(out.join(artifact_path), &artifact)?;
    let mut artifacts = vec![CookedArtifactRef {
        section_id: "compiled.project".to_string(),
        schema: "astra.cooked_project.v1".to_string(),
        path: artifact_path.to_string(),
        hash: Hash256::from_sha256(&artifact).to_string(),
        codec: SectionCodec::Raw,
        asset_path: None,
        asset_role: None,
        asset_sha256: None,
        asset_byte_size: None,
        asset_type: None,
        asset_id: None,
    }];
    if let Some(platform_profiles) = cook_platform_profiles(
        &project_yaml,
        &package_id,
        target,
        &target_descriptor.platforms,
    )? {
        let profile_bytes = serde_json::to_vec_pretty(&platform_profiles)?;
        let profile_path = "platform_profiles.json";
        fs::write(out.join(profile_path), &profile_bytes)?;
        artifacts.push(CookedArtifactRef {
            section_id: "platform.profiles".to_string(),
            schema: "astra.platform_profiles.v2".to_string(),
            path: profile_path.to_string(),
            hash: Hash256::from_sha256(&profile_bytes).to_string(),
            codec: SectionCodec::Raw,
            asset_path: None,
            asset_role: None,
            asset_sha256: None,
            asset_byte_size: None,
            asset_type: None,
            asset_id: None,
        });
    }
    if let Some(display_config) = project_player_display_config(&project_yaml)? {
        let display_bytes = serde_json::to_vec_pretty(&display_config)?;
        let display_path = "player_display_config.json";
        fs::write(out.join(display_path), &display_bytes)?;
        artifacts.push(CookedArtifactRef {
            section_id: "player.display_config".to_string(),
            schema: "astra.player_display_config.v1".to_string(),
            path: display_path.to_string(),
            hash: Hash256::from_sha256(&display_bytes).to_string(),
            codec: SectionCodec::Raw,
            asset_path: None,
            asset_role: None,
            asset_sha256: None,
            asset_byte_size: None,
            asset_type: None,
            asset_id: None,
        });
    }
    if project_uses_nativevn(&project_yaml) {
        artifacts.extend(cook_nativevn_sections(
            &project_yaml,
            project_dir,
            &out,
            profile,
            target,
        )?);
    }
    artifacts.extend(cook_project_package_sections(
        &project_yaml,
        project_dir,
        &out,
        profile,
        target,
        &artifacts,
    )?);
    let scenario_refs = scenario_refs_from_project(&project_yaml);
    artifacts.extend(cook_scenario_ref_sections(
        &scenario_refs,
        project_dir,
        &out,
        &artifacts,
    )?);
    let manifest = CookManifest {
        schema: "astra.cook_manifest.v1".to_string(),
        package_id,
        profile: profile.to_string(),
        target: target.to_string(),
        project_hash,
        target_manifest,
        scenario_refs,
        artifacts,
    };
    fs::write(
        out.join("cook_manifest.yaml"),
        serde_yaml::to_string(&manifest)?,
    )?;
    tracing::info!(
        target: "astra_cook",
        event = "cook.run.complete",
        profile,
        target = %manifest.target,
        artifact_count = manifest.artifacts.len(),
        scenario_count = manifest.scenario_refs.len(),
        "cook run completed"
    );
    Ok(manifest)
}

fn cook_platform_profiles(
    project: &serde_yaml::Value,
    package_id: &str,
    target: &str,
    target_platforms: &[String],
) -> Result<Option<CookedPlatformProfiles>, CliError> {
    let Some(value) = project.get("platform_profiles") else {
        return Ok(None);
    };
    let raw_profiles: BTreeMap<String, serde_json::Value> =
        serde_json::from_value(serde_json::to_value(value.clone())?)?;
    let mut selected = Vec::new();
    for (id, raw_profile) in raw_profiles {
        let profile = migrate_host_profile_json(raw_profile)?;
        if profile.id != id {
            return Err(format!("platform profile key {id} does not match profile id").into());
        }
        if profile.target != target || profile.package_id != package_id {
            return Err(
                format!("platform profile {id} is not bound to the cooked target/package").into(),
            );
        }
        if !target_platforms
            .iter()
            .any(|platform| platform == profile.platform.as_str())
        {
            return Err(format!("platform profile {id} is not declared by target {target}").into());
        }
        validate_host_profile(&profile)?;
        selected.push(profile);
    }
    selected.sort_by(|left, right| left.id.cmp(&right.id));
    if selected.is_empty() {
        return Err("platform_profiles must contain at least one selected profile".into());
    }
    Ok(Some(CookedPlatformProfiles {
        schema: "astra.platform_profiles.v2".to_string(),
        profiles: selected,
    }))
}

fn read_cook_manifest(cooked: &std::path::Path) -> Result<CookManifest, CliError> {
    let text = fs::read_to_string(cooked.join("cook_manifest.yaml"))?;
    Ok(serde_yaml::from_str(&text)?)
}

fn project_uses_nativevn(project: &serde_yaml::Value) -> bool {
    project.get("nativevn").is_some()
}

fn cook_nativevn_sections(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    out: &std::path::Path,
    profile: &str,
    target: &str,
) -> Result<Vec<CookedArtifactRef>, CliError> {
    let source_paths = nativevn_source_paths(project, project_dir)?;
    if source_paths.is_empty() {
        return Err("nativevn project must declare at least one .astra source".into());
    }
    let mut sources = Vec::with_capacity(source_paths.len());
    for source in source_paths {
        let source_text = fs::read_to_string(project_dir.join(&source))?;
        sources.push(AstraSource::new(
            normalize_relative_path(&source),
            source_text,
        ));
    }
    let compiled = compile_astra_sources(sources)?;
    let mut profiles = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("profiles")),
    );
    if profiles.is_empty() {
        profiles.push(profile.to_string());
    }
    if !profiles.iter().any(|entry| entry == profile) {
        profiles.push(profile.to_string());
    }

    let sections = package_sections_for_story(&compiled, &profiles, target)?;
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::new();
    for section in sections {
        if section.id == "scenario.refs" {
            continue;
        }
        let file_name = format!("{}.bin", section.id.replace('.', "_"));
        fs::write(section_dir.join(&file_name), &section.payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: section.id,
            schema: section.schema,
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&section.payload).to_string(),
            codec: section.codec,
            asset_path: None,
            asset_role: None,
            asset_sha256: None,
            asset_byte_size: None,
            asset_type: None,
            asset_id: None,
        });
    }
    artifacts.extend(cook_nativevn_asset_sections(
        project,
        project_dir,
        out,
        profile,
    )?);
    Ok(artifacts)
}

fn cook_nativevn_asset_sections(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    out: &std::path::Path,
    profile: &str,
) -> Result<Vec<CookedArtifactRef>, CliError> {
    let mut sidecar_paths = Vec::new();
    for root in nativevn_asset_roots(project) {
        let relative_root = validate_project_relative_path(&root)?;
        let absolute_root = project_dir.join(&relative_root);
        if !absolute_root.is_dir() {
            return Err(format!("nativevn asset root is missing: {root}").into());
        }
        collect_asset_sidecars(project_dir, &absolute_root, &mut sidecar_paths)?;
    }
    sidecar_paths.sort_by_key(|path| normalize_relative_path(path));
    sidecar_paths
        .dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));

    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::new();
    for sidecar_path in sidecar_paths {
        let sidecar_text = fs::read_to_string(project_dir.join(&sidecar_path))?;
        let sidecar = AssetSidecar::from_yaml(&sidecar_text)?;
        let diagnostics = sidecar.validate();
        if !diagnostics.is_empty() {
            return Err(format!("nativevn asset sidecar is invalid: {}", sidecar.source).into());
        }
        let source_path = validate_project_relative_path(&sidecar.source)?;
        let source_bytes = fs::read(project_dir.join(&source_path))?;
        let source_hash = Hash256::from_sha256(&source_bytes);
        if sidecar.source_hash.as_ref() != Some(&source_hash) {
            return Err(format!("nativevn asset hash mismatch: {}", sidecar.source).into());
        }

        let processor = DefaultCookProcessor::new(&sidecar.cook.processor, "1.0.0");
        let cooked = processor.cook(CookRequest {
            sidecar: sidecar.clone(),
            source_bytes,
            target_profile: profile.to_string(),
            processor_version: "1.0.0".to_string(),
        })?;
        let section = cooked.to_section();
        let file_name = format!("{}.bin", section.id.replace('.', "_"));
        fs::write(section_dir.join(&file_name), &section.payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: section.id,
            schema: section.schema,
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&section.payload).to_string(),
            codec: section.codec,
            asset_path: Some(normalize_relative_path(&source_path)),
            asset_role: Some(asset_role_for_path(&sidecar.source, &sidecar.asset_type)),
            asset_sha256: Some(source_hash.to_string()),
            asset_byte_size: Some(fs::metadata(project_dir.join(&source_path))?.len()),
            asset_type: Some(sidecar.asset_type),
            asset_id: Some(sidecar.id.to_string()),
        });
    }
    Ok(artifacts)
}

fn nativevn_asset_roots(project: &serde_yaml::Value) -> Vec<String> {
    string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("asset_roots")),
    )
}

fn collect_asset_sidecars(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_asset_sidecars(root, &path, out)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".astra-asset.yaml"))
        {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

fn asset_role_for_path(path: &str, asset_type: &str) -> String {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let parts = normalized.split('/').collect::<Vec<_>>();
    if parts.contains(&"backgrounds") {
        "background".to_string()
    } else if parts.contains(&"characters") {
        "character_sprite".to_string()
    } else if parts.contains(&"cg") {
        "cg".to_string()
    } else if parts.contains(&"ui") {
        "ui".to_string()
    } else if parts.contains(&"voice") {
        "voice".to_string()
    } else if parts
        .iter()
        .any(|part| matches!(*part, "audio" | "bgm" | "se"))
    {
        "audio".to_string()
    } else if parts.contains(&"movies") {
        "movie".to_string()
    } else if parts.contains(&"fonts") {
        "font".to_string()
    } else if asset_type.starts_with("image.") {
        "image".to_string()
    } else if asset_type.starts_with("audio.") {
        "audio".to_string()
    } else {
        "binary".to_string()
    }
}

fn cook_scenario_ref_sections(
    scenario_refs: &[String],
    project_dir: &std::path::Path,
    out: &std::path::Path,
    existing: &[CookedArtifactRef],
) -> Result<Vec<CookedArtifactRef>, CliError> {
    if scenario_refs.is_empty() {
        return Ok(Vec::new());
    }
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::new();
    for (index, scenario_ref) in scenario_refs.iter().enumerate() {
        if existing
            .iter()
            .any(|artifact| artifact.section_id == *scenario_ref)
            || artifacts
                .iter()
                .any(|artifact: &CookedArtifactRef| artifact.section_id == *scenario_ref)
        {
            return Err(format!("scenario ref {scenario_ref} is declared more than once").into());
        }
        let source_path = resolve_scenario_ref_path(project_dir, scenario_ref)?;
        let payload = fs::read(source_path)
            .map_err(|err| format!("scenario ref {scenario_ref} is not readable: {err}"))?;
        let file_name = format!("scenario_ref_{:04}.bin", index + 1);
        fs::write(section_dir.join(&file_name), &payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: scenario_ref.clone(),
            schema: scenario_ref_schema(&payload),
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&payload).to_string(),
            codec: SectionCodec::Raw,
            asset_path: None,
            asset_role: None,
            asset_sha256: None,
            asset_byte_size: None,
            asset_type: None,
            asset_id: None,
        });
    }
    Ok(artifacts)
}

fn resolve_scenario_ref_path(
    project_dir: &std::path::Path,
    scenario_ref: &str,
) -> Result<PathBuf, CliError> {
    let relative = validate_project_relative_path(scenario_ref)?;
    let project_candidate = project_dir.join(&relative);
    let invocation_candidate = std::path::Path::new(".").join(&relative);
    let project_exists = project_candidate.is_file();
    let invocation_exists = invocation_candidate.is_file();
    match (project_exists, invocation_exists) {
        (true, true) if project_candidate != invocation_candidate => Err(format!(
            "scenario ref {scenario_ref} is ambiguous between project root and invocation root"
        )
        .into()),
        (true, _) => Ok(project_candidate),
        (false, true) => Ok(invocation_candidate),
        (false, false) => Err(format!(
            "scenario ref {scenario_ref} is not readable from project root or invocation root"
        )
        .into()),
    }
}

fn scenario_ref_schema(payload: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(payload) {
        if let Some(schema) = value.get("schema").and_then(serde_json::Value::as_str) {
            return schema.to_string();
        }
    }
    if let Ok(text) = std::str::from_utf8(payload) {
        if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(text) {
            if let Some(schema) = value.get("schema").and_then(serde_yaml::Value::as_str) {
                return schema.to_string();
            }
        }
    }
    "astra.scenario.v1".to_string()
}

fn cook_project_package_sections(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    out: &std::path::Path,
    profile: &str,
    target: &str,
    existing: &[CookedArtifactRef],
) -> Result<Vec<CookedArtifactRef>, CliError> {
    let Some(sections_value) = project.get("package_sections") else {
        return Ok(Vec::new());
    };
    let section_specs: Vec<ProjectPackageSection> = serde_yaml::from_value(sections_value.clone())?;
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::new();
    for spec in section_specs {
        if !spec.targets.is_empty() && !spec.targets.iter().any(|entry| entry == target) {
            continue;
        }
        if !spec.profiles.is_empty() && !spec.profiles.iter().any(|entry| entry == profile) {
            continue;
        }
        if existing
            .iter()
            .any(|artifact| artifact.section_id == spec.id)
            || artifacts
                .iter()
                .any(|artifact: &CookedArtifactRef| artifact.section_id == spec.id)
        {
            return Err(format!("package section {} is declared more than once", spec.id).into());
        }
        let source_path = validate_project_relative_path(&spec.path)?;
        let payload = fs::read(project_dir.join(source_path))?;
        let file_name = format!("{}.bin", spec.id.replace('.', "_"));
        fs::write(section_dir.join(&file_name), &payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: spec.id,
            schema: spec.schema,
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&payload).to_string(),
            codec: spec.codec,
            asset_path: None,
            asset_role: None,
            asset_sha256: None,
            asset_byte_size: None,
            asset_type: None,
            asset_id: None,
        });
    }
    Ok(artifacts)
}

fn nativevn_source_paths(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
) -> Result<Vec<PathBuf>, CliError> {
    let mut sources = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("sources")),
    );
    if sources.is_empty() {
        sources = string_list(project.get("scripts"));
    }
    if sources.is_empty() {
        sources.push("Scripts".to_string());
    }

    let mut paths = Vec::new();
    for source in sources {
        let relative = PathBuf::from(&source);
        let absolute = project_dir.join(&relative);
        if absolute.is_dir() {
            collect_astra_sources(project_dir, &absolute, &mut paths)?;
        } else {
            paths.push(relative);
        }
    }
    paths.sort_by_key(|path| normalize_relative_path(path));
    paths.dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));
    Ok(paths)
}

fn collect_astra_sources(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_astra_sources(root, &path, out)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("astra") {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

fn scenario_refs_from_project(project: &serde_yaml::Value) -> Vec<String> {
    let mut refs = string_list(project.get("scenario_refs"));
    refs.extend(string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("scenario_refs")),
    ));
    refs.sort();
    refs.dedup();
    refs
}

fn string_list(value: Option<&serde_yaml::Value>) -> Vec<String> {
    match value {
        Some(serde_yaml::Value::Sequence(values)) => values
            .iter()
            .filter_map(serde_yaml::Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(serde_yaml::Value::String(value)) => vec![value.clone()],
        _ => Vec::new(),
    }
}

fn normalize_relative_path(path: impl AsRef<std::path::Path>) -> String {
    path.as_ref().to_string_lossy().replace('\\', "/")
}

fn validate_project_relative_path(path: &str) -> Result<PathBuf, CliError> {
    let relative = PathBuf::from(path);
    if relative.is_absolute() {
        return Err("project package section path must be relative".into());
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("project package section path must stay inside the project".into());
            }
        }
    }
    Ok(relative)
}

fn validate_bundle_relative_path(path: &str) -> Result<PathBuf, CliError> {
    let relative = PathBuf::from(path);
    if relative.is_absolute() {
        return Err("bundle path must be relative".into());
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return Err("bundle path must stay inside the bundle".into());
            }
        }
    }
    Ok(relative)
}

fn build_package_from_cooked(
    cooked: &std::path::Path,
    manifest: CookManifest,
    target: Option<&str>,
) -> Result<astra_package::ContainerBlob, CliError> {
    let target = target.unwrap_or(&manifest.target);
    if target != manifest.target {
        return Err(format!(
            "package target {target} does not match cooked target {}",
            manifest.target
        )
        .into());
    }
    let target_report = validate_manifest(&manifest.target_manifest, Some(target));
    if matches!(
        target_report.status,
        astra_target::TargetValidationStatus::Blocked
    ) {
        return Err(format!("target validation failed: {target}").into());
    }
    let package_target_manifest = package_target_manifest(&manifest.target_manifest, target)?;
    let mut artifacts = Vec::new();
    for artifact in &manifest.artifacts {
        let bytes = fs::read(cooked.join(&artifact.path))?;
        let hash = Hash256::from_sha256(&bytes).to_string();
        if hash != artifact.hash {
            return Err(format!("cooked artifact hash mismatch: {}", artifact.section_id).into());
        }
        artifacts.push(SectionPayload::new(
            artifact.section_id.clone(),
            artifact.schema.clone(),
            CURRENT_CONTAINER_VERSION,
            artifact.codec.clone(),
            bytes,
            MigrationPolicy::current(),
        ));
    }
    let asset_vfs_manifest = asset_vfs_manifest_from_cooked(&manifest, &artifacts)?;
    let asset_catalog = asset_catalog_from_cooked(&manifest, &artifacts)?;
    let mut request = PackageBuildRequest::minimal(
        manifest.package_id.clone(),
        manifest.profile.clone(),
        artifacts,
    );
    request.asset_vfs_manifest = asset_vfs_manifest;
    request.asset_catalog = asset_catalog;
    request.provider_policy = product_provider_policy(&manifest.profile)?;
    request.plugin_extension_registry = product_extension_registry()?;
    request.target_manifest = serde_json::to_vec(&package_target_manifest)?;
    request.platform_eligibility = platform_eligibility(&package_target_manifest, target)?;
    if !manifest.scenario_refs.is_empty() {
        request.scenario_refs = serde_json::to_vec(&serde_json::json!({
            "schema": "astra.scenario_refs.v1",
            "scenarios": manifest.scenario_refs,
        }))?;
    }
    request.release_summary = br#"{"schema":"astra.release_summary.v1","status":"built"}"#.to_vec();
    PackageBuilder::build(request).map_err(|err| err.to_string().into())
}

fn product_provider_policy(profile: &str) -> Result<Vec<u8>, CliError> {
    Ok(serde_json::to_vec(&serde_json::json!({
        "schema": "astra.provider_policy.v1",
        "profile": profile,
        "renderer": "astra.renderer2d.wgpu",
        "decode_fallback": "profile_bound",
        "runtime_provider": {
            "schema": "astra.product_runtime_descriptor.v1",
            "runtime_id": "native_vn",
            "product_kind": "visual_novel",
            "provider_id": "astra.runtime.native_vn",
            "supported_targets": ["game"],
            "capabilities": ["runtime.native_vn"],
            "package_sections": [
                "vn.compiled_story",
                "vn.profile_manifest",
                "vn.policy_bundle_manifest",
                "vn.extension_manifest",
                "vn.standard_command_manifest",
                "vn.presentation_provider_manifest",
                "vn.commercial_baseline_manifest",
                "vn.system_story_manifest",
                "vn.system_ui_profile_manifest",
                "vn.advanced_presentation_manifest"
            ],
            "release_checks": [
                "runtime_provider.native_vn",
                "vn.commercial_baseline",
                "vn.system_ui_profile",
                "vn.advanced_presentation",
                "player.full_playable"
            ]
        },
        "bindings": product_provider_bindings()
    }))?)
}

fn product_extension_registry() -> Result<Vec<u8>, CliError> {
    Ok(serde_json::to_vec(&serde_json::json!({
        "schema": "astra.plugin_extension_registry.v1",
        "providers": [{
            "slot": "presentation",
            "provider_id": "astra.vn.standard_presentation",
            "capability": "presentation.vn.standard",
            "phase": "runtime",
            "packaged": true
        }, {
            "slot": "renderer2d",
            "provider_id": "astra.renderer2d.wgpu",
            "capability": "renderer2d.wgpu",
            "phase": "runtime",
            "packaged": true
        }, {
            "slot": "vfs_provider",
            "provider_id": "astra.vfs.package",
            "capability": "vfs.backend.package",
            "phase": "runtime",
            "packaged": true
        }, {
            "slot": "game_runtime_provider",
            "provider_id": "astra.runtime.native_vn",
            "capability": "runtime.native_vn",
            "phase": "runtime",
            "packaged": true
        }],
        "bindings": product_provider_bindings(),
        "conflicts": []
    }))?)
}

fn product_provider_bindings() -> serde_json::Value {
    serde_json::json!([{
        "slot": "presentation",
        "provider_id": "astra.vn.standard_presentation"
    }, {
        "slot": "renderer2d",
        "provider_id": "astra.renderer2d.wgpu"
    }, {
        "slot": "game_runtime_provider",
        "provider_id": "astra.runtime.native_vn"
    }])
}

fn asset_vfs_manifest_from_cooked(
    manifest: &CookManifest,
    sections: &[SectionPayload],
) -> Result<Vec<u8>, CliError> {
    let section_by_id = sections
        .iter()
        .map(|section| (section.id.as_str(), section))
        .collect::<BTreeMap<_, _>>();
    let entries = manifest
        .artifacts
        .iter()
        .map(|artifact| {
            let section = section_by_id
                .get(artifact.section_id.as_str())
                .ok_or_else(|| format!("missing cooked section {}", artifact.section_id))?;
            Ok(serde_json::json!({
                "vfs_uri": artifact_vfs_uri(artifact)?,
                "layer_id": "package.base",
                "source": {
                    "kind": "package_section",
                    "section_id": artifact.section_id
                },
                "offset": 0,
                "size": section.payload.len() as u64,
                "hash": Hash256::from_sha256(&section.payload).to_string(),
                "codec": section_codec_name(&section.codec),
                "media_kind": artifact_media_kind(artifact),
                "diagnostics": []
            }))
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["package.read"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": {
                "kind": "package_section",
                "section_id": "package.manifest"
            },
            "targets": [manifest.target],
            "profiles": [manifest.profile]
        }],
        "entries": entries,
        "whiteouts": []
    }))
    .map_err(Into::into)
}

fn asset_catalog_from_cooked(
    manifest: &CookManifest,
    sections: &[SectionPayload],
) -> Result<Vec<u8>, CliError> {
    let section_ids = sections
        .iter()
        .map(|section| section.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let assets = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.asset_path.is_some())
        .map(|artifact| {
            if !section_ids.contains(artifact.section_id.as_str()) {
                return Err(format!("missing cooked asset section {}", artifact.section_id).into());
            }
            Ok(serde_json::json!({
                "asset_id": artifact.asset_id.as_deref().unwrap_or(&artifact.section_id),
                "vfs_uri": artifact_vfs_uri(artifact)?,
                "media_kind": artifact_media_kind(artifact),
                "tags": asset_tags_for_artifact(artifact),
                "bundle_id": manifest.profile,
                "chunk_id": "base",
                "profiles": [manifest.profile]
            }))
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_catalog.v1",
        "assets": assets
    }))
    .map_err(Into::into)
}

fn artifact_vfs_uri(artifact: &CookedArtifactRef) -> Result<String, CliError> {
    let path = artifact
        .asset_path
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| artifact.section_id.replace('.', "/"));
    Ok(format!("package:/{}", normalize_vfs_path(&path)?))
}

fn normalize_vfs_path(path: &str) -> Result<String, CliError> {
    let normalized = path.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.starts_with("~/")
        || normalized.contains("://")
        || normalized
            .split('/')
            .next()
            .is_some_and(|part| part.ends_with(':'))
    {
        return Err(format!("invalid VFS path {path}").into());
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') || part.chars().any(|ch| ch.is_control()) {
            return Err(format!("invalid VFS path {path}").into());
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(format!("invalid VFS path {path}").into());
    }
    Ok(parts.join("/"))
}

fn section_codec_name(codec: &SectionCodec) -> &'static str {
    match codec {
        SectionCodec::Postcard => "postcard",
        SectionCodec::Raw => "raw",
        SectionCodec::Zstd => "zstd",
    }
}

fn artifact_media_kind(artifact: &CookedArtifactRef) -> String {
    artifact
        .asset_role
        .as_deref()
        .or(artifact.asset_type.as_deref())
        .unwrap_or("data")
        .to_string()
}

fn asset_tags_for_artifact(artifact: &CookedArtifactRef) -> Vec<&str> {
    artifact
        .asset_role
        .as_deref()
        .into_iter()
        .chain(artifact.asset_type.as_deref())
        .collect()
}

fn build_standalone_bundle(
    package: &std::path::Path,
    out: &std::path::Path,
    target: &str,
    profile: &str,
    platform: PlatformId,
    artifacts: &BundleArtifactInputs,
) -> Result<StandaloneBundleManifest, CliError> {
    if out.exists() {
        return Err("ASTRA_BUNDLE_OUTPUT_EXISTS: refusing to replace existing output".into());
    }
    let parent = out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".astra-bundle-staging-")
        .tempdir_in(parent)?;
    let manifest = build_standalone_bundle_into(
        package,
        staging.path(),
        target,
        profile,
        platform,
        artifacts,
    )?;
    let staging_path = staging.keep();
    if let Err(error) = fs::rename(&staging_path, out) {
        let cleanup = fs::remove_dir_all(&staging_path);
        return Err(format!(
            "ASTRA_BUNDLE_COMMIT_FAILED: {error}; staging_cleanup={}",
            if cleanup.is_ok() {
                "complete"
            } else {
                "failed"
            }
        )
        .into());
    }
    Ok(manifest)
}

fn build_standalone_bundle_into(
    package: &std::path::Path,
    out: &std::path::Path,
    target: &str,
    profile: &str,
    platform: PlatformId,
    artifacts: &BundleArtifactInputs,
) -> Result<StandaloneBundleManifest, CliError> {
    let platform_name = platform_id_name(platform);
    let package_bytes = fs::read(package)?;
    let reader = PackageReader::open(&package_bytes)?;
    let package_manifest: PackageManifest =
        reader.container().decode_postcard("package.manifest")?;
    if !reader.has_section("platform.profiles") {
        return Err("standalone bundle requires cooked platform.profiles".into());
    }
    if package_manifest.profile != profile {
        return Err(format!(
            "bundle profile {profile} does not match package profile {}",
            package_manifest.profile
        )
        .into());
    }
    let target_manifest: TargetManifest =
        serde_json::from_slice(&reader.container().read_section("target.manifest")?)?;
    let target_report = validate_manifest(&target_manifest, Some(target));
    if matches!(target_report.status, TargetValidationStatus::Blocked) {
        return Err(format!("target validation failed: {target}").into());
    }
    let target_descriptor = target_manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    if !target_descriptor
        .platforms
        .iter()
        .any(|candidate| candidate == platform_name)
    {
        return Err(format!("target {target} does not support platform {platform_name}").into());
    }
    let display_config = package_player_display_config(&reader)?;

    fs::create_dir_all(out.join("package"))?;
    let bundled_package = out.join("package").join("nativevn.astrapkg");
    fs::write(&bundled_package, &package_bytes)?;
    let scenario_refs = package_scenario_refs(&reader);
    let mut files = vec![bundle_file(
        "package/nativevn.astrapkg",
        "package",
        &package_bytes,
    )];
    let mount_policy = bundle_mount_policy(&reader, out, &mut files)?;
    let mut bundle_checks = Vec::new();
    let mut crash_reporter_ref = None;
    for scenario_ref in &scenario_refs {
        let relative = validate_bundle_relative_path(scenario_ref)?;
        let scenario_bytes = reader
            .container()
            .read_section(scenario_ref)
            .map_err(|_| format!("scenario ref {scenario_ref} is not available in package"))?;
        let destination = out.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, &scenario_bytes)?;
        files.push(bundle_file(scenario_ref, "scenario_ref", &scenario_bytes));
    }

    let entrypoint = match platform {
        PlatformId::Windows => {
            let entrypoint = "AstraPlayer.exe";
            let player_source = artifacts.windows_player.as_deref().ok_or(
                "Windows bundle requires --windows-player pointing to a built AstraPlayer.exe",
            )?;
            let exe_bytes = fs::read(player_source)?;
            let entrypoint_path = out.join(entrypoint);
            fs::write(&entrypoint_path, &exe_bytes)?;
            make_executable(&entrypoint_path)?;
            files.push(bundle_file(entrypoint, "windows_player", &exe_bytes));

            let reporter_name = "AstraCrashReporter.exe";
            let reporter_source = artifacts.crash_reporter.as_deref().ok_or(
                "Windows bundle requires --crash-reporter pointing to a built AstraCrashReporter.exe",
            )?;
            let reporter_bytes = fs::read(reporter_source)?;
            let reporter_self_test = std::process::Command::new(reporter_source)
                .arg("--self-test")
                .output()?;
            if !reporter_self_test.status.success() {
                return Err("AstraCrashReporter self-test failed".into());
            }
            let reporter_report: serde_json::Value =
                serde_json::from_slice(&reporter_self_test.stdout)?;
            if reporter_report
                .get("schema")
                .and_then(serde_json::Value::as_str)
                != Some("astra.crash_reporter_self_test.v1")
                || reporter_report
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    != Some("pass")
            {
                return Err("AstraCrashReporter self-test output is invalid".into());
            }
            fs::write(out.join(reporter_name), &reporter_bytes)?;
            files.push(bundle_file(
                reporter_name,
                "windows_crash_reporter",
                &reporter_bytes,
            ));
            crash_reporter_ref = Some(reporter_name.to_string());
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.packaged".to_string(),
                status: "pass".to_string(),
            });
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.self_test".to_string(),
                status: "pass".to_string(),
            });

            let config = player_config_bytes(target, profile, platform_name, &display_config)?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            entrypoint.to_string()
        }
        PlatformId::Web => {
            let entrypoint = "index.html";
            let index = br#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>AstraVN Player</title></head>
<body><canvas id="astra-player"></canvas><script type="module" src="astra-player-loader.js"></script></body>
</html>
"#;
            fs::write(out.join(entrypoint), index)?;
            files.push(bundle_file(entrypoint, "web_entrypoint", index));
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.not_applicable".to_string(),
                status: "pass".to_string(),
            });

            let config = player_config_bytes(target, profile, platform_name, &display_config)?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));

            fs::write(out.join("astra-player-loader.js"), WEB_PLAYER_LOADER)?;
            files.push(bundle_file(
                "astra-player-loader.js",
                "web_player_loader",
                WEB_PLAYER_LOADER,
            ));
            fs::write(out.join("astra-audio-worklet.js"), WEB_AUDIO_WORKLET)?;
            files.push(bundle_file(
                "astra-audio-worklet.js",
                "web_audio_worklet",
                WEB_AUDIO_WORKLET,
            ));

            for (source, destination, role, missing) in [
                (
                    artifacts.web_player_wasm.as_deref(),
                    "astra_player_web_bg.wasm",
                    "web_player_wasm",
                    "Web bundle requires --web-player-wasm",
                ),
                (
                    artifacts.web_player_glue.as_deref(),
                    "astra_player_web.js",
                    "web_player_glue",
                    "Web bundle requires --web-player-glue",
                ),
            ] {
                let source = source.ok_or(missing)?;
                let bytes = fs::read(source)?;
                match role {
                    "web_player_wasm" => validate_web_player_wasm(&bytes)?,
                    "web_player_glue" => validate_web_player_glue(&bytes)?,
                    _ => unreachable!("web artifact role is closed"),
                }
                fs::write(out.join(destination), &bytes)?;
                files.push(bundle_file(destination, role, &bytes));
            }
            entrypoint.to_string()
        }
        PlatformId::Linux | PlatformId::Macos | PlatformId::Ios | PlatformId::Android => {
            return Err(format!("standalone bundle platform {platform_name} is Stage 6").into());
        }
    };

    let manifest = StandaloneBundleManifest {
        schema: "astra.standalone_bundle_manifest.v2".to_string(),
        target: target.to_string(),
        profile: profile.to_string(),
        platform: platform_name.to_string(),
        entrypoint,
        package_hash: Hash256::from_sha256(&package_bytes).to_string(),
        package: "package/nativevn.astrapkg".to_string(),
        scenario_refs,
        mount_policy,
        observability: BundleObservabilityEvidence {
            log_schema: astra_observability::LOG_EVENT_SCHEMA.to_string(),
            crash_reporting: if platform == PlatformId::Windows {
                "required".to_string()
            } else {
                "disabled".to_string()
            },
            crash_reporter: crash_reporter_ref,
        },
        checks: bundle_checks,
        files,
    };
    fs::write(
        out.join("bundle_manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

fn validate_web_player_wasm(bytes: &[u8]) -> Result<(), CliError> {
    wasmparser::Validator::new()
        .validate_all(bytes)
        .map_err(|error| format!("ASTRA_WEB_PLAYER_WASM_INVALID: {error}"))?;
    Ok(())
}

fn validate_web_player_glue(bytes: &[u8]) -> Result<(), CliError> {
    let source = std::str::from_utf8(bytes)
        .map_err(|error| format!("ASTRA_WEB_PLAYER_GLUE_INVALID: non-UTF-8 module: {error}"))?;
    for required in ["astra_player_web_bg.wasm", "WebAssembly"] {
        if !source.contains(required) {
            return Err(format!(
                "ASTRA_WEB_PLAYER_GLUE_INVALID: missing required wasm-bindgen marker {required}"
            )
            .into());
        }
    }
    if !source.contains("export default") && !source.contains(" as default") {
        return Err(
            "ASTRA_WEB_PLAYER_GLUE_INVALID: missing wasm-bindgen default initializer export".into(),
        );
    }
    for forbidden in [
        "astra-route-report",
        "AstraPlayer.route_model",
        "--dump-dom",
    ] {
        if source.contains(forbidden) {
            return Err(format!(
                "ASTRA_WEB_PLAYER_GLUE_BYPASS: forbidden route/input bypass marker {forbidden}"
            )
            .into());
        }
    }
    Ok(())
}

fn player_config_bytes(
    target: &str,
    profile: &str,
    platform_name: &str,
    display_config: &Option<PlayerDisplayConfig>,
) -> Result<Vec<u8>, CliError> {
    let observability = if platform_name == "windows" {
        serde_json::json!({
            "filter": "warn",
            "console_format": "compact",
            "log_dir": "Saved/Logs",
            "crash_dir": "Saved/Crashes",
            "crash_reporting": "required"
        })
    } else {
        serde_json::json!({
            "filter": "info",
            "console_format": "json",
            "crash_reporting": "disabled"
        })
    };
    let mut config = serde_json::json!({
        "schema": "astra.player_config.v2",
        "target": target,
        "profile": profile,
        "platform": platform_name,
        "package": "package/nativevn.astrapkg",
        "observability": observability
    });
    if let Some(display) = display_config {
        config["display"] = serde_json::to_value(display)?;
    }
    serde_json::to_vec_pretty(&config).map_err(Into::into)
}

fn package_player_display_config(
    reader: &PackageReader,
) -> Result<Option<PlayerDisplayConfig>, CliError> {
    if !reader.has_section("player.display_config") {
        return Ok(None);
    }
    let display: PlayerDisplayConfig =
        serde_json::from_slice(&reader.container().read_section("player.display_config")?)?;
    validate_player_display_config(&display)?;
    Ok(Some(display))
}

fn validate_player_display_config(display: &PlayerDisplayConfig) -> Result<(), CliError> {
    if display.schema != "astra.player_display_config.v1" {
        return Err("unsupported player display config schema".into());
    }
    if !(1..=16_384).contains(&display.original_resolution.width)
        || !(1..=16_384).contains(&display.original_resolution.height)
    {
        return Err("player display original resolution is out of range".into());
    }
    if !matches!(display.scale_filter.as_str(), "nearest" | "linear") {
        return Err("player display scale_filter must be nearest or linear".into());
    }
    for layer in &display.preview_layers {
        validate_player_display_layer_uri(&layer.vfs_uri)?;
        if layer.x > 16_384 || layer.y > 16_384 {
            return Err("player display preview layer coordinates are out of range".into());
        }
    }
    Ok(())
}

fn bundle_mount_policy(
    reader: &PackageReader,
    out: &Path,
    files: &mut Vec<StandaloneBundleFile>,
) -> Result<Option<String>, CliError> {
    if !reader.has_section("tsuinosora.mount_policy") {
        return Ok(None);
    }
    let bytes = reader
        .container()
        .read_bounded("tsuinosora.mount_policy", 256 * 1024)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if json_has_local_path_like(&value) {
        return Err("tsuinosora mount policy contains path-like values".into());
    }
    let path = "AstraPlayer.mount_policy.json";
    fs::write(out.join(path), &bytes)?;
    files.push(bundle_file(path, "mount_policy", &bytes));
    Ok(Some(path.to_string()))
}

fn json_has_local_path_like(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => looks_like_local_path(text),
        serde_json::Value::Array(items) => items.iter().any(json_has_local_path_like),
        serde_json::Value::Object(entries) => entries.values().any(json_has_local_path_like),
        _ => false,
    }
}

fn looks_like_local_path(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0].is_ascii_alphabetic() && pair[1] == b':')
        || value.split(['/', '\\']).any(|part| part == "..")
}

fn package_scenario_refs(reader: &PackageReader) -> Vec<String> {
    let Ok(bytes) = reader.container().read_section("scenario.refs") else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Vec::new();
    };
    let mut refs = value
        .get("scenarios")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    refs.sort();
    refs.dedup();
    refs
}

fn bundle_file(path: &str, role: &str, bytes: &[u8]) -> StandaloneBundleFile {
    StandaloneBundleFile {
        path: path.to_string(),
        role: role.to_string(),
        hash: Hash256::from_sha256(bytes).to_string(),
        byte_size: bytes.len() as u64,
    }
}

fn make_executable(path: &std::path::Path) -> Result<(), CliError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn platform_id_name(platform: PlatformId) -> &'static str {
    match platform {
        PlatformId::Windows => "windows",
        PlatformId::Linux => "linux",
        PlatformId::Macos => "macos",
        PlatformId::Ios => "ios",
        PlatformId::Android => "android",
        PlatformId::Web => "web",
    }
}

fn package_target_manifest(
    manifest: &TargetManifest,
    target: &str,
) -> Result<TargetManifest, CliError> {
    let target = manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    if target.kind != TargetKind::Game || !target.packaged {
        return Err(format!(
            "package target {} must be a packaged game target",
            target.id
        )
        .into());
    }
    Ok(TargetManifest::new(vec![target.clone()]))
}

fn platform_eligibility(manifest: &TargetManifest, target: &str) -> Result<Vec<u8>, CliError> {
    let target = manifest
        .find(target)
        .ok_or_else(|| format!("target {target} is not defined"))?;
    Ok(serde_json::to_vec(&serde_json::json!({
        "schema": "astra.platform_eligibility.v1",
        "target": target.id,
        "profiles": target.default_profile.iter().collect::<Vec<_>>(),
        "platforms": target.platforms,
    }))?)
}
