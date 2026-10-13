use std::path::PathBuf;

use clap::{Parser, Subcommand};

use super::*;

pub(crate) struct CheckpointCapture {
    pub(crate) sequence: u64,
    pub(crate) frame: astra_platform::CapturedFrame,
    pub(crate) audio: Option<astra_headless_protocol::CanonicalAudioSnapshot>,
}

#[derive(Debug, Parser)]
#[command(name = "astra-headless")]
pub(crate) struct Args {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    ValidateInput {
        #[arg(long)]
        input: PathBuf,
    },
    Run {
        #[arg(long)]
        gpu: bool,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        artifact_root: PathBuf,
        #[arg(long)]
        checkpoint_config: Option<PathBuf>,
        #[arg(long)]
        build_identity: PathBuf,
        #[arg(long, requires = "source_root")]
        source_profile: Option<PathBuf>,
        #[arg(long, requires = "source_profile")]
        source_root: Option<PathBuf>,
        #[arg(long, requires_all = ["performance_report", "performance_trace", "performance_trace_manifest"])]
        performance_budget: Option<PathBuf>,
        #[arg(long, requires_all = ["performance_budget", "performance_trace", "performance_trace_manifest"])]
        performance_report: Option<PathBuf>,
        #[arg(long, requires_all = ["performance_budget", "performance_report", "performance_trace_manifest"])]
        performance_trace: Option<PathBuf>,
        #[arg(long, requires_all = ["performance_budget", "performance_report", "performance_trace"])]
        performance_trace_manifest: Option<PathBuf>,
        #[arg(long, default_value_t = 0, requires = "performance_budget")]
        performance_warmup_frames: u64,
        #[arg(long, default_value_t = 1, requires = "performance_budget")]
        performance_start_sequence: u64,
        #[arg(long, default_value_t = 8)]
        worker_limit: usize,
    },
    RunBatch {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    Serve {
        #[arg(long)]
        stdio: bool,
        #[arg(long)]
        gpu: bool,
        #[arg(long)]
        build_identity: PathBuf,
    },
    BootstrapTestEnv {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        build_identity: PathBuf,
    },
    PrepareProfile {
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        product_profile: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        namespace: String,
        #[arg(long, default_value_t = 800)]
        viewport_width: u32,
        #[arg(long, default_value_t = 600)]
        viewport_height: u32,
        #[arg(long, default_value = "disabled")]
        video_decode: String,
        #[arg(long, default_value_t = 512 * 1024 * 1024)]
        max_decode_output_bytes: u64,
        #[arg(long, default_value_t = 192 * 1024 * 1024)]
        max_decoded_cache_bytes: u64,
        #[arg(long, default_value_t = 256 * 1024 * 1024)]
        max_gpu_resource_bytes: u64,
        #[arg(long, default_value_t = 18_000)]
        max_video_frames: u64,
        #[arg(long)]
        manifest_only: bool,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        build_identity: PathBuf,
    },
    PerformanceE2 {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        package: PathBuf,
        #[arg(long)]
        budget: PathBuf,
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        trace_manifest: PathBuf,
        #[arg(long)]
        build_identity: PathBuf,
        #[arg(long, value_enum)]
        workload: performance_e2::PerformanceWorkload,
        #[arg(long, value_parser = clap::value_parser!(u8).range(1..=3))]
        run_index: u8,
    },
    PreparePerformanceBudget {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        budget_id: String,
        #[arg(long, default_value_t = 72_000)]
        min_samples: usize,
        #[arg(long, default_value_t = 72_000)]
        max_samples: usize,
        #[arg(long, default_value_t = 600_000_000)]
        min_run_duration_us: u64,
        #[arg(long, value_enum, default_value_t = performance_e2::PerformanceBudgetKind::RendererStress)]
        kind: performance_e2::PerformanceBudgetKind,
    },
    PreparePerformanceProfile {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum)]
        backend: performance_e2::PerformanceGpuBackend,
        #[arg(long, value_enum)]
        device_type: performance_e2::PerformanceGpuDeviceType,
        #[arg(long)]
        adapter_identity_hash: Option<String>,
    },
    PrepareProductPerformanceInput {
        #[arg(long)]
        prefix: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        stop_after_sequence: Option<u64>,
        #[arg(long, default_value_t = 1_200)]
        warmup_frames: u64,
        #[arg(long, default_value_t = 72_000)]
        measurement_frames: u64,
        #[arg(long)]
        viewport_width: u32,
        #[arg(long)]
        viewport_height: u32,
        #[arg(long)]
        point_a_x: u32,
        #[arg(long)]
        point_a_y: u32,
        #[arg(long)]
        point_b_x: u32,
        #[arg(long)]
        point_b_y: u32,
    },
}
