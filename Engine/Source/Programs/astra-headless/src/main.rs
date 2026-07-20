use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use astra_core::Hash256;
use astra_headless_protocol::{
    ArtifactEntry, ArtifactManifest, CheckpointConfig, CheckpointResult, Envelope, JsonlReader,
    JsonlWriter, Message, ObservationPredicate, PhysicalInput, PlatformRunIdentity, PreflightLink,
    RenderPerformanceReport, RendererExecutionIdentity, ReviewArtifactRole,
    ReviewArtifactSelection, ReviewBundle, ReviewRecord, RunReport, RunStatus, SequenceValidator,
    HEADLESS_PREFLIGHT_LINK_SCHEMA, HEADLESS_PROTOCOL_SCHEMA, HEADLESS_RENDER_PERFORMANCE_SCHEMA,
    HEADLESS_REVIEW_BUNDLE_SCHEMA, HEADLESS_RUN_REPORT_SCHEMA, MIN_CPU_SPARSE_SPEEDUP,
    MIN_GPU_ALL_SPEEDUP, TICK_DURATION_NS,
};
use astra_headless_vn_adapter::NativeVnProductAdapterFactory;
use astra_media_core::{
    BlendMode, CpuRendererProvider, FilterGraph, FilterNode, FilterParam, FilterTarget,
    GlyphBitmap, GlyphBitmapFormat, MeshMaterial2D, MeshVertex2D, RectI, RenderTargetFormat,
    Renderer2DProvider, RendererCreateRequest, SceneCommand, TextureFrame,
};
use astra_package::{
    AuthorizedSourceReader, ContainerCryptoProvider, ContainerError,
    SourceFingerprintCryptoProvider, SourceUnlockPolicy, SourceVerificationManifest,
};
use astra_platform::{
    HeadlessHostProfile, PackageSourceRequest, PlatformHostFactory, PlatformHostSession,
};
use astra_platform_common::WgpuOffscreenRenderer;
use astra_platform_headless::HeadlessPlatformFactory;
use astra_product_host::{ProductAdapterRegistry, ProductOpenRequest, ProductSession};
use clap::{Parser, Subcommand};
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use sha2::{Digest, Sha256};

mod compare;

struct CheckpointCapture {
    sequence: u64,
    frame: astra_platform::CapturedFrame,
    audio: Option<astra_product_host::CanonicalAudioSnapshot>,
}

#[derive(Debug, Parser)]
#[command(name = "astra-headless")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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
        #[arg(long)]
        manifest_only: bool,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        build_identity: PathBuf,
    },
    PrepareReview {
        #[arg(long)]
        run_report: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        artifact_root: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    ValidateReview {
        #[arg(long)]
        run_report: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        review: PathBuf,
    },
    LinkPreflight {
        #[arg(long)]
        headless_run_report: PathBuf,
        #[arg(long)]
        platform_run_identity: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    BenchmarkRender {
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        build_identity: PathBuf,
        #[arg(long)]
        gpu: bool,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
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
        } => {
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
            })
            .await
        }
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
            manifest_only,
            &output,
            &build_identity,
        ),
        Command::PrepareReview {
            run_report,
            manifest,
            artifact_root,
            output,
        } => prepare_review(&run_report, &manifest, &artifact_root, &output),
        Command::ValidateReview {
            run_report,
            bundle,
            review,
        } => validate_review(&run_report, &bundle, &review),
        Command::LinkPreflight {
            headless_run_report,
            platform_run_identity,
            output,
        } => link_preflight(&headless_run_report, &platform_run_identity, &output),
        Command::BenchmarkRender {
            output,
            build_identity,
            gpu,
        } => benchmark_render(&output, &build_identity, gpu).await,
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

struct LiveSession {
    session_id: String,
    host: PlatformHostSession,
    profile: HeadlessHostProfile,
    product: Option<Box<dyn ProductSession>>,
    sequence: SequenceValidator,
    artifact_root: PathBuf,
    response_sequence: u64,
    checkpoint_config: Option<CheckpointConfig>,
    checkpoint_config_hash: String,
    checkpoint_config_root: Option<PathBuf>,
    checkpoint_captures: BTreeMap<String, CheckpointCapture>,
    checkpoint_results: Vec<CheckpointResult>,
    input_digest: Sha256,
    input_count: u64,
    last_tick: u64,
    last_sequence: u64,
}

async fn serve(build_identity: &Path, gpu: bool) -> Result<(), String> {
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
                let package_size = fs::metadata(&package_file)
                    .map_err(|e| format!("ASTRA_HEADLESS_PACKAGE_METADATA_FAILED: {e}"))?
                    .len();
                let bytes = read_verified_package(
                    &host,
                    package,
                    package_size,
                    profile.limits.max_package_read_bytes,
                )
                .await?;
                if astra_core::Hash256::from_sha256(&bytes).to_string() != profile.package_hash {
                    return Err("ASTRA_HEADLESS_EMPTY_PACKAGE_HASH_MISMATCH".into());
                }
                host.client
                    .close_package(package)
                    .await
                    .map_err(|e| e.to_string())?;
                let product = match package_path {
                    Some(_) => Some(open_product(&registry, &profile, &host, bytes).await?),
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
                session.last_tick = envelope.tick;
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
                    submitted_scene_stream_hash: manifest.submitted_scene_stream_hash.clone(),
                    rasterized_frame_stream_hash: manifest.rasterized_frame_stream_hash.clone(),
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
                session.last_tick = envelope.tick;
                session.last_sequence = envelope.sequence;
                let product = session.product.as_mut().ok_or_else(|| "ASTRA_HEADLESS_PRODUCT_SESSION_REQUIRED: lifecycle-only session rejects product input".to_string())?;
                let (observations, _) =
                    consume_input(product.as_mut(), envelope.tick, &input.event).await?;
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

async fn finalize_blocked_live_session(mut session: LiveSession, error: &str) {
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
            submitted_scene_stream_hash: empty_hash(),
            rasterized_frame_stream_hash: empty_hash(),
            audio_stream_hash: empty_hash(),
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
        submitted_scene_stream_hash: manifest.submitted_scene_stream_hash.clone(),
        rasterized_frame_stream_hash: manifest.rasterized_frame_stream_hash.clone(),
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

#[derive(Clone, Copy)]
struct RunRequest<'a> {
    profile_path: &'a Path,
    package_path: &'a Path,
    input_path: &'a Path,
    artifact_root: &'a Path,
    checkpoint_config: Option<&'a Path>,
    build_identity: &'a Path,
    gpu: bool,
    source_profile: Option<&'a Path>,
    source_root: Option<&'a Path>,
}

async fn run(request: RunRequest<'_>) -> Result<(), String> {
    let RunRequest {
        profile_path,
        input_path,
        artifact_root,
        checkpoint_config,
        ..
    } = request;
    match run_execution(request).await {
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
                submitted_scene_stream_hash: empty_hash(),
                rasterized_frame_stream_hash: empty_hash(),
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

fn ensure_blocked_manifest(
    profile_path: &Path,
    input_path: &Path,
    manifest_path: &Path,
) -> Result<(), String> {
    let profile: HeadlessHostProfile = serde_json::from_slice(
        &fs::read(profile_path)
            .map_err(|error| format!("ASTRA_HEADLESS_BLOCKED_PROFILE_READ_FAILED: {error}"))?,
    )
    .map_err(|error| format!("ASTRA_HEADLESS_BLOCKED_PROFILE_INVALID: {error}"))?;
    let input_hash = fs::read(input_path)
        .map(|bytes| canonical_or_raw_input_hash(&bytes))
        .unwrap_or_else(|_| empty_hash());
    let provider_identity_hash = provider_hash(&profile)?;
    let renderer_identity = blocked_renderer_identity(&profile);
    let render_policy = render_policy_name(&profile).to_string();
    write_atomic_json(
        manifest_path,
        &ArtifactManifest {
            schema: astra_headless_protocol::HEADLESS_ARTIFACT_MANIFEST_SCHEMA.into(),
            run_id: profile.artifacts.namespace,
            build_fingerprint: profile.build_fingerprint,
            package_hash: profile.package_hash,
            input_sequence_hash: input_hash,
            provider_identity_hash,
            renderer_identity_hash: renderer_identity_hash(&renderer_identity),
            renderer_identity,
            render_policy,
            submitted_frame_count: 0,
            rasterized_frame_count: 0,
            audio_frame_count: 0,
            submitted_scene_stream_hash: empty_hash(),
            rasterized_frame_stream_hash: empty_hash(),
            audio_stream_hash: empty_hash(),
            audio_peak_dbfs: None,
            audio_rms_dbfs: None,
            silence: true,
            clipping: false,
            artifacts: Vec::new(),
        },
    )
}

async fn run_execution(request: RunRequest<'_>) -> Result<(), String> {
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
    } = request;
    let identity_hash = read_identity_hash(build_identity)?;
    let profile = read_profile(profile_path, &identity_hash)?;
    fs::create_dir_all(artifact_root).map_err(|e| e.to_string())?;
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
        if message.tick > profile.input.max_tick {
            return Err("ASTRA_HEADLESS_INPUT_TICK_LIMIT_EXCEEDED".into());
        }
        if messages.len() as u64 >= profile.input.max_messages {
            return Err("ASTRA_HEADLESS_INPUT_LIMIT_EXCEEDED".into());
        }
        messages.push(message);
    }
    let input_hash = hash_input_messages(&messages)?;
    if !matches!(
        messages.last().map(|message| &message.event),
        Some(PhysicalInput::Shutdown)
    ) {
        return Err("ASTRA_HEADLESS_INPUT_SHUTDOWN_REQUIRED".into());
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
    let host = HeadlessPlatformFactory::new(artifact_root, package_root)
        .with_gpu(gpu)
        .with_input_sequence_hash(input_hash.clone())
        .start(profile.clone().into())
        .await
        .map_err(|error| error.to_string())?;
    let package = host
        .client
        .open_package(PackageSourceRequest::Bundled {
            relative_path: package_name.into(),
            expected_hash: profile.package_hash.clone(),
        })
        .await
        .map_err(|error| error.to_string())?;
    let package_size = fs::metadata(package_path)
        .map_err(|error| format!("ASTRA_HEADLESS_PACKAGE_METADATA_FAILED: {error}"))?
        .len();
    let package_bytes = read_verified_package(
        &host,
        package,
        package_size,
        profile.limits.max_package_read_bytes,
    )
    .await?;
    host.client
        .close_package(package)
        .await
        .map_err(|error| error.to_string())?;
    if astra_core::Hash256::from_sha256(&package_bytes).to_string() != profile.package_hash {
        return Err("ASTRA_HEADLESS_PRODUCT_PACKAGE_HASH_MISMATCH".into());
    }
    let package_crypto = source_package_crypto(&package_bytes, source_profile, source_root)?;
    let registry = product_registry(package_crypto)?;
    let mut product = open_product(&registry, &profile, &host, package_bytes).await?;
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
        final_tick = effective_tick;
        if matches!(message.event, PhysicalInput::Shutdown) {
            break;
        }
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
        if let PhysicalInput::Await {
            timeout_ticks,
            continue_at_match: true,
            ..
        } = &message.event
        {
            tracing::info!(
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
            final_tick = final_tick
                .checked_add(u64::from(await_advanced_ticks))
                .ok_or_else(|| "ASTRA_HEADLESS_AWAIT_FINAL_TICK_OVERFLOW".to_string())?;
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
    append_checkpoint_artifacts(artifact_root, &profile, &checkpoint_frames, &mut manifest)?;
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
        submitted_scene_stream_hash: manifest.submitted_scene_stream_hash.clone(),
        rasterized_frame_stream_hash: manifest.rasterized_frame_stream_hash.clone(),
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
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn physical_input_kind(input: &PhysicalInput) -> &'static str {
    match input {
        PhysicalInput::Resume => "resume",
        PhysicalInput::Focus { .. } => "focus",
        PhysicalInput::Keyboard { .. } => "keyboard",
        PhysicalInput::ImePreedit { .. } => "ime_preedit",
        PhysicalInput::ImeCommit { .. } => "ime_commit",
        PhysicalInput::PointerMove { .. } => "pointer_move",
        PhysicalInput::PointerButton { .. } => "pointer_button",
        PhysicalInput::Wheel { .. } => "wheel",
        PhysicalInput::Touch { .. } => "touch",
        PhysicalInput::GamepadConnection { .. } => "gamepad_connection",
        PhysicalInput::GamepadInput { .. } => "gamepad_input",
        PhysicalInput::AdvanceTicks { .. } => "advance_ticks",
        PhysicalInput::Await { .. } => "await",
        PhysicalInput::Checkpoint { .. } => "checkpoint",
        PhysicalInput::Shutdown => "shutdown",
    }
}

fn blocked_profile_field(
    path: &Path,
    select: impl FnOnce(HeadlessHostProfile) -> String,
) -> Option<String> {
    serde_json::from_slice::<HeadlessHostProfile>(&fs::read(path).ok()?)
        .ok()
        .map(select)
}

fn diagnostic_code(error: &str) -> String {
    error
        .split(|character: char| character == ':' || character.is_whitespace())
        .next()
        .filter(|code| {
            code.starts_with("ASTRA_")
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        })
        .unwrap_or("ASTRA_HEADLESS_RUN_BLOCKED")
        .to_string()
}

async fn benchmark_render(
    output: &Path,
    build_identity: &Path,
    include_gpu: bool,
) -> Result<(), String> {
    const WIDTH: u32 = 1920;
    const HEIGHT: u32 = 1080;
    const WARMUP: u32 = 60;
    const MEASURED: u32 = 600;

    let build_fingerprint = read_identity_hash(build_identity)?;
    let commands = benchmark_commands();
    let scenario_hash = format!(
        "sha256:{}",
        sha256_hex(
            &serde_json::to_vec(&(WIDTH, HEIGHT, WARMUP, MEASURED, &commands))
                .map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_SERIALIZE_FAILED: {error}"))?
        )
    );
    let request = RendererCreateRequest {
        width: WIDTH,
        height: HEIGHT,
        format: RenderTargetFormat::Rgba8Srgb,
        profile: "headless-performance".into(),
    };

    let mut cpu_all = CpuRendererProvider
        .create(request.clone())
        .map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_CPU_CREATE_FAILED: {error}"))?;
    for _ in 0..WARMUP {
        cpu_all
            .capture_frame(&commands)
            .map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_CPU_WARMUP_FAILED: {error}"))?;
    }
    let (cpu_all_duration_ns, cpu_all_samples) =
        measure_frames(MEASURED, || cpu_all.capture_frame(&commands).map(|_| ()))?;

    let mut cpu_sparse = CpuRendererProvider
        .create(request)
        .map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_CPU_CREATE_FAILED: {error}"))?;
    for _ in 0..WARMUP {
        cpu_sparse
            .submit_frame(&commands)
            .map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_SPARSE_WARMUP_FAILED: {error}"))?;
    }
    let mut sparse_index = 0_u32;
    let (cpu_sparse_duration_ns, cpu_sparse_samples) = measure_frames(MEASURED, || {
        let selected =
            sparse_index == 0 || sparse_index == MEASURED / 2 || sparse_index + 1 == MEASURED;
        sparse_index += 1;
        if selected {
            let mut snapshot = cpu_sparse.clone();
            snapshot.capture_frame(&commands).map(|_| ())
        } else {
            cpu_sparse.submit_frame(&commands)
        }
    })?;
    let cpu_sparse_speedup = cpu_all_duration_ns as f64 / cpu_sparse_duration_ns as f64;
    let (cpu_all_p50_frame_ns, cpu_all_p95_frame_ns) = percentiles(cpu_all_samples);
    let (cpu_sparse_p50_frame_ns, cpu_sparse_p95_frame_ns) = percentiles(cpu_sparse_samples);

    let mut gpu_all_duration_ns = None;
    let mut gpu_all_p50_frame_ns = None;
    let mut gpu_all_p95_frame_ns = None;
    let mut gpu_all_speedup = None;
    let mut gpu_identity = None;
    if include_gpu {
        let mut renderer = WgpuOffscreenRenderer::new()
            .await
            .map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_GPU_REQUIRED: {error}"))?;
        let mut sequence = 0_u64;
        for _ in 0..WARMUP {
            sequence += 1;
            renderer
                .render(&benchmark_scene(sequence, commands.clone()))
                .map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_GPU_WARMUP_FAILED: {error}"))?;
        }
        let (duration, samples) = measure_platform_frames(MEASURED, || {
            sequence += 1;
            renderer
                .render(&benchmark_scene(sequence, commands.clone()))
                .map(|_| ())
        })?;
        let (p50, p95) = percentiles(samples);
        gpu_all_duration_ns = Some(duration);
        gpu_all_p50_frame_ns = Some(p50);
        gpu_all_p95_frame_ns = Some(p95);
        gpu_all_speedup = Some(cpu_all_duration_ns as f64 / duration as f64);
        gpu_identity = Some(renderer.identity().clone());
    }

    let mut diagnostics = Vec::new();
    if cpu_sparse_speedup < MIN_CPU_SPARSE_SPEEDUP {
        diagnostics.push("ASTRA_HEADLESS_CPU_SPARSE_SPEEDUP_BELOW_THRESHOLD".into());
    }
    if gpu_all_speedup.is_some_and(|speedup| speedup < MIN_GPU_ALL_SPEEDUP) {
        diagnostics.push("ASTRA_HEADLESS_GPU_SPEEDUP_BELOW_THRESHOLD".into());
    }
    let status = if diagnostics.is_empty() {
        RunStatus::Passed
    } else {
        RunStatus::Blocked
    };
    let report = RenderPerformanceReport {
        schema: HEADLESS_RENDER_PERFORMANCE_SCHEMA.into(),
        status,
        build_fingerprint,
        scenario_hash,
        width: WIDTH,
        height: HEIGHT,
        warmup_frames: WARMUP,
        measured_frames: MEASURED,
        cpu_all_duration_ns,
        cpu_all_p50_frame_ns,
        cpu_all_p95_frame_ns,
        cpu_sparse_duration_ns,
        cpu_sparse_p50_frame_ns,
        cpu_sparse_p95_frame_ns,
        cpu_sparse_speedup,
        gpu_all_duration_ns,
        gpu_all_p50_frame_ns,
        gpu_all_p95_frame_ns,
        gpu_all_speedup,
        gpu_identity,
        diagnostics,
    };
    report.validate().map_err(|error| error.to_string())?;
    write_atomic_json(output, &report)?;
    if report.status == RunStatus::Blocked {
        return Err("ASTRA_HEADLESS_BENCHMARK_THRESHOLD_BLOCKED".into());
    }
    Ok(())
}

fn benchmark_scene(sequence: u64, commands: Vec<SceneCommand>) -> astra_platform::SceneFrame {
    astra_platform::SceneFrame {
        sequence,
        width: 1920,
        height: 1080,
        clear_rgba: [4, 8, 16, 255],
        commands,
        semantics: None,
    }
}

fn benchmark_commands() -> Vec<SceneCommand> {
    let texture_bytes = [64_u8, 128, 224, 255].repeat(64 * 64);
    let glyph_bytes = vec![192_u8; 32 * 32];
    let mut fade_params = BTreeMap::new();
    fade_params.insert("amount".into(), FilterParam::Float(0.92));
    vec![
        SceneCommand::Texture {
            id: "benchmark.texture".into(),
            frame: TextureFrame {
                width: 64,
                height: 64,
                hash: Hash256::from_sha256(&texture_bytes),
                rgba8: texture_bytes,
            },
            destination: RectI::new(64, 64, 1024, 640),
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        SceneCommand::Glyph {
            id: "benchmark.glyph".into(),
            glyph: GlyphBitmap {
                width: 32,
                height: 32,
                format: GlyphBitmapFormat::Alpha8,
                hash: Hash256::from_sha256(&glyph_bytes),
                pixels: glyph_bytes,
            },
            x: 128,
            y: 128,
            rgba: [255, 240, 220, 255],
            opacity: 1.0,
            blend: BlendMode::Alpha,
        },
        SceneCommand::Mesh2D {
            id: "benchmark.mesh".into(),
            vertices: vec![
                MeshVertex2D {
                    position: [960.0, 128.0],
                    uv: [0.0, 0.0],
                    premultiplied_rgba: [240, 80, 80, 255],
                },
                MeshVertex2D {
                    position: [1760.0, 900.0],
                    uv: [1.0, 1.0],
                    premultiplied_rgba: [80, 240, 80, 255],
                },
                MeshVertex2D {
                    position: [720.0, 900.0],
                    uv: [0.0, 1.0],
                    premultiplied_rgba: [80, 80, 240, 255],
                },
            ],
            indices: vec![0, 1, 2],
            material: MeshMaterial2D::Solid,
            texture_id: None,
            opacity: 0.9,
            blend: BlendMode::Alpha,
        },
        SceneCommand::FilterGraph {
            graph: FilterGraph {
                schema: "astra.filter_graph.v1".into(),
                nodes: vec![FilterNode {
                    id: "benchmark.fade".into(),
                    kind: "astra.filter.fade".into(),
                    input: FilterTarget::Final,
                    output: FilterTarget::Final,
                    params: fade_params,
                    deterministic: true,
                    allow_cpu_fallback: true,
                }],
            },
        },
    ]
}

fn measure_frames<E>(
    count: u32,
    mut frame: impl FnMut() -> Result<(), E>,
) -> Result<(u64, Vec<u64>), String>
where
    E: std::fmt::Display,
{
    let total = Instant::now();
    let mut samples = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let started = Instant::now();
        frame().map_err(|error| format!("ASTRA_HEADLESS_BENCHMARK_CPU_FAILED: {error}"))?;
        samples.push(duration_ns(started.elapsed()));
    }
    Ok((duration_ns(total.elapsed()), samples))
}

fn measure_platform_frames(
    count: u32,
    frame: impl FnMut() -> Result<(), astra_platform::PlatformError>,
) -> Result<(u64, Vec<u64>), String> {
    measure_frames(count, frame)
}

fn duration_ns(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos())
        .unwrap_or(u64::MAX)
        .max(1)
}

fn percentiles(mut samples: Vec<u64>) -> (u64, u64) {
    samples.sort_unstable();
    let index = |percent: usize| ((samples.len() - 1) * percent) / 100;
    (samples[index(50)], samples[index(95)])
}

fn read_identity_hash(path: &Path) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("ASTRA_BUILD_IDENTITY_READ_FAILED: {e}"))?,
    )
    .map_err(|e| format!("ASTRA_BUILD_IDENTITY_INVALID: {e}"))?;
    if value.get("schema").and_then(|v| v.as_str()) != Some("astra.build_identity.v1") {
        return Err("ASTRA_BUILD_IDENTITY_SCHEMA_INVALID".into());
    }
    let hash = value
        .get("identity_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "ASTRA_BUILD_IDENTITY_HASH_MISSING".to_string())?;
    if !is_hash(hash) {
        return Err("ASTRA_BUILD_IDENTITY_HASH_INVALID".into());
    }
    Ok(hash.to_owned())
}

fn update_input_digest(
    digest: &mut Sha256,
    message: &astra_headless_protocol::InputMessage,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(message)
        .map_err(|error| format!("ASTRA_HEADLESS_INPUT_CANONICALIZE_FAILED: {error}"))?;
    digest.update(bytes);
    digest.update(b"\n");
    Ok(())
}

fn hash_input_messages(
    messages: &[astra_headless_protocol::InputMessage],
) -> Result<String, String> {
    let mut digest = Sha256::new();
    for message in messages {
        update_input_digest(&mut digest, message)?;
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn canonical_or_raw_input_hash(bytes: &[u8]) -> String {
    let canonical = (|| {
        let mut reader = JsonlReader::new(BufReader::new(bytes), 1024 * 1024).ok()?;
        let mut messages = Vec::new();
        while let Some(message) = reader
            .read::<astra_headless_protocol::InputMessage>()
            .ok()?
        {
            messages.push(message);
        }
        hash_input_messages(&messages).ok()
    })();
    canonical.unwrap_or_else(|| astra_core::Hash256::from_sha256(bytes).to_string())
}

fn read_checkpoint_config(path: &Path) -> Result<(CheckpointConfig, String), String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_READ_FAILED: {error}"))?;
    let config: CheckpointConfig = serde_json::from_slice(&bytes)
        .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_INVALID: {error}"))?;
    config.validate().map_err(|error| error.to_string())?;
    let config_hash = astra_core::Hash256::from_sha256(&bytes).to_string();
    if let Some(binding) = &config.tolerance_approval {
        let root = path.parent().unwrap_or_else(|| Path::new("."));
        let approval_path = root.join(&binding.relative_path);
        let approval_bytes = fs::read(&approval_path)
            .map_err(|error| format!("ASTRA_HEADLESS_TOLERANCE_APPROVAL_READ_FAILED: {error}"))?;
        if astra_core::Hash256::from_sha256(&approval_bytes).to_string() != binding.sha256 {
            return Err("ASTRA_HEADLESS_TOLERANCE_APPROVAL_HASH_MISMATCH".into());
        }
        let approval: astra_headless_protocol::ToleranceApproval =
            serde_json::from_slice(&approval_bytes)
                .map_err(|error| format!("ASTRA_HEADLESS_TOLERANCE_APPROVAL_INVALID: {error}"))?;
        approval.validate().map_err(|error| error.to_string())?;
        if approval.approved_tolerance_hash
            != config.tolerance_hash().map_err(|error| error.to_string())?
        {
            return Err("ASTRA_HEADLESS_TOLERANCE_APPROVAL_CONFIG_MISMATCH".into());
        }
    }
    Ok((config, config_hash))
}

fn observation_matches(
    predicate: &ObservationPredicate,
    observations: &[astra_product_host::Observation],
) -> bool {
    match predicate {
        ObservationPredicate::Equals { key, value_hash } => observations
            .iter()
            .any(|observation| observation.key == *key && observation.value_hash == *value_hash),
        ObservationPredicate::Exists { key } => observations
            .iter()
            .any(|observation| observation.key == *key),
    }
}

async fn consume_input(
    product: &mut dyn ProductSession,
    tick: u64,
    input: &PhysicalInput,
) -> Result<(Vec<astra_product_host::Observation>, u32), String> {
    let mut observations = product
        .consume(tick, input)
        .await
        .map_err(|error| error.to_string())?;
    if let PhysicalInput::Await {
        observation,
        timeout_ticks,
        ..
    } = input
    {
        let mut advanced_ticks = 0;
        for offset in 0..*timeout_ticks {
            if observation_matches(observation, &observations) {
                break;
            }
            let next_tick = tick
                .checked_add(u64::from(offset) + 1)
                .ok_or_else(|| "ASTRA_HEADLESS_AWAIT_TICK_OVERFLOW".to_string())?;
            observations = product
                .consume(next_tick, &PhysicalInput::AdvanceTicks { count: 1 })
                .await
                .map_err(|error| error.to_string())?;
            advanced_ticks = offset + 1;
        }
        if !observation_matches(observation, &observations) {
            let (key, expected) = match observation {
                ObservationPredicate::Equals { key, value_hash } => {
                    (key.as_str(), value_hash.as_str())
                }
                ObservationPredicate::Exists { key } => (key.as_str(), "exists"),
            };
            let actual = observations
                .iter()
                .filter(|candidate| candidate.key == key)
                .map(|candidate| candidate.value_hash.as_str())
                .collect::<Vec<_>>();
            return Err(format!(
                "ASTRA_HEADLESS_AWAIT_TIMEOUT: key={key} expected={expected} actual_hashes={actual:?}"
            ));
        }
        return Ok((observations, advanced_ticks));
    }
    Ok((observations, 0))
}

async fn capture_checkpoint(
    product: &mut dyn ProductSession,
    id: &str,
    sequence: u64,
    observations: &[astra_product_host::Observation],
    config: Option<&CheckpointConfig>,
    config_root: Option<&Path>,
    capture_audio: bool,
) -> Result<(CheckpointCapture, CheckpointResult), String> {
    let frame = product
        .capture_frame()
        .await
        .map_err(|error| error.to_string())?;
    let audio = if capture_audio {
        let audio = product.capture_audio().map_err(|error| error.to_string())?;
        if audio.sample_rate != 48_000
            || audio.channels != 2
            || !audio
                .samples
                .len()
                .is_multiple_of(usize::from(audio.channels))
            || audio.samples.iter().any(|sample| !sample.is_finite())
        {
            return Err("ASTRA_HEADLESS_CHECKPOINT_AUDIO_FORMAT_INVALID".into());
        }
        Some(audio)
    } else {
        None
    };
    let observation_hash = astra_core::Hash256::from_sha256(
        &serde_json::to_vec(observations).map_err(|error| error.to_string())?,
    )
    .to_string();
    let expected = config.and_then(|config| config.checkpoints.iter().find(|item| item.id == id));
    let (image_metrics, image_passed) = match expected.and_then(|item| {
        item.image_baseline_path
            .as_deref()
            .zip(item.image_baseline_hash.as_deref())
            .map(|pair| (item, pair))
    }) {
        Some((expectation, (path, hash))) => {
            let root =
                config_root.ok_or_else(|| "ASTRA_HEADLESS_CHECKPOINT_ROOT_MISSING".to_string())?;
            let (metrics, passed) = compare::compare_image(
                &frame.rgba8,
                frame.width,
                frame.height,
                &root.join(path),
                hash,
                expectation.image_tolerance,
            )?;
            (Some(metrics), passed)
        }
        None => (None, true),
    };
    let observation_passed = expected
        .and_then(|item| item.observation_hash.as_ref())
        .is_none_or(|expected| expected == &observation_hash);
    Ok((
        CheckpointCapture {
            sequence,
            frame,
            audio,
        },
        CheckpointResult {
            id: id.to_owned(),
            passed: observation_passed && image_passed,
            observation_hash,
            image_metrics,
            audio_metrics: None,
        },
    ))
}

fn should_capture_checkpoint_audio(
    profile: &HeadlessHostProfile,
    config: Option<&CheckpointConfig>,
    checkpoint_id: &str,
) -> bool {
    !matches!(
        profile.artifacts.retention,
        astra_platform::HeadlessArtifactRetention::ManifestOnly
    ) || config
        .and_then(|config| {
            config
                .checkpoints
                .iter()
                .find(|checkpoint| checkpoint.id == checkpoint_id)
        })
        .is_some_and(|checkpoint| checkpoint.audio_baseline_path.is_some())
}

fn validate_required_checkpoints(
    config: Option<&CheckpointConfig>,
    profile_required: &[String],
    results: &[CheckpointResult],
) -> Result<(), String> {
    if let Some(failed) = results.iter().find(|result| !result.passed) {
        return Err(format!("ASTRA_HEADLESS_CHECKPOINT_FAILED: {}", failed.id));
    }
    let config_required = config
        .into_iter()
        .flat_map(|config| config.checkpoints.iter())
        .filter(|checkpoint| checkpoint.required)
        .map(|checkpoint| checkpoint.id.as_str());
    for expected_id in profile_required
        .iter()
        .map(String::as_str)
        .chain(config_required)
    {
        let result = results
            .iter()
            .find(|result| result.id == expected_id)
            .ok_or_else(|| format!("ASTRA_HEADLESS_REQUIRED_CHECKPOINT_MISSING: {expected_id}"))?;
        debug_assert!(result.passed);
    }
    Ok(())
}

fn append_checkpoint_artifacts(
    root: &Path,
    profile: &HeadlessHostProfile,
    captures: &BTreeMap<String, CheckpointCapture>,
    manifest: &mut ArtifactManifest,
) -> Result<(), String> {
    if matches!(
        profile.artifacts.retention,
        astra_platform::HeadlessArtifactRetention::ManifestOnly
    ) {
        return Ok(());
    }
    let directory = root.join("checkpoints");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_ARTIFACT_ROOT_FAILED: {error}"))?;
    let selected = match profile.artifacts.retention {
        astra_platform::HeadlessArtifactRetention::Final => {
            captures.iter().next_back().into_iter().collect::<Vec<_>>()
        }
        _ => captures.iter().collect::<Vec<_>>(),
    };
    for (id, capture) in selected {
        let frame = &capture.frame;
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                &frame.rgba8,
                frame.width,
                frame.height,
                ExtendedColorType::Rgba8,
            )
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_PNG_FAILED: {error}"))?;
        if manifest.artifacts.len() as u64 >= profile.artifacts.max_artifacts {
            return Err("ASTRA_HEADLESS_ARTIFACT_COUNT_LIMIT_EXCEEDED".into());
        }
        let current_bytes = manifest
            .artifacts
            .iter()
            .map(|entry| match entry {
                ArtifactEntry::Frame { byte_size, .. } | ArtifactEntry::Audio { byte_size, .. } => {
                    *byte_size
                }
            })
            .sum::<u64>();
        if current_bytes
            .checked_add(bytes.len() as u64)
            .is_none_or(|total| total > profile.artifacts.max_total_bytes)
        {
            return Err("ASTRA_HEADLESS_ARTIFACT_BYTE_LIMIT_EXCEEDED".into());
        }
        let relative = format!("checkpoints/{id}.png");
        let path = root.join(&relative);
        let partial = path.with_extension("partial");
        fs::write(&partial, &bytes)
            .and_then(|_| fs::rename(&partial, &path))
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_ARTIFACT_WRITE_FAILED: {error}"))?;
        manifest.artifacts.push(ArtifactEntry::Frame {
            relative_path: relative,
            sha256: astra_core::Hash256::from_sha256(&bytes).to_string(),
            byte_size: bytes.len() as u64,
            width: frame.width,
            height: frame.height,
            color_space: "rgba8_srgb".into(),
            sequence: capture.sequence,
            checkpoint_ids: vec![id.clone()],
        });
        let audio = capture.audio.as_ref().ok_or_else(|| {
            "ASTRA_HEADLESS_CHECKPOINT_AUDIO_CAPTURE_MISSING: retained checkpoint requires audio"
                .to_string()
        })?;
        let audio_bytes = wav_bytes(&audio.samples)?;
        reserve_appended_artifact(profile, manifest, audio_bytes.len() as u64)?;
        let audio_relative = format!("checkpoints/{id}.wav");
        let audio_path = root.join(&audio_relative);
        let audio_partial = audio_path.with_extension("partial");
        fs::write(&audio_partial, &audio_bytes)
            .and_then(|_| fs::rename(&audio_partial, &audio_path))
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_AUDIO_WRITE_FAILED: {error}"))?;
        let frame_count = (audio.samples.len() / usize::from(audio.channels)) as u64;
        manifest.artifacts.push(ArtifactEntry::Audio {
            relative_path: audio_relative,
            sha256: astra_core::Hash256::from_sha256(&audio_bytes).to_string(),
            byte_size: audio_bytes.len() as u64,
            sample_rate: audio.sample_rate,
            channels: audio.channels,
            frame_count,
            duration_ns: frame_count
                .checked_mul(1_000_000_000)
                .and_then(|value| value.checked_div(u64::from(audio.sample_rate)))
                .ok_or_else(|| "ASTRA_HEADLESS_CHECKPOINT_AUDIO_DURATION_OVERFLOW".to_string())?,
            checkpoint: Some(id.clone()),
        });
    }
    Ok(())
}

fn reserve_appended_artifact(
    profile: &HeadlessHostProfile,
    manifest: &ArtifactManifest,
    added_bytes: u64,
) -> Result<(), String> {
    if manifest.artifacts.len() as u64 >= profile.artifacts.max_artifacts {
        return Err("ASTRA_HEADLESS_ARTIFACT_COUNT_LIMIT_EXCEEDED".into());
    }
    let current_bytes = manifest
        .artifacts
        .iter()
        .map(|entry| match entry {
            ArtifactEntry::Frame { byte_size, .. } | ArtifactEntry::Audio { byte_size, .. } => {
                *byte_size
            }
        })
        .sum::<u64>();
    if current_bytes
        .checked_add(added_bytes)
        .is_none_or(|total| total > profile.artifacts.max_total_bytes)
    {
        return Err("ASTRA_HEADLESS_ARTIFACT_BYTE_LIMIT_EXCEEDED".into());
    }
    Ok(())
}

fn apply_audio_comparisons(
    config: Option<&CheckpointConfig>,
    config_root: Option<&Path>,
    captures: &BTreeMap<String, CheckpointCapture>,
    results: &mut [CheckpointResult],
) -> Result<(), String> {
    let Some(config) = config else {
        return Ok(());
    };
    for result in results {
        let Some(expectation) = config.checkpoints.iter().find(|item| item.id == result.id) else {
            continue;
        };
        let Some((relative, hash)) = expectation
            .audio_baseline_path
            .as_deref()
            .zip(expectation.audio_baseline_hash.as_deref())
        else {
            continue;
        };
        let root =
            config_root.ok_or_else(|| "ASTRA_HEADLESS_CHECKPOINT_ROOT_MISSING".to_string())?;
        let actual = captures
            .get(&result.id)
            .ok_or_else(|| "ASTRA_HEADLESS_AUDIO_CHECKPOINT_MISSING".to_string())?;
        let audio = actual
            .audio
            .as_ref()
            .ok_or_else(|| "ASTRA_HEADLESS_AUDIO_CHECKPOINT_CAPTURE_MISSING".to_string())?;
        let (metrics, passed) = compare::compare_audio_samples(
            &audio.samples,
            &root.join(relative),
            hash,
            expectation.audio_tolerance,
        )?;
        result.audio_metrics = Some(metrics);
        result.passed &= passed;
    }
    Ok(())
}

fn wav_bytes(samples: &[f32]) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_WAV_OPEN_FAILED: {error}"))?;
        for sample in samples {
            if !sample.is_finite() {
                return Err("ASTRA_HEADLESS_CHECKPOINT_WAV_SAMPLE_INVALID".into());
            }
            let sample = sample.clamp(-1.0, 1.0);
            let scale = if sample < 0.0 { 32768.0 } else { 32767.0 };
            writer
                .write_sample((sample * scale).round() as i16)
                .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_WAV_WRITE_FAILED: {error}"))?;
        }
        writer
            .finalize()
            .map_err(|error| format!("ASTRA_HEADLESS_CHECKPOINT_WAV_FINALIZE_FAILED: {error}"))?;
    }
    Ok(cursor.into_inner())
}
fn read_profile(path: &Path, identity_hash: &str) -> Result<HeadlessHostProfile, String> {
    let profile: HeadlessHostProfile = serde_json::from_slice(
        &fs::read(path).map_err(|e| format!("ASTRA_HEADLESS_PROFILE_READ_FAILED: {e}"))?,
    )
    .map_err(|e| format!("ASTRA_HEADLESS_PROFILE_INVALID: {e}"))?;
    astra_platform::validate_headless_host_profile(&profile).map_err(|e| e.to_string())?;
    if profile.build_fingerprint != identity_hash {
        return Err("ASTRA_HEADLESS_BUILD_IDENTITY_MISMATCH".into());
    }
    Ok(profile)
}
fn provider_hash(profile: &HeadlessHostProfile) -> Result<String, String> {
    let bytes = serde_json::to_vec(&profile.providers).map_err(|e| e.to_string())?;
    Ok(format!("sha256:{}", sha256_hex(&bytes)))
}

fn render_policy_name(profile: &HeadlessHostProfile) -> &'static str {
    match profile.render_policy {
        astra_platform::HeadlessRenderPolicy::All => "all",
        astra_platform::HeadlessRenderPolicy::Checkpoints => "checkpoints",
    }
}

fn blocked_renderer_identity(profile: &HeadlessHostProfile) -> RendererExecutionIdentity {
    let gpu = profile.providers.renderer == "wgpu_offscreen";
    RendererExecutionIdentity {
        provider: profile.providers.renderer.clone(),
        backend: if gpu { "unavailable" } else { "cpu" }.into(),
        device_type: if gpu { "unavailable" } else { "cpu" }.into(),
        vendor_id: 0,
        device_id: 0,
        adapter_name_hash: empty_hash(),
        driver_identity_hash: empty_hash(),
    }
}

fn renderer_identity_hash(identity: &RendererExecutionIdentity) -> String {
    serde_json::to_vec(identity)
        .map(|bytes| format!("sha256:{:x}", Sha256::digest(bytes)))
        .unwrap_or_else(|_| empty_hash())
}
fn hash_file(path: &Path) -> Result<String, String> {
    Ok(format!(
        "sha256:{}",
        sha256_hex(&fs::read(path).map_err(|e| e.to_string())?)
    ))
}
fn empty_hash() -> String {
    format!("sha256:{}", sha256_hex(&[]))
}
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn is_hash(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn prepare_review(
    run_report_path: &Path,
    manifest_path: &Path,
    artifact_root: &Path,
    output: &Path,
) -> Result<(), String> {
    let report: RunReport = read_json(run_report_path, "ASTRA_HEADLESS_REVIEW_RUN_REPORT")?;
    report.validate().map_err(|error| error.to_string())?;
    let manifest: ArtifactManifest = read_json(manifest_path, "ASTRA_HEADLESS_REVIEW_MANIFEST")?;
    manifest.validate().map_err(|error| error.to_string())?;
    let run_report_hash = hash_file(run_report_path)?;
    let manifest_hash = hash_file(manifest_path)?;
    if manifest_hash != report.manifest_hash
        || manifest.run_id != report.run_id
        || manifest.build_fingerprint != report.build_fingerprint
        || manifest.package_hash != report.package_hash
        || manifest.input_sequence_hash != report.input_sequence_hash
        || manifest.renderer_identity_hash != report.renderer_identity_hash
        || manifest.render_policy != report.render_policy
        || manifest.submitted_frame_count != report.submitted_frame_count
        || manifest.rasterized_frame_count != report.rasterized_frame_count
        || manifest.submitted_scene_stream_hash != report.submitted_scene_stream_hash
        || manifest.rasterized_frame_stream_hash != report.rasterized_frame_stream_hash
        || manifest.audio_frame_count != report.audio_frame_count
    {
        return Err(
            "ASTRA_HEADLESS_REVIEW_IDENTITY: run report and artifact manifest do not match".into(),
        );
    }

    let mut frames = manifest
        .artifacts
        .iter()
        .filter_map(|entry| match entry {
            ArtifactEntry::Frame {
                relative_path,
                sha256,
                sequence,
                checkpoint_ids,
                ..
            } => Some((relative_path, sha256, *sequence, checkpoint_ids.as_slice())),
            ArtifactEntry::Audio { .. } => None,
        })
        .collect::<Vec<_>>();
    frames.sort_by_key(|(_, _, sequence, _)| *sequence);
    if frames.is_empty() {
        return Err(
            "ASTRA_HEADLESS_REVIEW_FRAMES: review requires retained frame artifacts".into(),
        );
    }
    let mut selected_frames = Vec::new();
    let mut push_frame = |role: ReviewArtifactRole,
                          frame: &(&String, &String, u64, &[String]),
                          checkpoint: Option<&str>| {
        selected_frames.push(ReviewArtifactSelection {
            role,
            relative_path: frame.0.clone(),
            sha256: frame.1.clone(),
            sequence: Some(frame.2),
            checkpoint: checkpoint.map(str::to_string),
        });
    };
    push_frame(
        ReviewArtifactRole::FirstFrame,
        frames.first().unwrap(),
        None,
    );
    push_frame(ReviewArtifactRole::LastFrame, frames.last().unwrap(), None);

    let required_checkpoints = report
        .checkpoint_results
        .iter()
        .map(|result| result.id.clone())
        .collect::<Vec<_>>();
    for checkpoint in &required_checkpoints {
        let frame = frames
            .iter()
            .find(|frame| frame.3.iter().any(|id| id == checkpoint))
            .ok_or_else(|| {
                format!(
                    "ASTRA_HEADLESS_REVIEW_CHECKPOINT: checkpoint {checkpoint} has no retained frame"
                )
            })?;
        push_frame(
            ReviewArtifactRole::RequiredCheckpoint,
            frame,
            Some(checkpoint),
        );
    }

    if let Some(maximum) = report.checkpoint_results.iter().max_by(|left, right| {
        let left = left
            .image_metrics
            .as_ref()
            .map_or(0.0, |metrics| metrics.changed_pixel_ratio);
        let right = right
            .image_metrics
            .as_ref()
            .map_or(0.0, |metrics| metrics.changed_pixel_ratio);
        left.total_cmp(&right)
    }) {
        if let Some(frame) = frames
            .iter()
            .find(|frame| frame.3.iter().any(|id| id == &maximum.id))
        {
            push_frame(
                ReviewArtifactRole::MaximumDifference,
                frame,
                Some(&maximum.id),
            );
        }
    }

    for failed in report
        .checkpoint_results
        .iter()
        .filter(|result| !result.passed)
    {
        if let Some((_, _, sequence, _)) = frames
            .iter()
            .find(|frame| frame.3.iter().any(|id| id == &failed.id))
        {
            for neighbor in frames.iter().filter(|frame| {
                frame.2.abs_diff(*sequence) == 1 && !frame.3.iter().any(|id| id == &failed.id)
            }) {
                push_frame(ReviewArtifactRole::FailureNeighbor, neighbor, None);
            }
        }
    }

    let selected_audio = manifest
        .artifacts
        .iter()
        .filter_map(|entry| match entry {
            ArtifactEntry::Audio {
                relative_path,
                sha256,
                checkpoint,
                ..
            } => Some(ReviewArtifactSelection {
                role: if checkpoint.is_some() {
                    ReviewArtifactRole::CheckpointAudio
                } else {
                    ReviewArtifactRole::FullAudio
                },
                relative_path: relative_path.clone(),
                sha256: sha256.clone(),
                sequence: None,
                checkpoint: checkpoint.clone(),
            }),
            ArtifactEntry::Frame { .. } => None,
        })
        .collect::<Vec<_>>();
    if !selected_audio
        .iter()
        .any(|artifact| artifact.role == ReviewArtifactRole::FullAudio)
    {
        return Err(
            "ASTRA_HEADLESS_REVIEW_AUDIO: review requires the complete WAV artifact".into(),
        );
    }
    let bundle = ReviewBundle {
        schema: HEADLESS_REVIEW_BUNDLE_SCHEMA.into(),
        run_report_hash,
        manifest_hash,
        automatic_passed: report.status == RunStatus::Passed,
        selected_frames,
        selected_audio,
        required_checkpoints,
    };
    bundle.validate().map_err(|error| error.to_string())?;
    for artifact in bundle.selected_frames.iter().chain(&bundle.selected_audio) {
        let path = artifact_root.join(&artifact.relative_path);
        if hash_file(&path)? != artifact.sha256 {
            return Err(format!(
                "ASTRA_HEADLESS_REVIEW_ARTIFACT_HASH: {}",
                artifact.relative_path
            ));
        }
    }
    write_atomic_json(output, &bundle)
}

fn validate_review(
    run_report_path: &Path,
    bundle_path: &Path,
    review_path: &Path,
) -> Result<(), String> {
    let report: RunReport = read_json(run_report_path, "ASTRA_HEADLESS_REVIEW_RUN_REPORT")?;
    report.validate().map_err(|error| error.to_string())?;
    let bundle: ReviewBundle = read_json(bundle_path, "ASTRA_HEADLESS_REVIEW_BUNDLE")?;
    bundle.validate().map_err(|error| error.to_string())?;
    let review: ReviewRecord = read_json(review_path, "ASTRA_HEADLESS_REVIEW_RECORD")?;
    review.validate().map_err(|error| error.to_string())?;
    let report_hash = hash_file(run_report_path)?;
    if report.status != RunStatus::Passed
        || !bundle.automatic_passed
        || bundle.run_report_hash != report_hash
        || review.run_report_hash != report_hash
        || bundle.required_checkpoints.iter().any(|checkpoint| {
            !review
                .checkpoints
                .iter()
                .any(|verdict| verdict.checkpoint == *checkpoint && verdict.passed)
        })
        || review.checkpoints.iter().any(|verdict| !verdict.passed)
    {
        return Err(
            "ASTRA_HEADLESS_REVIEW_BLOCKED: review cannot override automatic failure and must pass every required checkpoint".into(),
        );
    }
    Ok(())
}

fn link_preflight(
    headless_run_report_path: &Path,
    platform_run_identity_path: &Path,
    output: &Path,
) -> Result<(), String> {
    let headless: RunReport = read_json(
        headless_run_report_path,
        "ASTRA_HEADLESS_PREFLIGHT_RUN_REPORT",
    )?;
    headless.validate().map_err(|error| error.to_string())?;
    let platform: PlatformRunIdentity = read_json(
        platform_run_identity_path,
        "ASTRA_HEADLESS_PREFLIGHT_PLATFORM_IDENTITY",
    )?;
    platform.validate().map_err(|error| error.to_string())?;
    if headless.status != RunStatus::Passed
        || headless.build_fingerprint != platform.build_fingerprint
        || headless.package_hash != platform.cooked_package_hash
        || headless.input_sequence_hash != platform.input_sequence_hash
        || headless.scenario != platform.scenario
        || headless.target != platform.target
        || headless.content_identity != platform.content_identity
    {
        return Err(
            "ASTRA_HEADLESS_PREFLIGHT_IDENTITY: Headless and platform runs do not share the required identity".into(),
        );
    }
    let link = PreflightLink {
        schema: HEADLESS_PREFLIGHT_LINK_SCHEMA.into(),
        headless_run_report_hash: hash_file(headless_run_report_path)?,
        platform_run_report_hash: platform.run_report_hash,
        build_fingerprint: headless.build_fingerprint,
        cooked_package_hash: headless.package_hash,
        input_sequence_hash: headless.input_sequence_hash,
        scenario: headless.scenario,
        target: headless.target,
        content_identity: headless.content_identity,
        headless_profile_id: headless.profile_id,
        headless_session_id: headless.session_id,
        platform_profile_id: platform.profile_id,
        platform_session_id: platform.session_id,
    };
    link.validate().map_err(|error| error.to_string())?;
    write_atomic_json(output, &link)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, code: &str) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|error| format!("{code}: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{code}: {error}"))
}
fn write_atomic_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let partial = path.with_extension("partial");
    fs::write(
        &partial,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(partial, path).map_err(|e| e.to_string())
}

fn bootstrap_test_env(output: &Path, build_identity: &Path) -> Result<(), String> {
    let identity_hash = read_identity_hash(build_identity)?;
    fs::create_dir_all(output).map_err(|e| format!("ASTRA_HEADLESS_BOOTSTRAP_ROOT_FAILED: {e}"))?;
    let package =
        astra_package::PackageBuilder::build(astra_package::PackageBuildRequest::fixture(
            "astra.headless.empty",
            "headless-test",
            Vec::new(),
        ))
        .map_err(|e| format!("ASTRA_HEADLESS_EMPTY_PACKAGE_FAILED: {e}"))?;
    let package_path = output.join("empty.astrapkg");
    fs::write(&package_path, package.as_bytes())
        .map_err(|e| format!("ASTRA_HEADLESS_EMPTY_PACKAGE_WRITE_FAILED: {e}"))?;
    let package_hash = format!("sha256:{}", sha256_hex(package.as_bytes()));
    let mut profile = HeadlessHostProfile::reference(
        "headless-test",
        "astra.headless.empty",
        identity_hash,
        package_hash,
    );
    profile.id = "worktree-local-test".into();
    let profile_path = output.join("headless-profile.json");
    write_atomic_json(&profile_path, &profile)?;
    fs::create_dir_all(output.join("artifacts"))
        .map_err(|e| format!("ASTRA_HEADLESS_ARTIFACT_ROOT_FAILED: {e}"))?;
    println!(
        "{}",
        serde_json::json!({"schema":"astra.headless_test_environment.v1","profile":"headless-profile.json","package":"empty.astrapkg","artifact_root":"artifacts","profile_hash":profile.hash().map_err(|e| e.to_string())?})
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn prepare_product_profile(
    package_path: &Path,
    target: &str,
    product_profile: &str,
    id: &str,
    namespace: &str,
    viewport_width: u32,
    viewport_height: u32,
    manifest_only: bool,
    output: &Path,
    build_identity: &Path,
) -> Result<(), String> {
    if target.trim().is_empty()
        || product_profile.trim().is_empty()
        || id.trim().is_empty()
        || namespace.trim().is_empty()
        || viewport_width == 0
        || viewport_height == 0
    {
        return Err("ASTRA_HEADLESS_PREPARE_PROFILE_ARGUMENT_INVALID".into());
    }
    let identity_hash = read_identity_hash(build_identity)?;
    let package_bytes = fs::read(package_path)
        .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_PACKAGE_READ_FAILED: {error}"))?;
    let reader = astra_package::AstraContainerReader::new(&package_bytes)
        .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_PACKAGE_INVALID: {error}"))?;
    let manifest: astra_package::PackageManifest = reader
        .decode_postcard("package.manifest")
        .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_MANIFEST_INVALID: {error}"))?;
    if manifest.profile != product_profile {
        return Err("ASTRA_HEADLESS_PREPARE_PROFILE_PRODUCT_PROFILE_MISMATCH".into());
    }
    let mut profile = HeadlessHostProfile::reference(
        target,
        manifest.package_id,
        identity_hash,
        // Platform package sources verify the complete stored container bytes. The
        // package content root is an internal section identity and can differ from
        // the storage hash when container metadata is present.
        astra_core::Hash256::from_sha256(&package_bytes).to_string(),
    );
    profile.id = id.to_string();
    profile.product_profile = product_profile.to_string();
    profile.viewport_width = viewport_width;
    profile.viewport_height = viewport_height;
    profile.artifacts.namespace = namespace.to_string();
    if manifest_only {
        profile.artifacts.retention = astra_platform::HeadlessArtifactRetention::ManifestOnly;
    }
    astra_platform::validate_headless_host_profile(&profile).map_err(|error| error.to_string())?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_ROOT_FAILED: {error}"))?;
    }
    write_atomic_json(output, &profile)
        .map_err(|error| format!("ASTRA_HEADLESS_PREPARE_PROFILE_WRITE_FAILED: {error}"))?;
    println!(
        "{}",
        serde_json::json!({
            "schema": "astra.headless_product_profile_preparation.v1",
            "profile_hash": profile.hash().map_err(|error| error.to_string())?,
            "package_hash": profile.package_hash,
            "build_fingerprint": profile.build_fingerprint,
            "target": profile.target,
            "product_profile": profile.product_profile,
        })
    );
    Ok(())
}

fn product_registry(
    package_crypto: Option<Arc<dyn ContainerCryptoProvider>>,
) -> Result<ProductAdapterRegistry, String> {
    let mut registry = ProductAdapterRegistry::default();
    let factory = match package_crypto {
        Some(crypto) => NativeVnProductAdapterFactory::with_package_crypto(crypto),
        None => NativeVnProductAdapterFactory::default(),
    };
    registry
        .register(Arc::new(factory))
        .map_err(|error| error.to_string())?;
    Ok(registry)
}

fn source_package_crypto(
    package_bytes: &[u8],
    source_profile: Option<&Path>,
    source_root: Option<&Path>,
) -> Result<Option<Arc<dyn ContainerCryptoProvider>>, String> {
    let raw = astra_package::AstraContainerReader::new(package_bytes)
        .map_err(|error| format!("ASTRA_HEADLESS_PACKAGE_INSPECT_FAILED: {error}"))?;
    let source_locked = raw.has_section("source.unlock");
    match (source_locked, source_profile, source_root) {
        (false, None, None) => Ok(None),
        (false, _, _) => Err(
            "ASTRA_HEADLESS_SOURCE_UNLOCK_UNEXPECTED: plaintext package must not receive source input"
                .into(),
        ),
        (true, Some(profile_path), Some(root)) => {
            let policy: SourceUnlockPolicy = raw
                .decode_postcard("source.unlock")
                .map_err(|error| format!("ASTRA_HEADLESS_SOURCE_POLICY_INVALID: {error}"))?;
            let profile_bytes = fs::read(profile_path)
                .map_err(|_| "ASTRA_HEADLESS_SOURCE_PROFILE_READ_FAILED".to_string())?;
            let profile: SourceVerificationManifest = serde_json::from_slice(&profile_bytes)
                .map_err(|_| "ASTRA_HEADLESS_SOURCE_PROFILE_INVALID".to_string())?;
            let mut reader = HeadlessSourceReader::open(root)?;
            let provider = SourceFingerprintCryptoProvider::unlock(&policy, &profile, &mut reader)
                .map_err(|error| format!("ASTRA_HEADLESS_SOURCE_UNLOCK_FAILED: {error}"))?;
            Ok(Some(Arc::new(provider)))
        }
        (true, _, _) => Err(
            "ASTRA_HEADLESS_SOURCE_UNLOCK_REQUIRED: source-locked package requires --source-profile and --source-root"
                .into(),
        ),
    }
}

struct HeadlessSourceReader {
    root: PathBuf,
}

impl HeadlessSourceReader {
    fn open(root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|_| "ASTRA_HEADLESS_SOURCE_ROOT_INVALID".to_string())?;
        if !root.is_dir() {
            return Err("ASTRA_HEADLESS_SOURCE_ROOT_INVALID".into());
        }
        Ok(Self { root })
    }

    fn resolve(&self, relative_path: &str) -> Result<PathBuf, ContainerError> {
        astra_platform::validate_safe_relative_path(relative_path)
            .map_err(|_| ContainerError::Crypto("authorized source path is invalid".into()))?;
        let path = self
            .root
            .join(relative_path)
            .canonicalize()
            .map_err(|_| ContainerError::Crypto("authorized source file is missing".into()))?;
        if !path.starts_with(&self.root) {
            return Err(ContainerError::Crypto(
                "authorized source path escaped its root".into(),
            ));
        }
        Ok(path)
    }
}

impl AuthorizedSourceReader for HeadlessSourceReader {
    fn stat_relative(&mut self, relative_path: &str) -> Result<u64, ContainerError> {
        let path = self.resolve(relative_path)?;
        let metadata = path
            .metadata()
            .map_err(|_| ContainerError::Crypto("authorized source file is missing".into()))?;
        if !metadata.is_file() {
            return Err(ContainerError::Crypto(
                "authorized source entry is not a file".into(),
            ));
        }
        Ok(metadata.len())
    }

    fn read_relative(
        &mut self,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, ContainerError> {
        let path = self.resolve(relative_path)?;
        let metadata = path
            .metadata()
            .map_err(|_| ContainerError::Crypto("authorized source file is missing".into()))?;
        if !metadata.is_file() || metadata.len() > max_bytes {
            return Err(ContainerError::Crypto(
                "authorized source file exceeds its read bound".into(),
            ));
        }
        fs::read(path)
            .map_err(|_| ContainerError::Crypto("authorized source file read failed".into()))
    }
}

async fn open_product(
    registry: &ProductAdapterRegistry,
    profile: &HeadlessHostProfile,
    host: &PlatformHostSession,
    bytes: Vec<u8>,
) -> Result<Box<dyn ProductSession>, String> {
    if astra_core::Hash256::from_sha256(&bytes).to_string() != profile.package_hash {
        return Err("ASTRA_HEADLESS_PRODUCT_PACKAGE_HASH_MISMATCH".into());
    }
    registry
        .open(
            &profile.providers.product_adapter,
            ProductOpenRequest {
                package_bytes: bytes.into(),
                profile: profile.product_profile.clone(),
                target: profile.target.clone(),
                locale: None,
                width: profile.viewport_width,
                height: profile.viewport_height,
                max_video_frames: profile.max_video_frames,
                max_decode_output_bytes: profile.max_decode_output_bytes,
                retain_audio_timeline: !matches!(
                    profile.artifacts.retention,
                    astra_platform::HeadlessArtifactRetention::ManifestOnly
                ),
                platform: host.client.clone(),
            },
        )
        .await
        .map_err(|error| error.to_string())
}

async fn read_verified_package(
    host: &PlatformHostSession,
    handle: astra_platform::PackageSourceHandle,
    total_size: u64,
    max_range: usize,
) -> Result<Vec<u8>, String> {
    if total_size == 0 || max_range == 0 {
        return Err("ASTRA_HEADLESS_PACKAGE_SIZE_INVALID".into());
    }
    let capacity = usize::try_from(total_size)
        .map_err(|_| "ASTRA_HEADLESS_PACKAGE_SIZE_OVERFLOW".to_string())?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut offset = 0_u64;
    while offset < total_size {
        let remaining = usize::try_from((total_size - offset).min(max_range as u64))
            .map_err(|_| "ASTRA_HEADLESS_PACKAGE_RANGE_OVERFLOW".to_string())?;
        let chunk = host
            .client
            .read_package_range(handle, offset, remaining)
            .await
            .map_err(|error| error.to_string())?;
        if chunk.len() != remaining {
            return Err("ASTRA_HEADLESS_PACKAGE_RANGE_TRUNCATED".into());
        }
        bytes.extend_from_slice(&chunk);
        offset = offset
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| "ASTRA_HEADLESS_PACKAGE_RANGE_OVERFLOW".to_string())?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod evidence_tests {
    use std::fs;

    use astra_headless_protocol::{
        ArtifactEntry, ArtifactManifest, CheckpointResult, PlatformRunIdentity,
        RendererExecutionIdentity, ReviewRecord, ReviewVerdict, ReviewerKind, RunReport, RunStatus,
        HEADLESS_ARTIFACT_MANIFEST_SCHEMA, HEADLESS_REVIEW_SCHEMA, HEADLESS_RUN_REPORT_SCHEMA,
        PLATFORM_RUN_IDENTITY_SCHEMA,
    };

    use super::{hash_file, link_preflight, prepare_review, validate_review, write_atomic_json};

    fn hash(value: &[u8]) -> String {
        astra_core::Hash256::from_sha256(value).to_string()
    }

    #[astra_headless_test::test]
    fn review_bundle_and_preflight_link_require_complete_matching_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let frame = temp.path().join("frame.png");
        let audio = temp.path().join("audio.wav");
        fs::write(&frame, b"frame").unwrap();
        fs::write(&audio, b"audio").unwrap();
        let common = hash(b"common");
        let renderer_identity = RendererExecutionIdentity::cpu_reference();
        let renderer_identity_hash = renderer_identity.hash().unwrap();
        let manifest = ArtifactManifest {
            schema: HEADLESS_ARTIFACT_MANIFEST_SCHEMA.into(),
            run_id: "formal-run".into(),
            build_fingerprint: common.clone(),
            package_hash: hash(b"package"),
            input_sequence_hash: hash(b"input"),
            provider_identity_hash: hash(b"provider"),
            renderer_identity_hash: renderer_identity_hash.clone(),
            renderer_identity,
            render_policy: "checkpoints".into(),
            submitted_frame_count: 1,
            rasterized_frame_count: 1,
            audio_frame_count: 800,
            submitted_scene_stream_hash: hash(b"scene-stream"),
            rasterized_frame_stream_hash: hash(b"frame-stream"),
            audio_stream_hash: hash(b"audio-stream"),
            audio_peak_dbfs: Some(-1.0),
            audio_rms_dbfs: Some(-3.0),
            silence: false,
            clipping: false,
            artifacts: vec![
                ArtifactEntry::Frame {
                    relative_path: "frame.png".into(),
                    sha256: hash(b"frame"),
                    byte_size: 5,
                    width: 1,
                    height: 1,
                    color_space: "rgba8_srgb".into(),
                    sequence: 1,
                    checkpoint_ids: vec!["required.final".into()],
                },
                ArtifactEntry::Audio {
                    relative_path: "audio.wav".into(),
                    sha256: hash(b"audio"),
                    byte_size: 5,
                    sample_rate: 48_000,
                    channels: 2,
                    frame_count: 800,
                    duration_ns: 16_666_667,
                    checkpoint: None,
                },
            ],
        };
        let manifest_path = temp.path().join("artifact-manifest.json");
        write_atomic_json(&manifest_path, &manifest).unwrap();
        let report = RunReport {
            schema: HEADLESS_RUN_REPORT_SCHEMA.into(),
            run_id: "formal-run".into(),
            build_fingerprint: common.clone(),
            package_hash: manifest.package_hash.clone(),
            input_sequence_hash: manifest.input_sequence_hash.clone(),
            checkpoint_config_hash: hash(b"checkpoint-config"),
            profile_id: "headless-formal".into(),
            session_id: "headless-session".into(),
            scenario: "native-vn-route".into(),
            target: "native-vn-game".into(),
            content_identity: "native-vn-public".into(),
            status: RunStatus::Passed,
            manifest_hash: hash_file(&manifest_path).unwrap(),
            renderer_identity_hash,
            render_policy: manifest.render_policy.clone(),
            submitted_frame_count: 1,
            rasterized_frame_count: 1,
            submitted_scene_stream_hash: manifest.submitted_scene_stream_hash.clone(),
            rasterized_frame_stream_hash: manifest.rasterized_frame_stream_hash.clone(),
            audio_frame_count: 800,
            duration_ns: 16_666_667,
            completed_sequence: 4,
            checkpoint_results: vec![CheckpointResult {
                id: "required.final".into(),
                passed: true,
                observation_hash: hash(b"observation"),
                image_metrics: None,
                audio_metrics: None,
            }],
            diagnostics: Vec::new(),
        };
        let report_path = temp.path().join("run-report.json");
        write_atomic_json(&report_path, &report).unwrap();
        let bundle_path = temp.path().join("review-bundle.json");
        prepare_review(&report_path, &manifest_path, temp.path(), &bundle_path).unwrap();

        let review = ReviewRecord {
            schema: HEADLESS_REVIEW_SCHEMA.into(),
            run_report_hash: hash_file(&report_path).unwrap(),
            reviewer_kind: ReviewerKind::Human,
            reviewer_identity: "release-reviewer".into(),
            tool_identity_hash: hash(b"review-tool"),
            checkpoints: vec![ReviewVerdict {
                checkpoint: "required.final".into(),
                passed: true,
                diagnostic_codes: Vec::new(),
            }],
        };
        let review_path = temp.path().join("review.json");
        write_atomic_json(&review_path, &review).unwrap();
        validate_review(&report_path, &bundle_path, &review_path).unwrap();

        let platform = PlatformRunIdentity {
            schema: PLATFORM_RUN_IDENTITY_SCHEMA.into(),
            run_report_hash: hash(b"platform-report"),
            build_fingerprint: report.build_fingerprint.clone(),
            cooked_package_hash: report.package_hash.clone(),
            input_sequence_hash: report.input_sequence_hash.clone(),
            scenario: report.scenario.clone(),
            target: report.target.clone(),
            content_identity: report.content_identity.clone(),
            profile_id: "windows-release".into(),
            session_id: "windows-session".into(),
        };
        let platform_path = temp.path().join("platform.json");
        write_atomic_json(&platform_path, &platform).unwrap();
        let link_path = temp.path().join("preflight-link.json");
        link_preflight(&report_path, &platform_path, &link_path).unwrap();

        let mut mismatch = platform;
        mismatch.input_sequence_hash = hash(b"different-input");
        write_atomic_json(&platform_path, &mismatch).unwrap();
        assert!(link_preflight(&report_path, &platform_path, &link_path).is_err());
    }
}
