use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use astra_core::{Hash256, PerformanceReport};
use astra_headless_protocol::RunReport;
use astra_observability::{sample_process_cpu_time_us_by_pid, sample_process_memory_by_pid};
use sha2::{Digest, Sha256};

use super::*;

pub(crate) fn run_child(
    executable: &Path,
    job: &BatchJob,
    artifact_root: &Path,
    worker_limit: usize,
) -> BatchChildOutcome {
    let identity = (|| {
        Ok::<_, String>(BatchJobIdentity {
            profile_hash: sha256_file(&job.profile)?,
            package_hash: sha256_file(&job.package)?,
            input_hash: sha256_file(&job.input)?,
            build_identity_hash: sha256_file(&job.build_identity)?,
        })
    })();
    let identity = match identity {
        Ok(identity) => identity,
        Err(diagnostic) => {
            return BatchChildOutcome {
                identity: None,
                output_identity_hash: None,
                performance: None,
                peak_private_memory_bytes: None,
                cpu_time_us: None,
                diagnostic: Some(diagnostic),
            };
        }
    };
    let mut command = Command::new(executable);
    command
        .arg("run")
        .arg("--profile")
        .arg(&job.profile)
        .arg("--package")
        .arg(&job.package)
        .arg("--input")
        .arg(&job.input)
        .arg("--artifact-root")
        .arg(artifact_root)
        .arg("--build-identity")
        .arg(&job.build_identity)
        .arg("--worker-limit")
        .arg(worker_limit.to_string());
    if let Some(performance) = &job.performance {
        command
            .arg("--performance-budget")
            .arg(&performance.budget)
            .arg("--performance-report")
            .arg(artifact_root.join("performance-report.json"))
            .arg("--performance-trace")
            .arg(artifact_root.join("performance-trace.pftrace"))
            .arg("--performance-trace-manifest")
            .arg(artifact_root.join("performance-trace-manifest.json"))
            .arg("--performance-warmup-frames")
            .arg(performance.warmup_frames.to_string())
            .arg("--performance-start-sequence")
            .arg(performance.start_sequence.to_string());
    }
    if job.gpu {
        command.arg("--gpu");
    }
    if let Some(checkpoint_config) = &job.checkpoint_config {
        command.arg("--checkpoint-config").arg(checkpoint_config);
    }
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let child_run = run_child_with_timeout(&mut command, job.timeout_ms);
    let (output_identity_hash, performance, peak_private_memory_bytes, cpu_time_us, diagnostic) =
        match child_run {
            Ok(observation) => {
                match read_child_evidence(artifact_root, job.performance.is_some()) {
                    Ok((output_identity_hash, performance)) => {
                        let reported_peak = performance
                            .as_ref()
                            .and_then(|summary| summary.reported_peak_private_memory_bytes);
                        (
                            Some(output_identity_hash),
                            performance,
                            Some(
                                reported_peak
                                    .unwrap_or(0)
                                    .max(observation.peak_private_memory_bytes),
                            ),
                            Some(observation.cpu_time_us),
                            None,
                        )
                    }
                    Err(error) => (None, None, None, None, Some(error)),
                }
            }
            Err(diagnostic) => (None, None, None, None, Some(diagnostic)),
        };
    BatchChildOutcome {
        identity: Some(identity),
        output_identity_hash,
        performance,
        peak_private_memory_bytes,
        cpu_time_us,
        diagnostic,
    }
}

pub(crate) fn run_child_with_timeout(
    command: &mut Command,
    timeout_ms: u64,
) -> Result<ChildProcessObservation, String> {
    let mut child = command
        .spawn()
        .map_err(|_| "ASTRA_HEADLESS_BATCH_CHILD_START".to_string())?;
    let process_id = child.id();
    let started = Instant::now();
    let mut peak_private_memory_bytes = 0_u64;
    let mut cpu_time_us = 0_u64;
    loop {
        match child.try_wait() {
            Ok(Some(status))
                if status.success() && peak_private_memory_bytes > 0 && cpu_time_us > 0 =>
            {
                return Ok(ChildProcessObservation {
                    peak_private_memory_bytes,
                    cpu_time_us,
                });
            }
            Ok(Some(status)) if status.success() && peak_private_memory_bytes == 0 => {
                return Err("ASTRA_HEADLESS_BATCH_CHILD_MEMORY_EMPTY".to_string());
            }
            Ok(Some(status)) if status.success() => {
                return Err("ASTRA_HEADLESS_BATCH_CHILD_CPU_TIME_EMPTY".to_string());
            }
            Ok(Some(_)) => return Err("ASTRA_HEADLESS_BATCH_CHILD_FAILED".to_string()),
            Ok(None) if started.elapsed() < Duration::from_millis(timeout_ms) => {}
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ASTRA_HEADLESS_BATCH_CHILD_TIMEOUT".to_string());
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ASTRA_HEADLESS_BATCH_CHILD_WAIT".to_string());
            }
        }
        match sample_process_memory_by_pid(process_id) {
            Ok(memory) => {
                peak_private_memory_bytes = peak_private_memory_bytes.max(memory.private_bytes);
            }
            Err(error) => match child.try_wait() {
                Ok(Some(status))
                    if status.success() && peak_private_memory_bytes > 0 && cpu_time_us > 0 =>
                {
                    return Ok(ChildProcessObservation {
                        peak_private_memory_bytes,
                        cpu_time_us,
                    });
                }
                Ok(Some(_)) => return Err("ASTRA_HEADLESS_BATCH_CHILD_FAILED".to_string()),
                _ => return Err(format!("ASTRA_HEADLESS_BATCH_CHILD_MEMORY: {error}")),
            },
        }
        match sample_process_cpu_time_us_by_pid(process_id) {
            Ok(observed_cpu_time_us) => {
                cpu_time_us = cpu_time_us.max(observed_cpu_time_us);
            }
            Err(error) => match child.try_wait() {
                Ok(Some(status))
                    if status.success() && peak_private_memory_bytes > 0 && cpu_time_us > 0 =>
                {
                    return Ok(ChildProcessObservation {
                        peak_private_memory_bytes,
                        cpu_time_us,
                    });
                }
                Ok(Some(_)) => return Err("ASTRA_HEADLESS_BATCH_CHILD_FAILED".to_string()),
                _ => return Err(format!("ASTRA_HEADLESS_BATCH_CHILD_CPU_TIME: {error}")),
            },
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn read_child_evidence(
    artifact_root: &Path,
    expect_performance: bool,
) -> Result<(String, Option<BatchPerformanceSummary>), String> {
    let run_report_path = artifact_root.join("run-report.json");
    let report: RunReport = serde_json::from_slice(
        &fs::read(&run_report_path)
            .map_err(|error| format!("ASTRA_HEADLESS_BATCH_RUN_REPORT_READ: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_HEADLESS_BATCH_RUN_REPORT_INVALID: {error}"))?;
    report
        .validate()
        .map_err(|error| format!("ASTRA_HEADLESS_BATCH_RUN_REPORT_INVALID: {error}"))?;
    let output_bytes = serde_json::to_vec(&(
        &report.build_fingerprint,
        &report.package_hash,
        &report.input_sequence_hash,
        &report.profile_id,
        &report.content_identity,
        &report.status,
        report.submitted_frame_count,
        report.rasterized_frame_count,
        report.audio_frame_count,
        report.completed_sequence,
        &report.checkpoint_results,
    ))
    .map_err(|error| format!("ASTRA_HEADLESS_BATCH_OUTPUT_IDENTITY: {error}"))?;
    let output_identity_hash = format!("sha256:{:x}", Sha256::digest(output_bytes));

    if !expect_performance {
        return Ok((output_identity_hash, None));
    }
    let performance: PerformanceReport = serde_json::from_slice(
        &fs::read(artifact_root.join("performance-report.json"))
            .map_err(|error| format!("ASTRA_HEADLESS_BATCH_PERFORMANCE_READ: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_HEADLESS_BATCH_PERFORMANCE_INVALID: {error}"))?;
    let metric = |id: &str| {
        performance
            .metrics
            .iter()
            .find(|metric| metric.id == id)
            .ok_or_else(|| format!("ASTRA_HEADLESS_BATCH_PERFORMANCE_METRIC_MISSING:{id}"))
    };
    let cpu = metric("frame.cpu_ns")?;
    let end_to_end = metric("frame.end_to_end_ns")?;
    let private = metric("memory.private_bytes")?;
    Ok((
        output_identity_hash,
        Some(BatchPerformanceSummary {
            frame_cpu_p95_ns: cpu.p95,
            frame_cpu_p99_ns: cpu.p99,
            frame_end_to_end_p95_ns: end_to_end.p95,
            frame_end_to_end_p99_ns: end_to_end.p99,
            reported_peak_private_memory_bytes: Some(private.max),
        }),
    ))
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    Hash256::from_sha256_file(path)
        .map(|hash| hash.to_string())
        .map_err(|error| format!("ASTRA_HEADLESS_BATCH_IDENTITY_READ: {error}"))
}

pub(crate) fn validate_manifest(manifest: &BatchManifest) -> Result<(), String> {
    if manifest.schema != MANIFEST_SCHEMA {
        return Err("ASTRA_HEADLESS_BATCH_SCHEMA: unsupported batch manifest schema".into());
    }
    if !(1..=MAX_PARALLEL_SESSIONS).contains(&manifest.worker_limit) {
        return Err("ASTRA_HEADLESS_BATCH_WORKER_LIMIT: worker_limit must be within 1..=8".into());
    }
    if manifest.jobs.is_empty() {
        return Err("ASTRA_HEADLESS_BATCH_EMPTY: batch must contain at least one job".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    let mut artifact_roots = std::collections::BTreeSet::new();
    for job in &manifest.jobs {
        if job.session_id.is_empty()
            || job.session_id.len() > 128
            || !job
                .session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err("ASTRA_HEADLESS_BATCH_SESSION_ID: session id is invalid".into());
        }
        if !ids.insert(job.session_id.as_str()) {
            return Err("ASTRA_HEADLESS_BATCH_SESSION_DUPLICATE: session id is duplicated".into());
        }
        match (job.kind, job.performance.as_ref()) {
            (BatchJobKind::Performance, Some(performance)) if performance.start_sequence > 0 => {}
            (BatchJobKind::Performance, Some(_)) => {
                return Err(
                    "ASTRA_HEADLESS_BATCH_PERFORMANCE_START: performance start_sequence must be non-zero"
                        .into(),
                );
            }
            (BatchJobKind::Performance, None) => {
                return Err(
                    "ASTRA_HEADLESS_BATCH_PERFORMANCE_REQUIRED: performance jobs require a performance configuration"
                        .into(),
                );
            }
            (_, Some(_)) => {
                return Err(
                    "ASTRA_HEADLESS_BATCH_PERFORMANCE_UNEXPECTED: route and replay jobs cannot declare a performance configuration"
                        .into(),
                );
            }
            (_, None) => {}
        }
        if !(100..=86_400_000).contains(&job.timeout_ms) {
            return Err(
                "ASTRA_HEADLESS_BATCH_TIMEOUT: timeout_ms must be within 100..=86400000".into(),
            );
        }
        if job.artifact_root == job.serial_artifact_root
            || !artifact_roots.insert(job.artifact_root.as_path())
            || !artifact_roots.insert(job.serial_artifact_root.as_path())
        {
            return Err(
                "ASTRA_HEADLESS_BATCH_ARTIFACT_ROOT: serial and concurrent artifact roots must be unique"
                    .into(),
            );
        }
    }
    Ok(())
}

pub(crate) fn select_concurrency(
    configured_worker_limit: usize,
    job_count: usize,
    available_parallelism: usize,
) -> Result<usize, String> {
    if !(1..=MAX_PARALLEL_SESSIONS).contains(&configured_worker_limit) {
        return Err("ASTRA_HEADLESS_BATCH_WORKER_LIMIT: worker_limit must be within 1..=8".into());
    }
    if job_count == 0 {
        return Err("ASTRA_HEADLESS_BATCH_EMPTY: batch must contain at least one job".into());
    }
    if available_parallelism == 0 {
        return Err(
            "ASTRA_HEADLESS_BATCH_PARALLELISM_QUERY: available parallelism was zero".into(),
        );
    }
    Ok(configured_worker_limit
        .min(job_count)
        .min(available_parallelism))
}

pub(crate) fn concurrent_session_worker_limit(
    configured_worker_limit: usize,
    selected_concurrency: usize,
) -> Result<usize, String> {
    if !(1..=MAX_PARALLEL_SESSIONS).contains(&configured_worker_limit) {
        return Err("ASTRA_HEADLESS_BATCH_WORKER_LIMIT: worker_limit must be within 1..=8".into());
    }
    if selected_concurrency == 0 || selected_concurrency > configured_worker_limit {
        return Err(
            "ASTRA_HEADLESS_BATCH_SELECTED_CONCURRENCY: selected concurrency exceeds the worker budget"
                .into(),
        );
    }
    Ok(configured_worker_limit / selected_concurrency)
}

pub(crate) fn utilization_permille(
    cpu_or_active_time_us: u64,
    wall_time_us: u64,
    capacity: usize,
) -> u16 {
    let denominator = wall_time_us.saturating_mul(capacity as u64);
    if denominator == 0 {
        return 0;
    }
    cpu_or_active_time_us
        .saturating_mul(1_000)
        .saturating_div(denominator)
        .min(1_000) as u16
}

pub(crate) fn elapsed_us(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}
