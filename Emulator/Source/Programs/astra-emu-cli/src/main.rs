use std::{path::PathBuf, process::Command};

use astra_emu_cli::{run_headless, HeadlessLaunch};
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
    /// Launch the Slint Manager directly into a selected legacy game.
    Run {
        #[arg(long, value_enum)]
        engine: EngineType,
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        entry: Option<String>,
        #[arg(long)]
        manager: Option<PathBuf>,
        #[arg(long, requires = "family_library")]
        family_manifest: Option<PathBuf>,
        #[arg(long, requires = "family_manifest")]
        family_library: Option<PathBuf>,
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
            manager,
            family_manifest,
            family_library,
        } => {
            tracing::info!(
                event = "astra_emu_cli_native_launch_started",
                engine = engine.id()
            );
            run_native(
                engine,
                game_dir,
                entry,
                manager,
                family_manifest,
                family_library,
            )?;
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

fn run_native(
    engine: EngineType,
    game_dir: PathBuf,
    entry: Option<String>,
    manager: Option<PathBuf>,
    family_manifest: Option<PathBuf>,
    family_library: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let game_dir = std::fs::canonicalize(game_dir).map_err(|_| "ASTRA_EMU_CLI_GAME_DIR_INVALID")?;
    if !game_dir.is_dir() {
        return Err("ASTRA_EMU_CLI_GAME_DIR_INVALID".into());
    }
    let executable = std::env::current_exe().map_err(|_| "ASTRA_EMU_CLI_EXECUTABLE_PATH")?;
    let manager = manager.unwrap_or_else(|| {
        executable
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(manager_file_name())
    });
    if !manager.is_file() {
        return Err("ASTRA_EMU_CLI_MANAGER_MISSING".into());
    }
    let mut command = Command::new(manager);
    command
        .env("ASTRA_EMU_QUICK_ENGINE", engine.id())
        .env("ASTRA_EMU_QUICK_GAME_DIR", game_dir);
    if let Some(entry) = entry {
        command.env("ASTRA_EMU_QUICK_ENTRY", entry);
    }
    match (family_manifest, family_library) {
        (Some(manifest), Some(library)) => {
            command
                .arg("--family-manifest")
                .arg(manifest)
                .arg("--family-library")
                .arg(library);
        }
        (None, None) => {}
        _ => return Err("ASTRA_EMU_CLI_FAMILY_PATH_PAIR_REQUIRED".into()),
    }
    let status = command
        .status()
        .map_err(|_| "ASTRA_EMU_CLI_MANAGER_START")?;
    if !status.success() {
        return Err("ASTRA_EMU_CLI_MANAGER_FAILED".into());
    }
    Ok(())
}

fn manager_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "astra-emu-manager.exe"
    } else {
        "astra-emu-manager"
    }
}
