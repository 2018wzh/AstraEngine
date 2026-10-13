use clap::Parser;

mod compare;
mod performance_e2;
mod product_performance;
mod product_performance_input;
mod session_batch;

#[global_allocator]
static ASTRA_ALLOCATOR: astra_observability::TrackingAllocator =
    astra_observability::TrackingAllocator::new();

mod args;
mod checkpoint;
mod identity;
mod input;
mod product;
mod run;
mod serve;

pub(crate) use args::*;
pub(crate) use checkpoint::*;
pub(crate) use identity::*;
pub(crate) use input::*;
pub(crate) use product::*;
pub(crate) use run::*;
pub(crate) use serve::*;

#[tokio::main(flavor = "current_thread")]
pub(crate) async fn main() {
    let filter = std::env::var("ASTRA_LOG").unwrap_or_else(|_| "info".into());
    let observability = match astra_observability::init_host(
        astra_observability::HostObservabilityConfig::for_cli(filter),
    ) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("ASTRA_HEADLESS_OBSERVABILITY_INIT_FAILED: {error}");
            std::process::exit(2);
        }
    };
    let result = match Args::parse().command {
        Command::ValidateInput { input } => validate_input(&input),
        Command::Serve {
            stdio,
            gpu,
            build_identity,
        } => {
            if !stdio {
                Err("ASTRA_HEADLESS_TRANSPORT_REQUIRED: serve requires --stdio".into())
            } else {
                serve(&build_identity, gpu).await
            }
        }
        Command::Run {
            gpu,
            profile,
            package,
            input,
            artifact_root,
            checkpoint_config,
            build_identity,
            source_profile,
            source_root,
            performance_budget,
            performance_report,
            performance_trace,
            performance_trace_manifest,
            performance_warmup_frames,
            performance_start_sequence,
            worker_limit,
        } => match astra_plugin::WorkerBudgetBroker::global_with_limit(worker_limit) {
            Ok(_) => {
                run(RunRequest {
                    profile_path: &profile,
                    package_path: &package,
                    input_path: &input,
                    artifact_root: &artifact_root,
                    checkpoint_config: checkpoint_config.as_deref(),
                    build_identity: &build_identity,
                    gpu,
                    source_profile: source_profile.as_deref(),
                    source_root: source_root.as_deref(),
                    performance_budget: performance_budget.as_deref(),
                    performance_report: performance_report.as_deref(),
                    performance_trace: performance_trace.as_deref(),
                    performance_trace_manifest: performance_trace_manifest.as_deref(),
                    performance_warmup_frames,
                    performance_start_sequence,
                })
                .await
            }
            Err(error) => Err(error.to_string()),
        },
        Command::RunBatch { manifest, report } => session_batch::run(&manifest, &report).await,
        Command::BootstrapTestEnv {
            output,
            build_identity,
        } => bootstrap_test_env(&output, &build_identity),
        Command::PrepareProfile {
            package,
            target,
            product_profile,
            id,
            namespace,
            viewport_width,
            viewport_height,
            video_decode,
            max_decode_output_bytes,
            max_decoded_cache_bytes,
            max_gpu_resource_bytes,
            max_video_frames,
            manifest_only,
            output,
            build_identity,
        } => prepare_product_profile(
            &package,
            &target,
            &product_profile,
            &id,
            &namespace,
            viewport_width,
            viewport_height,
            &video_decode,
            max_decode_output_bytes,
            max_decoded_cache_bytes,
            max_gpu_resource_bytes,
            max_video_frames,
            manifest_only,
            &output,
            &build_identity,
        ),
        Command::PerformanceE2 {
            profile,
            package,
            budget,
            report,
            trace,
            trace_manifest,
            build_identity,
            workload,
            run_index,
        } => {
            performance_e2::run(performance_e2::PerformanceE2Request {
                profile: &profile,
                package: &package,
                budget: &budget,
                report: &report,
                trace: &trace,
                trace_manifest: &trace_manifest,
                build_identity: &build_identity,
                workload,
                run_index,
            })
            .await
        }
        Command::PreparePerformanceBudget {
            profile,
            output,
            budget_id,
            min_samples,
            max_samples,
            min_run_duration_us,
            kind,
        } => performance_e2::prepare_budget(
            &profile,
            &output,
            budget_id,
            min_samples,
            max_samples,
            min_run_duration_us,
            kind,
        ),
        Command::PreparePerformanceProfile {
            input,
            output,
            backend,
            device_type,
            adapter_identity_hash,
        } => performance_e2::prepare_profile(
            &input,
            &output,
            backend,
            device_type,
            adapter_identity_hash,
        ),
        Command::PrepareProductPerformanceInput {
            prefix,
            output,
            stop_after_sequence,
            warmup_frames,
            measurement_frames,
            viewport_width,
            viewport_height,
            point_a_x,
            point_a_y,
            point_b_x,
            point_b_y,
        } => product_performance_input::prepare(
            product_performance_input::ProductPerformanceInputRequest {
                prefix: &prefix,
                output: &output,
                stop_after_sequence,
                warmup_frames,
                measurement_frames,
                viewport_width,
                viewport_height,
                point_a: (point_a_x, point_a_y),
                point_b: (point_b_x, point_b_y),
            },
        ),
    };
    if let Err(error) = result {
        tracing::error!(
            event = "astra.headless.command.failed",
            diagnostic = %diagnostic_code(&error),
            "Headless command failed"
        );
        eprintln!("{error}");
        let _ = observability.flush();
        std::process::exit(2);
    }
    let _ = observability.flush();
}

#[cfg(test)]
mod input_clock_tests {
    use super::consumed_input_tick;
    use astra_headless_protocol::{ObservationPredicate, PhysicalInput};

    #[test]
    fn consumed_tick_accounts_for_polling_and_rejects_overflow() {
        let await_input = PhysicalInput::Await {
            observation: ObservationPredicate::Exists {
                key: "vn.text_reveal_complete".into(),
            },
            timeout_ticks: 30,
            continue_at_match: false,
        };
        assert_eq!(consumed_input_tick(10, &await_input, 0).unwrap(), 10);
        assert_eq!(consumed_input_tick(10, &await_input, 7).unwrap(), 17);
        assert_eq!(
            consumed_input_tick(u64::MAX, &await_input, 1).unwrap_err(),
            "ASTRA_HEADLESS_CONSUMED_TICK_OVERFLOW"
        );
        assert_eq!(
            consumed_input_tick(u64::MAX, &PhysicalInput::AdvanceTicks { count: 2 }, 0)
                .unwrap_err(),
            "ASTRA_HEADLESS_CONSUMED_TICK_OVERFLOW"
        );
        assert_eq!(
            consumed_input_tick(0, &PhysicalInput::AdvanceTicks { count: 0 }, 0).unwrap_err(),
            "ASTRA_HEADLESS_ADVANCE_TICKS_ZERO"
        );
    }
}
