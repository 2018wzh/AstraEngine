use std::path::PathBuf;

use astra_platform::PlatformId;
use clap::{Parser, Subcommand, ValueEnum};

pub(crate) type CliError = Box<dyn std::error::Error + Send + Sync>;
pub(crate) const WEB_PLAYER_LOADER: &[u8] =
    include_bytes!("../../astra-player-web/web/astra-player-loader.js");
pub(crate) const WEB_AUDIO_WORKLET: &[u8] =
    include_bytes!("../../astra-player-web/web/astra-audio-worklet.js");

#[derive(Parser)]
#[command(name = "astra")]
#[command(about = "AstraEngine command line")]
pub(crate) struct Cli {
    #[arg(long, global = true)]
    pub(crate) log_filter: Option<String>,
    #[arg(long, global = true, value_enum, default_value_t = LogFormat::Compact)]
    pub(crate) log_format: LogFormat,
    #[arg(long, global = true)]
    pub(crate) log_dir: Option<PathBuf>,
    #[arg(long, global = true, default_value_t = astra_observability::DEFAULT_MAX_FILE_BYTES)]
    pub(crate) log_max_file_bytes: usize,
    #[arg(long, global = true, default_value_t = astra_observability::DEFAULT_MAX_ARCHIVES)]
    pub(crate) log_max_archives: usize,
    #[arg(long, global = true)]
    pub(crate) crash_dir: Option<PathBuf>,
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
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
pub(crate) enum UiCommand {
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
pub(crate) enum ScriptCommand {
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
pub(crate) enum PackageCommand {
    Build {
        cooked: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, requires_all = ["source_profile", "source_root"])]
        source_unlock_policy: Option<PathBuf>,
        #[arg(long, requires_all = ["source_unlock_policy", "source_root"])]
        source_profile: Option<PathBuf>,
        #[arg(long, requires_all = ["source_unlock_policy", "source_profile"])]
        source_root: Option<PathBuf>,
    },
    Bundle(Box<PackageBundleArgs>),
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

#[derive(clap::Args)]
pub(crate) struct PackageBundleArgs {
    pub(crate) package: PathBuf,
    #[arg(long)]
    pub(crate) out: PathBuf,
    #[arg(long)]
    pub(crate) target: String,
    #[arg(long)]
    pub(crate) profile: String,
    #[arg(long, value_enum)]
    pub(crate) platform: PlatformArg,
    #[arg(long)]
    pub(crate) windows_player: Option<PathBuf>,
    /// Directory containing the matching Microsoft VC x64 CRT redistributable DLLs.
    #[arg(long)]
    pub(crate) windows_runtime: Option<PathBuf>,
    #[arg(long)]
    pub(crate) linux_player: Option<PathBuf>,
    #[arg(long)]
    pub(crate) macos_player: Option<PathBuf>,
    #[arg(long)]
    pub(crate) crash_reporter: Option<PathBuf>,
    #[arg(long)]
    pub(crate) web_player_wasm: Option<PathBuf>,
    #[arg(long)]
    pub(crate) web_player_glue: Option<PathBuf>,
    #[arg(long)]
    pub(crate) android_arm64_library: Option<PathBuf>,
    #[arg(long)]
    pub(crate) android_x86_64_library: Option<PathBuf>,
    #[arg(long, requires = "source_root")]
    pub(crate) source_profile: Option<PathBuf>,
    #[arg(long, requires = "source_profile")]
    pub(crate) source_root: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = ReportFormat::Yaml)]
    pub(crate) format: ReportFormat,
}

#[derive(Subcommand)]
pub(crate) enum ReportCommand {
    Explain { report: PathBuf },
}

#[derive(Subcommand)]
pub(crate) enum TargetCommand {
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
pub(crate) enum PlatformCommand {
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
pub(crate) enum ReportFormat {
    Json,
    Yaml,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum LogFormat {
    Compact,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum PlatformArg {
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
