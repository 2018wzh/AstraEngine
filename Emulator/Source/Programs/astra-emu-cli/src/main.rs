use std::path::PathBuf;

use astra_emu_cli::{run_headless, run_native, HeadlessLaunch, NativeLaunch};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum EngineType {
    Fvp,
}

impl EngineType {
    fn id(self) -> &'static str {
        match self {
            Self::Fvp => "fvp",
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "astra-emu-cli",
    about = "Explicit AstraEMU quick launch and headless automation"
)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Launch the selected family directly in an overlay-free native game host.
    Run {
        #[arg(long, value_enum)]
        engine: EngineType,
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long, requires = "family_library")]
        family_manifest: Option<PathBuf>,
        #[arg(long, requires = "family_manifest")]
        family_library: Option<PathBuf>,
        /// Enable native audio. Overlay-free visual acceptance is muted by default.
        #[arg(long, default_value_t = false)]
        enable_audio: bool,
    },
    /// Run the same AstraEMU RuntimeWorld/provider path on astra-platform-headless.
    Headless {
        #[arg(long, value_enum)]
        engine: EngineType,
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        artifacts: PathBuf,
        #[arg(long, requires = "family_library")]
        family_manifest: Option<PathBuf>,
        #[arg(long, requires = "family_manifest")]
        family_library: Option<PathBuf>,
        #[arg(long, default_value_t = 1280)]
        viewport_width: u32,
        #[arg(long, default_value_t = 720)]
        viewport_height: u32,
        #[arg(long, default_value = "disabled", value_parser = ["disabled", "ffmpeg-vcpkg"])]
        video_provider: String,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        verify_snapshot: bool,
        #[arg(
            long,
            default_value = "checkpoints",
            value_parser = ["all", "checkpoints", "final", "manifest-only"]
        )]
        artifact_retention: String,
        /// Stream and hash every visible resource after the gameplay run.
        #[arg(long, default_value_t = false)]
        audit_all_resources: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut observability = astra_observability::HostObservabilityConfig::for_cli("info");
    observability.role = astra_observability::HostRole::Cli;
    let _observability = astra_observability::init_host(observability)?;
    match Cli::parse().command {
        CliCommand::Run {
            engine,
            game_dir,
            entry,
            family_manifest,
            family_library,
            enable_audio,
        } => {
            tracing::info!(
                event = "astra_emu_cli_native_launch_started",
                engine = engine.id()
            );
            if engine.id() != "fvp" {
                return Err("ASTRA_EMU_CLI_ENGINE_UNSUPPORTED".into());
            }
            run_native(NativeLaunch {
                game_dir,
                entry,
                family_manifest,
                family_library,
                enable_audio,
            })
            .await?;
            tracing::info!(
                event = "astra_emu_cli_native_launch_completed",
                engine = engine.id()
            );
        }
        CliCommand::Headless {
            engine,
            game_dir,
            entry,
            input,
            artifacts,
            family_manifest,
            family_library,
            viewport_width,
            viewport_height,
            video_provider,
            verify_snapshot,
            artifact_retention,
            audit_all_resources,
        } => {
            if engine.id() != "fvp" {
                return Err("ASTRA_EMU_CLI_ENGINE_UNSUPPORTED".into());
            }
            tracing::info!(
                event = "astra_emu_cli_headless_started",
                engine = engine.id()
            );
            let report = run_headless(HeadlessLaunch {
                game_dir,
                entry,
                input_path: input,
                artifact_root: artifacts,
                family_manifest,
                family_library,
                viewport_width,
                viewport_height,
                video_provider,
                verify_snapshot,
                artifact_retention,
                audit_all_resources,
            })
            .await?;
            tracing::info!(
                event = "astra_emu_cli_headless_completed",
                engine = engine.id(),
                fixed_steps = report.fixed_steps,
                presented_frames = report.presented_frames,
                terminal = report.terminal_reached
            );
            println!("{}", serde_json::to_string(&report)?);
        }
    }
    Ok(())
}
