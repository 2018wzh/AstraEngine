use std::{
    fs,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use astra_core::PerformanceReport;
use astra_headless_protocol::RunReport;
use astra_observability::sample_process_memory_by_pid;
use astra_plugin::WorkerBudgetBroker;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::task::JoinSet;

use super::write_atomic_json;

const MANIFEST_SCHEMA: &str = "astra.headless_session_batch.v2";
const REPORT_SCHEMA: &str = "astra.headless_session_batch_report.v2";
const MAX_PARALLEL_SESSIONS: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchManifest {
    schema: String,
    worker_limit: usize,
    jobs: Vec<BatchJob>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchJob {
    session_id: String,
    kind: BatchJobKind,
    gpu: bool,
    profile: PathBuf,
    package: PathBuf,
    input: PathBuf,
    artifact_root: PathBuf,
    serial_artifact_root: PathBuf,
    checkpoint_config: Option<PathBuf>,
    build_identity: PathBuf,
    performance: Option<BatchPerformanceConfig>,
    timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum BatchJobKind {
    Route,
    Replay,
    Performance,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchPerformanceConfig {
    budget: PathBuf,
    warmup_frames: u64,
    start_sequence: u64,
}

#[derive(Debug, Serialize)]
struct BatchReport {
    schema: &'static str,
    status: &'static str,
    configured_worker_limit: usize,
    available_parallelism: usize,
    selected_concurrency: usize,
    worker_limit: usize,
    peak_workers: usize,
    wall_time_us: u64,
    total_queue_time_us: u64,
    throughput_milli_sessions_per_second: u64,
    worker_utilization_permille: u16,
    serial_baseline_wall_time_us: u64,
    jobs: Vec<BatchJobReport>,
}

#[derive(Debug, Serialize)]
struct BatchJobReport {
    session_id: String,
    kind: BatchJobKind,
    status: &'static str,
    queue_time_us: u64,
    run_time_us: u64,
    serial_baseline_run_time_us: u64,
    output_identity_hash: Option<String>,
    serial_output_identity_hash: Option<String>,
    frame_cpu_p95_ns: Option<u64>,
    frame_cpu_p99_ns: Option<u64>,
    frame_end_to_end_p95_ns: Option<u64>,
    frame_end_to_end_p99_ns: Option<u64>,
    peak_private_memory_bytes: Option<u64>,
    serial_frame_cpu_p95_ns: Option<u64>,
    serial_frame_cpu_p99_ns: Option<u64>,
    serial_frame_end_to_end_p95_ns: Option<u64>,
    serial_frame_end_to_end_p99_ns: Option<u64>,
    serial_peak_private_memory_bytes: Option<u64>,
    profile_hash: Option<String>,
    package_hash: Option<String>,
    input_hash: Option<String>,
    build_identity_hash: Option<String>,
    diagnostic: Option<String>,
}

struct BatchJobIdentity {
    profile_hash: String,
    package_hash: String,
    input_hash: String,
    build_identity_hash: String,
}

struct BatchChildOutcome {
    identity: Option<BatchJobIdentity>,
    output_identity_hash: Option<String>,
    performance: Option<BatchPerformanceSummary>,
    peak_private_memory_bytes: Option<u64>,
    diagnostic: Option<String>,
}

#[derive(Clone)]
struct BatchPerformanceSummary {
    frame_cpu_p95_ns: u64,
    frame_cpu_p99_ns: u64,
    frame_end_to_end_p95_ns: u64,
    frame_end_to_end_p99_ns: u64,
    reported_peak_private_memory_bytes: Option<u64>,
}

struct SerialBaseline {
    run_time_us: u64,
    outcome: BatchChildOutcome,
}

pub(super) async fn run(manifest_path: &Path, report_path: &Path) -> Result<(), String> {
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
    tracing::info!(
        event = "headless.session_batch.start",
        session_count = manifest.jobs.len(),
        configured_worker_limit = manifest.worker_limit,
        available_parallelism,
        selected_concurrency,
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
            let result =
                tokio::task::spawn_blocking(move || run_child(&executable, &job, &artifact_root))
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
                    diagnostic: Some(diagnostic),
                },
            };
            let identity = outcome.identity;
            let performance = outcome.performance;
            let peak_private_memory_bytes = outcome.peak_private_memory_bytes;
            let output_identity_hash = outcome.output_identity_hash;
            let diagnostic = outcome.diagnostic;
            let report = BatchJobReport {
                session_id,
                kind,
                status: if diagnostic.is_none() {
                    "pass"
                } else {
                    "blocked"
                },
                queue_time_us,
                run_time_us: elapsed_us(run_started),
                serial_baseline_run_time_us: 0,
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
    let throughput_milli_sessions_per_second = if wall_time_us == 0 {
        0
    } else {
        (jobs.len() as u64)
            .saturating_mul(1_000_000_000)
            .saturating_div(wall_time_us)
    };
    let utilization_denominator = wall_time_us.saturating_mul(budget.limit() as u64);
    let worker_utilization_permille = if utilization_denominator == 0 {
        0
    } else {
        total_run_time_us
            .saturating_mul(1_000)
            .saturating_div(utilization_denominator)
            .min(1_000) as u16
    };
    let report = BatchReport {
        schema: REPORT_SCHEMA,
        status: if passed { "pass" } else { "blocked" },
        configured_worker_limit: manifest.worker_limit,
        available_parallelism,
        selected_concurrency,
        worker_limit: budget.limit(),
        peak_workers: budget.peak_acquired(),
        wall_time_us,
        total_queue_time_us,
        throughput_milli_sessions_per_second,
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
        peak_workers = report.peak_workers,
        wall_time_us = report.wall_time_us,
        "completed identity-bound Headless session batch"
    );
    if passed {
        Ok(())
    } else {
        Err("ASTRA_HEADLESS_BATCH_BLOCKED: one or more sessions failed".into())
    }
}

fn run_child(executable: &Path, job: &BatchJob, artifact_root: &Path) -> BatchChildOutcome {
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
        .arg("1");
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
    let (output_identity_hash, performance, peak_private_memory_bytes, diagnostic) = match child_run
    {
        Ok(observed_peak_private_memory_bytes) => {
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
                                .max(observed_peak_private_memory_bytes),
                        ),
                        None,
                    )
                }
                Err(error) => (None, None, None, Some(error)),
            }
        }
        Err(diagnostic) => (None, None, None, Some(diagnostic)),
    };
    BatchChildOutcome {
        identity: Some(identity),
        output_identity_hash,
        performance,
        peak_private_memory_bytes,
        diagnostic,
    }
}

fn run_child_with_timeout(command: &mut Command, timeout_ms: u64) -> Result<u64, String> {
    let mut child = command
        .spawn()
        .map_err(|_| "ASTRA_HEADLESS_BATCH_CHILD_START".to_string())?;
    let process_id = child.id();
    let started = Instant::now();
    let mut peak_private_memory_bytes = 0_u64;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() && peak_private_memory_bytes > 0 => {
                return Ok(peak_private_memory_bytes);
            }
            Ok(Some(status)) if status.success() => {
                return Err("ASTRA_HEADLESS_BATCH_CHILD_MEMORY_EMPTY".to_string());
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
                Ok(Some(status)) if status.success() && peak_private_memory_bytes > 0 => {
                    return Ok(peak_private_memory_bytes);
                }
                Ok(Some(_)) => return Err("ASTRA_HEADLESS_BATCH_CHILD_FAILED".to_string()),
                _ => return Err(format!("ASTRA_HEADLESS_BATCH_CHILD_MEMORY: {error}")),
            },
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_child_evidence(
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
        &report.submitted_scene_stream_hash,
        &report.rasterized_frame_stream_hash,
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

fn sha256_file(path: &Path) -> Result<String, String> {
    let file = fs::File::open(path)
        .map_err(|error| format!("ASTRA_HEADLESS_BATCH_IDENTITY_READ: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("ASTRA_HEADLESS_BATCH_IDENTITY_READ: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn validate_manifest(manifest: &BatchManifest) -> Result<(), String> {
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

fn select_concurrency(
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

fn elapsed_us(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(session_id: &str, root: &str) -> BatchJob {
        BatchJob {
            session_id: session_id.to_string(),
            kind: BatchJobKind::Route,
            gpu: false,
            profile: "profile.json".into(),
            package: "game.astrapak".into(),
            input: "input.jsonl".into(),
            artifact_root: format!("{root}/concurrent").into(),
            serial_artifact_root: format!("{root}/serial").into(),
            checkpoint_config: None,
            build_identity: "build-identity.json".into(),
            performance: None,
            timeout_ms: 1_000,
        }
    }

    #[astra_headless_test::test]
    fn batch_manifest_requires_unique_session_and_artifact_identity() {
        let mut manifest = BatchManifest {
            schema: MANIFEST_SCHEMA.to_string(),
            worker_limit: 2,
            jobs: vec![job("route.a", "a"), job("route.b", "b")],
        };
        validate_manifest(&manifest).unwrap();

        manifest.jobs[1].session_id = "route.a".into();
        assert!(validate_manifest(&manifest)
            .unwrap_err()
            .starts_with("ASTRA_HEADLESS_BATCH_SESSION_DUPLICATE"));
        manifest.jobs[1].session_id = "route.b".into();
        manifest.jobs[1].artifact_root = manifest.jobs[0].artifact_root.clone();
        assert!(validate_manifest(&manifest)
            .unwrap_err()
            .starts_with("ASTRA_HEADLESS_BATCH_ARTIFACT_ROOT"));
    }

    #[astra_headless_test::test]
    fn batch_manifest_bounds_global_workers_and_child_timeout() {
        let mut manifest = BatchManifest {
            schema: MANIFEST_SCHEMA.to_string(),
            worker_limit: 9,
            jobs: vec![job("route.a", "a")],
        };
        assert!(validate_manifest(&manifest)
            .unwrap_err()
            .starts_with("ASTRA_HEADLESS_BATCH_WORKER_LIMIT"));
        manifest.worker_limit = 1;
        manifest.jobs[0].timeout_ms = 99;
        assert!(validate_manifest(&manifest)
            .unwrap_err()
            .starts_with("ASTRA_HEADLESS_BATCH_TIMEOUT"));
    }

    #[astra_headless_test::test]
    fn batch_selects_concurrency_from_cap_jobs_and_hardware() {
        assert_eq!(select_concurrency(8, 12, 6).unwrap(), 6);
        assert_eq!(select_concurrency(8, 3, 16).unwrap(), 3);
        assert_eq!(select_concurrency(2, 12, 16).unwrap(), 2);
        assert!(select_concurrency(8, 0, 16)
            .unwrap_err()
            .starts_with("ASTRA_HEADLESS_BATCH_EMPTY"));
        assert!(select_concurrency(8, 1, 0)
            .unwrap_err()
            .starts_with("ASTRA_HEADLESS_BATCH_PARALLELISM_QUERY"));
    }
}
