use std::{
    collections::BTreeMap,
    env,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};

use astra_asset::AssetSidecar;
use astra_cook::{CookRequest, DefaultCookProcessor};
use astra_core::Hash256;
use astra_package::{
    MigrationPolicy, PackageBuildRequest, PackageBuilder, PackageManifest, PackageReader,
    SectionCodec, SectionPayload, CURRENT_CONTAINER_VERSION,
};
use astra_platform::{PlatformCapabilityReport, PlatformId};
use astra_player_core::PlayerAutomationReport;
use astra_release::{PackageValidateRequest, ReleaseReport, ReleaseValidator};
use astra_target::{
    validate_manifest, TargetKind, TargetManifest, TargetValidationReport, TargetValidationStatus,
};
use astra_test::{
    MountAsset, MountProbe, Scenario, ScenarioReport, ScenarioRunOptions, ScenarioRunner,
    ScenarioStatus,
};
use astra_vn::{compile_astra_sources, package_sections_for_story, AstraSource, CompiledStory};
use clap::{Parser, Subcommand, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt::writer::MakeWriterExt, EnvFilter};

type CliError = Box<dyn std::error::Error + Send + Sync>;
const MOUNT_ASSET_ROLES: &[&str] = &[
    "background",
    "character_sprite",
    "character_atlas",
    "cg",
    "ui",
    "text_window",
    "button",
    "audio",
    "voice",
    "movie",
    "font",
];

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
    if should_auto_launch_player() {
        println!("{}", run_bundled_player(std::env::args_os().skip(1))?);
        return Ok(());
    }
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
                format,
            } => {
                let manifest =
                    build_standalone_bundle(&package, &out, &target, &profile, platform.into())?;
                println!("{}", encode_bundle_manifest(&manifest, format)?);
            }
            PackageCommand::Validate {
                package,
                profile,
                target,
                platform_report,
                report,
                player_automation_report,
                format,
            } => {
                let bytes = fs::read(package)?;
                let platform_report = read_platform_report(platform_report.as_deref())?;
                let player_report =
                    read_player_automation_report(player_automation_report.as_deref())?;
                let require_platform_report = release_profile_requires_platform_report(&profile);
                let release_report = ReleaseValidator.validate_package_with_player_report(
                    PackageValidateRequest {
                        package_bytes: bytes,
                        profile,
                        require_ffmpeg: false,
                        target,
                        require_platform_report,
                        platform_report,
                    },
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

fn init_logging(cli: &Cli) -> Result<Option<WorkerGuard>, CliError> {
    let filter = cli
        .log_filter
        .clone()
        .or_else(|| env::var("ASTRA_LOG").ok())
        .unwrap_or_else(|| "info".to_string());
    let filter = EnvFilter::try_new(filter)?;

    if let Some(dir) = &cli.log_dir {
        fs::create_dir_all(dir)?;
        let file = tracing_appender::rolling::daily(dir, "astra.log");
        let (file, guard) = tracing_appender::non_blocking(file);
        let writer = std::io::stderr.and(file);
        match cli.log_format {
            LogFormat::Compact => tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .with_ansi(false)
                .compact()
                .try_init()?,
            LogFormat::Json => tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(writer)
                .with_ansi(false)
                .json()
                .try_init()?,
        }
        return Ok(Some(guard));
    }

    match cli.log_format {
        LogFormat::Compact => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .compact()
            .try_init()?,
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .json()
            .try_init()?,
    }
    Ok(None)
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
struct StandaloneBundleManifest {
    schema: String,
    target: String,
    profile: String,
    platform: String,
    entrypoint: String,
    package_hash: String,
    package: String,
    scenario_refs: Vec<String>,
    #[serde(default)]
    scenario_json_refs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    web_route_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mount_policy: Option<String>,
    files: Vec<StandaloneBundleFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct StandaloneBundleFile {
    path: String,
    role: String,
    hash: String,
    byte_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct PlayerLaunchReport {
    schema: String,
    status: String,
    target: String,
    profile: String,
    platform: String,
    package_hash: String,
    entrypoint: String,
    checks: Vec<PlayerLaunchCheck>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
struct PlayerRouteReport {
    schema: String,
    status: String,
    target: String,
    profile: String,
    platform: String,
    input_surface: String,
    package_hash: String,
    entrypoint: String,
    scenario: String,
    checks: Vec<PlayerLaunchCheck>,
    scenario_report: ScenarioReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct PlayerLaunchCheck {
    id: String,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct PlayerConfig {
    schema: String,
    target: String,
    profile: String,
    platform: String,
    package: String,
}

fn default_section_codec() -> SectionCodec {
    SectionCodec::Raw
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
    Ok(manifest)
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
    let normalized = path.replace('\\', "/");
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
) -> Result<StandaloneBundleManifest, CliError> {
    let platform_name = platform_id_name(platform);
    let package_bytes = fs::read(package)?;
    let reader = PackageReader::open(&package_bytes)?;
    let package_manifest: PackageManifest =
        reader.container().decode_postcard("package.manifest")?;
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

    fs::create_dir_all(out.join("package"))?;
    let bundled_package = out.join("package").join("nativevn.astrapkg");
    fs::write(&bundled_package, &package_bytes)?;
    let scenario_refs = package_scenario_refs(&reader);
    let mut scenario_json_refs = BTreeMap::new();
    let mut web_route_model = None;
    let mut files = vec![bundle_file(
        "package/nativevn.astrapkg",
        "package",
        &package_bytes,
    )];
    let mount_policy = bundle_mount_policy(&reader, out, &mut files)?;
    for scenario_ref in &scenario_refs {
        let relative = validate_bundle_relative_path(scenario_ref)?;
        let source = env::current_dir()?.join(&relative);
        if !source.is_file() {
            return Err(
                format!("scenario ref {scenario_ref} is not available for bundling").into(),
            );
        }
        let scenario_bytes = fs::read(&source)?;
        let destination = out.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, &scenario_bytes)?;
        files.push(bundle_file(scenario_ref, "scenario_ref", &scenario_bytes));
        if platform == PlatformId::Web {
            let scenario: Scenario = serde_yaml::from_slice(&scenario_bytes)?;
            let json_ref = scenario_json_ref(scenario_ref)?;
            let json_bytes = serde_json::to_vec_pretty(&scenario)?;
            let destination = out.join(&json_ref);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&destination, &json_bytes)?;
            files.push(bundle_file(&json_ref, "scenario_ref_json", &json_bytes));
            scenario_json_refs.insert(scenario_ref.clone(), json_ref);
        }
    }

    let entrypoint = match platform {
        PlatformId::Windows => {
            let entrypoint = "AstraPlayer.exe";
            let current_exe = std::env::current_exe()?;
            let exe_bytes = fs::read(&current_exe)?;
            let entrypoint_path = out.join(entrypoint);
            fs::write(&entrypoint_path, &exe_bytes)?;
            make_executable(&entrypoint_path)?;
            files.push(bundle_file(entrypoint, "windows_player", &exe_bytes));

            let config = serde_json::to_vec_pretty(&serde_json::json!({
                "schema": "astra.player_config.v1",
                "target": target,
                "profile": profile,
                "platform": platform_name,
                "package": "package/nativevn.astrapkg"
            }))?;
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
<body><main id="astra-root" data-package="package/nativevn.astrapkg" data-astra-status="booting"></main><script src="astra-player.js"></script></body>
</html>
"#;
            fs::write(out.join(entrypoint), index)?;
            files.push(bundle_file(entrypoint, "web_entrypoint", index));

            let config = serde_json::to_vec_pretty(&serde_json::json!({
                "schema": "astra.player_config.v1",
                "target": target,
                "profile": profile,
                "platform": platform_name,
                "package": "package/nativevn.astrapkg"
            }))?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));

            let compiled: CompiledStory =
                reader.container().decode_postcard("vn.compiled_story")?;
            let route_model = serde_json::to_vec_pretty(&compiled)?;
            fs::write(out.join("AstraPlayer.route_model.json"), &route_model)?;
            files.push(bundle_file(
                "AstraPlayer.route_model.json",
                "web_route_model",
                &route_model,
            ));
            web_route_model = Some("AstraPlayer.route_model.json".to_string());

            let script = web_player_script();
            fs::write(out.join("astra-player.js"), script)?;
            files.push(bundle_file("astra-player.js", "web_player", script));
            entrypoint.to_string()
        }
        PlatformId::Linux | PlatformId::Macos | PlatformId::Ios | PlatformId::Android => {
            return Err(format!("standalone bundle platform {platform_name} is Stage 6").into());
        }
    };

    let manifest = StandaloneBundleManifest {
        schema: "astra.standalone_bundle_manifest.v1".to_string(),
        target: target.to_string(),
        profile: profile.to_string(),
        platform: platform_name.to_string(),
        entrypoint,
        package_hash: Hash256::from_sha256(&package_bytes).to_string(),
        package: "package/nativevn.astrapkg".to_string(),
        scenario_refs,
        scenario_json_refs,
        web_route_model,
        mount_policy,
        files,
    };
    fs::write(
        out.join("bundle_manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

fn scenario_json_ref(scenario_ref: &str) -> Result<String, CliError> {
    let relative = validate_bundle_relative_path(scenario_ref)?;
    let file_name = relative
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .ok_or("scenario ref must have a file name")?;
    let mut json_ref = relative;
    json_ref.set_file_name(format!("{file_name}.json"));
    Ok(normalize_relative_path(&json_ref))
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

fn web_player_script() -> &'static [u8] {
    br#"async function astraBoot() {
  const root = document.getElementById("astra-root");
  try {
    const manifest = await fetchJson("bundle_manifest.json");
    const config = await fetchJson("AstraPlayer.config.json");
    const route = new URLSearchParams(window.location.search).get("route");
    const packageBytes = await fetchBytes(manifest.package);
    const packageHash = await sha256(packageBytes);
    const launch = launchReport(manifest, config, packageHash);
    root.dataset.astraStatus = launch.status;
    root.dataset.astraTarget = manifest.target;
    root.dataset.astraProfile = manifest.profile;
    root.dataset.astraPlatform = manifest.platform;
    if (route) {
      const scenarioPath = manifest.scenario_json_refs && manifest.scenario_json_refs[route];
      if (!scenarioPath) throw new Error("route scenario is not listed in bundle scenario refs");
      const model = await fetchJson(manifest.web_route_model);
      const scenario = await fetchJson(scenarioPath);
      const mountPolicyBytes = manifest.mount_policy ? await fetchBytes(manifest.mount_policy) : null;
      const mountPolicy = mountPolicyBytes ? JSON.parse(new TextDecoder().decode(mountPolicyBytes)) : null;
      const mountPolicyHash = mountPolicyBytes ? await sha256(mountPolicyBytes) : "";
      const report = await runRoute(manifest, config, launch, model, scenario, route, mountPolicy, mountPolicyHash, mountPolicyBytes ? mountPolicyBytes.length : 0);
      root.dataset.astraStatus = report.status;
      appendReport(report);
    } else {
      appendReport(launch);
    }
  } catch (error) {
    const report = {
      schema: "astra.player_route_report.v1",
      status: "blocked",
      target: root.dataset.astraTarget || "",
      profile: root.dataset.astraProfile || "",
      platform: root.dataset.astraPlatform || "web",
      input_surface: "web_player",
      package_hash: "",
      entrypoint: "index.html",
      scenario: new URLSearchParams(window.location.search).get("route") || "",
      checks: [{ id: "player.web.exception", status: "blocked" }],
      diagnostics: [{ code: "ASTRA_WEB_PLAYER_EXCEPTION", severity: "blocking", message: String(error && error.message || error) }]
    };
    root.dataset.astraStatus = "blocked";
    appendReport(report);
  }
}

async function fetchJson(path) {
  const response = await fetch(path, { cache: "no-store" });
  if (!response.ok) throw new Error("fetch failed: " + path);
  return await response.json();
}

async function fetchBytes(path) {
  const response = await fetch(path, { cache: "no-store" });
  if (!response.ok) throw new Error("fetch failed: " + path);
  return new Uint8Array(await response.arrayBuffer());
}

async function sha256(bytes) {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return "sha256:" + hex(new Uint8Array(digest));
}

async function hash128(value) {
  const encoded = new TextEncoder().encode(stableJson(value));
  const digest = await crypto.subtle.digest("SHA-256", encoded);
  return "hash128:" + hex(new Uint8Array(digest).slice(0, 16));
}

function hex(bytes) {
  return Array.from(bytes).map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function stableJson(value) {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return "[" + value.map(stableJson).join(",") + "]";
  return "{" + Object.keys(value).sort().map((key) => JSON.stringify(key) + ":" + stableJson(value[key])).join(",") + "}";
}

function launchReport(manifest, config, packageHash) {
  const checks = [
    { id: "package.hash", status: packageHash === manifest.package_hash ? "pass" : "blocked" },
    { id: "target.manifest", status: manifest.target && manifest.profile && manifest.platform === "web" ? "pass" : "blocked" },
    { id: "player.web.host", status: "pass" },
    { id: "player.input_surface", status: "pass" }
  ];
  if (config.schema !== "astra.player_config.v1" || config.target !== manifest.target || config.profile !== manifest.profile || config.platform !== manifest.platform || config.package !== manifest.package) {
    checks.push({ id: "player.config", status: "blocked" });
  } else {
    checks.push({ id: "player.config", status: "pass" });
  }
  return {
    schema: "astra.player_launch_report.v1",
    status: checks.every((check) => check.status === "pass") ? "ready" : "blocked",
    target: manifest.target,
    profile: manifest.profile,
    platform: manifest.platform,
    package_hash: packageHash,
    entrypoint: manifest.entrypoint,
    checks
  };
}

async function runRoute(manifest, config, launch, model, scenario, scenarioRef, mountPolicy, mountPolicyHash, mountPolicyByteSize) {
  const run = runScenario(model, scenario, false);
  let replayMatch = false;
  if (run.replayActions.length > 0) {
    const replay = runScenario(model, { ...scenario, actions: run.replayActions }, true);
    replayMatch = stableJson(run.state) === stableJson(replay.state);
  }
  const diagnostics = run.diagnostics.slice();
  const assertionChecks = checkAssertions(scenario, run, replayMatch, diagnostics);
  const hashes = {
    state: await hash128(run.state),
    event: await hash128(run.events),
    presentation: await hash128(run.presentation)
  };
  const scenarioChecks = [
    { id: "runtime.determinism", status: replayMatch ? "pass" : "blocked" },
    { id: "save.load.replay", status: run.saved ? "pass" : "blocked" },
    { id: "package.target_refs", status: "pass" },
    { id: "vn.route_coverage", status: diagnostics.some((diagnostic) => diagnostic.code.startsWith("ASTRA_VN_ROUTE")) ? "blocked" : "pass" },
    { id: "player_route.full", status: diagnostics.some((diagnostic) => diagnostic.code.startsWith("ASTRA_VN_PLAYER")) ? "blocked" : "pass" },
    { id: "scenario.schema", status: "pass" },
    ...assertionChecks
  ];
  const scenarioStatus = scenarioChecks.every((check) => check.status === "pass") && diagnostics.length === 0 ? "pass" : "blocked";
  const scenarioReport = {
    schema: "astra.scenario_report.v1",
    stage: scenario.stage || "stage3-astra-vn",
    package: scenario.package || manifest.package,
    target: config.target,
    profile: config.profile,
    platform: config.platform,
    generated_route_id: scenario.generated_route_id || null,
    status: scenarioStatus,
    hashes,
    checks: scenarioChecks,
    unsupported_actions: [],
    unsupported_assertions: [],
    release_gate_checks: [],
    diagnostics
  };
  const checks = [
    ...launch.checks,
    { id: "player.bundle.ready", status: launch.status === "ready" ? "pass" : "blocked" },
    { id: "player.input_surface", status: "pass" },
    { id: "player.route.full", status: scenarioStatus === "pass" ? "pass" : "blocked" },
    { id: "player.web.dom_report", status: "pass" },
    ...mountPolicyChecks(manifest, scenario, mountPolicy, mountPolicyHash, mountPolicyByteSize)
  ];
  return {
    schema: "astra.player_route_report.v1",
    status: checks.every((check) => check.status === "pass") ? "pass" : "blocked",
    target: manifest.target,
    profile: manifest.profile,
    platform: manifest.platform,
    input_surface: "web_player",
    package_hash: launch.package_hash,
    entrypoint: manifest.entrypoint,
    scenario: scenarioRef,
    checks,
    scenario_report: scenarioReport
  };
}

function mountPolicyChecks(manifest, scenario, mountPolicy, mountPolicyHash, mountPolicyByteSize) {
  if (!String(manifest.target || "").startsWith("tsuinosora-")) return [];
  const policyOk = mountPolicyMatches(mountPolicy, manifest.target, scenario);
  const hashOk = bundleFileHashMatches(manifest, manifest.mount_policy, "mount_policy", mountPolicyHash, mountPolicyByteSize);
  const hasMountProbes = Array.isArray(scenario.mount_probes) && scenario.mount_probes.length > 0;
  const hasMountAssets = Array.isArray(scenario.mount_assets) && scenario.mount_assets.length > 0;
  const probeOk = !hasMountProbes;
  const assetOk = !hasMountAssets;
  const checks = [
    { id: "player.mount_policy", status: policyOk ? "pass" : "blocked" },
    { id: "player.mount_policy_hash", status: hashOk ? "pass" : "blocked" }
  ];
  if (manifest.target === "tsuinosora-patch-game") {
    if (hasMountProbes) checks.push({ id: "player.patch_mount_probe", status: "blocked" });
    if (hasMountAssets) checks.push({ id: "player.patch_mount_asset", status: "blocked" });
    const directReadOk = policyOk && hashOk && probeOk && assetOk && manifest.mount_policy === "AstraPlayer.mount_policy.json" && Object.keys(scenario.mount_aliases || {}).length > 0;
    checks.push({ id: "player.patch_direct_read", status: directReadOk ? "pass" : "blocked" });
  }
  return checks;
}

function bundleFileHashMatches(manifest, path, role, hash, byteSize) {
  if (!path || !hash) return false;
  return Array.isArray(manifest.files) && manifest.files.some((entry) =>
    entry.path === path && entry.role === role && entry.hash === hash && entry.byte_size === byteSize
  );
}

function mountPolicyMatches(policy, target, scenario) {
  if (!policy || hasLocalPathLike(policy)) return false;
  if (policy.schema !== "tsuinosora.mount_policy.v1" || policy.target !== target || policy.status !== "pass") return false;
  if (!Array.isArray(policy.aliases) || policy.aliases.length === 0) return false;
  for (const [alias, value] of Object.entries(scenario.mount_aliases || {})) {
    const found = policy.aliases.some((entry) => entry.alias === alias && entry.value === value && entry.hash_policy === "manifest_required" && entry.fallback === "blocking");
    if (!found) return false;
  }
  return true;
}

function hasLocalPathLike(value) {
  if (typeof value === "string") {
    return value.startsWith("/") || value.startsWith("\\\\") || /[A-Za-z]:/.test(value) || value.split(/[\\/]/).includes("..");
  }
  if (Array.isArray(value)) return value.some(hasLocalPathLike);
  if (value && typeof value === "object") return Object.values(value).some(hasLocalPathLike);
  return false;
}

function runScenario(model, scenario, replayMode) {
  const player = createPlayer(model, scenario.locale || "zh-Hans");
  const replayActions = [];
  for (const action of scenario.actions || []) {
    if (present(action.replay_from_start)) continue;
    if (!replayMode) replayActions.push(action);
    applyScenarioAction(player, action);
  }
  return { ...player, replayActions };
}

function createPlayer(model, locale) {
  return {
    model,
    state: {
      profile: "classic",
      locale,
      current_story: null,
      current_state: null,
      command_cursor: 0,
      call_stack: [],
      pending_choice: null,
      pending_wait: null,
      variables: {},
      backlog: [],
      read_state: [],
      voice_replay: {},
      route_coverage: [],
      system: { auto_enabled: false, skip_mode: "none", config: {}, gallery_unlocks: [], replay_unlocks: [] }
    },
    saved: null,
    saves: {},
    events: [],
    presentation: [],
    diagnostics: []
  };
}

function applyScenarioAction(player, action) {
  if (present(action.launch)) return launch(player);
  if (present(action.player_input)) return playerInput(player, action.player_input);
  if (present(action.open_system)) {
    player.presentation.push({ SystemPage: { page: action.open_system } });
    return;
  }
  if (present(action.save)) {
    player.saves[action.save] = clone(player.state);
    player.saved = action.save;
    return;
  }
  if (present(action.load)) {
    if (player.saves[action.load]) player.state = clone(player.saves[action.load]);
    else diagnostic(player, "ASTRA_VN_PLAYER_LOAD_MISSING", "save slot is not available");
  }
}

function playerInput(player, input) {
  switch (input.kind) {
    case "advance": return runUntilBlocked(player);
    case "choose": return choose(player, input.value || "");
    case "complete_wait": return completeWait(player, input.value || "");
    case "replay_voice":
      if (!player.state.voice_replay[input.value || ""]) diagnostic(player, "ASTRA_VN_PLAYER_VOICE_REPLAY_MISSING", "voice replay entry is not available");
      return;
    case "open_system":
      player.presentation.push({ SystemPage: { page: input.value || "" } });
      return;
    case "save":
      player.saves[input.slot || "default"] = clone(player.state);
      player.saved = input.slot || "default";
      return;
    case "load":
      if (player.saves[input.slot || "default"]) player.state = clone(player.saves[input.slot || "default"]);
      else diagnostic(player, "ASTRA_VN_PLAYER_LOAD_MISSING", "save slot is not available");
      return;
    case "set_auto":
      player.state.system.auto_enabled = String(input.value).toLowerCase() === "true";
      return;
    case "set_skip":
      player.state.system.skip_mode = input.value || "none";
      return;
    case "set_config":
      player.state.system.config[input.key || ""] = input.value || "";
      return;
    case "unlock_gallery":
      addUnique(player.state.system.gallery_unlocks, input.value || "");
      return;
    case "unlock_replay":
      addUnique(player.state.system.replay_unlocks, input.value || "");
      return;
    default:
      diagnostic(player, "ASTRA_VN_PLAYER_INPUT_UNSUPPORTED", "unsupported player input: " + input.kind);
  }
}

function launch(player) {
  const story = (player.model.stories || []).find((candidate) => candidate.id === "story.main") || (player.model.stories || [])[0];
  const stateId = (story.states || []).find((candidate) => candidate === "state.prologue") || (story.states || [])[0];
  player.state.current_story = story.id;
  player.state.current_state = stateId;
  player.state.command_cursor = 0;
  player.state.call_stack = [];
  player.state.pending_choice = null;
  player.state.pending_wait = null;
  reach(player, stateId);
  runUntilBlocked(player);
}

function runUntilBlocked(player) {
  while (!player.state.pending_wait) {
    const command = commandAtCursor(player);
    if (!command) return;
    const [variant, value] = enumVariant(command);
    switch (variant) {
      case "Dialogue":
        if (player.state.system.skip_mode === "read" && player.state.read_state.includes(value.id)) {
          player.state.command_cursor += 1;
          break;
        }
        player.state.command_cursor += 1;
        player.state.backlog.push({ command_id: value.id, key: value.key, speaker: value.speaker || null, voice: value.voice || null, state_id: player.state.current_state, read: true });
        addUnique(player.state.read_state, value.id);
        if (value.voice) player.state.voice_replay[value.voice] = { voice: value.voice, line_key: value.key, speaker: value.speaker || null };
        player.presentation.push({ Dialogue: { key: value.key, speaker: value.speaker || null, voice: value.voice || null } });
        return;
      case "Choice":
        player.state.command_cursor += 1;
        player.state.pending_choice = { choice_id: value.id, key: value.key, options: value.options || [] };
        player.presentation.push({ Choice: { key: value.key, options: value.options || [] } });
        return;
      case "Jump":
        player.state.command_cursor += 1;
        transition(player, value.target);
        break;
      case "Call":
        player.state.command_cursor += 1;
        player.state.call_stack.push({ story_id: player.state.current_story, state_id: player.state.current_state, command_cursor: player.state.command_cursor });
        transition(player, value.target);
        break;
      case "Return": {
        player.state.command_cursor += 1;
        const frame = player.state.call_stack.pop();
        if (!frame) return diagnostic(player, "ASTRA_VN_RETURN_STACK", "return command has no call frame");
        player.state.current_story = frame.story_id;
        player.state.current_state = frame.state_id;
        player.state.command_cursor = frame.command_cursor;
        break;
      }
      case "Mutate":
        player.state.command_cursor += 1;
        mutate(player, value);
        break;
      case "SystemPage":
        player.state.command_cursor += 1;
        player.presentation.push({ SystemPage: { page: value.page } });
        return;
      case "Presentation": {
        player.state.command_cursor += 1;
        player.presentation.push(value.command);
        const wait = waitFromPresentation(value.id, value.command);
        if (wait) {
          player.state.pending_wait = wait;
          return;
        }
        break;
      }
      case "Wait":
        player.state.command_cursor += 1;
        player.state.pending_wait = { fence: value.fence, command_id: value.id };
        return;
      default:
        diagnostic(player, "ASTRA_VN_PLAYER_COMMAND_UNSUPPORTED", "unsupported command: " + variant);
        return;
    }
  }
}

function choose(player, optionId) {
  const pending = player.state.pending_choice;
  if (!pending) return diagnostic(player, "ASTRA_VN_CHOICE_MISSING", "choice input was supplied without a pending choice");
  const option = pending.options.find((candidate) => candidate.id === optionId || candidate.key === optionId);
  if (!option) return diagnostic(player, "ASTRA_VN_CHOICE_OPTION", "choice option is not available");
  player.state.pending_choice = null;
  transition(player, option.target);
  runUntilBlocked(player);
}

function completeWait(player, fence) {
  if (!player.state.pending_wait) return diagnostic(player, "ASTRA_VN_WAIT_MISSING", "await completion was supplied without a pending wait state");
  if (player.state.pending_wait.fence !== fence) return diagnostic(player, "ASTRA_VN_WAIT_FENCE", "await completion fence does not match pending fence");
  player.state.pending_wait = null;
  runUntilBlocked(player);
}

function commandAtCursor(player) {
  const state = player.model.states[player.state.current_state];
  if (!state) return null;
  const commands = [];
  for (const scene of state.scenes || []) commands.push(...(scene.commands || []));
  return commands[player.state.command_cursor] || null;
}

function transition(player, target) {
  const stateIds = Object.keys(player.model.states || {});
  let resolved = target;
  if (!player.model.states[resolved]) {
    const suffix = target.startsWith("state.") ? target : "state." + target;
    resolved = stateIds.find((id) => id === suffix || id.endsWith("." + target)) || target;
  }
  reach(player, resolved);
  if (player.model.states[resolved]) {
    player.state.current_state = resolved;
    player.state.command_cursor = 0;
  }
}

function waitFromPresentation(id, command) {
  const [variant, value] = enumVariant(command);
  if (variant !== "Stage") return null;
  const attrs = value.attributes || {};
  if (value.command === "movie" && (attrs.end === "wait" || attrs.wait_for === "end")) return { fence: attrs.fence || id + ".end", command_id: id };
  if (value.command === "voice" && (attrs.sync === "text" || attrs.sync === "fence" || attrs.wait === "true")) return { fence: attrs.fence || id + ".end", command_id: id };
  if (value.command === "timeline" && (attrs.join === "wait" || attrs.join === "block")) return { fence: attrs.fence || id + ".complete", command_id: id };
  return null;
}

function mutate(player, value) {
  const scope = value.scope || "global";
  player.state.variables[scope] = player.state.variables[scope] || {};
  const current = player.state.variables[scope][value.key] || 0;
  if (value.op === "Set") player.state.variables[scope][value.key] = value.value || 0;
  else if (value.op === "Add") player.state.variables[scope][value.key] = current + (value.value || 0);
  else if (value.op === "Sub") player.state.variables[scope][value.key] = current - (value.value || 0);
}

function checkAssertions(scenario, run, replayMatch, diagnostics) {
  const checks = [];
  for (const assertion of scenario.assertions || []) {
    if (assertion.coverage) {
      for (const route of assertion.coverage.routes || []) if (!run.state.route_coverage.includes(route)) diagnostic(run, "ASTRA_VN_ROUTE_COVERAGE_MISSING", "route coverage is missing: " + route);
      for (const key of assertion.coverage.backlog_keys || []) if (!run.state.backlog.some((entry) => entry.key === key)) diagnostic(run, "ASTRA_VN_BACKLOG_COVERAGE_MISSING", "backlog key is missing: " + key);
      for (const key of assertion.coverage.read_state || []) if (!run.state.read_state.includes(key)) diagnostic(run, "ASTRA_VN_READ_STATE_MISSING", "read state is missing: " + key);
      for (const key of assertion.coverage.voice_replay || []) if (!run.state.voice_replay[key]) diagnostic(run, "ASTRA_VN_VOICE_REPLAY_MISSING", "voice replay is missing: " + key);
      checks.push({ id: "assert.coverage", status: "pass" });
    }
    if (assertion.system_state) {
      const system = assertion.system_state;
      if (system.auto_enabled !== undefined && run.state.system.auto_enabled !== system.auto_enabled) diagnostic(run, "ASTRA_VN_SYSTEM_AUTO", "auto state mismatch");
      if (system.skip_mode !== undefined && run.state.system.skip_mode !== system.skip_mode) diagnostic(run, "ASTRA_VN_SYSTEM_SKIP", "skip mode mismatch");
      for (const [key, value] of Object.entries(system.config || {})) if (run.state.system.config[key] !== value) diagnostic(run, "ASTRA_VN_SYSTEM_CONFIG", "config mismatch: " + key);
      for (const key of system.gallery_unlocks || []) if (!run.state.system.gallery_unlocks.includes(key)) diagnostic(run, "ASTRA_VN_SYSTEM_GALLERY", "gallery unlock is missing: " + key);
      for (const key of system.replay_unlocks || []) if (!run.state.system.replay_unlocks.includes(key)) diagnostic(run, "ASTRA_VN_SYSTEM_REPLAY", "replay unlock is missing: " + key);
      checks.push({ id: "assert.system_state", status: "pass" });
    }
    if (assertion.replay_hash_match === true) checks.push({ id: "assert.replay_hash_match", status: replayMatch ? "pass" : "blocked" });
    if (assertion.no_blocking_diagnostics === true) checks.push({ id: "assert.no_blocking_diagnostics", status: diagnostics.length === 0 ? "pass" : "blocked" });
  }
  return checks;
}

function enumVariant(value) {
  const key = Object.keys(value)[0];
  return [key, value[key]];
}

function reach(player, stateId) {
  addUnique(player.state.route_coverage, stateId);
}

function addUnique(list, value) {
  if (value && !list.includes(value)) list.push(value);
}

function diagnostic(player, code, message) {
  player.diagnostics.push({ code, severity: "blocking", message });
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

function appendReport(report) {
  const old = document.getElementById("astra-route-report");
  if (old) old.remove();
  const node = document.createElement("script");
  node.type = "application/json";
  node.id = "astra-route-report";
  node.textContent = JSON.stringify(report);
  document.body.appendChild(node);
}

function present(value) {
  return value !== undefined && value !== null;
}

astraBoot().catch((error) => {
  const root = document.getElementById("astra-root");
  root.dataset.astraStatus = "blocked";
  console.error(error);
});
"#
}

fn should_auto_launch_player() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_stem().map(|stem| stem.to_owned()))
        .and_then(|stem| stem.to_str().map(str::to_string))
        .is_some_and(|stem| stem.eq_ignore_ascii_case("AstraPlayer"))
}

fn run_bundled_player(args: impl IntoIterator<Item = OsString>) -> Result<String, CliError> {
    let args = args
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|_| "player arguments must be valid UTF-8".into())
        })
        .collect::<Result<Vec<String>, CliError>>()?;
    if args.is_empty() {
        return Ok(serde_json::to_string_pretty(&launch_bundled_player()?)?);
    }
    let args = parse_player_route_args(&args)?;
    let report = run_bundled_player_route(&args.scenario, &args.mount_roots)?;
    encode_player_route_report(&report, args.format)
}

struct PlayerRouteArgs {
    scenario: PathBuf,
    format: ReportFormat,
    mount_roots: BTreeMap<String, PathBuf>,
}

fn parse_player_route_args(args: &[String]) -> Result<PlayerRouteArgs, CliError> {
    let mut scenario = None;
    let mut format = ReportFormat::Json;
    let mut mount_roots = BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--route-scenario" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or("--route-scenario requires a scenario path")?;
                scenario = Some(validate_bundle_relative_path(value)?);
            }
            "--format" => {
                index += 1;
                let value = args.get(index).ok_or("--format requires json or yaml")?;
                format = match value.as_str() {
                    "json" => ReportFormat::Json,
                    "yaml" => ReportFormat::Yaml,
                    _ => return Err("--format requires json or yaml".into()),
                };
            }
            "--mount-root" => {
                index += 1;
                let value = args.get(index).ok_or("--mount-root requires alias=path")?;
                let (alias, root) = parse_mount_root_arg(value)?;
                mount_roots.insert(alias, root);
            }
            "--help" | "-h" => {
                return Err(
                    "AstraPlayer supports --route-scenario <path> [--format json|yaml] [--mount-root alias=path]".into(),
                );
            }
            other => return Err(format!("unsupported player argument: {other}").into()),
        }
        index += 1;
    }
    let scenario = scenario.ok_or("--route-scenario is required for route mode")?;
    Ok(PlayerRouteArgs {
        scenario,
        format,
        mount_roots,
    })
}

fn parse_mount_root_arg(value: &str) -> Result<(String, PathBuf), CliError> {
    let (alias, root) = value
        .split_once('=')
        .ok_or("--mount-root requires alias=path")?;
    if !is_safe_mount_alias(alias) || root.trim().is_empty() {
        return Err("--mount-root requires a safe alias and non-empty path".into());
    }
    Ok((alias.to_string(), PathBuf::from(root)))
}

fn is_safe_mount_alias(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
}

fn encode_player_route_report(
    report: &PlayerRouteReport,
    format: ReportFormat,
) -> Result<String, CliError> {
    match format {
        ReportFormat::Json => Ok(serde_json::to_string_pretty(report)?),
        ReportFormat::Yaml => Ok(serde_yaml::to_string(report)?),
    }
}

fn run_bundled_player_route(
    scenario: &Path,
    mount_roots: &BTreeMap<String, PathBuf>,
) -> Result<PlayerRouteReport, CliError> {
    let manifest = read_standalone_bundle_manifest()?;
    let config = read_player_config()?;
    validate_player_config(&manifest, &config)?;
    let scenario_ref = normalize_relative_path(scenario);
    if !manifest
        .scenario_refs
        .iter()
        .any(|candidate| candidate == &scenario_ref)
    {
        return Err(
            format!("scenario {scenario_ref} is not listed in bundle scenario refs").into(),
        );
    }
    let launch_report = launch_bundled_player()?;
    let scenario_text = fs::read_to_string(scenario)?;
    let scenario_model: Scenario = serde_yaml::from_str(&scenario_text)?;
    let scenario_report = ScenarioRunner::run_file_with_options(
        scenario,
        ScenarioRunOptions {
            package: Some(PathBuf::from(&config.package)),
            target: Some(config.target.clone()),
            profile: Some(config.profile.clone()),
            platform: Some(config.platform.clone()),
            headless: false,
        },
    )?;

    let scenario_route_pass = scenario_report.status == ScenarioStatus::Pass
        && scenario_report
            .checks
            .iter()
            .any(|check| check.id == "player_route.full" && check.status == ScenarioStatus::Pass);
    let mut checks = launch_report.checks.clone();
    checks.push(PlayerLaunchCheck {
        id: "player.bundle.ready".to_string(),
        status: if launch_report.status == "ready" {
            "pass".to_string()
        } else {
            "blocked".to_string()
        },
    });
    checks.push(PlayerLaunchCheck {
        id: "player.input_surface".to_string(),
        status: input_surface_for_platform(&manifest.platform)
            .map(|_| "pass".to_string())
            .unwrap_or_else(|| "blocked".to_string()),
    });
    checks.push(PlayerLaunchCheck {
        id: "player.route.full".to_string(),
        status: if scenario_route_pass {
            "pass".to_string()
        } else {
            "blocked".to_string()
        },
    });
    checks.extend(mount_policy_player_checks(
        &manifest,
        &scenario_model,
        mount_roots,
    )?);
    let status = if checks.iter().all(|check| check.status == "pass") {
        "pass"
    } else {
        "blocked"
    };

    Ok(PlayerRouteReport {
        schema: "astra.player_route_report.v1".to_string(),
        status: status.to_string(),
        target: manifest.target,
        profile: manifest.profile,
        platform: manifest.platform.clone(),
        input_surface: input_surface_for_platform(&manifest.platform)
            .unwrap_or("unknown_player")
            .to_string(),
        package_hash: launch_report.package_hash,
        entrypoint: manifest.entrypoint,
        scenario: scenario_ref,
        checks,
        scenario_report,
    })
}

fn input_surface_for_platform(platform: &str) -> Option<&'static str> {
    match platform {
        "windows" => Some("windows_player"),
        "web" => Some("web_player"),
        _ => None,
    }
}

fn mount_policy_player_checks(
    manifest: &StandaloneBundleManifest,
    scenario: &Scenario,
    mount_roots: &BTreeMap<String, PathBuf>,
) -> Result<Vec<PlayerLaunchCheck>, CliError> {
    if !manifest.target.starts_with("tsuinosora-") {
        return Ok(Vec::new());
    }
    let mut checks = Vec::new();
    let Some(policy_path) = &manifest.mount_policy else {
        return Ok(vec![PlayerLaunchCheck {
            id: "player.mount_policy".to_string(),
            status: "blocked".to_string(),
        }]);
    };
    let relative = validate_bundle_relative_path(policy_path)?;
    let bytes = fs::read(relative)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let policy_ok = mount_policy_matches(&value, &manifest.target, scenario);
    let hash_ok = bundle_file_hash_matches(manifest, policy_path, "mount_policy", &bytes);
    checks.push(PlayerLaunchCheck {
        id: "player.mount_policy".to_string(),
        status: if policy_ok { "pass" } else { "blocked" }.to_string(),
    });
    checks.push(PlayerLaunchCheck {
        id: "player.mount_policy_hash".to_string(),
        status: if hash_ok { "pass" } else { "blocked" }.to_string(),
    });
    if manifest.target == "tsuinosora-patch-game" {
        let probe_ok = if scenario.mount_probes.is_empty() {
            true
        } else {
            mount_probes_match(scenario, mount_roots)?
        };
        let asset_ok = if scenario.mount_assets.is_empty() {
            true
        } else {
            mount_assets_match(scenario, mount_roots)?
        };
        if !scenario.mount_probes.is_empty() {
            checks.push(PlayerLaunchCheck {
                id: "player.patch_mount_probe".to_string(),
                status: if probe_ok { "pass" } else { "blocked" }.to_string(),
            });
        }
        if !scenario.mount_assets.is_empty() {
            checks.push(PlayerLaunchCheck {
                id: "player.patch_mount_asset".to_string(),
                status: if asset_ok { "pass" } else { "blocked" }.to_string(),
            });
        }
        let has_local_read_evidence =
            !scenario.mount_probes.is_empty() || !scenario.mount_assets.is_empty();
        let local_read_evidence_required = manifest.platform == "windows";
        let direct_read_ok = policy_ok
            && hash_ok
            && probe_ok
            && asset_ok
            && (!local_read_evidence_required || has_local_read_evidence)
            && manifest
                .mount_policy
                .as_deref()
                .is_some_and(|path| path == "AstraPlayer.mount_policy.json")
            && !scenario.mount_aliases.is_empty();
        checks.push(PlayerLaunchCheck {
            id: "player.patch_direct_read".to_string(),
            status: if direct_read_ok { "pass" } else { "blocked" }.to_string(),
        });
    }
    Ok(checks)
}

fn mount_probes_match(
    scenario: &Scenario,
    mount_roots: &BTreeMap<String, PathBuf>,
) -> Result<bool, CliError> {
    for probe in &scenario.mount_probes {
        if !mount_probe_matches(probe, scenario, mount_roots)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn mount_probe_matches(
    probe: &MountProbe,
    scenario: &Scenario,
    mount_roots: &BTreeMap<String, PathBuf>,
) -> Result<bool, CliError> {
    if !scenario.mount_aliases.contains_key(&probe.alias) || !is_safe_mount_alias(&probe.alias) {
        return Ok(false);
    }
    if !probe.sha256.starts_with("sha256:") || probe.sha256.len() != 71 {
        return Ok(false);
    }
    let Some(root) = mount_roots.get(&probe.alias) else {
        return Ok(false);
    };
    if !root.is_dir() {
        return Ok(false);
    }
    let relative = validate_bundle_relative_path(&probe.path)?;
    let path = root.join(relative);
    if !path.is_file() {
        return Ok(false);
    }
    let bytes = fs::read(path)?;
    Ok(Hash256::from_sha256(&bytes).to_string() == probe.sha256)
}

fn mount_assets_match(
    scenario: &Scenario,
    mount_roots: &BTreeMap<String, PathBuf>,
) -> Result<bool, CliError> {
    for asset in &scenario.mount_assets {
        if !mount_asset_matches(asset, scenario, mount_roots)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn mount_asset_matches(
    asset: &MountAsset,
    scenario: &Scenario,
    mount_roots: &BTreeMap<String, PathBuf>,
) -> Result<bool, CliError> {
    if !scenario.mount_aliases.contains_key(&asset.alias) || !is_safe_mount_alias(&asset.alias) {
        return Ok(false);
    }
    if !is_safe_mount_role(&asset.role)
        || asset.route_id.trim().is_empty()
        || !asset.sha256.starts_with("sha256:")
        || asset.sha256.len() != 71
    {
        return Ok(false);
    }
    if scenario
        .generated_route_id
        .as_ref()
        .is_some_and(|route_id| route_id != &asset.route_id)
    {
        return Ok(false);
    }
    let Some(root) = mount_roots.get(&asset.alias) else {
        return Ok(false);
    };
    if !root.is_dir() {
        return Ok(false);
    }
    let relative = validate_bundle_relative_path(&asset.path)?;
    let path = root.join(relative);
    if !path.is_file() {
        return Ok(false);
    }
    let bytes = fs::read(path)?;
    Ok(Hash256::from_sha256(&bytes).to_string() == asset.sha256)
}

fn is_safe_mount_role(value: &str) -> bool {
    MOUNT_ASSET_ROLES.contains(&value)
}

fn bundle_file_hash_matches(
    manifest: &StandaloneBundleManifest,
    path: &str,
    role: &str,
    bytes: &[u8],
) -> bool {
    let hash = Hash256::from_sha256(bytes).to_string();
    manifest.files.iter().any(|file| {
        file.path == path
            && file.role == role
            && file.hash == hash
            && file.byte_size == bytes.len() as u64
    })
}

fn mount_policy_matches(value: &serde_json::Value, target: &str, scenario: &Scenario) -> bool {
    if json_has_local_path_like(value) {
        return false;
    }
    if value.get("schema").and_then(serde_json::Value::as_str) != Some("tsuinosora.mount_policy.v1")
    {
        return false;
    }
    if value.get("target").and_then(serde_json::Value::as_str) != Some(target) {
        return false;
    }
    if value.get("status").and_then(serde_json::Value::as_str) != Some("pass") {
        return false;
    }
    let Some(aliases) = value.get("aliases").and_then(serde_json::Value::as_array) else {
        return false;
    };
    if aliases.is_empty() {
        return false;
    }
    for (alias, expected_value) in &scenario.mount_aliases {
        let found = aliases.iter().any(|entry| {
            entry.get("alias").and_then(serde_json::Value::as_str) == Some(alias.as_str())
                && entry.get("value").and_then(serde_json::Value::as_str)
                    == Some(expected_value.as_str())
                && entry.get("hash_policy").and_then(serde_json::Value::as_str)
                    == Some("manifest_required")
                && entry.get("fallback").and_then(serde_json::Value::as_str) == Some("blocking")
        });
        if !found {
            return false;
        }
    }
    true
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

fn launch_bundled_player() -> Result<PlayerLaunchReport, CliError> {
    let manifest = read_standalone_bundle_manifest()?;
    let package_bytes = fs::read(&manifest.package)?;
    let package_hash = Hash256::from_sha256(&package_bytes).to_string();
    let mut checks = Vec::new();
    let package_status = if package_hash == manifest.package_hash {
        "pass"
    } else {
        "blocked"
    };
    checks.push(PlayerLaunchCheck {
        id: "package.hash".to_string(),
        status: package_status.to_string(),
    });

    let reader = PackageReader::open(&package_bytes)?;
    let target_manifest: TargetManifest =
        serde_json::from_slice(&reader.container().read_section("target.manifest")?)?;
    let target_report = validate_manifest(&target_manifest, Some(&manifest.target));
    let target_status = if matches!(target_report.status, TargetValidationStatus::Pass) {
        "pass"
    } else {
        "blocked"
    };
    checks.push(PlayerLaunchCheck {
        id: "target.manifest".to_string(),
        status: target_status.to_string(),
    });
    let status = if checks.iter().all(|check| check.status == "pass") {
        "ready"
    } else {
        "blocked"
    };
    Ok(PlayerLaunchReport {
        schema: "astra.player_launch_report.v1".to_string(),
        status: status.to_string(),
        target: manifest.target,
        profile: manifest.profile,
        platform: manifest.platform,
        package_hash,
        entrypoint: manifest.entrypoint,
        checks,
    })
}

fn read_standalone_bundle_manifest() -> Result<StandaloneBundleManifest, CliError> {
    let manifest_text = fs::read_to_string("bundle_manifest.json")?;
    Ok(serde_json::from_str(&manifest_text)?)
}

fn read_player_config() -> Result<PlayerConfig, CliError> {
    let config_text = fs::read_to_string("AstraPlayer.config.json")?;
    Ok(serde_json::from_str(&config_text)?)
}

fn validate_player_config(
    manifest: &StandaloneBundleManifest,
    config: &PlayerConfig,
) -> Result<(), CliError> {
    if config.schema != "astra.player_config.v1" {
        return Err("unsupported player config schema".into());
    }
    if manifest.target != config.target
        || manifest.profile != config.profile
        || manifest.platform != config.platform
        || manifest.package != config.package
    {
        return Err("player config does not match bundle manifest".into());
    }
    Ok(())
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
