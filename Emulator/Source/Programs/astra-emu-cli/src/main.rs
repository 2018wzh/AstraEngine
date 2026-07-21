use std::path::PathBuf;

use astra_emu_cli::{run_headless, run_native, HeadlessLaunch, NativeLaunch};
use clap::{Parser, Subcommand};

mod vfs;

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
    /// Inspect, verify, extract, or mount a family-owned legacy VFS.
    Vfs(vfs::VfsArgs),
    /// Launch the selected family directly in an overlay-free native game host.
    Run {
        #[arg(long)]
        family: String,
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        mount_profile: PathBuf,
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
        #[arg(long)]
        family: String,
        #[arg(long)]
        game_dir: PathBuf,
        #[arg(long)]
        mount_profile: PathBuf,
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
        /// Restore an identity-bound local-private Headless continuation snapshot.
        #[arg(long)]
        resume_snapshot: Option<PathBuf>,
        /// Atomically export an identity-bound local-private continuation snapshot.
        #[arg(long)]
        snapshot_output: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut observability = astra_observability::HostObservabilityConfig::for_cli("info");
    observability.role = astra_observability::HostRole::Cli;
    let _observability = astra_observability::init_host(observability)?;
    match Cli::parse().command {
        CliCommand::Vfs(arguments) => vfs::run(arguments)?,
        CliCommand::Run {
            family,
            game_dir,
            mount_profile,
            entry,
            family_manifest,
            family_library,
            enable_audio,
        } => {
            tracing::info!(
                event = "astra_emu_cli_native_launch_started",
                family = family.as_str()
            );
            run_native(NativeLaunch {
                family_id: family.clone(),
                game_dir,
                mount_profile,
                entry,
                family_manifest,
                family_library,
                enable_audio,
            })
            .await?;
            tracing::info!(
                event = "astra_emu_cli_native_launch_completed",
                family = family.as_str()
            );
        }
        CliCommand::Headless {
            family,
            game_dir,
            mount_profile,
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
            resume_snapshot,
            snapshot_output,
        } => {
            tracing::info!(
                event = "astra_emu_cli_headless_started",
                family = family.as_str()
            );
            let report = run_headless(HeadlessLaunch {
                family_id: family.clone(),
                game_dir,
                mount_profile,
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
                resume_snapshot,
                snapshot_output,
            })
            .await?;
            tracing::info!(
                event = "astra_emu_cli_headless_completed",
                family = family.as_str(),
                fixed_steps = report.fixed_steps,
                presented_frames = report.presented_frames,
                terminal = report.terminal_reached
            );
            println!("{}", serde_json::to_string(&report)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_minori_command_is_not_accepted() {
        assert!(Cli::try_parse_from(["astra-emu-cli", "minori"]).is_err());
    }

    #[test]
    fn generic_vfs_command_requires_explicit_family_and_profile() {
        assert!(Cli::try_parse_from(["astra-emu-cli", "vfs", "verify"]).is_err());
    }

    #[test]
    fn runtime_commands_hard_cut_from_engine_to_family_and_mount_profile() {
        assert!(Cli::try_parse_from([
            "astra-emu-cli",
            "headless",
            "--engine",
            "fvp",
            "--game-dir",
            "game",
            "--input",
            "input.jsonl",
            "--artifacts",
            "artifacts"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "astra-emu-cli",
            "headless",
            "--family",
            "minori",
            "--game-dir",
            "game",
            "--input",
            "input.jsonl",
            "--artifacts",
            "artifacts"
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "astra-emu-cli",
            "headless",
            "--family",
            "minori",
            "--game-dir",
            "game",
            "--mount-profile",
            "mount.yaml",
            "--input",
            "input.jsonl",
            "--artifacts",
            "artifacts"
        ])
        .is_ok());
    }
}
