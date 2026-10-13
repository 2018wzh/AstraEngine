use std::{fs, path::Path, time::Instant};

use astra_plugin::WorkerBudgetBroker;
use tokio::task::JoinSet;

use crate::write_atomic_json;

use super::*;

pub(crate) async fn run(manifest_path: &Path, report_path: &Path) -> Result<(), String> {
    let manifest: BatchManifest = serde_json::from_slice(
        &fs::read(manifest_path)
            .map_err(|error| format!("ASTRA_HEADLESS_BATCH_MANIFEST_READ: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_HEADLESS_BATCH_MANIFEST_INVALID: {error}"))?;
    validate_manifest(&manifest)?;
    let available_parallelism = std::thread::available_parallelism()
        .map(usize::from)
        .map_err(|error| format!("ASTRA_HEADLESS_BATCH_PARALLELISM_QUERY: {error}"))?;
    let selected_concurrency = select_concurrency(
        manifest.worker_limit,
        manifest.jobs.len(),
        available_parallelism,
    )?;
    let concurrent_session_worker_limit =
        concurrent_session_worker_limit(manifest.worker_limit, selected_concurrency)?;
    let concurrent_worker_capacity = concurrent_session_worker_limit
        .checked_mul(selected_concurrency)
        .ok_or_else(|| "ASTRA_HEADLESS_BATCH_WORKER_CAPACITY_OVERFLOW".to_string())?;
    if concurrent_worker_capacity > manifest.worker_limit {
        return Err("ASTRA_HEADLESS_BATCH_WORKER_OVERSUBSCRIPTION".into());
    }
    tracing::info!(
        event = "headless.session_batch.start",
        session_count = manifest.jobs.len(),
        configured_worker_limit = manifest.worker_limit,
        available_parallelism,
        selected_concurrency,
        serial_session_worker_limit = manifest.worker_limit,
        concurrent_session_worker_limit,
        concurrent_worker_capacity,
        "starting identity-bound Headless session batch"
    );

    let budget = WorkerBudgetBroker::global_with_limit(selected_concurrency)
        .map_err(|error| error.to_string())?
        .clone();
    let executable = std::env::current_exe()
        .map_err(|error| format!("ASTRA_HEADLESS_BATCH_EXECUTABLE: {error}"))?;
    let serial_started = Instant::now();
    let mut serial_baselines = std::collections::BTreeMap::new();
    for job in &manifest.jobs {
        let executable = executable.clone();
        let baseline_job = job.clone();
        let run_started = Instant::now();
        let outcome = tokio::task::spawn_blocking(move || {
            run_child(
                &executable,
                &baseline_job,
                &baseline_job.serial_artifact_root,
                manifest.worker_limit,
            )
        })
        .await
        .map_err(|error| format!("ASTRA_HEADLESS_BATCH_BASELINE_WORKER_FAILED: {error}"))?;
        serial_baselines.insert(
            job.session_id.clone(),
            SerialBaseline {
                run_time_us: elapsed_us(run_started),
                outcome,
            },
        );
    }
    let serial_baseline_wall_time_us = elapsed_us(serial_started);
    let started = Instant::now();
    let mut tasks = JoinSet::new();
    for job in manifest.jobs {
        let session_budget = budget.clone();
        let executable = executable.clone();
        tasks.spawn(async move {
            let session_id = job.session_id.clone();
            let kind = job.kind;
            let queued = Instant::now();
            let lease = session_budget.acquire().await;
            let queue_time_us = elapsed_us(queued);
            let lease = match lease {
                Ok(lease) => lease,
                Err(error) => {
                    return BatchJobReport {
                        session_id: job.session_id,
                        kind,
                        status: "blocked",
                        queue_time_us,
                        run_time_us: 0,
                        serial_baseline_run_time_us: 0,
                        worker_limit: concurrent_session_worker_limit,
                        serial_worker_limit: manifest.worker_limit,
                        cpu_time_us: None,
                        serial_cpu_time_us: None,
                        cpu_utilization_permille: None,
                        serial_cpu_utilization_permille: None,
                        output_identity_hash: None,
                        serial_output_identity_hash: None,
                        frame_cpu_p95_ns: None,
                        frame_cpu_p99_ns: None,
                        frame_end_to_end_p95_ns: None,
                        frame_end_to_end_p99_ns: None,
                        peak_private_memory_bytes: None,
                        serial_frame_cpu_p95_ns: None,
                        serial_frame_cpu_p99_ns: None,
                        serial_frame_end_to_end_p95_ns: None,
                        serial_frame_end_to_end_p99_ns: None,
                        serial_peak_private_memory_bytes: None,
                        profile_hash: None,
                        package_hash: None,
                        input_hash: None,
                        build_identity_hash: None,
                        diagnostic: Some(error.code().to_string()),
                    };
                }
            };
            let run_started = Instant::now();
            tracing::info!(
                event = "headless.session_batch.session.start",
                session_id,
                queue_time_us,
                "starting queued Headless batch session"
            );
            let artifact_root = job.artifact_root.clone();
            let result = tokio::task::spawn_blocking(move || {
                run_child(
                    &executable,
                    &job,
                    &artifact_root,
                    concurrent_session_worker_limit,
                )
            })
            .await
            .map_err(|error| {
                format!("ASTRA_HEADLESS_BATCH_WORKER_FAILED: child worker failed: {error}")
            });
            drop(lease);
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(diagnostic) => BatchChildOutcome {
                    identity: None,
                    output_identity_hash: None,
                    performance: None,
                    peak_private_memory_bytes: None,
                    cpu_time_us: None,
                    diagnostic: Some(diagnostic),
                },
            };
            let identity = outcome.identity;
            let performance = outcome.performance;
            let peak_private_memory_bytes = outcome.peak_private_memory_bytes;
            let cpu_time_us = outcome.cpu_time_us;
            let output_identity_hash = outcome.output_identity_hash;
            let diagnostic = outcome.diagnostic;
            let run_time_us = elapsed_us(run_started);
            let report = BatchJobReport {
                session_id,
                kind,
                status: if diagnostic.is_none() {
                    "pass"
                } else {
                    "blocked"
                },
                queue_time_us,
                run_time_us,
                serial_baseline_run_time_us: 0,
                worker_limit: concurrent_session_worker_limit,
                serial_worker_limit: manifest.worker_limit,
                cpu_time_us,
                serial_cpu_time_us: None,
                cpu_utilization_permille: cpu_time_us.map(|cpu_time_us| {
                    utilization_permille(cpu_time_us, run_time_us, concurrent_session_worker_limit)
                }),
                serial_cpu_utilization_permille: None,
                output_identity_hash,
                serial_output_identity_hash: None,
                frame_cpu_p95_ns: performance.as_ref().map(|summary| summary.frame_cpu_p95_ns),
                frame_cpu_p99_ns: performance.as_ref().map(|summary| summary.frame_cpu_p99_ns),
                frame_end_to_end_p95_ns: performance
                    .as_ref()
                    .map(|summary| summary.frame_end_to_end_p95_ns),
                frame_end_to_end_p99_ns: performance
                    .as_ref()
                    .map(|summary| summary.frame_end_to_end_p99_ns),
                peak_private_memory_bytes,
                serial_frame_cpu_p95_ns: None,
                serial_frame_cpu_p99_ns: None,
                serial_frame_end_to_end_p95_ns: None,
                serial_frame_end_to_end_p99_ns: None,
                serial_peak_private_memory_bytes: None,
                profile_hash: identity.as_ref().map(|value| value.profile_hash.clone()),
                package_hash: identity.as_ref().map(|value| value.package_hash.clone()),
                input_hash: identity.as_ref().map(|value| value.input_hash.clone()),
                build_identity_hash: identity
                    .as_ref()
                    .map(|value| value.build_identity_hash.clone()),
                diagnostic,
            };
            tracing::info!(
                event = "headless.session_batch.session.complete",
                session_id = report.session_id,
                status = report.status,
                run_time_us = report.run_time_us,
                "completed Headless batch session"
            );
            report
        });
    }

    let mut jobs = Vec::new();
    while let Some(result) = tasks.join_next().await {
        jobs.push(result.map_err(|error| {
            format!("ASTRA_HEADLESS_BATCH_WORKER_FAILED: session worker failed: {error}")
        })?);
    }
    for job in &mut jobs {
        let Some(baseline) = serial_baselines.remove(&job.session_id) else {
            job.status = "blocked";
            job.diagnostic = Some("ASTRA_HEADLESS_BATCH_BASELINE_MISSING".into());
            continue;
        };
        job.serial_baseline_run_time_us = baseline.run_time_us;
        job.serial_output_identity_hash = baseline.outcome.output_identity_hash.clone();
        if let Some(performance) = baseline.outcome.performance.as_ref() {
            job.serial_frame_cpu_p95_ns = Some(performance.frame_cpu_p95_ns);
            job.serial_frame_cpu_p99_ns = Some(performance.frame_cpu_p99_ns);
            job.serial_frame_end_to_end_p95_ns = Some(performance.frame_end_to_end_p95_ns);
            job.serial_frame_end_to_end_p99_ns = Some(performance.frame_end_to_end_p99_ns);
        }
        job.serial_peak_private_memory_bytes = baseline.outcome.peak_private_memory_bytes;
        job.serial_cpu_time_us = baseline.outcome.cpu_time_us;
        job.serial_cpu_utilization_permille = baseline.outcome.cpu_time_us.map(|cpu_time_us| {
            utilization_permille(cpu_time_us, baseline.run_time_us, job.serial_worker_limit)
        });
        if let Some(diagnostic) = baseline.outcome.diagnostic {
            job.status = "blocked";
            job.diagnostic = Some(format!(
                "ASTRA_HEADLESS_BATCH_BASELINE_BLOCKED:{diagnostic}"
            ));
        } else if baseline.outcome.output_identity_hash != job.output_identity_hash {
            job.status = "blocked";
            job.diagnostic = Some("ASTRA_HEADLESS_BATCH_OUTPUT_MISMATCH".into());
        }
    }
    jobs.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    let passed = jobs.iter().all(|job| job.status == "pass");
    let wall_time_us = elapsed_us(started);
    let total_queue_time_us = jobs
        .iter()
        .fold(0_u64, |total, job| total.saturating_add(job.queue_time_us));
    let total_run_time_us = jobs
        .iter()
        .fold(0_u64, |total, job| total.saturating_add(job.run_time_us));
    let total_cpu_time_us = jobs.iter().try_fold(0_u64, |total, job| {
        job.cpu_time_us
            .and_then(|cpu_time_us| total.checked_add(cpu_time_us))
            .ok_or_else(|| "ASTRA_HEADLESS_BATCH_CPU_TIME_MISSING_OR_OVERFLOW".to_string())
    })?;
    let throughput_milli_sessions_per_second = if wall_time_us == 0 {
        0
    } else {
        (jobs.len() as u64)
            .saturating_mul(1_000_000_000)
            .saturating_div(wall_time_us)
    };
    let session_slot_utilization_permille =
        utilization_permille(total_run_time_us, wall_time_us, budget.limit());
    let worker_utilization_permille =
        utilization_permille(total_cpu_time_us, wall_time_us, manifest.worker_limit);
    let report = BatchReport {
        schema: REPORT_SCHEMA,
        status: if passed { "pass" } else { "blocked" },
        configured_worker_limit: manifest.worker_limit,
        available_parallelism,
        selected_concurrency,
        serial_session_worker_limit: manifest.worker_limit,
        concurrent_session_worker_limit,
        concurrent_worker_capacity,
        worker_limit: budget.limit(),
        peak_workers: budget.peak_acquired(),
        wall_time_us,
        total_queue_time_us,
        throughput_milli_sessions_per_second,
        session_slot_utilization_permille,
        worker_utilization_permille,
        serial_baseline_wall_time_us,
        jobs,
    };
    write_atomic_json(report_path, &report)
        .map_err(|error| format!("ASTRA_HEADLESS_BATCH_REPORT_WRITE: {error}"))?;
    tracing::info!(
        event = "headless.session_batch.complete",
        status = report.status,
        session_count = report.jobs.len(),
        configured_worker_limit = report.configured_worker_limit,
        available_parallelism = report.available_parallelism,
        selected_concurrency = report.selected_concurrency,
        serial_session_worker_limit = report.serial_session_worker_limit,
        concurrent_session_worker_limit = report.concurrent_session_worker_limit,
        concurrent_worker_capacity = report.concurrent_worker_capacity,
        peak_workers = report.peak_workers,
        wall_time_us = report.wall_time_us,
        session_slot_utilization_permille = report.session_slot_utilization_permille,
        worker_utilization_permille = report.worker_utilization_permille,
        "completed identity-bound Headless session batch"
    );
    if passed {
        Ok(())
    } else {
        Err("ASTRA_HEADLESS_BATCH_BLOCKED: one or more sessions failed".into())
    }
}
