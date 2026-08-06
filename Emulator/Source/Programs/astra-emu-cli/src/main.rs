use std::path::PathBuf;

use astra_emu_cli::{
    run_headless, run_native, HeadlessLaunch, HeadlessPerformanceArtifacts, NativeLaunch,
    NativeLaunchMode,
};
use clap::{Parser, Subcommand};

mod vfs;

#[global_allocator]
static ASTRA_EMU_ALLOCATOR: astra_observability::TrackingAllocator =
    astra_observability::TrackingAllocator::new();

fn parse_presentation_rate(value: &str) -> Result<u32, String> {
    match value {
        "60" => Ok(60),
        "120" => Ok(120),
        _ => Err("presentation rate must be 60 or 120".into()),
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
// Parsed once at process startup; boxing individual clap fields would add
// indirection without reducing any runtime hot-path allocation.
#[allow(clippy::large_enum_variant)]
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
        /// Write a local-private Perfetto Trace Event file for this native Windows run.
        #[arg(long)]
        perfetto_trace: Option<PathBuf>,
        /// Replay a validated physical-input JSONL sequence through the native host.
        /// Checkpoints are retained as trace markers only; they are not Headless captures.
        #[arg(long)]
        input: Option<PathBuf>,
        /// Stop after this many 60 Hz Runtime fixed steps. This is intended for
        /// bounded native soak runs and is never a throughput sampling shortcut.
        #[arg(long, requires = "input")]
        max_fixed_steps: Option<u64>,
    },
    /// Launch a real native window/device path while replaying only the
    /// validated Headless physical-input JSONL sequence.
    WindowedE2 {
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
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        artifacts: PathBuf,
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        enable_audio: bool,
        #[arg(long)]
        perfetto_trace: Option<PathBuf>,
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
        /// Raster and present one out of every N fixed steps; parity runs must use 1.
        #[arg(long, default_value_t = 1)]
        frame_sample_interval: u64,
        /// Presentation cadence. The Runtime remains at 60 Hz; 120 Hz uses two
        /// semantic GPU presentations per fixed step.
        #[arg(long, default_value_t = 60, value_parser = parse_presentation_rate)]
        presentation_rate_hz: u32,
        #[arg(long)]
        perfetto_trace: Option<PathBuf>,
        /// A profile-bound shared performance budget. Requires all performance outputs.
        #[arg(long, requires_all = ["performance_report", "performance_trace_manifest", "perfetto_trace"])]
        performance_budget: Option<PathBuf>,
        /// Local-private shared performance report output.
        #[arg(long, requires_all = ["performance_budget", "performance_trace_manifest", "perfetto_trace"])]
        performance_report: Option<PathBuf>,
        /// Local-private manifest binding report and Perfetto trace identities.
        #[arg(long, requires_all = ["performance_budget", "performance_report", "perfetto_trace"])]
        performance_trace_manifest: Option<PathBuf>,
        /// Warmup semantic presentations before the fixed 72,000-frame measurement.
        #[arg(long, default_value_t = 1_200)]
        performance_warmup_presentations: u64,
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
    let filter = std::env::var("ASTRA_LOG").unwrap_or_else(|_| "info".to_owned());
    let mut observability = astra_observability::HostObservabilityConfig::for_cli(&filter);
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
            perfetto_trace,
            input,
            max_fixed_steps,
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
                perfetto_trace,
                input_path: input,
                max_fixed_steps,
                mode: NativeLaunchMode::Interactive,
            })
            .await?;
            tracing::info!(
                event = "astra_emu_cli_native_launch_completed",
                family = family.as_str()
            );
        }
        CliCommand::WindowedE2 {
            family,
            game_dir,
            mount_profile,
            entry,
            family_manifest,
            family_library,
            input,
            artifacts,
            enable_audio,
            perfetto_trace,
        } => {
            tracing::info!(
                event = "astra_emu_cli_windowed_e2_started",
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
                perfetto_trace,
                input_path: Some(input),
                max_fixed_steps: None,
                mode: NativeLaunchMode::WindowedE2 {
                    artifact_root: artifacts,
                },
            })
            .await?;
            tracing::info!(
                event = "astra_emu_cli_windowed_e2_completed",
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
            frame_sample_interval,
            presentation_rate_hz,
            perfetto_trace,
            performance_budget,
            performance_report,
            performance_trace_manifest,
            performance_warmup_presentations,
            audit_all_resources,
            resume_snapshot,
            snapshot_output,
        } => {
            tracing::info!(
                event = "astra_emu_cli_headless_started",
                family = family.as_str()
            );
            let performance = match (
                performance_budget,
                performance_report,
                performance_trace_manifest,
            ) {
                (None, None, None) => None,
                (Some(budget_path), Some(report_path), Some(trace_manifest_path)) => {
                    Some(HeadlessPerformanceArtifacts {
                        budget_path,
                        report_path,
                        trace_manifest_path,
                        warmup_presentations: performance_warmup_presentations,
                    })
                }
                _ => return Err("ASTRA_EMU_PERFORMANCE_ARTIFACT_SET_INCOMPLETE".into()),
            };
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
                frame_sample_interval,
                presentation_rate_hz,
                perfetto_trace,
                performance,
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
