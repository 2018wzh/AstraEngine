use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use astra_asset::{AssetSidecar, VfsUri};
use astra_cook::{
    CookBatchExecutor, CookBatchLimits, CookBatchRequest, CookCancellationToken, CookNode,
    CookProcessorRegistry, CookRequest, DefaultCookProcessor, FileCookCache,
};
use astra_core::Hash256;
use astra_media::{
    CpuRendererProvider, FontPackageEntry, FontPackageManifest, RenderTargetFormat,
    Renderer2DProvider, RendererCreateRequest, UnicodeRange, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_observability::{
    init_host, ConsoleFormat, CrashReportingMode, HostObservabilityConfig, ObservabilityGuard,
};
use astra_package::{
    CookSummaryManifest, MigrationPolicy, PackageBuildRequest, PackageBuilder, PackageManifest,
    PackageReader, ScenarioReference, ScenarioRefsManifest, SectionCodec, SectionPayload,
    CURRENT_CONTAINER_VERSION,
};
use astra_platform::{
    migrate_host_profile_json, validate_host_profile, PlatformCapabilityReport,
    PlatformHostConformanceReport, PlatformHostProfile, PlatformId,
};
use astra_player_core::{PlayerAutomationReport, PlayerHostCommand, PlayerHostResourceId};
use astra_player_vn::NativeVnHostCommandSource;
use astra_release::{
    HeadlessFormalEvidence, PackageValidateRequest, ReleaseReport, ReleaseValidator,
};
use astra_target::{
    validate_manifest, TargetKind, TargetManifest, TargetValidationReport, TargetValidationStatus,
};
use astra_test::ScenarioReport;
use astra_vn::{
    compile_astra_project, format_astra_source, load_player_locale_config,
    load_ui_component_artifact, package_sections_for_project_with_components, AstraSource,
    CompileAstraProjectOptions, FormatOptions, PlayerLocaleConfig, VnUiComponentArtifactInput,
    VnUiComponentBundleManifest, VnUiComponentTarget, PLAYER_LOCALE_CONFIG_SCHEMA,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use clap::{Parser, Subcommand, ValueEnum};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use tracing::info;

type CliError = Box<dyn std::error::Error + Send + Sync>;
const WEB_PLAYER_LOADER: &[u8] =
    include_bytes!("../../astra-player-web/web/astra-player-loader.js");
const WEB_AUDIO_WORKLET: &[u8] =
    include_bytes!("../../astra-player-web/web/astra-audio-worklet.js");
const WEB_UI_COMPONENT_HOST: &[u8] =
    include_bytes!("../../astra-player-web/web/astra-ui-component-host.js");

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
    Ui {
        #[command(subcommand)]
        command: UiCommand,
    },
}

#[derive(Subcommand)]
enum UiCommand {
    Check {
        project: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        target: String,
    },
    Preview {
        project: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        target: String,
        #[arg(long, default_value = "en")]
        locale: String,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
        #[arg(long)]
        out: PathBuf,
    },
    Snapshot {
        project: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        target: String,
        #[arg(long, default_value = "en")]
        locale: String,
        #[arg(long, default_value_t = 1280)]
        width: u32,
        #[arg(long, default_value_t = 720)]
        height: u32,
        #[arg(long)]
        out: PathBuf,
    },
    Matrix {
        project: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        target: String,
        #[arg(long, value_delimiter = ',', default_value = "en,ja,zh-Hans")]
        locales: Vec<String>,
        #[arg(long, value_delimiter = ',', default_value = "1280x720,1920x1080")]
        sizes: Vec<String>,
        #[arg(long, value_delimiter = ',', default_value = "1.0,1.5,2.0")]
        scales: Vec<f32>,
        #[arg(long)]
        out: PathBuf,
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
        linux_player: Option<PathBuf>,
        #[arg(long)]
        macos_player: Option<PathBuf>,
        #[arg(long)]
        crash_reporter: Option<PathBuf>,
        #[arg(long)]
        ui_component_host: Option<PathBuf>,
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
        #[arg(long)]
        headless_run_report: Option<PathBuf>,
        #[arg(long)]
        headless_review_bundle: Option<PathBuf>,
        #[arg(long)]
        headless_review: Option<PathBuf>,
        #[arg(long)]
        headless_preflight_link: Option<PathBuf>,
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
            let cancellation = CookCancellationToken::default();
            let signal_token = cancellation.clone();
            ctrlc::set_handler(move || signal_token.cancel())?;
            let manifest = cook_project(project, &profile, target.as_deref(), out, &cancellation)?;
            println!("{}", serde_yaml::to_string(&manifest)?);
        }
        Command::Ui { command } => run_ui_command(command)?,
        Command::Script { command } => match command {
            ScriptCommand::Check { sources } => {
                let sources = read_astra_sources(&sources)?;
                let compiled =
                    compile_astra_project(sources, CompileAstraProjectOptions::default())?;
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
                            Ok(AstraSource::story(
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
                                        AstraSource::story(source.path.clone(), text.clone())
                                    },
                                )
                        })
                        .collect::<Vec<_>>();
                    let before = compile_astra_project(original, Default::default())?;
                    let after = compile_astra_project(formatted, Default::default())?;
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
                atomic_replace(&out, &package.into_bytes())?;
            }
            PackageCommand::Bundle {
                package,
                out,
                target,
                profile,
                platform,
                windows_player,
                linux_player,
                macos_player,
                crash_reporter,
                ui_component_host,
                web_player_wasm,
                web_player_glue,
                format,
            } => {
                let artifacts = BundleArtifactInputs {
                    windows_player,
                    linux_player,
                    macos_player,
                    crash_reporter,
                    ui_component_host,
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
                headless_run_report,
                headless_review_bundle,
                headless_review,
                headless_preflight_link,
                format,
            } => {
                let bytes = fs::read(package)?;
                let platform_report = read_platform_report(platform_report.as_deref())?;
                let platform_conformance_report =
                    read_platform_conformance_report(platform_conformance_report.as_deref())?;
                let player_report =
                    read_player_automation_report(player_automation_report.as_deref())?;
                let headless = read_headless_formal_evidence(
                    headless_run_report.as_deref(),
                    headless_review_bundle.as_deref(),
                    headless_review.as_deref(),
                    headless_preflight_link.as_deref(),
                    player_automation_report.as_deref(),
                )?;
                let require_platform_report = release_profile_requires_platform_report(&profile);
                let release_report = ReleaseValidator.validate_package_with_headless_preflight(
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
                    headless,
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
            command: TestCommand::Run { headless, .. },
        } => {
            if !headless {
                return Err("ASTRA_TEST_RUN_RETIRED: YAML product scenarios are retired; use Rust Headless tests for semantic coverage or astra-headless with astra.user_input_sequence.v1 for product automation".into());
            }
            return Err("ASTRA_TEST_HEADLESS_MIGRATED: astra test run --headless no longer aliases the Headless backend; invoke astra-headless run with JSONL physical input".into());
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

#[derive(Serialize)]
struct UiSnapshotReport {
    schema: String,
    profile: String,
    target: String,
    locale: String,
    width: u32,
    height: u32,
    scale_factor: f32,
    frame_hash: String,
    semantic_hash: String,
    scene_hash: String,
    performance: astra_ui_core::UiPerformanceReport,
}

struct UiSnapshotOutput {
    rgba8: Vec<u8>,
    semantics: astra_ui_core::UiSemanticSnapshot,
    commands: Vec<astra_media::SceneCommand>,
    report: UiSnapshotReport,
}

#[derive(serde::Serialize)]
struct UiSceneEvidence {
    schema: &'static str,
    scene_hash: String,
    command_count: u32,
    command_counts: BTreeMap<&'static str, u32>,
}

fn run_ui_command(command: UiCommand) -> Result<(), CliError> {
    match command {
        UiCommand::Check {
            project,
            profile,
            target,
        } => {
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            println!(
                "{}",
                serde_json::json!({
                    "schema": "astra.ui_check_report.v1",
                    "status": "pass",
                    "profile": profile,
                    "target": target,
                    "package_hash": Hash256::from_sha256(&package).to_string(),
                })
            );
        }
        UiCommand::Preview {
            project,
            profile,
            target,
            locale,
            width,
            height,
            out,
        } => {
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            let snapshot =
                render_ui_snapshot(&package, &profile, &target, &locale, width, height, 1.0)?;
            write_png(&out, width, height, &snapshot.rgba8)?;
            println!("{}", serde_json::to_string(&snapshot.report)?);
        }
        UiCommand::Snapshot {
            project,
            profile,
            target,
            locale,
            width,
            height,
            out,
        } => {
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            let snapshot =
                render_ui_snapshot(&package, &profile, &target, &locale, width, height, 1.0)?;
            write_ui_snapshot(&out, &snapshot)?;
            println!("{}", serde_json::to_string(&snapshot.report)?);
        }
        UiCommand::Matrix {
            project,
            profile,
            target,
            locales,
            sizes,
            scales,
            out,
        } => {
            if locales.is_empty() || sizes.is_empty() || scales.is_empty() {
                return Err("ASTRA_UI_MATRIX_EMPTY: locales, sizes and scales are required".into());
            }
            let package = compile_ui_preview_package(&project, &profile, &target)?;
            fs::create_dir_all(&out)?;
            let mut reports = Vec::new();
            for locale in locales {
                for size in &sizes {
                    let (width, height) = parse_ui_matrix_size(size)?;
                    for scale in &scales {
                        if !scale.is_finite() || !(0.5..=4.0).contains(scale) {
                            return Err(
                                "ASTRA_UI_MATRIX_SCALE: scale must be within 0.5..=4.0".into()
                            );
                        }
                        let snapshot = render_ui_snapshot(
                            &package, &profile, &target, &locale, width, height, *scale,
                        )?;
                        let id = format!(
                            "{locale}-{width}x{height}-{}",
                            scale.to_string().replace('.', "_")
                        );
                        write_ui_snapshot(&out.join(&id), &snapshot)?;
                        reports.push(snapshot.report);
                    }
                }
            }
            let matrix = serde_json::json!({
                "schema": "astra.ui_matrix_report.v1",
                "profile": profile,
                "target": target,
                "package_hash": Hash256::from_sha256(&package).to_string(),
                "entries": reports,
            });
            atomic_replace(
                &out.join("matrix.report.json"),
                &serde_json::to_vec_pretty(&matrix)?,
            )?;
            println!("{}", serde_json::to_string(&matrix)?);
        }
    }
    Ok(())
}

fn compile_ui_preview_package(
    project: &Path,
    profile: &str,
    target: &str,
) -> Result<Vec<u8>, CliError> {
    let temp = tempfile::tempdir()?;
    let cooked = temp.path().join("cooked");
    let manifest = cook_project(
        project.to_path_buf(),
        profile,
        Some(target),
        cooked.clone(),
        &CookCancellationToken::default(),
    )?;
    Ok(build_package_from_cooked(&cooked, manifest, Some(target))?.into_bytes())
}

fn render_ui_snapshot(
    package_bytes: &[u8],
    profile: &str,
    target: &str,
    locale: &str,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> Result<UiSnapshotOutput, CliError> {
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("ASTRA_UI_PREVIEW_VIEWPORT: dimensions must be within 1..=8192".into());
    }
    let package = PackageReader::open(package_bytes)?;
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        astra_vn::VnRunConfig {
            profile: profile.to_string(),
            locale: locale.to_string(),
        },
        width,
        height,
        PlayerHostResourceId(1),
    )?;
    let mut batches = vec![source.launch()?];
    if scale_factor != 1.0 {
        batches.push(
            source.dispatch_ui_event(astra_ui_core::UiInputEventKind::Resize {
                viewport: astra_ui_core::UiViewport {
                    physical_width: width,
                    physical_height: height,
                    scale_factor,
                    safe_area_points: astra_ui_core::UiInsets {
                        left: 0.0,
                        top: 0.0,
                        right: 0.0,
                        bottom: 0.0,
                    },
                    font_scale: 1.0,
                },
            })?,
        );
    }
    batches.reverse();
    let mut renderer = CpuRendererProvider.create(RendererCreateRequest {
        width,
        height,
        format: RenderTargetFormat::Rgba8Srgb,
        profile: "ui-preview".into(),
    })?;
    let mut selected = None;
    for attempt in 0..64_u64 {
        let batch = if let Some(batch) = batches.pop() {
            batch
        } else if let Some(wait) = source.pending_wait().cloned() {
            match wait.kind {
                astra_vn::VnWaitKind::Dialogue
                | astra_vn::VnWaitKind::Choice
                | astra_vn::VnWaitKind::SystemPage => {
                    source.dispatch_ui_event(astra_ui_core::UiInputEventKind::Keyboard {
                        logical_key: "Enter".into(),
                        physical_key: "Enter".into(),
                        state: astra_ui_core::UiButtonState::Pressed,
                        repeat: false,
                        modifiers: 0,
                    })?
                }
                _ => source.complete_wait(wait.fence)?,
            }
        } else {
            source.dispatch_ui_event(astra_ui_core::UiInputEventKind::Keyboard {
                logical_key: "Enter".into(),
                physical_key: "Enter".into(),
                state: astra_ui_core::UiButtonState::Pressed,
                repeat: false,
                modifiers: 0,
            })?
        };
        for command in batch.commands {
            let PlayerHostCommand::PresentScene {
                clear_rgba,
                commands,
                semantics,
                ..
            } = command
            else {
                continue;
            };
            let mut scene = Vec::with_capacity(commands.len() + 1);
            scene.push(astra_media::SceneCommand::Clear { rgba: clear_rgba });
            scene.extend(commands.iter().cloned());
            let frame = renderer.capture_frame(&scene)?;
            if let Some(semantics) = semantics {
                selected = Some((frame, commands, semantics));
                break;
            }
        }
        if selected.is_some() {
            break;
        }
        if attempt == 63 {
            return Err(
                "ASTRA_UI_PREVIEW_SEMANTICS: no UI surface became active within 64 physical inputs"
                    .into(),
            );
        }
    }
    let (mut frame, mut commands, mut semantics) = selected
        .ok_or("ASTRA_UI_PREVIEW_PRESENT_SCENE: launch did not produce a semantic Scene2D frame")?;
    for tick in 1..=30_u64 {
        let batch = source.dispatch_ui_event(astra_ui_core::UiInputEventKind::FixedTime {
            time_ns: tick * 16_666_667,
        })?;
        for command in batch.commands {
            let PlayerHostCommand::PresentScene {
                clear_rgba,
                commands: next_commands,
                semantics: next_semantics,
                ..
            } = command
            else {
                continue;
            };
            let mut scene = Vec::with_capacity(next_commands.len() + 1);
            scene.push(astra_media::SceneCommand::Clear { rgba: clear_rgba });
            scene.extend(next_commands.iter().cloned());
            frame = renderer.capture_frame(&scene)?;
            commands = next_commands;
            if let Some(next_semantics) = next_semantics {
                semantics = next_semantics;
            }
        }
    }
    let scene_hash = Hash256::from_sha256(&postcard::to_allocvec(&commands)?);
    Ok(UiSnapshotOutput {
        rgba8: frame.bytes,
        semantics: semantics.clone(),
        commands,
        report: UiSnapshotReport {
            schema: "astra.ui_snapshot_report.v1".into(),
            profile: profile.into(),
            target: target.into(),
            locale: locale.into(),
            width,
            height,
            scale_factor,
            frame_hash: frame.hash.to_string(),
            semantic_hash: semantics.hash.to_string(),
            scene_hash: scene_hash.to_string(),
            performance: source.ui_performance_report(),
        },
    })
}

fn write_ui_snapshot(out: &Path, snapshot: &UiSnapshotOutput) -> Result<(), CliError> {
    fs::create_dir_all(out)?;
    write_png(
        &out.join("frame.png"),
        snapshot.report.width,
        snapshot.report.height,
        &snapshot.rgba8,
    )?;
    atomic_replace(
        &out.join("semantic.json"),
        &serde_json::to_vec_pretty(&snapshot.semantics)?,
    )?;
    atomic_replace(
        &out.join("scene.json"),
        &serde_json::to_vec_pretty(&ui_scene_evidence(
            &snapshot.commands,
            &snapshot.report.scene_hash,
        )?)?,
    )?;
    atomic_replace(
        &out.join("report.json"),
        &serde_json::to_vec_pretty(&snapshot.report)?,
    )?;
    Ok(())
}

fn ui_scene_evidence(
    commands: &[astra_media::SceneCommand],
    scene_hash: &str,
) -> Result<UiSceneEvidence, CliError> {
    let command_count = u32::try_from(commands.len())
        .map_err(|_| "ASTRA_UI_SNAPSHOT_COMMAND_LIMIT: scene command count exceeds u32")?;
    let mut command_counts = BTreeMap::new();
    for command in commands {
        let count = command_counts
            .entry(scene_command_kind(command))
            .or_insert(0_u32);
        *count = count
            .checked_add(1)
            .ok_or("ASTRA_UI_SNAPSHOT_COMMAND_LIMIT: command kind count overflow")?;
    }
    Ok(UiSceneEvidence {
        schema: "astra.ui_scene_evidence.v1",
        scene_hash: scene_hash.to_string(),
        command_count,
        command_counts,
    })
}

fn scene_command_kind(command: &astra_media::SceneCommand) -> &'static str {
    use astra_media::SceneCommand;
    match command {
        SceneCommand::UploadTexture { .. } => "upload_texture",
        SceneCommand::UploadGlyph { .. } => "upload_glyph",
        SceneCommand::ReleaseResource { .. } => "release_resource",
        SceneCommand::Sprite { .. } => "sprite",
        SceneCommand::GlyphRun { .. } => "glyph_run",
        SceneCommand::Mesh2D { .. } => "mesh2d",
        SceneCommand::Clear { .. } => "clear",
        SceneCommand::Rect { .. } => "rect",
        SceneCommand::Texture { .. } => "texture",
        SceneCommand::VideoFrame { .. } => "video_frame",
        SceneCommand::Glyph { .. } => "glyph",
        SceneCommand::PushClip { .. } => "push_clip",
        SceneCommand::PopClip => "pop_clip",
        SceneCommand::PushTransform { .. } => "push_transform",
        SceneCommand::PopTransform => "pop_transform",
        SceneCommand::SetCamera { .. } => "set_camera",
        SceneCommand::PushOpacity { .. } => "push_opacity",
        SceneCommand::PopOpacity => "pop_opacity",
        SceneCommand::FilterGraph { .. } => "filter_graph",
    }
}

fn write_png(path: &Path, width: u32, height: u32, rgba8: &[u8]) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    image::save_buffer_with_format(
        path,
        rgba8,
        width,
        height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    Ok(())
}

fn parse_ui_matrix_size(value: &str) -> Result<(u32, u32), CliError> {
    let (width, height) = value
        .split_once('x')
        .ok_or("ASTRA_UI_MATRIX_SIZE: size must use WIDTHxHEIGHT")?;
    let width = width.parse::<u32>()?;
    let height = height.parse::<u32>()?;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("ASTRA_UI_MATRIX_SIZE: dimensions must be within 1..=8192".into());
    }
    Ok((width, height))
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
            Ok(AstraSource::story(
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

fn read_headless_formal_evidence(
    run: Option<&Path>,
    bundle: Option<&Path>,
    review: Option<&Path>,
    link: Option<&Path>,
    platform_run: Option<&Path>,
) -> Result<Option<HeadlessFormalEvidence>, CliError> {
    if [
        run.is_some(),
        bundle.is_some(),
        review.is_some(),
        link.is_some(),
    ]
    .iter()
    .all(|value| !value)
    {
        return Ok(None);
    }
    let (run, bundle, review, link, platform_run) = match (run, bundle, review, link, platform_run) {
        (Some(run), Some(bundle), Some(review), Some(link), Some(platform_run)) => {
            (run, bundle, review, link, platform_run)
        }
        _ => {
            return Err("ASTRA_RELEASE_HEADLESS_EVIDENCE_INCOMPLETE: run, review bundle, review, preflight link, and player automation report must be supplied together".into())
        }
    };
    let run_bytes = fs::read(run)?;
    let bundle_bytes = fs::read(bundle)?;
    let review_bytes = fs::read(review)?;
    let link_bytes = fs::read(link)?;
    let platform_bytes = fs::read(platform_run)?;
    Ok(Some(HeadlessFormalEvidence {
        run_report: serde_json::from_slice(&run_bytes)?,
        run_report_hash: Hash256::from_sha256(&run_bytes).to_string(),
        review_bundle: serde_json::from_slice(&bundle_bytes)?,
        review_bundle_hash: Hash256::from_sha256(&bundle_bytes).to_string(),
        review: serde_json::from_slice(&review_bytes)?,
        review_hash: Hash256::from_sha256(&review_bytes).to_string(),
        preflight_link: serde_json::from_slice(&link_bytes)?,
        platform_run_report_hash: Hash256::from_sha256(&platform_bytes).to_string(),
    }))
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
    asset_cook: CookSummaryManifest,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font: Option<CookedFontMetadata>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scenario_path: Option<String>,
}

#[derive(Debug)]
struct PreparedAssetCook {
    asset_id: String,
    source_path: PathBuf,
    source_hash: Hash256,
    source_byte_size: u64,
    asset_role: String,
    asset_type: String,
    font: Option<CookedFontMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
struct CookedFontMetadata {
    family: String,
    face_index: u32,
    license_id: String,
    subset: Option<String>,
    coverage: Vec<UnicodeRange>,
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
    linux_player: Option<PathBuf>,
    macos_player: Option<PathBuf>,
    crash_reporter: Option<PathBuf>,
    ui_component_host: Option<PathBuf>,
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
    cancellation: &CookCancellationToken,
) -> Result<CookManifest, CliError> {
    cancellation.check_cancelled()?;
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".astra-cook-stage-")
        .tempdir_in(parent)?;
    let manifest = cook_project_into(
        project,
        profile,
        target,
        staging.path().to_path_buf(),
        cancellation,
    )?;
    cancellation.check_cancelled()?;
    atomic_replace_directory(staging, &out)?;
    Ok(manifest)
}

fn atomic_replace_directory(
    staging: tempfile::TempDir,
    destination: &Path,
) -> Result<(), CliError> {
    if destination.exists() && !destination.is_dir() {
        return Err("ASTRA_COOK_COMMIT_PATH: cook output exists and is not a directory".into());
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let staging_path = staging.keep();
    if !destination.exists() {
        if let Err(error) =
            rename_directory_with_transient_retry(&staging_path, destination, "commit")
        {
            let _ = fs::remove_dir_all(&staging_path);
            return Err(format!("ASTRA_COOK_COMMIT_SWAP: {error}").into());
        }
        return Ok(());
    }
    let staging_name = staging_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("ASTRA_COOK_COMMIT_PATH: staging directory name is invalid")?;
    let backup = parent.join(format!("{staging_name}.backup"));
    rename_directory_with_transient_retry(destination, &backup, "backup")
        .map_err(|error| format!("ASTRA_COOK_COMMIT_BACKUP: {error}"))?;
    if let Err(error) = rename_directory_with_transient_retry(&staging_path, destination, "commit")
    {
        let rollback = rename_directory_with_transient_retry(&backup, destination, "rollback");
        let _ = fs::remove_dir_all(&staging_path);
        return match rollback {
            Ok(()) => Err(format!("ASTRA_COOK_COMMIT_SWAP: {error}").into()),
            Err(rollback_error) => Err(format!(
                "ASTRA_COOK_COMMIT_ROLLBACK: swap failed ({error}) and rollback failed ({rollback_error})"
            )
            .into()),
        };
    }
    if let Err(error) = fs::remove_dir_all(&backup) {
        let failed_output = parent.join(format!("{staging_name}.rollback"));
        let rollback =
            fs::rename(destination, &failed_output).and_then(|_| fs::rename(&backup, destination));
        let _ = fs::remove_dir_all(&failed_output);
        return match rollback {
            Ok(()) => Err(format!(
                "ASTRA_COOK_COMMIT_CLEANUP: {error}; previous output restored"
            )
            .into()),
            Err(rollback_error) => Err(format!(
                "ASTRA_COOK_COMMIT_ROLLBACK: cleanup failed ({error}) and rollback failed ({rollback_error})"
            )
            .into()),
        };
    }
    Ok(())
}

fn rename_directory_with_transient_retry(
    source: &Path,
    destination: &Path,
    operation: &'static str,
) -> std::io::Result<()> {
    const RETRY_DELAYS_MS: [u64; 5] = [10, 20, 40, 80, 160];
    for (index, delay_ms) in RETRY_DELAYS_MS.into_iter().enumerate() {
        match fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::WouldBlock
                ) =>
            {
                tracing::warn!(
                    event = "filesystem.atomic_directory_rename.retry",
                    operation,
                    attempt = index + 1,
                    error_kind = ?error.kind(),
                    "atomic directory rename encountered a transient filesystem lock"
                );
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            }
            Err(error) => return Err(error),
        }
    }
    fs::rename(source, destination)
}

fn cook_project_into(
    project: PathBuf,
    profile: &str,
    target: Option<&str>,
    out: PathBuf,
    cancellation: &CookCancellationToken,
) -> Result<CookManifest, CliError> {
    cancellation.check_cancelled()?;
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
    cancellation.check_cancelled()?;
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
        font: None,
        scenario_path: None,
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
            font: None,
            scenario_path: None,
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
            font: None,
            scenario_path: None,
        });
    }
    let mut asset_cook = CookSummaryManifest::empty();
    if project_uses_nativevn(&project_yaml) {
        let (nativevn_artifacts, summary) = cook_nativevn_sections(
            &project_yaml,
            project_dir,
            &out,
            profile,
            target,
            cancellation,
        )?;
        artifacts.extend(nativevn_artifacts);
        asset_cook = summary;
    }
    artifacts.extend(cook_project_package_sections(
        &project_yaml,
        project_dir,
        &out,
        profile,
        target,
        &artifacts,
    )?);
    if project_uses_nativevn(&project_yaml) {
        artifacts.push(cook_player_locale_config(&project_yaml, &artifacts, &out)?);
    }
    let scenario_refs = scenario_refs_from_project(&project_yaml);
    artifacts.extend(cook_scenario_ref_sections(
        &scenario_refs,
        project_dir,
        &out,
        &artifacts,
    )?);
    if project_uses_nativevn(&project_yaml) {
        artifacts.push(cook_font_manifest_section(
            &artifacts, target, profile, &out,
        )?);
    }
    let manifest = CookManifest {
        schema: "astra.cook_manifest.v2".to_string(),
        package_id,
        profile: profile.to_string(),
        target: target.to_string(),
        project_hash,
        target_manifest,
        scenario_refs,
        artifacts,
        asset_cook,
    };
    cancellation.check_cancelled()?;
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
    let manifest: CookManifest = serde_yaml::from_str(&text)?;
    if manifest.schema != "astra.cook_manifest.v2"
        || manifest.asset_cook.schema != "astra.cook_batch_summary.v1"
    {
        return Err("ASTRA_COOK_MANIFEST_VERSION: unsupported cook manifest schema".into());
    }
    let asset_count = manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.asset_id.is_some())
        .count() as u64;
    if asset_count != manifest.asset_cook.artifact_count
        || manifest.asset_cook.cache_hit_count + manifest.asset_cook.cooked_count
            != manifest.asset_cook.artifact_count
        || manifest.asset_cook.max_concurrency == 0
    {
        return Err("ASTRA_COOK_MANIFEST_IDENTITY: asset cook summary is inconsistent".into());
    }
    Ok(manifest)
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
    cancellation: &CookCancellationToken,
) -> Result<(Vec<CookedArtifactRef>, CookSummaryManifest), CliError> {
    cancellation.check_cancelled()?;
    let source_paths = nativevn_source_paths(project, project_dir, "sources", "Scripts")?;
    if source_paths.is_empty() {
        return Err("nativevn project must declare at least one .astra source".into());
    }
    let mut sources = Vec::with_capacity(source_paths.len());
    for source in source_paths {
        let source_text = fs::read_to_string(project_dir.join(&source))?;
        sources.push(AstraSource::story(
            normalize_relative_path(&source),
            source_text,
        ));
    }
    let ui_source_paths = nativevn_source_paths(project, project_dir, "ui_sources", "UI")?;
    if ui_source_paths.is_empty() {
        return Err("nativevn project must declare at least one UI .astra source".into());
    }
    for source in ui_source_paths {
        let source_text = fs::read_to_string(project_dir.join(&source))?;
        sources.push(AstraSource::ui(
            normalize_relative_path(&source),
            source_text,
        ));
    }
    let theme_paths = nativevn_theme_paths(project, project_dir)?;
    if theme_paths.is_empty() {
        return Err("nativevn project must declare at least one UI theme manifest".into());
    }
    let mut compile_options = CompileAstraProjectOptions::default();
    for theme_path in theme_paths {
        let text = fs::read_to_string(project_dir.join(&theme_path))?;
        #[derive(serde::Deserialize)]
        struct UiThemeSource {
            schema: String,
            id: String,
            #[serde(default)]
            parent: Option<String>,
            tokens: std::collections::BTreeMap<String, astra_ui_core::UiThemeValue>,
            #[serde(default)]
            high_contrast_tokens: std::collections::BTreeMap<String, astra_ui_core::UiThemeValue>,
        }
        let source: UiThemeSource = match theme_path.extension().and_then(std::ffi::OsStr::to_str) {
            Some("json") => serde_json::from_str(&text)?,
            Some("yaml" | "yml") => serde_yaml::from_str(&text)?,
            _ => {
                return Err(format!(
                    "ASTRA_UI_THEME_FORMAT: {} must use .json, .yaml, or .yml",
                    normalize_relative_path(&theme_path)
                )
                .into())
            }
        };
        let mut theme = astra_ui_core::UiThemeManifest {
            schema: source.schema,
            id: source.id,
            parent: source.parent,
            tokens: source.tokens,
            high_contrast_tokens: source.high_contrast_tokens,
            content_hash: astra_core::Hash256::from_sha256(&[]),
        };
        theme.content_hash = theme.compute_hash()?;
        compile_options = compile_options.with_ui_theme(theme);
    }
    let controller_paths = nativevn_controller_paths(project, project_dir)?;
    if controller_paths.is_empty() {
        return Err("nativevn project must declare at least one UI controller source".into());
    }
    let mut controller_host = astra_vn::LuauUiControllerHost::with_default_budget()?;
    for controller_path in controller_paths {
        let source = fs::read_to_string(project_dir.join(&controller_path))?;
        controller_host.register_source(source)?;
    }
    for manifest in controller_host.manifests() {
        let source = controller_host
            .source(&manifest.id)
            .ok_or("ASTRA_UI_CONTROLLER_SOURCE_IDENTITY: validated controller source is missing")?;
        compile_options =
            compile_options.with_ui_controller_source(manifest.id.clone(), source.to_string());
    }
    let compiled = compile_astra_project(sources, compile_options)?;
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

    let component_artifacts = nativevn_ui_component_artifacts(project, project_dir)?;
    let sections = package_sections_for_project_with_components(
        &compiled,
        &profiles,
        target,
        &component_artifacts,
    )?;
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
            font: None,
            scenario_path: None,
        });
    }
    let (asset_artifacts, asset_cook) =
        cook_nativevn_asset_sections(project, project_dir, out, profile, cancellation)?;
    artifacts.extend(asset_artifacts);
    Ok((artifacts, asset_cook))
}

fn cook_nativevn_asset_sections(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    out: &std::path::Path,
    profile: &str,
    cancellation: &CookCancellationToken,
) -> Result<(Vec<CookedArtifactRef>, CookSummaryManifest), CliError> {
    cancellation.check_cancelled()?;
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

    let mut nodes = Vec::with_capacity(sidecar_paths.len());
    let mut prepared = BTreeMap::new();
    let mut processor_ids = std::collections::BTreeSet::new();
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

        let source_byte_size = source_bytes.len() as u64;
        let asset_id = sidecar.id.to_string();
        if prepared
            .insert(
                asset_id.clone(),
                PreparedAssetCook {
                    asset_id: asset_id.clone(),
                    source_path,
                    source_hash,
                    source_byte_size,
                    asset_role: asset_role_for_path(&sidecar.source, &sidecar.asset_type),
                    asset_type: sidecar.asset_type.clone(),
                    font: sidecar.font.as_ref().map(|font| CookedFontMetadata {
                        family: font.family.clone(),
                        face_index: font.face_index,
                        license_id: sidecar
                            .license
                            .clone()
                            .expect("validated asset sidecar has a license"),
                        subset: font.subset.clone(),
                        coverage: font
                            .coverage
                            .iter()
                            .map(|range| UnicodeRange {
                                start: range.start,
                                end: range.end,
                            })
                            .collect(),
                    }),
                },
            )
            .is_some()
        {
            return Err(format!("nativevn asset id is duplicated: {asset_id}").into());
        }
        processor_ids.insert(sidecar.cook.processor.clone());
        nodes.push(CookNode {
            request: CookRequest {
                sidecar: sidecar.clone(),
                source_bytes,
                target_profile: profile.to_string(),
                processor_version: "1.0.0".to_string(),
                dependency_artifacts: Default::default(),
            },
        });
    }
    let mut registry = CookProcessorRegistry::default();
    for processor_id in processor_ids {
        registry.register(DefaultCookProcessor::new(processor_id, "1.0.0"))?;
    }
    let cache = FileCookCache::new(project_dir.join(".astra-cache").join("cook"));
    let executor = CookBatchExecutor::new(&registry, Some(&cache));
    let max_concurrency = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8);
    let batch = executor.execute(
        CookBatchRequest {
            nodes,
            max_concurrency,
            limits: CookBatchLimits {
                max_node_count: 65_536,
                max_source_bytes_per_node: 1024 * 1024 * 1024,
                max_total_source_bytes: 8 * 1024 * 1024 * 1024,
                max_concurrency: 8,
            },
        },
        cancellation,
    )?;

    let summary = CookSummaryManifest {
        schema: "astra.cook_batch_summary.v1".to_string(),
        graph_hash: batch.graph_hash,
        artifact_count: batch.artifacts.len() as u64,
        cache_hit_count: batch.cache_hit_count,
        cooked_count: batch.cooked_count,
        max_concurrency: max_concurrency as u64,
    };
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let mut artifacts = Vec::with_capacity(batch.artifacts.len());
    for cooked in batch.artifacts {
        let metadata = prepared
            .remove(&cooked.asset_id)
            .ok_or_else(|| format!("cooked asset has no prepared metadata: {}", cooked.asset_id))?;
        let section = cooked.to_section();
        let file_name = format!("{}.bin", section.id.replace('.', "_"));
        fs::write(section_dir.join(&file_name), &section.payload)?;
        artifacts.push(CookedArtifactRef {
            section_id: section.id,
            schema: section.schema,
            path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
            hash: Hash256::from_sha256(&section.payload).to_string(),
            codec: section.codec,
            asset_path: Some(normalize_relative_path(&metadata.source_path)),
            asset_role: Some(metadata.asset_role),
            asset_sha256: Some(metadata.source_hash.to_string()),
            asset_byte_size: Some(metadata.source_byte_size),
            asset_type: Some(metadata.asset_type),
            asset_id: Some(metadata.asset_id),
            font: metadata.font,
            scenario_path: None,
        });
    }
    if !prepared.is_empty() {
        return Err("cook batch did not produce every prepared asset".into());
    }
    Ok((artifacts, summary))
}

fn nativevn_asset_roots(project: &serde_yaml::Value) -> Vec<String> {
    string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("asset_roots")),
    )
}

fn cook_font_manifest_section(
    artifacts: &[CookedArtifactRef],
    target: &str,
    profile: &str,
    out: &std::path::Path,
) -> Result<CookedArtifactRef, CliError> {
    if artifacts
        .iter()
        .any(|artifact| artifact.section_id == "media.font_manifest")
    {
        return Err(
            "ASTRA_COOK_FONT_MANIFEST_DUPLICATE: media.font_manifest is generated from font assets"
                .into(),
        );
    }
    let mut fonts = artifacts
        .iter()
        .filter_map(|artifact| artifact.font.as_ref().map(|font| (artifact, font)))
        .map(|(artifact, font)| {
            let asset_id = artifact
                .asset_id
                .clone()
                .ok_or("ASTRA_COOK_FONT_IDENTITY: font artifact is missing its asset id")?;
            let asset_path = artifact
                .asset_path
                .as_deref()
                .ok_or("ASTRA_COOK_FONT_IDENTITY: font artifact is missing its package path")?;
            let hash: Hash256 = artifact
                .hash
                .parse()
                .map_err(|_| "ASTRA_COOK_FONT_HASH: font artifact hash is invalid")?;
            Ok(FontPackageEntry {
                asset_id,
                uri: VfsUri::parse(&format!("package:/{}", normalize_vfs_path(asset_path)?))?,
                family: font.family.clone(),
                face_index: font.face_index,
                hash,
                license_id: font.license_id.clone(),
                subset: font.subset.clone(),
                coverage: font.coverage.clone(),
                targets: vec![target.to_string()],
                profiles: vec![profile.to_string()],
            })
        })
        .collect::<Result<Vec<_>, CliError>>()?;
    fonts.sort_by(|left, right| left.asset_id.cmp(&right.asset_id));
    if fonts.is_empty() {
        return Err(
            "ASTRA_COOK_FONT_MANIFEST_MISSING: NativeVN projects require at least one packaged font asset"
                .into(),
        );
    }
    let manifest = FontPackageManifest {
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.to_string(),
        target: target.to_string(),
        profile: profile.to_string(),
        provider_binding: "astra.vfs.package".to_string(),
        fonts,
    };
    let payload = serde_json::to_vec_pretty(&manifest)?;
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let file_name = "media_font_manifest.json";
    fs::write(section_dir.join(file_name), &payload)?;
    Ok(CookedArtifactRef {
        section_id: "media.font_manifest".to_string(),
        schema: FONT_PACKAGE_MANIFEST_SCHEMA.to_string(),
        path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
        hash: Hash256::from_sha256(&payload).to_string(),
        codec: SectionCodec::Raw,
        asset_path: None,
        asset_role: None,
        asset_sha256: None,
        asset_byte_size: None,
        asset_type: None,
        asset_id: None,
        font: None,
        scenario_path: None,
    })
}

fn cook_player_locale_config(
    project: &serde_yaml::Value,
    artifacts: &[CookedArtifactRef],
    out: &std::path::Path,
) -> Result<CookedArtifactRef, CliError> {
    let default_locale = project
        .get("nativevn")
        .and_then(|nativevn| nativevn.get("default_locale"))
        .and_then(serde_yaml::Value::as_str)
        .ok_or("ASTRA_COOK_LOCALE_DEFAULT: nativevn.default_locale is required")?;
    if default_locale.is_empty()
        || default_locale.len() > 64
        || !default_locale
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("ASTRA_COOK_LOCALE_DEFAULT: default locale id is unsafe".into());
    }
    let mut available_locales = artifacts
        .iter()
        .filter(|artifact| artifact.schema == "astra.vn.localization_table.v1")
        .filter_map(|artifact| artifact.section_id.strip_prefix("vn.localization."))
        .map(str::to_string)
        .collect::<Vec<_>>();
    available_locales.sort();
    available_locales.dedup();
    if available_locales.is_empty()
        || !available_locales
            .iter()
            .any(|locale| locale == default_locale)
    {
        return Err(
            "ASTRA_COOK_LOCALE_COVERAGE: default locale has no profile-eligible localization section"
                .into(),
        );
    }
    let config = PlayerLocaleConfig {
        schema: PLAYER_LOCALE_CONFIG_SCHEMA.to_string(),
        default_locale: default_locale.to_string(),
        available_locales,
    };
    let payload = serde_json::to_vec_pretty(&config)?;
    let section_dir = out.join("sections");
    fs::create_dir_all(&section_dir)?;
    let file_name = "player_locale_config.json";
    fs::write(section_dir.join(file_name), &payload)?;
    Ok(CookedArtifactRef {
        section_id: "player.locale_config".to_string(),
        schema: PLAYER_LOCALE_CONFIG_SCHEMA.to_string(),
        path: normalize_relative_path(std::path::Path::new("sections").join(file_name)),
        hash: Hash256::from_sha256(&payload).to_string(),
        codec: SectionCodec::Raw,
        asset_path: None,
        asset_role: None,
        asset_sha256: None,
        asset_byte_size: None,
        asset_type: None,
        asset_id: None,
        font: None,
        scenario_path: None,
    })
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
        let normalized_path = normalize_vfs_path(scenario_ref)?;
        artifacts.push(CookedArtifactRef {
            section_id: scenario_section_id(&normalized_path),
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
            font: None,
            scenario_path: Some(normalized_path),
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
            font: None,
            scenario_path: None,
        });
    }
    Ok(artifacts)
}

fn nativevn_source_paths(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
    field: &str,
    default_directory: &str,
) -> Result<Vec<PathBuf>, CliError> {
    let mut sources = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get(field)),
    );
    if sources.is_empty() && field == "sources" {
        sources = string_list(project.get("scripts"));
    }
    if sources.is_empty() {
        let default = project_dir.join(default_directory);
        if default.is_dir() {
            sources.push(default_directory.to_string());
        }
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

fn nativevn_theme_paths(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
) -> Result<Vec<PathBuf>, CliError> {
    let mut sources = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("ui_themes")),
    );
    if sources.is_empty() && project_dir.join("Themes").is_dir() {
        sources.push("Themes".to_string());
    }
    let mut paths = Vec::new();
    for source in sources {
        let relative = validate_project_relative_path(&source)?;
        let absolute = project_dir.join(&relative);
        if absolute.is_dir() {
            collect_ui_theme_sources(project_dir, &absolute, &mut paths)?;
        } else {
            paths.push(relative);
        }
    }
    paths.sort_by_key(|path| normalize_relative_path(path));
    paths.dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));
    Ok(paths)
}

fn nativevn_controller_paths(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
) -> Result<Vec<PathBuf>, CliError> {
    let mut sources = string_list(
        project
            .get("nativevn")
            .and_then(|nativevn| nativevn.get("ui_controllers")),
    );
    if sources.is_empty() && project_dir.join("Controllers").is_dir() {
        sources.push("Controllers".to_string());
    }
    let mut paths = Vec::new();
    for source in sources {
        let relative = validate_project_relative_path(&source)?;
        let absolute = project_dir.join(&relative);
        if absolute.is_dir() {
            collect_sources_with_extension(project_dir, &absolute, "luau", &mut paths)?;
        } else if relative
            .extension()
            .and_then(|extension| extension.to_str())
            == Some("luau")
        {
            paths.push(relative);
        } else {
            return Err("ASTRA_UI_CONTROLLER_FORMAT: controller source must use .luau".into());
        }
    }
    paths.sort_by_key(|path| normalize_relative_path(path));
    paths.dedup_by(|left, right| normalize_relative_path(left) == normalize_relative_path(right));
    Ok(paths)
}

fn nativevn_ui_component_artifacts(
    project: &serde_yaml::Value,
    project_dir: &std::path::Path,
) -> Result<Vec<VnUiComponentArtifactInput>, CliError> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ComponentTargetSource {
        manifest: String,
        artifact: String,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ComponentSource {
        id: String,
        signer_public_key: String,
        windows: ComponentTargetSource,
        web: ComponentTargetSource,
    }

    let Some(value) = project
        .get("nativevn")
        .and_then(|nativevn| nativevn.get("ui_components"))
    else {
        return Ok(Vec::new());
    };
    let sources: Vec<ComponentSource> = serde_yaml::from_value(value.clone())?;
    let mut ids = std::collections::BTreeSet::new();
    let mut result = Vec::with_capacity(sources.len().saturating_mul(2));
    for source in sources {
        if !ids.insert(source.id.clone()) {
            return Err("ASTRA_UI_COMPONENT_PROJECT_DUPLICATE: component id is duplicated".into());
        }
        if source.signer_public_key.len() != 64
            || !source
                .signer_public_key
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("ASTRA_UI_COMPONENT_SIGNER_KEY: signer_public_key must be 64 lowercase hex characters".into());
        }
        let public_bytes = hex::decode(&source.signer_public_key).map_err(|_| {
            "ASTRA_UI_COMPONENT_SIGNER_KEY: signer_public_key must be 64 lowercase hex characters"
        })?;
        let public_key: [u8; 32] = public_bytes.try_into().map_err(|_| {
            "ASTRA_UI_COMPONENT_SIGNER_KEY: signer_public_key must encode exactly 32 bytes"
        })?;
        for (target, target_source) in [
            (VnUiComponentTarget::Windows, source.windows),
            (VnUiComponentTarget::Web, source.web),
        ] {
            let manifest_path = validate_project_relative_path(&target_source.manifest)?;
            let artifact_path = validate_project_relative_path(&target_source.artifact)?;
            let manifest_text = fs::read_to_string(project_dir.join(&manifest_path))?;
            let manifest: astra_ui_plugin_abi::UiComponentManifest =
                match manifest_path.extension().and_then(std::ffi::OsStr::to_str) {
                    Some("json") => serde_json::from_str(&manifest_text)?,
                    Some("yaml" | "yml") => serde_yaml::from_str(&manifest_text)?,
                    _ => {
                        return Err(
                            "ASTRA_UI_COMPONENT_MANIFEST_FORMAT: manifest must use JSON or YAML"
                                .into(),
                        )
                    }
                };
            if manifest.component_id != source.id {
                return Err(
                    "ASTRA_UI_COMPONENT_PROJECT_IDENTITY: source id differs from signed manifest"
                        .into(),
                );
            }
            let artifact = fs::read(project_dir.join(artifact_path))?;
            if artifact.is_empty() || artifact.len() > 64 * 1024 * 1024 {
                return Err(
                    "ASTRA_UI_COMPONENT_ARTIFACT_SIZE: artifact must contain 1..=64 MiB".into(),
                );
            }
            result.push(VnUiComponentArtifactInput {
                target,
                manifest,
                artifact,
                signer_public_key: public_key,
            });
        }
    }
    Ok(result)
}

fn collect_sources_with_extension(
    root: &std::path::Path,
    dir: &std::path::Path,
    extension: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_sources_with_extension(root, &path, extension, out)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
}

fn collect_ui_theme_sources(
    root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_ui_theme_sources(root, &path, out)?;
        } else if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("json" | "yaml" | "yml")
        ) {
            out.push(path.strip_prefix(root)?.to_path_buf());
        }
    }
    Ok(())
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
    let mut request = production_package_request(
        manifest.package_id.clone(),
        manifest.profile.clone(),
        artifacts,
        &package_target_manifest,
        target,
    )?;
    request.cook_summary = serde_json::to_vec(&manifest.asset_cook)?;
    request.asset_vfs_manifest = asset_vfs_manifest;
    request.asset_catalog = asset_catalog;
    request.target_manifest = serde_json::to_vec(&package_target_manifest)?;
    request.platform_eligibility = platform_eligibility(&package_target_manifest, target)?;
    request.scenario_refs = serde_json::to_vec(&scenario_refs_manifest_from_cooked(
        &manifest,
        &request.cooked_assets,
    )?)?;
    request.release_summary = br#"{"schema":"astra.release_summary.v1","status":"built"}"#.to_vec();
    PackageBuilder::build(request).map_err(|err| err.to_string().into())
}

fn production_package_request(
    package_id: String,
    profile: String,
    cooked_assets: Vec<SectionPayload>,
    target_manifest: &TargetManifest,
    target_id: &str,
) -> Result<PackageBuildRequest, CliError> {
    let has_font_manifest = cooked_assets.iter().any(|section| {
        section.id == "media.font_manifest" && section.schema == FONT_PACKAGE_MANIFEST_SCHEMA
    });
    let target = target_manifest
        .targets
        .iter()
        .find(|target| target.id == target_id && target.kind == astra_target::TargetKind::Game)
        .ok_or_else(|| format!("package target {target_id} is not a game target"))?;
    let runtime_provider = target
        .runtime_provider
        .as_deref()
        .ok_or_else(|| format!("package target {target_id} has no runtime provider binding"))?;
    if runtime_provider != "native_vn" {
        return Err(format!(
            "runtime provider {runtime_provider} is not implemented for product packaging"
        )
        .into());
    }
    let provider_specs = [
        ("presentation", "astra.renderer.wgpu", "renderer2d.wgpu"),
        ("vfs_provider", "astra.vfs.package", "vfs.backend.package"),
        (
            "game_runtime_provider",
            "astra.runtime.native_vn",
            "runtime.native_vn",
        ),
    ];
    let bindings = provider_specs
        .iter()
        .map(|(slot, provider_id, capability)| {
            astra_plugin_abi::ProviderBinding::new(
                *slot,
                *provider_id,
                astra_plugin_abi::ProviderBindingContext {
                    package_id: package_id.clone(),
                    target: target_id.to_string(),
                    profile: profile.clone(),
                    required_capability: capability.to_string(),
                    engine_version: env!("CARGO_PKG_VERSION").to_string(),
                    rustc_fingerprint: "rustc-stable".to_string(),
                    feature_fingerprint: "runtime-envelope-v2".to_string(),
                    abi_fingerprint: "astra-plugin-abi-v2".to_string(),
                },
            )
            .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let provider_policy = serde_json::to_vec(&astra_plugin_abi::ProviderPolicy {
        schema: astra_plugin_abi::PROVIDER_POLICY_SCHEMA.to_string(),
        profile: profile.clone(),
        renderer: "astra.renderer.wgpu".to_string(),
        decode_fallback: "profile_bound".to_string(),
        runtime_provider: astra_vn_runtime_provider::NativeVnRuntimeProvider::descriptor(),
        bindings: bindings.clone(),
    })?;
    let plugin_extension_registry =
        serde_json::to_vec(&astra_plugin_abi::PluginExtensionRegistrySnapshot {
            schema: astra_plugin_abi::PLUGIN_EXTENSION_REGISTRY_SCHEMA.to_string(),
            providers: provider_specs
                .iter()
                .map(
                    |(slot, provider_id, capability)| astra_plugin_abi::ProviderExtensionRecord {
                        slot: slot.to_string(),
                        provider_id: provider_id.to_string(),
                        capability: capability.to_string(),
                        phase: astra_plugin_abi::LoadPhase::Runtime,
                        packaged: true,
                        engine_version: env!("CARGO_PKG_VERSION").to_string(),
                        rustc_fingerprint: "rustc-stable".to_string(),
                        feature_fingerprint: "runtime-envelope-v2".to_string(),
                        abi_fingerprint: "astra-plugin-abi-v2".to_string(),
                    },
                )
                .collect(),
            bindings,
            conflicts: vec![],
        })?;
    Ok(PackageBuildRequest {
        package_id,
        profile: profile.clone(),
        cooked_assets,
        cook_summary: vec![],
        asset_vfs_manifest: vec![],
        asset_catalog: vec![],
        media_manifest: serde_json::to_vec(&serde_json::json!({
            "schema": "astra.media_manifest.v1",
            "codecs": ["png", "jpeg", "webp", "wav", "ogg", "flac", "mp3"],
            "ffmpeg": "profile_bound",
            "font_manifest_required": has_font_manifest,
            "font_manifest_section": if has_font_manifest { "media.font_manifest" } else { "" }
        }))?,
        provider_policy,
        plugin_extension_registry,
        plugin_dependency_graph:
            br#"{"schema":"astra.plugin_dependency_graph.v1","dependencies":[]}"#.to_vec(),
        module_fingerprint: serde_json::to_vec(&serde_json::json!({
            "schema": "astra.module_fingerprint.v1",
            "modules": [{"crate": target.crate_name, "target": target.id}]
        }))?,
        target_manifest: serde_json::to_vec(target_manifest)?,
        release_summary: br#"{"schema":"astra.release_summary.v1","status":"built"}"#.to_vec(),
        scenario_refs: serde_json::to_vec(&ScenarioRefsManifest::empty())?,
        platform_eligibility: vec![],
        extra_sections: vec![],
    })
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
            "capabilities": ["vfs.backend.package"]
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

fn scenario_refs_manifest_from_cooked(
    manifest: &CookManifest,
    sections: &[SectionPayload],
) -> Result<ScenarioRefsManifest, CliError> {
    let section_by_id = sections
        .iter()
        .map(|section| (section.id.as_str(), section))
        .collect::<BTreeMap<_, _>>();
    let mut refs_by_path = manifest
        .artifacts
        .iter()
        .filter_map(|artifact| {
            artifact
                .scenario_path
                .as_deref()
                .map(|path| (path, artifact))
        })
        .collect::<BTreeMap<_, _>>();
    if refs_by_path.len() != manifest.scenario_refs.len() {
        return Err("cook manifest scenario path bindings are incomplete or duplicated".into());
    }
    let mut scenarios = Vec::with_capacity(manifest.scenario_refs.len());
    for path in &manifest.scenario_refs {
        let normalized = normalize_vfs_path(path)?;
        let artifact = refs_by_path
            .remove(normalized.as_str())
            .ok_or_else(|| format!("scenario ref {normalized} has no cooked section binding"))?;
        let section = section_by_id
            .get(artifact.section_id.as_str())
            .ok_or_else(|| format!("scenario ref {normalized} section is missing"))?;
        scenarios.push(ScenarioReference {
            path: normalized,
            section_id: artifact.section_id.clone(),
            hash: Hash256::from_sha256(&section.payload),
            byte_size: section.payload.len() as u64,
        });
    }
    scenarios.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(ScenarioRefsManifest {
        schema: "astra.scenario_refs.v2".to_string(),
        scenarios,
    })
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

fn scenario_section_id(path: &str) -> String {
    format!(
        "scenario.ref.{}",
        Hash256::from_sha256(path.as_bytes()).to_hex()
    )
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
    if let Err(error) = rename_directory_with_transient_retry(&staging_path, out, "bundle_commit") {
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
    let locale_config = package_player_locale_config(&reader)?;

    fs::create_dir_all(out.join("package"))?;
    let bundled_package = out.join("package").join("nativevn.astrapkg");
    fs::write(&bundled_package, &package_bytes)?;
    let scenario_bindings = package_scenario_refs(&reader)?;
    let mut files = vec![bundle_file(
        "package/nativevn.astrapkg",
        "package",
        &package_bytes,
    )];
    let mut mount_policy = bundle_mount_policy(&reader, out, &mut files)?;
    let mut bundle_checks = Vec::new();
    let mut crash_reporter_ref = None;
    for scenario_ref in &scenario_bindings {
        let relative = validate_bundle_relative_path(&scenario_ref.path)?;
        let scenario_bytes = reader
            .container()
            .read_section(&scenario_ref.section_id)
            .map_err(|_| {
                format!(
                    "scenario ref {} section {} is not available in package",
                    scenario_ref.path, scenario_ref.section_id
                )
            })?;
        let destination = out.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, &scenario_bytes)?;
        files.push(bundle_file(
            &scenario_ref.path,
            "scenario_ref",
            &scenario_bytes,
        ));
    }
    let ui_components = bundle_ui_components(&reader, out, platform, artifacts, &mut files)?;

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

            let config = player_config_bytes(
                target,
                profile,
                platform_name,
                &display_config,
                &locale_config,
                ui_components.as_ref(),
            )?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            entrypoint.to_string()
        }
        PlatformId::Linux => {
            let entrypoint = "astra-player";
            let player_source = artifacts
                .linux_player
                .as_deref()
                .ok_or("Linux bundle requires --linux-player pointing to a built astra-player")?;
            let player_bytes = fs::read(player_source)?;
            let entrypoint_path = out.join(entrypoint);
            fs::write(&entrypoint_path, &player_bytes)?;
            make_executable(&entrypoint_path)?;
            files.push(bundle_file(entrypoint, "linux_player", &player_bytes));
            bundle_checks.push(PlayerLaunchCheck {
                id: "crash_reporter.not_applicable".to_string(),
                status: "pass".to_string(),
            });
            let config = player_config_bytes(
                target,
                profile,
                platform_name,
                &display_config,
                &locale_config,
                ui_components.as_ref(),
            )?;
            fs::write(out.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            entrypoint.to_string()
        }
        PlatformId::Macos => {
            let player_source = artifacts.macos_player.as_deref().ok_or(
                "macOS bundle requires --macos-player pointing to a Universal 2 astra-player",
            )?;
            let player_bytes = fs::read(player_source)?;
            validate_universal_macho(&player_bytes)?;
            let contents = out.join("Contents");
            let macos = contents.join("MacOS");
            let resources = contents.join("Resources");
            fs::create_dir_all(&macos)?;
            fs::create_dir_all(&resources)?;
            fs::rename(out.join("package"), resources.join("package"))?;
            for scenario in &scenario_bindings {
                let relative = validate_bundle_relative_path(&scenario.path)?;
                let source = out.join(&relative);
                if source.is_file() {
                    let destination = resources.join(&relative);
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(source, destination)?;
                }
            }
            if let Some(path) = mount_policy.as_mut() {
                let relative = validate_bundle_relative_path(path)?;
                let source = out.join(&relative);
                if source.is_file() {
                    let destination = resources.join(&relative);
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(source, destination)?;
                }
                *path = format!("Contents/Resources/{path}");
            }
            for file in &mut files {
                file.path = format!("Contents/Resources/{}", file.path);
            }
            let entrypoint = "Contents/MacOS/astra-player";
            let entrypoint_path = out.join(entrypoint);
            fs::write(&entrypoint_path, &player_bytes)?;
            make_executable(&entrypoint_path)?;
            files.push(bundle_file(
                entrypoint,
                "macos_universal_player",
                &player_bytes,
            ));
            let config = player_config_bytes(
                target,
                profile,
                platform_name,
                &display_config,
                &locale_config,
                ui_components.as_ref(),
            )?;
            fs::write(resources.join("AstraPlayer.config.json"), &config)?;
            files.push(bundle_file(
                "Contents/Resources/AstraPlayer.config.json",
                "player_config",
                &config,
            ));
            let plist = macos_info_plist(&package_manifest.package_id);
            fs::write(contents.join("Info.plist"), plist.as_bytes())?;
            files.push(bundle_file(
                "Contents/Info.plist",
                "macos_info_plist",
                plist.as_bytes(),
            ));
            bundle_checks.extend([
                PlayerLaunchCheck {
                    id: "macos.universal2".into(),
                    status: "pass".into(),
                },
                PlayerLaunchCheck {
                    id: "macos.codesign".into(),
                    status: "required_external".into(),
                },
                PlayerLaunchCheck {
                    id: "macos.notarization".into(),
                    status: "required_external".into(),
                },
                PlayerLaunchCheck {
                    id: "crash_reporter.not_applicable".into(),
                    status: "pass".into(),
                },
            ]);
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

            let config = player_config_bytes(
                target,
                profile,
                platform_name,
                &display_config,
                &locale_config,
                ui_components.as_ref(),
            )?;
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
            fs::write(
                out.join("astra-ui-component-host.js"),
                WEB_UI_COMPONENT_HOST,
            )?;
            files.push(bundle_file(
                "astra-ui-component-host.js",
                "web_ui_component_host",
                WEB_UI_COMPONENT_HOST,
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
        PlatformId::Ios | PlatformId::Android => {
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
        package: if platform == PlatformId::Macos {
            "Contents/Resources/package/nativevn.astrapkg".to_string()
        } else {
            "package/nativevn.astrapkg".to_string()
        },
        scenario_refs: scenario_bindings
            .into_iter()
            .map(|scenario| {
                if platform == PlatformId::Macos {
                    format!("Contents/Resources/{}", scenario.path)
                } else {
                    scenario.path
                }
            })
            .collect(),
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

fn bundle_ui_components(
    reader: &PackageReader,
    out: &Path,
    platform: PlatformId,
    artifacts: &BundleArtifactInputs,
    files: &mut Vec<StandaloneBundleFile>,
) -> Result<Option<serde_json::Value>, CliError> {
    let bundle: VnUiComponentBundleManifest = reader
        .container()
        .decode_postcard("vn.ui_component_manifest")?;
    if bundle.schema != "astra.vn.ui_component_bundle.v1" {
        return Err("ASTRA_UI_COMPONENT_BUNDLE_SCHEMA: package must be re-cooked".into());
    }
    if bundle.ids.is_empty() {
        if !bundle.bindings.is_empty() {
            return Err(
                "ASTRA_UI_COMPONENT_BUNDLE_IDENTITY: empty declaration has bindings".into(),
            );
        }
        return Ok(None);
    }
    if bundle.bindings.len() != bundle.ids.len() {
        return Err("ASTRA_UI_COMPONENT_BUNDLE_IDENTITY: declaration/binding count differs".into());
    }

    let mut allowlist = BTreeMap::new();
    for binding in &bundle.bindings {
        if !bundle.ids.contains(&binding.component_id)
            || allowlist
                .insert(binding.signer_id.clone(), binding.signer_public_key)
                .is_some_and(|key| key != binding.signer_public_key)
        {
            return Err(
                "ASTRA_UI_COMPONENT_ALLOWLIST_CONFLICT: signer identity is ambiguous".into(),
            );
        }
    }
    let root = out.join("ui-components");
    fs::create_dir_all(&root)?;
    let allowlist_bytes = postcard::to_allocvec(&allowlist)?;
    let allowlist_path = "ui-components/allowlist.postcard";
    fs::write(out.join(allowlist_path), &allowlist_bytes)?;
    files.push(bundle_file(
        allowlist_path,
        "ui_component_allowlist",
        &allowlist_bytes,
    ));

    let target = match platform {
        PlatformId::Windows => VnUiComponentTarget::Windows,
        PlatformId::Web => VnUiComponentTarget::Web,
        _ => return Err("ASTRA_UI_COMPONENT_PLATFORM: components require Windows or Web".into()),
    };
    let mut entries = Vec::new();
    for component_id in &bundle.ids {
        let loaded = load_ui_component_artifact(reader, component_id, target, &allowlist)?;
        let component_root = format!("ui-components/{component_id}");
        let manifest_path = format!("{component_root}/manifest.postcard");
        let manifest_bytes = postcard::to_allocvec(&loaded.manifest)?;
        fs::create_dir_all(out.join(&component_root))?;
        fs::write(out.join(&manifest_path), &manifest_bytes)?;
        files.push(bundle_file(
            &manifest_path,
            "ui_component_manifest",
            &manifest_bytes,
        ));
        let entry = if platform == PlatformId::Windows {
            let artifact_path = format!("{component_root}/component.dll");
            fs::write(out.join(&artifact_path), &loaded.artifact)?;
            files.push(bundle_file(
                &artifact_path,
                "ui_component_artifact",
                &loaded.artifact,
            ));
            serde_json::json!({
                "id": component_id,
                "manifest": manifest_path,
                "artifact": artifact_path,
                "artifact_hash": loaded.manifest.artifact_hash.to_string(),
                "signer_id": loaded.manifest.signer_id,
            })
        } else {
            bundle_web_ui_component(
                component_id,
                &component_root,
                &manifest_path,
                &loaded,
                out,
                files,
            )?
        };
        entries.push(entry);
    }

    let host = if platform == PlatformId::Windows {
        let source = artifacts
            .ui_component_host
            .as_deref()
            .ok_or("Windows bundle with UI components requires --ui-component-host")?;
        let bytes = fs::read(source)?;
        if bytes.is_empty() {
            return Err("ASTRA_UI_COMPONENT_HOST_EMPTY: host executable is empty".into());
        }
        let path = "AstraUiComponentHost.exe";
        fs::write(out.join(path), &bytes)?;
        make_executable(&out.join(path))?;
        files.push(bundle_file(path, "ui_component_host", &bytes));
        Some(path)
    } else {
        None
    };
    Ok(Some(serde_json::json!({
        "schema": "astra.player_ui_components.v1",
        "host": host,
        "allowlist": allowlist_path,
        "deadline_ms": 100,
        "components": entries,
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebUiComponentArtifact {
    schema: String,
    bindings: String,
    files: Vec<WebUiComponentArtifactFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebUiComponentArtifactFile {
    path: String,
    sha256: String,
    byte_size: usize,
    base64: String,
}

fn bundle_web_ui_component(
    component_id: &str,
    component_root: &str,
    manifest_path: &str,
    loaded: &astra_vn::LoadedVnUiComponentArtifact,
    out: &Path,
    files: &mut Vec<StandaloneBundleFile>,
) -> Result<serde_json::Value, CliError> {
    let artifact: WebUiComponentArtifact = serde_json::from_slice(&loaded.artifact)
        .map_err(|error| format!("ASTRA_UI_COMPONENT_WEB_ARTIFACT_JSON: {error}"))?;
    if artifact.schema != "astra.ui_component_web_artifact.v1"
        || artifact.files.is_empty()
        || artifact.files.len() > 64
    {
        return Err("ASTRA_UI_COMPONENT_WEB_ARTIFACT_SCHEMA: invalid Web artifact bundle".into());
    }
    let bindings = validate_component_relative_path(&artifact.bindings)?;
    if !bindings.ends_with(".js") {
        return Err("ASTRA_UI_COMPONENT_WEB_BINDINGS: bindings must be a JavaScript module".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut core_artifacts = serde_json::Map::new();
    let mut bindings_found = false;
    for file in artifact.files {
        let relative = validate_component_relative_path(&file.path)?;
        if !seen.insert(relative.clone()) {
            return Err("ASTRA_UI_COMPONENT_WEB_DUPLICATE: artifact path is duplicated".into());
        }
        let bytes = BASE64_STANDARD
            .decode(file.base64.as_bytes())
            .map_err(|_| "ASTRA_UI_COMPONENT_WEB_BASE64: artifact payload is invalid")?;
        if bytes.len() != file.byte_size
            || file.byte_size == 0
            || file.byte_size > 64 * 1024 * 1024
            || file.sha256 != Hash256::from_sha256(&bytes).to_string()
        {
            return Err("ASTRA_UI_COMPONENT_WEB_HASH: artifact size or hash differs".into());
        }
        if relative == bindings {
            bindings_found = true;
        }
        if relative.ends_with(".wasm") {
            core_artifacts.insert(
                relative.clone(),
                serde_json::json!({"sha256": file.sha256, "byte_size": file.byte_size}),
            );
        }
        let bundle_path = format!("{component_root}/{relative}");
        let destination = out.join(&bundle_path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&destination, &bytes)?;
        files.push(bundle_file(
            &bundle_path,
            "ui_component_web_artifact",
            &bytes,
        ));
    }
    if !bindings_found || core_artifacts.is_empty() {
        return Err("ASTRA_UI_COMPONENT_WEB_INCOMPLETE: bindings or core Wasm is missing".into());
    }
    Ok(serde_json::json!({
        "id": component_id,
        "manifest": manifest_path,
        "artifact_hash": loaded.manifest.artifact_hash.to_string(),
        "signer_id": loaded.manifest.signer_id,
        "bindings": format!("{component_root}/{bindings}"),
        "core_artifacts": core_artifacts,
    }))
}

fn validate_component_relative_path(value: &str) -> Result<String, CliError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(
            "ASTRA_UI_COMPONENT_WEB_PATH: artifact path must be contained and relative".into(),
        );
    }
    Ok(value.to_string())
}

fn player_config_bytes(
    target: &str,
    profile: &str,
    platform_name: &str,
    display_config: &Option<PlayerDisplayConfig>,
    locale_config: &PlayerLocaleConfig,
    ui_components: Option<&serde_json::Value>,
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
        "locale": locale_config.default_locale,
        "package": "package/nativevn.astrapkg",
        "observability": observability
    });
    if let Some(display) = display_config {
        config["display"] = serde_json::to_value(display)?;
    }
    if let Some(components) = ui_components {
        config["ui_components"] = components.clone();
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

fn package_player_locale_config(reader: &PackageReader) -> Result<PlayerLocaleConfig, CliError> {
    load_player_locale_config(reader).map_err(|error| error.to_string().into())
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

fn package_scenario_refs(reader: &PackageReader) -> Result<Vec<ScenarioReference>, CliError> {
    let bytes = reader.container().read_section("scenario.refs")?;
    let mut manifest: ScenarioRefsManifest = serde_json::from_slice(&bytes)?;
    manifest
        .scenarios
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(manifest.scenarios)
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

fn validate_universal_macho(bytes: &[u8]) -> Result<(), CliError> {
    const FAT_MAGIC: u32 = 0xcafebabe;
    const CPU_X86_64: u32 = 0x01000007;
    const CPU_ARM64: u32 = 0x0100000c;
    if bytes.len() < 8 || u32::from_be_bytes(bytes[0..4].try_into()?) != FAT_MAGIC {
        return Err("ASTRA_MACOS_UNIVERSAL_REQUIRED: player is not a fat Mach-O".into());
    }
    let count = usize::try_from(u32::from_be_bytes(bytes[4..8].try_into()?))?;
    let table_end = 8usize
        .checked_add(
            count
                .checked_mul(20)
                .ok_or("Mach-O architecture table overflow")?,
        )
        .filter(|end| *end <= bytes.len())
        .ok_or("Mach-O architecture table is truncated")?;
    let mut architectures = std::collections::BTreeSet::new();
    for record in bytes[8..table_end].chunks_exact(20) {
        architectures.insert(u32::from_be_bytes(record[0..4].try_into()?));
    }
    if !architectures.contains(&CPU_X86_64) || !architectures.contains(&CPU_ARM64) {
        return Err("ASTRA_MACOS_UNIVERSAL_REQUIRED: x86_64 and arm64 slices are required".into());
    }
    Ok(())
}

fn macos_info_plist(bundle_id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>CFBundleDevelopmentRegion</key><string>en</string>\n<key>CFBundleExecutable</key><string>astra-player</string>\n<key>CFBundleIdentifier</key><string>{bundle_id}</string>\n<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>\n<key>CFBundleName</key><string>AstraPlayer</string>\n<key>CFBundlePackageType</key><string>APPL</string>\n<key>LSMinimumSystemVersion</key><string>13.0</string>\n<key>NSHighResolutionCapable</key><true/>\n</dict></plist>\n"
    )
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

#[cfg(test)]
mod ui_cli_tests {
    use super::*;

    #[astra_headless_test::test]
    fn matrix_size_parser_enforces_the_preview_contract() {
        assert_eq!(parse_ui_matrix_size("1920x1080").unwrap(), (1920, 1080));
        for invalid in ["1920", "0x720", "8193x720", "640x8193", "640X480"] {
            assert!(parse_ui_matrix_size(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[astra_headless_test::test]
    fn scene_evidence_contains_only_counts_and_the_precomputed_hash() {
        let commands = vec![
            astra_media::SceneCommand::rect("panel", 1, 2, 3, 4, [5, 6, 7, 8]),
            astra_media::SceneCommand::Clear {
                rgba: [9, 10, 11, 12],
            },
        ];
        let evidence = ui_scene_evidence(&commands, "sha256:scene").unwrap();
        let encoded = serde_json::to_value(evidence).unwrap();
        assert_eq!(encoded["schema"], "astra.ui_scene_evidence.v1");
        assert_eq!(encoded["scene_hash"], "sha256:scene");
        assert_eq!(encoded["command_count"], 2);
        assert_eq!(encoded["command_counts"]["rect"], 1);
        assert_eq!(encoded["command_counts"]["clear"], 1);
        for forbidden in ["rgba", "glyphs", "frame", "vertices", "indices"] {
            assert!(!encoded.to_string().contains(forbidden));
        }
    }

    #[astra_headless_test::test]
    fn macos_bundle_requires_both_universal_slices() {
        let mut binary = vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2];
        for cpu in [0x01000007_u32, 0x0100000c] {
            binary.extend_from_slice(&cpu.to_be_bytes());
            binary.extend_from_slice(&[0; 16]);
        }
        assert!(validate_universal_macho(&binary).is_ok());
        binary[7] = 1;
        assert!(validate_universal_macho(&binary).is_err());
    }
}
