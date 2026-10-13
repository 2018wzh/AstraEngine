use std::{collections::BTreeMap, fs, io::BufReader, path::Path, sync::Arc, time::Instant};

use astra_headless_protocol::{
    ArtifactManifest, JsonlReader, PhysicalInput, RunReport, RunStatus, SequenceValidator,
    HEADLESS_RUN_REPORT_SCHEMA, TICK_DURATION_NS,
};
use astra_platform::{PackageSourceRequest, PlatformHostFactory};
use astra_platform_headless::{HeadlessPlatformFactory, HeadlessThreadOwner};

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct RunRequest<'a> {
    pub(crate) profile_path: &'a Path,
    pub(crate) package_path: &'a Path,
    pub(crate) input_path: &'a Path,
    pub(crate) artifact_root: &'a Path,
    pub(crate) checkpoint_config: Option<&'a Path>,
    pub(crate) build_identity: &'a Path,
    pub(crate) gpu: bool,
    pub(crate) source_profile: Option<&'a Path>,
    pub(crate) source_root: Option<&'a Path>,
    pub(crate) performance_budget: Option<&'a Path>,
    pub(crate) performance_report: Option<&'a Path>,
    pub(crate) performance_trace: Option<&'a Path>,
    pub(crate) performance_trace_manifest: Option<&'a Path>,
    pub(crate) performance_warmup_frames: u64,
    pub(crate) performance_start_sequence: u64,
}

pub(crate) struct ValidatedInput {
    pub(crate) messages: Vec<astra_headless_protocol::InputMessage>,
    pub(crate) input_hash: String,
}

pub(crate) fn read_and_validate_input(
    input_path: &Path,
    limits: Option<(u64, u64)>,
) -> Result<ValidatedInput, String> {
    let input_bytes =
        fs::read(input_path).map_err(|e| format!("ASTRA_HEADLESS_INPUT_OPEN_FAILED: {e}"))?;
    let mut reader = JsonlReader::new(BufReader::new(input_bytes.as_slice()), 1024 * 1024)
        .map_err(|e| e.to_string())?;
    let mut sequence = SequenceValidator::default();
    let mut messages = Vec::new();
    while let Some(message) = reader
        .read::<astra_headless_protocol::InputMessage>()
        .map_err(|e| e.to_string())?
    {
        message.validate().map_err(|e| e.to_string())?;
        sequence
            .accept(&message.session, message.sequence, message.tick)
            .map_err(|e| e.to_string())?;
        if let Some((max_tick, max_messages)) = limits {
            if message.tick > max_tick {
                return Err("ASTRA_HEADLESS_INPUT_TICK_LIMIT_EXCEEDED".into());
            }
            if messages.len() as u64 >= max_messages {
                return Err("ASTRA_HEADLESS_INPUT_LIMIT_EXCEEDED".into());
            }
        }
        messages.push(message);
    }
    if !matches!(
        messages.last().map(|message| &message.event),
        Some(PhysicalInput::Shutdown)
    ) {
        return Err("ASTRA_HEADLESS_INPUT_SHUTDOWN_REQUIRED".into());
    }
    let input_hash = hash_input_messages(&messages)?;
    Ok(ValidatedInput {
        messages,
        input_hash,
    })
}

pub(crate) fn validate_input(input_path: &Path) -> Result<(), String> {
    let validated = read_and_validate_input(input_path, None)?;
    println!(
        "{}",
        serde_json::json!({
            "schema": "astra.headless_input_validation.v1",
            "status": "pass",
            "message_count": validated.messages.len(),
            "input_sequence_hash": validated.input_hash,
        })
    );
    Ok(())
}

pub(crate) async fn run(request: RunRequest<'_>) -> Result<(), String> {
    let thread_owner = HeadlessThreadOwner::default();
    let RunRequest {
        profile_path,
        input_path,
        artifact_root,
        checkpoint_config,
        ..
    } = request;
    match run_execution(request, &thread_owner).await {
        Ok(()) => Ok(()),
        Err(error) => {
            fs::create_dir_all(artifact_root)
                .map_err(|io| format!("{error}; ASTRA_HEADLESS_BLOCKED_REPORT_ROOT: {io}"))?;
            let manifest_path = artifact_root.join("artifact-manifest.json");
            if !manifest_path.is_file() {
                ensure_blocked_manifest(profile_path, input_path, &manifest_path)?;
            }
            let manifest_hash = if manifest_path.is_file() {
                hash_file(&manifest_path).unwrap_or_else(|_| empty_hash())
            } else {
                empty_hash()
            };
            let report = RunReport {
                schema: HEADLESS_RUN_REPORT_SCHEMA.into(),
                run_id: "blocked-run".into(),
                build_fingerprint: blocked_profile_field(profile_path, |profile| {
                    profile.build_fingerprint
                })
                .unwrap_or_else(empty_hash),
                package_hash: blocked_profile_field(profile_path, |profile| profile.package_hash)
                    .unwrap_or_else(empty_hash),
                input_sequence_hash: fs::read(input_path)
                    .map(|bytes| canonical_or_raw_input_hash(&bytes))
                    .unwrap_or_else(|_| empty_hash()),
                checkpoint_config_hash: checkpoint_config
                    .and_then(|path| hash_file(path).ok())
                    .unwrap_or_else(empty_hash),
                profile_id: blocked_profile_field(profile_path, |profile| profile.id)
                    .unwrap_or_else(|| "blocked-profile".into()),
                session_id: "blocked-session".into(),
                scenario: "blocked-scenario".into(),
                target: blocked_profile_field(profile_path, |profile| profile.target)
                    .unwrap_or_else(|| "blocked-target".into()),
                content_identity: blocked_profile_field(profile_path, |profile| profile.package_id)
                    .unwrap_or_else(|| "blocked-content".into()),
                status: RunStatus::Blocked,
                manifest_hash,
                renderer_identity_hash: blocked_profile_field(profile_path, |profile| {
                    renderer_identity_hash(&blocked_renderer_identity(&profile))
                })
                .unwrap_or_else(empty_hash),
                render_policy: blocked_profile_field(profile_path, |profile| {
                    render_policy_name(&profile).into()
                })
                .unwrap_or_else(|| "checkpoints".into()),
                submitted_frame_count: 0,
                rasterized_frame_count: 0,
                audio_frame_count: 0,
                duration_ns: 0,
                completed_sequence: 0,
                checkpoint_results: Vec::new(),
                diagnostics: vec![astra_headless_protocol::Diagnostic {
                    code: diagnostic_code(&error),
                    operation: "headless.run".into(),
                    message:
                        "headless run blocked; inspect stderr and the structured diagnostic code"
                            .into(),
                }],
            };
            report.validate().map_err(|validation| {
                format!("{error}; ASTRA_HEADLESS_BLOCKED_REPORT_INVALID: {validation}")
            })?;
            write_atomic_json(&artifact_root.join("run-report.json"), &report)
                .map_err(|io| format!("{error}; ASTRA_HEADLESS_BLOCKED_REPORT_WRITE: {io}"))?;
            Err(error)
        }
    }
}

pub(crate) async fn run_execution(
    request: RunRequest<'_>,
    thread_owner: &HeadlessThreadOwner,
) -> Result<(), String> {
    let RunRequest {
        profile_path,
        package_path,
        input_path,
        artifact_root,
        checkpoint_config,
        build_identity,
        gpu,
        source_profile,
        source_root,
        performance_budget,
        performance_report,
        performance_trace,
        performance_trace_manifest,
        performance_warmup_frames,
        performance_start_sequence,
    } = request;
    let identity_hash = read_identity_hash(build_identity)?;
    let profile = read_profile(profile_path, &identity_hash)?;
    let performance_observer = match (performance_budget, performance_trace) {
        (Some(budget_path), Some(trace_path)) => {
            astra_platform::validate_headless_performance_profile(&profile)
                .map_err(|error| format!("ASTRA_PERFORMANCE_PROFILE_INVALID: {error}"))?;
            let budget: astra_core::PerformanceBudget = serde_json::from_slice(
                &fs::read(budget_path)
                    .map_err(|error| format!("ASTRA_PERFORMANCE_BUDGET_READ_FAILED: {error}"))?,
            )
            .map_err(|error| format!("ASTRA_PERFORMANCE_BUDGET_INVALID: {error}"))?;
            Some(Arc::new(
                product_performance::ProductPerformanceRecorder::create(
                    budget,
                    trace_path,
                    performance_warmup_frames,
                    profile.presentation_rate_hz,
                    performance_start_sequence,
                )?,
            ))
        }
        (None, None) if performance_warmup_frames == 0 => None,
        _ => return Err("ASTRA_PERFORMANCE_ARGUMENT_SET_INCOMPLETE".into()),
    };
    let performance_scheduling = performance_observer
        .as_ref()
        .map(|_| astra_platform_common::PerformanceSchedulingGuard::activate_coordinator())
        .transpose()?;
    fs::create_dir_all(artifact_root).map_err(|e| e.to_string())?;
    let validated_input = read_and_validate_input(
        input_path,
        Some((profile.input.max_tick, profile.input.max_messages)),
    )?;
    let messages = validated_input.messages;
    let input_hash = validated_input.input_hash;
    if let Some(observer) = &performance_observer {
        observer.record_memory_snapshot("input.loaded")?;
    }
    let checkpoint_config_root = checkpoint_config
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let loaded_checkpoint_config = checkpoint_config.map(read_checkpoint_config).transpose()?;
    let checkpoint_config_hash = loaded_checkpoint_config
        .as_ref()
        .map(|(_, hash)| hash.clone())
        .unwrap_or_else(empty_hash);
    let checkpoint_config = loaded_checkpoint_config.map(|(config, _)| config);
    if checkpoint_config
        .as_ref()
        .is_some_and(|config| config.input_sequence_hash != input_hash)
    {
        return Err("ASTRA_HEADLESS_CHECKPOINT_INPUT_HASH_MISMATCH".into());
    }
    let package_root = package_path
        .parent()
        .ok_or_else(|| "ASTRA_HEADLESS_PACKAGE_PATH_INVALID".to_string())?;
    let package_name = package_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "ASTRA_HEADLESS_PACKAGE_PATH_INVALID".to_string())?;
    let mut host_factory = HeadlessPlatformFactory::new(artifact_root, package_root)
        .with_thread_owner(thread_owner)
        .with_gpu(gpu)
        .with_input_sequence_hash(input_hash.clone());
    if let Some(observer) = &performance_observer {
        host_factory = host_factory.with_performance_observer(observer.clone());
    }
    let host = host_factory
        .start(profile.clone().into())
        .await
        .map_err(|error| error.to_string())?;
    if let Some(observer) = &performance_observer {
        observer.record_memory_snapshot("host.started")?;
    }
    let package_verify_started = Instant::now();
    let package = host
        .client
        .open_package(PackageSourceRequest::Bundled {
            relative_path: package_name.into(),
            expected_hash: profile.package_hash.clone(),
        })
        .await
        .map_err(|error| error.to_string())?;
    if let Some(observer) = &performance_observer {
        observer.record_cpu_slice(
            "package.cpu",
            "package.storage_verify",
            None,
            package_verify_started,
        )?;
        observer.record_memory_snapshot("package.storage_verified")?;
    }
    host.client
        .close_package(package)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(observer) = &performance_observer {
        observer.record_memory_snapshot("package.verifier_closed")?;
    }
    let package_storage_hash = profile
        .package_hash
        .parse::<astra_core::Hash256>()
        .map_err(|_| "ASTRA_HEADLESS_PRODUCT_PACKAGE_HASH_INVALID".to_string())?;
    let package_open_started = Instant::now();
    let package_source: Arc<dyn astra_byte_source::BoundedByteSource> = Arc::new(
        astra_byte_source::FileByteSource::open(package_path)
            .map_err(|error| format!("ASTRA_HEADLESS_PRODUCT_PACKAGE_OPEN_FAILED: {error}"))?,
    );
    let package_container = astra_package::AstraContainerReader::open_storage_verified_source(
        package_source,
        package_storage_hash,
    )
    .map_err(|error| format!("ASTRA_HEADLESS_PRODUCT_PACKAGE_INVALID: {error}"))?;
    if let Some(observer) = &performance_observer {
        observer.record_cpu_slice(
            "package.cpu",
            "package.table_open",
            None,
            package_open_started,
        )?;
        observer.record_memory_snapshot("package.table_opened")?;
    }
    let package_unlock_started = Instant::now();
    let package_crypto = source_package_crypto(&package_container, source_profile, source_root)?;
    if let Some(observer) = &performance_observer {
        observer.record_cpu_slice(
            "package.cpu",
            "package.source_unlock",
            None,
            package_unlock_started,
        )?;
        observer.record_memory_snapshot("package.source_unlocked")?;
        observer.begin_cpu_scope("product.cpu", "product.open", None)?;
    }
    let registry = product_registry(package_crypto)?;
    let mut product = open_product(
        &registry,
        &profile,
        &host,
        package_container,
        performance_observer.clone().map(|observer| {
            observer as Arc<dyn astra_headless_protocol::ProductPerformanceObserver>
        }),
    )
    .await?;
    if let Some(observer) = &performance_observer {
        observer.end_cpu_scope("product.cpu", "product.open", None)?;
        observer.record_memory_snapshot("product.opened")?;
    }
    let mut checkpoint_results = Vec::new();
    let mut checkpoint_frames = BTreeMap::new();
    let mut completed_sequence = 0;
    let mut final_tick = 0;
    let mut await_tick_shift = 0_u64;
    for message in &messages {
        completed_sequence = message.sequence;
        let effective_tick = message
            .tick
            .checked_sub(await_tick_shift)
            .ok_or_else(|| "ASTRA_HEADLESS_AWAIT_TICK_SHIFT_INVALID".to_string())?;
        if matches!(message.event, PhysicalInput::Shutdown) {
            break;
        }
        if let Some(observer) = &performance_observer {
            observer.begin_input_flow(message.sequence)?;
            observer.begin_cpu_scope(
                "runtime.cpu",
                "physical_input.consume",
                Some(message.sequence),
            )?;
        }
        tracing::trace!(
            event = "astra.headless.input.started",
            sequence = message.sequence,
            effective_tick,
            input_kind = physical_input_kind(&message.event),
            "Headless started consuming physical input"
        );
        let (observations, await_advanced_ticks) =
            consume_input(product.as_mut(), effective_tick, &message.event)
                .await
                .map_err(|error| {
                    format!(
                        "{error}: sequence={} tick={} input={}",
                        message.sequence,
                        effective_tick,
                        physical_input_kind(&message.event)
                    )
                })?;
        final_tick = consumed_input_tick(effective_tick, &message.event, await_advanced_ticks)?;
        tracing::trace!(
            event = "astra.headless.input.completed",
            sequence = message.sequence,
            effective_tick,
            input_kind = physical_input_kind(&message.event),
            advanced_ticks = await_advanced_ticks,
            "Headless finished consuming physical input"
        );
        if let Some(observer) = &performance_observer {
            observer.end_cpu_scope(
                "runtime.cpu",
                "physical_input.consume",
                Some(message.sequence),
            )?;
            observer.record_product_sample(message.sequence, product.take_performance_sample())?;
            observer.end_input_flow(message.sequence)?;
            observer.set_decoded_cache_bytes(product.decoded_cache_bytes());
        }
        if let PhysicalInput::Await {
            timeout_ticks,
            continue_at_match: true,
            ..
        } = &message.event
        {
            tracing::trace!(
                event = "astra.headless.await.completed",
                sequence = message.sequence,
                effective_tick,
                advanced_ticks = await_advanced_ticks,
                timeout_ticks,
                "Headless observation wait reached its declared predicate"
            );
            let unused_ticks = u64::from(
                timeout_ticks
                    .checked_sub(await_advanced_ticks)
                    .ok_or_else(|| "ASTRA_HEADLESS_AWAIT_TICK_ACCOUNTING_INVALID".to_string())?,
            );
            await_tick_shift = await_tick_shift
                .checked_add(unused_ticks)
                .ok_or_else(|| "ASTRA_HEADLESS_AWAIT_TICK_SHIFT_OVERFLOW".to_string())?;
        }
        if let PhysicalInput::Checkpoint { id } = &message.event {
            let (capture, result) = capture_checkpoint(
                product.as_mut(),
                id,
                message.sequence,
                &observations,
                checkpoint_config.as_ref(),
                checkpoint_config_root.as_deref(),
                should_capture_checkpoint_audio(&profile, checkpoint_config.as_ref(), id),
            )
            .await?;
            if checkpoint_frames.insert(id.clone(), capture).is_some() {
                return Err(format!("ASTRA_HEADLESS_CHECKPOINT_DUPLICATE: {id}"));
            }
            checkpoint_results.push(result);
        }
    }
    if let Some(observer) = &performance_observer {
        observer.stop_gpu_measurement()?;
    }
    product
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
    host.client
        .shutdown()
        .await
        .map_err(|error| error.to_string())?;
    let manifest_path = artifact_root.join("artifact-manifest.json");
    let mut manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if checkpoint_config
        .as_ref()
        .is_some_and(|config| config.renderer_identity_hash != manifest.renderer_identity_hash)
    {
        return Err("ASTRA_HEADLESS_CHECKPOINT_RENDERER_IDENTITY_MISMATCH".into());
    }
    append_checkpoint_artifacts(
        artifact_root,
        &profile,
        &checkpoint_frames,
        &mut manifest,
        performance_observer.as_deref(),
    )?;
    apply_audio_comparisons(
        checkpoint_config.as_ref(),
        checkpoint_config_root.as_deref(),
        &checkpoint_frames,
        &mut checkpoint_results,
    )?;
    validate_required_checkpoints(
        checkpoint_config.as_ref(),
        &profile.artifacts.required_checkpoints,
        &checkpoint_results,
    )?;
    manifest.validate().map_err(|error| error.to_string())?;
    write_atomic_json(&manifest_path, &manifest)?;
    let report = RunReport {
        schema: HEADLESS_RUN_REPORT_SCHEMA.into(),
        run_id: messages
            .first()
            .map(|message| message.session.clone())
            .unwrap_or_else(|| "empty-input".into()),
        build_fingerprint: profile.build_fingerprint.clone(),
        package_hash: profile.package_hash.clone(),
        input_sequence_hash: input_hash,
        checkpoint_config_hash,
        profile_id: profile.id.clone(),
        session_id: messages
            .first()
            .map(|message| message.session.clone())
            .unwrap_or_else(|| "empty-input".into()),
        scenario: checkpoint_config
            .as_ref()
            .map(|config| config.id.clone())
            .unwrap_or_else(|| "default".into()),
        target: profile.target.clone(),
        content_identity: profile.package_id.clone(),
        status: RunStatus::Passed,
        manifest_hash: hash_file(&manifest_path)?,
        renderer_identity_hash: manifest.renderer_identity_hash.clone(),
        render_policy: manifest.render_policy.clone(),
        submitted_frame_count: manifest.submitted_frame_count,
        rasterized_frame_count: manifest.rasterized_frame_count,
        audio_frame_count: manifest.audio_frame_count,
        duration_ns: final_tick
            .checked_mul(TICK_DURATION_NS)
            .ok_or_else(|| "ASTRA_HEADLESS_DURATION_OVERFLOW".to_string())?,
        completed_sequence,
        checkpoint_results,
        diagnostics: Vec::new(),
    };
    report.validate().map_err(|error| error.to_string())?;
    write_atomic_json(&artifact_root.join("run-report.json"), &report)?;
    if let Some(observer) = performance_observer {
        let report_path = performance_report.ok_or("ASTRA_PERFORMANCE_REPORT_PATH_MISSING")?;
        let manifest_path =
            performance_trace_manifest.ok_or("ASTRA_PERFORMANCE_TRACE_MANIFEST_PATH_MISSING")?;
        let build = read_performance_build_identity(build_identity)?;
        let performance_identity = astra_core::PerformanceRunIdentity {
            source_revision: build.checkout_id,
            dirty: build.dirty,
            target: profile.target.clone(),
            profile: profile.product_profile.clone(),
            profile_hash: profile.hash().map_err(|error| error.to_string())?,
            package_hash: profile.package_hash.clone(),
            build_fingerprint: profile.build_fingerprint.clone(),
            session_id: report.session_id.clone(),
        };
        performance_identity
            .validate()
            .map_err(|error| error.to_string())?;
        let adapter_identity_hash = astra_core::Hash256::from_sha256(
            &serde_json::to_vec(&manifest.renderer_identity).map_err(|error| error.to_string())?,
        )
        .to_string();
        observer.finish(
            performance_identity,
            &report.scenario,
            adapter_identity_hash,
            manifest.renderer_identity.driver_identity_hash.clone(),
            report_path,
            manifest_path,
        )?;
    }
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    if let Some(scheduling) = performance_scheduling {
        scheduling.restore()?;
    }
    Ok(())
}
