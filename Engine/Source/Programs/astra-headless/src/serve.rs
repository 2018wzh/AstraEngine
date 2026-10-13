use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
};

use astra_headless_protocol::{
    ArtifactManifest, CheckpointConfig, CheckpointResult, Envelope, JsonlReader, JsonlWriter,
    Message, PhysicalInput, ProductSession, RunReport, RunStatus, SequenceValidator,
    HEADLESS_PROTOCOL_SCHEMA, HEADLESS_RUN_REPORT_SCHEMA, TICK_DURATION_NS,
};
use astra_platform::{
    HeadlessHostProfile, PackageSourceRequest, PlatformHostFactory, PlatformHostSession,
};
use astra_platform_headless::{HeadlessPlatformFactory, HeadlessThreadOwner};
use sha2::{Digest, Sha256};

use super::*;

pub(crate) struct LiveSession {
    pub(crate) session_id: String,
    pub(crate) host: PlatformHostSession,
    pub(crate) profile: HeadlessHostProfile,
    pub(crate) product: Option<Box<dyn ProductSession>>,
    pub(crate) sequence: SequenceValidator,
    pub(crate) artifact_root: PathBuf,
    pub(crate) response_sequence: u64,
    pub(crate) checkpoint_config: Option<CheckpointConfig>,
    pub(crate) checkpoint_config_hash: String,
    pub(crate) checkpoint_config_root: Option<PathBuf>,
    pub(crate) checkpoint_captures: BTreeMap<String, CheckpointCapture>,
    pub(crate) checkpoint_results: Vec<CheckpointResult>,
    pub(crate) input_digest: Sha256,
    pub(crate) input_count: u64,
    pub(crate) last_tick: u64,
    pub(crate) last_sequence: u64,
}

pub(crate) async fn serve(build_identity: &Path, gpu: bool) -> Result<(), String> {
    let thread_owner = HeadlessThreadOwner::default();
    let identity_hash = read_identity_hash(build_identity)?;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader =
        JsonlReader::new(BufReader::new(stdin.lock()), 1024 * 1024).map_err(|e| e.to_string())?;
    let mut writer = JsonlWriter::new(stdout.lock());
    let mut sessions = BTreeMap::<String, LiveSession>::new();
    let registry = product_registry(None)?;
    while let Some(envelope) = reader.read::<Envelope>().map_err(|e| e.to_string())? {
        let failure_session = envelope.session.clone();
        let handled: Result<(), String> = async {
            envelope.validate().map_err(|e| e.to_string())?;
            match envelope.message {
            Message::Open {
                profile_path,
                package_path,
                checkpoint_config_path,
                artifact_root,
            } => {
                if sessions.contains_key(&envelope.session) {
                    return Err("ASTRA_HEADLESS_SESSION_DUPLICATE: session is already open".into());
                }
                let checkpoint_config_root = checkpoint_config_path
                    .as_deref()
                    .and_then(|path| Path::new(path).parent())
                    .map(Path::to_path_buf);
                let loaded_checkpoint_config = checkpoint_config_path
                    .as_deref()
                    .map(Path::new)
                    .map(read_checkpoint_config)
                    .transpose()?;
                let checkpoint_config_hash = loaded_checkpoint_config
                    .as_ref()
                    .map(|(_, hash)| hash.clone())
                    .unwrap_or_else(empty_hash);
                let checkpoint_config =
                    loaded_checkpoint_config.map(|(config, _)| config);
                let profile = read_profile(Path::new(&profile_path), &identity_hash)?;
                let root = PathBuf::from(artifact_root).join(&envelope.session);
                fs::create_dir_all(&root)
                    .map_err(|e| format!("ASTRA_HEADLESS_ARTIFACT_ROOT_FAILED: {e}"))?;
                let package_file =
                    package_path
                        .as_deref()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            Path::new(&profile_path)
                                .parent()
                                .unwrap_or_else(|| Path::new("."))
                                .join("empty.astrapkg")
                        });
                let package_root = package_file.parent().unwrap_or_else(|| Path::new("."));
                let package_name = package_file
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| "ASTRA_HEADLESS_PACKAGE_PATH_INVALID".to_string())?
                    .to_owned();
                let host = HeadlessPlatformFactory::new(&root, package_root)
                    .with_thread_owner(&thread_owner)
                    .with_gpu(gpu)
                    .start(profile.clone().into())
                    .await
                    .map_err(|e| e.to_string())?;
                let package = host
                    .client
                    .open_package(PackageSourceRequest::Bundled {
                        relative_path: package_name,
                        expected_hash: profile.package_hash.clone(),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                host.client
                    .close_package(package)
                    .await
                    .map_err(|e| e.to_string())?;
                let product = match package_path {
                    Some(_) => {
                        let storage_hash = profile
                            .package_hash
                            .parse::<astra_core::Hash256>()
                            .map_err(|_| "ASTRA_HEADLESS_PACKAGE_HASH_INVALID".to_string())?;
                        let source: Arc<dyn astra_byte_source::BoundedByteSource> = Arc::new(
                            astra_byte_source::FileByteSource::open(&package_file).map_err(
                                |error| format!("ASTRA_HEADLESS_PACKAGE_OPEN_FAILED: {error}"),
                            )?,
                        );
                        let container =
                            astra_package::AstraContainerReader::open_storage_verified_source(
                                source,
                                storage_hash,
                            )
                            .map_err(|error| {
                                format!("ASTRA_HEADLESS_PACKAGE_INVALID: {error}")
                            })?;
                        Some(open_product(&registry, &profile, &host, container, None).await?)
                    }
                    None => None,
                };
                let profile_hash = profile.hash().map_err(|e| e.to_string())?;
                let provider_identity_hash = provider_hash(&profile)?;
                let mut sequence = SequenceValidator::default();
                sequence
                    .accept(&envelope.session, envelope.sequence, envelope.tick)
                    .map_err(|e| e.to_string())?;
                sessions.insert(
                    envelope.session.clone(),
                    LiveSession {
                        session_id: envelope.session.clone(),
                        host,
                        profile,
                        product,
                        sequence,
                        artifact_root: root,
                        response_sequence: 1,
                        checkpoint_config,
                        checkpoint_config_hash,
                        checkpoint_config_root,
                        checkpoint_captures: BTreeMap::new(),
                        checkpoint_results: Vec::new(),
                        input_digest: Sha256::new(),
                        input_count: 0,
                        last_tick: envelope.tick,
                        last_sequence: envelope.sequence,
                    },
                );
                writer
                    .write(&Envelope {
                        schema: HEADLESS_PROTOCOL_SCHEMA.into(),
                        session: envelope.session,
                        sequence: 1,
                        tick: 0,
                        message: Message::Opened {
                            profile_hash,
                            provider_identity_hash,
                        },
                    })
                    .map_err(|e| e.to_string())?;
            }
            Message::Shutdown => {
                let mut session = sessions.remove(&envelope.session).ok_or_else(|| {
                    "ASTRA_HEADLESS_SESSION_UNKNOWN: shutdown session is not open".to_string()
                })?;
                let shutdown_result: Result<(), String> = async {
                    session
                    .sequence
                    .accept(&envelope.session, envelope.sequence, envelope.tick)
                    .map_err(|e| e.to_string())?;
                if session.input_count >= session.profile.input.max_messages
                    || envelope.tick > session.profile.input.max_tick
                {
                    return Err("ASTRA_HEADLESS_INPUT_LIMIT_EXCEEDED".into());
                }
                update_input_digest(
                    &mut session.input_digest,
                    &astra_headless_protocol::InputMessage {
                        schema: astra_headless_protocol::USER_INPUT_SEQUENCE_SCHEMA.into(),
                        session: envelope.session.clone(),
                        sequence: envelope.sequence,
                        tick: envelope.tick,
                        event: PhysicalInput::Shutdown,
                    },
                )?;
                session.last_sequence = envelope.sequence;
                let input_hash = format!("sha256:{:x}", session.input_digest.clone().finalize());
                if session
                    .checkpoint_config
                    .as_ref()
                    .is_some_and(|config| config.input_sequence_hash != input_hash)
                {
                    return Err("ASTRA_HEADLESS_CHECKPOINT_INPUT_HASH_MISMATCH".into());
                }
                if let Some(product) = &mut session.product {
                    product.shutdown().await.map_err(|e| e.to_string())?;
                }
                session
                    .host
                    .client
                    .shutdown()
                    .await
                    .map_err(|e| e.to_string())?;
                let manifest_path = session.artifact_root.join("artifact-manifest.json");
                let mut manifest: ArtifactManifest = serde_json::from_slice(
                    &fs::read(&manifest_path)
                        .map_err(|e| format!("ASTRA_HEADLESS_MANIFEST_READ_FAILED: {e}"))?,
                )
                .map_err(|e| format!("ASTRA_HEADLESS_MANIFEST_INVALID: {e}"))?;
                manifest.input_sequence_hash = input_hash.clone();
                if session.checkpoint_config.as_ref().is_some_and(|config| {
                    config.renderer_identity_hash != manifest.renderer_identity_hash
                }) {
                    return Err("ASTRA_HEADLESS_CHECKPOINT_RENDERER_IDENTITY_MISMATCH".into());
                }
                append_checkpoint_artifacts(
                    &session.artifact_root,
                    &session.profile,
                    &session.checkpoint_captures,
                    &mut manifest,
                    None,
                )?;
                apply_audio_comparisons(
                    session.checkpoint_config.as_ref(),
                    session.checkpoint_config_root.as_deref(),
                    &session.checkpoint_captures,
                    &mut session.checkpoint_results,
                )?;
                validate_required_checkpoints(
                    session.checkpoint_config.as_ref(),
                    &session.profile.artifacts.required_checkpoints,
                    &session.checkpoint_results,
                )?;
                manifest.validate().map_err(|error| error.to_string())?;
                write_atomic_json(&manifest_path, &manifest)?;
                let report_path = session
                    .artifact_root
                    .join(format!("{}.run-report.json", envelope.session));
                let report = RunReport {
                    schema: HEADLESS_RUN_REPORT_SCHEMA.into(),
                    run_id: envelope.session.clone(),
                    build_fingerprint: session.profile.build_fingerprint.clone(),
                    package_hash: session.profile.package_hash.clone(),
                    input_sequence_hash: input_hash,
                    checkpoint_config_hash: session.checkpoint_config_hash.clone(),
                    profile_id: session.profile.id.clone(),
                    session_id: envelope.session.clone(),
                    scenario: session
                        .checkpoint_config
                        .as_ref()
                        .map(|config| config.id.clone())
                        .unwrap_or_else(|| "test-lifecycle".into()),
                    target: session.profile.target.clone(),
                    content_identity: session.profile.package_id.clone(),
                    status: RunStatus::Passed,
                    manifest_hash: hash_file(&manifest_path)?,
                    renderer_identity_hash: manifest.renderer_identity_hash.clone(),
                    render_policy: manifest.render_policy.clone(),
                    submitted_frame_count: manifest.submitted_frame_count,
                    rasterized_frame_count: manifest.rasterized_frame_count,
                    audio_frame_count: manifest.audio_frame_count,
                    duration_ns: session
                        .last_tick
                        .checked_mul(TICK_DURATION_NS)
                        .ok_or_else(|| "ASTRA_HEADLESS_DURATION_OVERFLOW".to_string())?,
                    completed_sequence: envelope.sequence,
                    checkpoint_results: session.checkpoint_results.clone(),
                    diagnostics: Vec::new(),
                };
                report.validate().map_err(|error| error.to_string())?;
                write_atomic_json(&report_path, &report)?;
                let report_hash = hash_file(&report_path)?;
                let relative = report_path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .ok_or_else(|| "ASTRA_HEADLESS_REPORT_PATH_INVALID".to_string())?
                    .to_owned();
                writer
                    .write(&Envelope {
                        schema: HEADLESS_PROTOCOL_SCHEMA.into(),
                        session: envelope.session,
                        sequence: session.response_sequence.checked_add(1).ok_or_else(|| {
                            "ASTRA_HEADLESS_RESPONSE_SEQUENCE_OVERFLOW".to_string()
                        })?,
                        tick: envelope.tick,
                        message: Message::ShutdownComplete {
                            run_report_path: relative,
                            run_report_hash: report_hash,
                        },
                    })
                    .map_err(|e| e.to_string())?;
                    Ok(())
                }
                .await;
                if let Err(error) = shutdown_result {
                    finalize_blocked_live_session(session, &error).await;
                    return Err(error);
                }
            }
            Message::Input { input } => {
                if matches!(input.event, PhysicalInput::Shutdown) {
                    return Err("ASTRA_HEADLESS_DIRECTION_INVALID: stdio shutdown uses the protocol shutdown envelope".into());
                }
                let session = sessions.get_mut(&envelope.session).ok_or_else(|| {
                    "ASTRA_HEADLESS_SESSION_UNKNOWN: input session is not open".to_string()
                })?;
                session
                    .sequence
                    .accept(&envelope.session, envelope.sequence, envelope.tick)
                    .map_err(|e| e.to_string())?;
                if session.input_count >= session.profile.input.max_messages
                    || envelope.tick > session.profile.input.max_tick
                {
                    return Err("ASTRA_HEADLESS_INPUT_LIMIT_EXCEEDED".into());
                }
                update_input_digest(&mut session.input_digest, &input)?;
                session.input_count = session
                    .input_count
                    .checked_add(1)
                    .ok_or_else(|| "ASTRA_HEADLESS_INPUT_COUNT_OVERFLOW".to_string())?;
                session.last_sequence = envelope.sequence;
                let product = session.product.as_mut().ok_or_else(|| "ASTRA_HEADLESS_PRODUCT_SESSION_REQUIRED: lifecycle-only session rejects product input".to_string())?;
                let (observations, advanced_ticks) =
                    consume_input(product.as_mut(), envelope.tick, &input.event).await?;
                session.last_tick = consumed_input_tick(envelope.tick, &input.event, advanced_ticks)?;
                if let PhysicalInput::Checkpoint { id } = &input.event {
                    let (capture, result) = capture_checkpoint(
                        product.as_mut(),
                        id,
                        envelope.sequence,
                        &observations,
                        session.checkpoint_config.as_ref(),
                        session.checkpoint_config_root.as_deref(),
                        should_capture_checkpoint_audio(
                            &session.profile,
                            session.checkpoint_config.as_ref(),
                            id,
                        ),
                    )
                    .await?;
                    if session
                        .checkpoint_captures
                        .insert(id.clone(), capture)
                        .is_some()
                    {
                        return Err(format!("ASTRA_HEADLESS_CHECKPOINT_DUPLICATE: {id}"));
                    }
                    session.checkpoint_results.push(result);
                }
                for observation in observations {
                    session.response_sequence = session
                        .response_sequence
                        .checked_add(1)
                        .ok_or_else(|| "ASTRA_HEADLESS_RESPONSE_SEQUENCE_OVERFLOW".to_string())?;
                    writer
                        .write(&Envelope {
                            schema: HEADLESS_PROTOCOL_SCHEMA.into(),
                            session: envelope.session.clone(),
                            sequence: session.response_sequence,
                            tick: envelope.tick,
                            message: Message::Observation {
                                key: observation.key,
                                value_hash: observation.value_hash,
                            },
                        })
                        .map_err(|e| e.to_string())?;
                }
            }
                _ => {
                    return Err(
                        "ASTRA_HEADLESS_DIRECTION_INVALID: client sent a server-only message"
                            .into(),
                    )
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = handled {
            if let Some(session) = sessions.remove(&failure_session) {
                finalize_blocked_live_session(session, &error).await;
            }
            return Err(error);
        }
    }
    if sessions.is_empty() {
        Ok(())
    } else {
        let error = "ASTRA_HEADLESS_STDIO_CLOSED_WITH_LIVE_SESSIONS";
        for session in sessions.into_values() {
            finalize_blocked_live_session(session, error).await;
        }
        Err(error.into())
    }
}

pub(crate) async fn finalize_blocked_live_session(mut session: LiveSession, error: &str) {
    if let Some(product) = &mut session.product {
        let _ = product.shutdown().await;
    }
    let _ = session.host.client.shutdown().await;
    let input_hash = format!("sha256:{:x}", session.input_digest.clone().finalize());
    let manifest_path = session.artifact_root.join("artifact-manifest.json");
    let mut manifest = fs::read(&manifest_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ArtifactManifest>(&bytes).ok())
        .unwrap_or_else(|| ArtifactManifest {
            schema: astra_headless_protocol::HEADLESS_ARTIFACT_MANIFEST_SCHEMA.into(),
            run_id: session.profile.artifacts.namespace.clone(),
            build_fingerprint: session.profile.build_fingerprint.clone(),
            package_hash: session.profile.package_hash.clone(),
            input_sequence_hash: input_hash.clone(),
            provider_identity_hash: provider_hash(&session.profile)
                .unwrap_or_else(|_| empty_hash()),
            renderer_identity_hash: renderer_identity_hash(&blocked_renderer_identity(
                &session.profile,
            )),
            renderer_identity: blocked_renderer_identity(&session.profile),
            render_policy: render_policy_name(&session.profile).into(),
            submitted_frame_count: 0,
            rasterized_frame_count: 0,
            audio_frame_count: 0,
            audio_peak_dbfs: None,
            audio_rms_dbfs: None,
            silence: true,
            clipping: false,
            artifacts: Vec::new(),
        });
    manifest.input_sequence_hash = input_hash.clone();
    if let Err(write_error) = write_atomic_json(&manifest_path, &manifest) {
        tracing::error!(
            event = "astra.headless.blocked_manifest.failed",
            diagnostic = %diagnostic_code(&write_error),
            "Could not commit blocked live-session manifest"
        );
        return;
    }
    let report = RunReport {
        schema: HEADLESS_RUN_REPORT_SCHEMA.into(),
        run_id: session.session_id.clone(),
        build_fingerprint: session.profile.build_fingerprint.clone(),
        package_hash: session.profile.package_hash.clone(),
        input_sequence_hash: input_hash,
        checkpoint_config_hash: session.checkpoint_config_hash,
        profile_id: session.profile.id.clone(),
        session_id: session.session_id.clone(),
        scenario: session
            .checkpoint_config
            .as_ref()
            .map(|config| config.id.clone())
            .unwrap_or_else(|| "test-lifecycle".into()),
        target: session.profile.target.clone(),
        content_identity: session.profile.package_id.clone(),
        status: RunStatus::Blocked,
        manifest_hash: hash_file(&manifest_path).unwrap_or_else(|_| empty_hash()),
        renderer_identity_hash: manifest.renderer_identity_hash.clone(),
        render_policy: manifest.render_policy.clone(),
        submitted_frame_count: manifest.submitted_frame_count,
        rasterized_frame_count: manifest.rasterized_frame_count,
        audio_frame_count: manifest.audio_frame_count,
        duration_ns: session.last_tick.saturating_mul(TICK_DURATION_NS),
        completed_sequence: session.last_sequence,
        checkpoint_results: session.checkpoint_results,
        diagnostics: vec![astra_headless_protocol::Diagnostic {
            code: diagnostic_code(error),
            operation: "headless.serve".into(),
            message: "live Headless session blocked; inspect stderr and diagnostic code".into(),
        }],
    };
    let report_path = session
        .artifact_root
        .join(format!("{}.run-report.json", session.session_id));
    if report.validate().is_err() || write_atomic_json(&report_path, &report).is_err() {
        tracing::error!(
            event = "astra.headless.blocked_report.failed",
            diagnostic = "ASTRA_HEADLESS_BLOCKED_REPORT_WRITE",
            "Could not commit blocked live-session report"
        );
    }
}
