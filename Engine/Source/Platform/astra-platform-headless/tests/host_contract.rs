use std::fs;
use std::sync::Arc;

use astra_headless_protocol::ArtifactManifest;
use astra_media_core::{BlendMode, RectI, SceneCommand, TextureFrame};
#[cfg(feature = "ffmpeg-vcpkg")]
use astra_platform::DecodeOutput;
use astra_platform::{
    AudioOutputRequest, AudioPacket, DecodeKind, HeadlessHostProfile, HeadlessReadbackPolicy,
    HeadlessRenderPolicy, PackageSourcePolicy, PackageSourceRequest, PlatformDecodeRequest,
    PlatformErrorCode, PlatformHostFactory, RgbaFrame, SceneFrame, SurfaceRequest, WindowRequest,
};
use astra_platform_headless::HeadlessPlatformFactory;
use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn spawn_tls_range_server(
    payload: &'static [u8],
) -> (String, Vec<u8>, tokio::task::JoinHandle<()>) {
    if tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
    assert!(
        tokio_rustls::rustls::crypto::CryptoProvider::get_default().is_some(),
        "the TLS range-server fixture requires an explicit rustls crypto provider"
    );
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let certificate_pem = certified.cert.pem().into_bytes();
    let certificate = certified.cert.der().clone();
    let key = tokio_rustls::rustls::pki_types::PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    );
    let config = tokio_rustls::rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key.into())
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let mut stream = acceptor.accept(stream).await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0_u8; 1024];
                    let count = stream.read(&mut chunk).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..count]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                    assert!(request.len() <= 16 * 1024);
                }
                let request = std::str::from_utf8(&request).unwrap();
                let range = request
                    .lines()
                    .find_map(|line| line.strip_prefix("range: bytes="))
                    .unwrap();
                let (start, end) = range.split_once('-').unwrap();
                let start = start.parse::<usize>().unwrap();
                let end = end.parse::<usize>().unwrap();
                let body = &payload[start..=end];
                let header = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len(),
                    body.len()
                );
                stream.write_all(header.as_bytes()).await.unwrap();
                stream.write_all(body).await.unwrap();
                stream.shutdown().await.unwrap();
            });
        }
    });
    (
        format!("https://localhost:{}/package.astrapkg", address.port()),
        certificate_pem,
        task,
    )
}

fn hash(bytes: &[u8]) -> String {
    astra_core::Hash256::from_sha256(bytes).to_string()
}

#[tokio::test]
async fn checkpoint_policy_validates_every_scene_and_rasterizes_only_first_and_final() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.sparse",
        hash(b"build"),
        hash(b"package"),
    );
    let root = temp.path().join("artifacts");
    let session = HeadlessPlatformFactory::new(&root, temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let window = client
        .create_window(WindowRequest {
            title: "Sparse".into(),
            width: 2,
            height: 2,
            visible: false,
        })
        .await
        .unwrap();
    let surface = client
        .create_surface(SurfaceRequest {
            window,
            width: 2,
            height: 2,
        })
        .await
        .unwrap();
    for sequence in 1..=3 {
        client
            .present_scene(
                surface,
                SceneFrame {
                    sequence,
                    width: 2,
                    height: 2,
                    clear_rgba: [sequence as u8, 0, 0, 255],
                    commands: Vec::new(),
                    semantics: None,
                },
            )
            .await
            .unwrap();
    }
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();

    let manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(root.join("artifact-manifest.json")).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.submitted_frame_count, 3);
    assert_eq!(manifest.rasterized_frame_count, 2);
    assert_ne!(
        manifest.submitted_scene_stream_hash,
        manifest.rasterized_frame_stream_hash
    );
}

#[tokio::test]
async fn renderer_flag_and_profile_binding_must_match() {
    let temp = tempfile::tempdir().unwrap();
    let cpu = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.binding",
        hash(b"build"),
        hash(b"package"),
    );
    let cpu_error = HeadlessPlatformFactory::new(temp.path().join("cpu"), temp.path())
        .with_gpu(true)
        .start(cpu.into())
        .await
        .err()
        .expect("CPU profile with --gpu must block");
    assert_eq!(cpu_error.code, PlatformErrorCode::InvalidProfile);

    let mut gpu = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.binding",
        hash(b"build"),
        hash(b"package"),
    );
    gpu.providers.renderer = "wgpu_offscreen".into();
    let gpu_error = HeadlessPlatformFactory::new(temp.path().join("gpu"), temp.path())
        .start(gpu.into())
        .await
        .err()
        .expect("GPU profile without --gpu must block");
    assert_eq!(gpu_error.code, PlatformErrorCode::InvalidProfile);
}

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn gpu_profile_renders_captures_and_records_native_backend_identity() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-gpu-test",
        "com.example.gpu",
        hash(b"build"),
        hash(b"package"),
    );
    profile.providers.renderer = "wgpu_offscreen".into();
    let root = temp.path().join("artifacts");
    let session = HeadlessPlatformFactory::new(&root, temp.path())
        .with_gpu(true)
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let window = client
        .create_window(WindowRequest {
            title: "GPU".into(),
            width: 8,
            height: 8,
            visible: false,
        })
        .await
        .unwrap();
    let surface = client
        .create_surface(SurfaceRequest {
            window,
            width: 8,
            height: 8,
        })
        .await
        .unwrap();
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 1,
                width: 8,
                height: 8,
                clear_rgba: [4, 8, 16, 255],
                commands: vec![astra_media_core::SceneCommand::rect(
                    "gpu.rect",
                    1,
                    1,
                    6,
                    6,
                    [200, 100, 50, 255],
                )],
                semantics: None,
            },
        )
        .await
        .unwrap();
    let capture = client.capture_surface(surface).await.unwrap();
    assert_eq!(capture.rgba8.len(), 8 * 8 * 4);
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();
    let manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(root.join("artifact-manifest.json")).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.renderer_identity.provider, "wgpu_offscreen");
    assert_eq!(manifest.renderer_identity.backend, expected_gpu_backend());
    assert!(matches!(
        manifest.renderer_identity.device_type.as_str(),
        "discrete_gpu" | "integrated_gpu"
    ));
}

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn gpu_sparse_frames_defer_retained_resource_mutations_until_materialization() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-gpu-test",
        "com.example.gpu-retained",
        hash(b"build"),
        hash(b"package"),
    );
    profile.providers.renderer = "wgpu_offscreen".into();
    let session = HeadlessPlatformFactory::new(temp.path().join("artifacts"), temp.path())
        .with_gpu(true)
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let window = client
        .create_window(WindowRequest {
            title: "Retained".into(),
            width: 2,
            height: 2,
            visible: false,
        })
        .await
        .unwrap();
    let surface = client
        .create_surface(SurfaceRequest {
            window,
            width: 2,
            height: 2,
        })
        .await
        .unwrap();
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 1,
                width: 2,
                height: 2,
                clear_rgba: [0, 0, 0, 255],
                commands: Vec::new(),
                semantics: None,
            },
        )
        .await
        .unwrap();
    let pixels: Arc<[u8]> = [255, 0, 0, 255].repeat(4).into();
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 2,
                width: 2,
                height: 2,
                clear_rgba: [0, 0, 0, 255],
                commands: vec![SceneCommand::UploadTexture {
                    resource_id: "retained.texture".into(),
                    frame: TextureFrame {
                        width: 2,
                        height: 2,
                        hash: astra_core::Hash256::from_sha256(&pixels),
                        rgba8: pixels,
                    },
                }],
                semantics: None,
            },
        )
        .await
        .unwrap();
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 3,
                width: 2,
                height: 2,
                clear_rgba: [0, 0, 0, 255],
                commands: vec![SceneCommand::Sprite {
                    id: "retained.sprite".into(),
                    texture_id: "retained.texture".into(),
                    source: None,
                    destination: RectI::new(0, 0, 2, 2),
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                }],
                semantics: None,
            },
        )
        .await
        .unwrap();
    let captured = client.capture_surface(surface).await.unwrap();
    assert!(captured
        .rgba8
        .chunks_exact(4)
        .all(|pixel| pixel == [255, 0, 0, 255]));
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn performance_gpu_submits_every_frame_but_reads_back_only_checkpoints() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-performance-test",
        "com.example.performance",
        hash(b"build"),
        hash(b"package"),
    );
    profile.providers.renderer = "wgpu_offscreen".into();
    profile.render_policy = HeadlessRenderPolicy::All;
    profile.readback_policy = HeadlessReadbackPolicy::CheckpointsOnly;
    let root = temp.path().join("artifacts");
    let session = HeadlessPlatformFactory::new(&root, temp.path())
        .with_gpu(true)
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let window = client
        .create_window(WindowRequest {
            title: "GPU performance".into(),
            width: 8,
            height: 8,
            visible: false,
        })
        .await
        .unwrap();
    let surface = client
        .create_surface(SurfaceRequest {
            window,
            width: 8,
            height: 8,
        })
        .await
        .unwrap();
    for sequence in 1..=2 {
        client
            .present_scene(
                surface,
                SceneFrame {
                    sequence,
                    width: 8,
                    height: 8,
                    clear_rgba: [sequence as u8, 8, 16, 255],
                    commands: Vec::new(),
                    semantics: None,
                },
            )
            .await
            .unwrap();
    }
    let capture = client.capture_surface(surface).await.unwrap();
    assert_eq!(capture.rgba8.len(), 8 * 8 * 4);
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();
    let manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(root.join("artifact-manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest.submitted_frame_count, 2);
    assert_eq!(manifest.rasterized_frame_count, 1);
}

fn expected_gpu_backend() -> &'static str {
    if cfg!(target_os = "windows") {
        "dx12"
    } else if cfg!(target_os = "linux") {
        "vulkan"
    } else if cfg!(target_os = "macos") {
        "metal"
    } else {
        "unsupported"
    }
}

#[tokio::test]
async fn rejected_skipped_scene_does_not_advance_submitted_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.transaction",
        hash(b"build"),
        hash(b"package"),
    );
    let root = temp.path().join("artifacts");
    let session = HeadlessPlatformFactory::new(&root, temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let window = client
        .create_window(WindowRequest {
            title: "Transaction".into(),
            width: 2,
            height: 2,
            visible: false,
        })
        .await
        .unwrap();
    let surface = client
        .create_surface(SurfaceRequest {
            window,
            width: 2,
            height: 2,
        })
        .await
        .unwrap();
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 1,
                width: 2,
                height: 2,
                clear_rgba: [0, 0, 0, 255],
                commands: Vec::new(),
                semantics: None,
            },
        )
        .await
        .unwrap();
    let error = client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 2,
                width: 2,
                height: 2,
                clear_rgba: [0, 0, 0, 255],
                commands: vec![astra_media_core::SceneCommand::PopClip],
                semantics: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::InvalidState);
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();
    let manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(root.join("artifact-manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest.submitted_frame_count, 1);
    assert_eq!(manifest.rasterized_frame_count, 1);
}

#[tokio::test]
async fn executes_render_audio_save_package_and_zero_leak_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let package = b"real package bytes";
    fs::write(temp.path().join("fixture.astrapkg"), package).unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.headless",
        hash(b"build"),
        hash(package),
    );
    profile.artifacts.namespace = "contract".into();
    profile.artifacts.max_audio_frames = 1_600;
    profile.artifacts.max_duration_ns = 800 * 1_000_000_000 / 48_000;
    let session = HeadlessPlatformFactory::new(temp.path().join("artifacts"), temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;

    let package_handle = client
        .open_package(PackageSourceRequest::Bundled {
            relative_path: "fixture.astrapkg".into(),
            expected_hash: hash(package),
        })
        .await
        .unwrap();
    assert_eq!(
        client
            .read_package_range(package_handle, 5, 7)
            .await
            .unwrap(),
        b"package"
    );
    client.close_package(package_handle).await.unwrap();

    let save = client.begin_save("slot-a").await.unwrap();
    client.write_save(save, b"state".to_vec()).await.unwrap();
    assert_eq!(client.commit_save(save).await.unwrap(), hash(b"state"));
    assert_eq!(client.read_save("slot-a").await.unwrap(), b"state");
    client.delete_save("slot-a").await.unwrap();
    assert_eq!(
        client.read_save("slot-a").await.unwrap_err().code,
        PlatformErrorCode::Io
    );
    let aborted = client.begin_save("slot-aborted").await.unwrap();
    client
        .write_save(aborted, b"discarded".to_vec())
        .await
        .unwrap();
    client.abort_save(aborted).await.unwrap();
    assert_eq!(
        client.commit_save(aborted).await.unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );

    let window = client
        .create_window(WindowRequest {
            title: "Headless".into(),
            width: 2,
            height: 2,
            visible: false,
        })
        .await
        .unwrap();
    let surface = client
        .create_surface(SurfaceRequest {
            window,
            width: 2,
            height: 2,
        })
        .await
        .unwrap();
    let pixels = vec![
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 1,
                width: 2,
                height: 2,
                rgba8: pixels.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        client
            .capture_surface(surface)
            .await
            .unwrap()
            .rgba8
            .as_ref(),
        pixels.as_slice()
    );
    client
        .present_scene(
            surface,
            SceneFrame {
                sequence: 2,
                width: 2,
                height: 2,
                clear_rgba: [0, 0, 0, 255],
                semantics: None,
                commands: vec![astra_media_core::SceneCommand::rect(
                    "real-rect",
                    0,
                    0,
                    1,
                    1,
                    [12, 34, 56, 255],
                )],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        &client.capture_surface(surface).await.unwrap().rgba8[0..4],
        &[12, 34, 56, 255]
    );
    let duplicate = client
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 2,
                width: 2,
                height: 2,
                rgba8: vec![0; 16],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(duplicate.code, PlatformErrorCode::InvalidState);
    client.destroy_surface(surface).await.unwrap();
    assert_eq!(
        client.capture_surface(surface).await.unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );
    client.destroy_window(window).await.unwrap();

    let mut encoded_image = Vec::new();
    PngEncoder::new(&mut encoded_image)
        .write_image(
            &[255, 0, 0, 255, 0, 255, 0, 255],
            2,
            1,
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    let image = client.open_decode(DecodeKind::Image).await.unwrap();
    let decoded_image = client
        .decode(
            image,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Image,
                codec: "png".into(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                bytes: encoded_image,
            },
        )
        .await
        .unwrap();
    let astra_platform::DecodeOutput::CpuBuffer {
        format,
        bytes,
        hash: output_hash,
    } = decoded_image
    else {
        panic!("headless image decode returned a native media frame");
    };
    assert_eq!(format, "rgba8");
    assert_eq!(bytes, [255, 0, 0, 255, 0, 255, 0, 255]);
    assert_eq!(output_hash, hash(&bytes));
    client.close_decode(image).await.unwrap();

    let video = client.open_decode(DecodeKind::Video).await.unwrap();
    let video_error = client
        .decode(
            video,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Video,
                codec: "h264".into(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: Some(2),
                coded_height: Some(2),
                keyframe: true,
                bytes: vec![1, 2, 3],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(video_error.code, PlatformErrorCode::ProviderUnavailable);
    client.close_decode(video).await.unwrap();

    let audio = client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 800,
        })
        .await
        .unwrap();
    let samples = (0..800)
        .flat_map(|frame| {
            let value = ((frame as f32 / 48_000.0) * 440.0 * std::f32::consts::TAU).sin() * 0.25;
            [value, value]
        })
        .collect();
    client
        .submit_audio(
            audio,
            AudioPacket {
                sequence: 1,
                channels: 2,
                samples,
            },
        )
        .await
        .unwrap();
    client.pause_audio(audio).await.unwrap();
    assert_eq!(client.query_audio(audio).await.unwrap().consumed_samples, 0);
    client.resume_audio(audio).await.unwrap();
    let state = client.query_audio(audio).await.unwrap();
    assert_eq!(state.callback_count, 1);
    assert_eq!(state.queued_frames, 0);
    assert!(client
        .drain_audio(audio)
        .await
        .unwrap()
        .peak_dbfs
        .is_finite());
    client.close_audio(audio).await.unwrap();
    assert_eq!(
        client.close_audio(audio).await.unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );

    // Recreated outputs restart their packet sequence after restore. Artifact paths must
    // remain unique for the whole host session instead of reusing the packet sequence.
    let restored_audio = client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 800,
        })
        .await
        .unwrap();
    client
        .submit_audio(
            restored_audio,
            AudioPacket {
                sequence: 1,
                channels: 2,
                samples: vec![0.125; 1_600],
            },
        )
        .await
        .unwrap();
    client.close_audio(restored_audio).await.unwrap();
    client.shutdown().await.unwrap();

    assert!(temp
        .path()
        .join("artifacts/frames/frame-0000000001.png")
        .is_file());
    assert!(temp
        .path()
        .join("artifacts/audio/output-0000000001-source-0000000001.wav")
        .is_file());
    assert!(temp
        .path()
        .join("artifacts/audio/output-0000000002-source-0000000001.wav")
        .is_file());
    assert!(temp
        .path()
        .join("artifacts/artifact-manifest.json")
        .is_file());
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[tokio::test]
async fn ffmpeg_video_decode_returns_the_complete_ordered_frame_stream() {
    let temp = tempfile::tempdir().unwrap();
    let package = b"video package identity";
    fs::write(temp.path().join("fixture.astrapkg"), package).unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-video-test",
        "fixture.astrapkg",
        hash(b"build"),
        hash(package),
    );
    profile.providers.video_decode = "ffmpeg-vcpkg".into();
    profile.max_video_frames = 1_000;
    profile.max_decode_output_bytes = 512 * 1024 * 1024;
    let factory = HeadlessPlatformFactory::new(temp.path().join("run"), temp.path());
    let host = factory.start(profile.into()).await.unwrap();
    let client = host.client.clone();
    let session = client.open_decode(DecodeKind::Video).await.unwrap();
    let bytes = include_bytes!("../../../../Fixtures/PublicDomainMedia/flower.mp4").to_vec();
    let decoded = client
        .decode(
            session,
            PlatformDecodeRequest {
                sequence: 1,
                kind: DecodeKind::Video,
                codec: "mp4".into(),
                description: Vec::new(),
                sample_rate: None,
                channels: None,
                coded_width: None,
                coded_height: None,
                keyframe: true,
                bytes,
            },
        )
        .await
        .unwrap();
    let DecodeOutput::CpuBuffer {
        format,
        bytes,
        hash: output_hash,
    } = decoded
    else {
        panic!("headless FFmpeg returned a native media frame");
    };
    assert_eq!(format, "postcard:astra.decoded_video_stream.v1");
    assert_eq!(output_hash, hash(&bytes));
    let stream = astra_media::DecodedVideoStream::decode(&bytes, 1_000, 512 * 1024 * 1024).unwrap();
    assert!(stream.frames.len() > 2);
    assert_ne!(
        stream.frames.first().unwrap().content_hash,
        stream.frames.last().unwrap().content_hash
    );
    client.close_decode(session).await.unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejects_legacy_profile_shape_and_audio_limit_before_commit() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.profile",
        hash(b"build"),
        hash(b"package"),
    );
    let mut legacy = serde_json::to_value(&profile).unwrap();
    legacy.as_object_mut().unwrap().remove("product_profile");
    assert!(serde_json::from_value::<HeadlessHostProfile>(legacy).is_err());

    let mut unavailable = profile.clone();
    unavailable.providers.renderer = "missing_renderer".into();
    assert_eq!(
        HeadlessPlatformFactory::new(temp.path().join("missing"), temp.path())
            .start(unavailable.into())
            .await
            .err()
            .unwrap()
            .code,
        PlatformErrorCode::ProviderUnavailable
    );

    #[cfg(not(feature = "ffmpeg-vcpkg"))]
    {
        let mut unavailable = profile.clone();
        unavailable.providers.video_decode = "ffmpeg-vcpkg".into();
        assert_eq!(
            HeadlessPlatformFactory::new(temp.path().join("video"), temp.path())
                .start(unavailable.into())
                .await
                .err()
                .unwrap()
                .code,
            PlatformErrorCode::ProviderUnavailable
        );
    }

    let mut limited = profile;
    limited.artifacts.max_audio_frames = 799;
    let session = HeadlessPlatformFactory::new(temp.path(), temp.path())
        .start(limited.into())
        .await
        .unwrap();
    let audio = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 800,
        })
        .await
        .unwrap();
    let error = session
        .client
        .submit_audio(
            audio,
            AudioPacket {
                sequence: 1,
                channels: 2,
                samples: vec![0.0; 1_600],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, PlatformErrorCode::QueueOverflow);
    session.client.abort_audio(audio).await.unwrap();
    session.client.shutdown().await.unwrap();

    let mut close_limited = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.close-limit",
        hash(b"build"),
        hash(b"package"),
    );
    close_limited.artifacts.max_audio_frames = 1_000;
    let session = HeadlessPlatformFactory::new(temp.path().join("close-limit"), temp.path())
        .start(close_limited.into())
        .await
        .unwrap();
    let first = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 800,
        })
        .await
        .unwrap();
    let second = session
        .client
        .open_audio_output(AudioOutputRequest {
            sample_rate: 48_000,
            channels: 2,
            max_buffered_frames: 800,
        })
        .await
        .unwrap();
    for output in [first, second] {
        session
            .client
            .submit_audio(
                output,
                AudioPacket {
                    sequence: 1,
                    channels: 2,
                    samples: vec![0.0; 1_600],
                },
            )
            .await
            .unwrap();
    }
    session.client.close_audio(first).await.unwrap();
    assert_eq!(
        session.client.close_audio(second).await.unwrap_err().code,
        PlatformErrorCode::QueueOverflow
    );
    assert_eq!(
        session.client.close_audio(second).await.unwrap_err().code,
        PlatformErrorCode::StaleHandle
    );
    session.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_blocks_live_resource_leaks() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.leak",
        hash(b"build"),
        hash(b"package"),
    );
    let session = HeadlessPlatformFactory::new(temp.path(), temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let window = session
        .client
        .create_window(WindowRequest {
            title: "Leak".into(),
            width: 1,
            height: 1,
            visible: false,
        })
        .await
        .unwrap();
    assert_eq!(
        session.client.shutdown().await.unwrap_err().code,
        PlatformErrorCode::ResourceLeak
    );
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn package_save_and_handle_boundaries_fail_closed_and_reopen_transactionally() {
    let temp = tempfile::tempdir().unwrap();
    let package = b"bounded package source";
    let package_path = temp.path().join("authorized.astrapkg");
    fs::write(&package_path, package).unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-boundaries",
        "com.example.boundaries",
        hash(b"build"),
        hash(package),
    );
    profile
        .package_sources
        .push(PackageSourcePolicy::UserAuthorized);
    let run_root = temp.path().join("run");
    let session = HeadlessPlatformFactory::new(&run_root, temp.path())
        .with_user_authorized_package(&package_path)
        .start(profile.clone().into())
        .await
        .unwrap();
    let client = session.client;

    let authorized = client
        .open_package(PackageSourceRequest::UserAuthorized {
            expected_hash: hash(package),
        })
        .await
        .unwrap();
    assert_eq!(
        client.read_package_range(authorized, 8, 7).await.unwrap(),
        b"package"
    );
    assert_eq!(
        client
            .read_package_range(authorized, 0, 0)
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::InvalidState
    );
    client.close_package(authorized).await.unwrap();
    assert_eq!(
        client
            .read_package_range(authorized, 0, 1)
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::StaleHandle
    );
    assert_eq!(
        client
            .open_package(PackageSourceRequest::Bundled {
                relative_path: "../authorized.astrapkg".into(),
                expected_hash: hash(package),
            })
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::PermissionDenied
    );
    assert_eq!(
        client
            .open_package(PackageSourceRequest::Bundled {
                relative_path: "authorized.astrapkg".into(),
                expected_hash: hash(b"different"),
            })
            .await
            .unwrap_err()
            .code,
        PlatformErrorCode::IntegrityMismatch
    );

    let save = client.begin_save("persistent-slot").await.unwrap();
    client
        .write_save(save, b"persisted".to_vec())
        .await
        .unwrap();
    client.commit_save(save).await.unwrap();
    client.shutdown().await.unwrap();

    let reopened = HeadlessPlatformFactory::new(&run_root, temp.path())
        .start(profile.into())
        .await
        .unwrap();
    assert_eq!(
        reopened.client.list_saves().await.unwrap(),
        ["persistent-slot"]
    );
    assert_eq!(
        reopened.client.read_save("persistent-slot").await.unwrap(),
        b"persisted"
    );
    reopened.client.shutdown().await.unwrap();

    let occupied_root = temp.path().join("occupied-root");
    fs::write(&occupied_root, b"not a directory").unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-conflict",
        "com.example.conflict",
        hash(b"build"),
        hash(package),
    );
    assert!(HeadlessPlatformFactory::new(occupied_root, temp.path())
        .start(profile.into())
        .await
        .is_err());
}

#[tokio::test]
async fn https_package_source_uses_strict_verified_byte_ranges() {
    static PACKAGE: &[u8] = b"deterministic TLS package range fixture";
    let (url, certificate, server) = spawn_tls_range_server(PACKAGE).await;
    let temp = tempfile::tempdir().unwrap();
    let origin = url::Url::parse(&url)
        .unwrap()
        .origin()
        .ascii_serialization();
    let mut profile = HeadlessHostProfile::reference(
        "headless-https-range",
        "com.example.https-range",
        hash(b"build"),
        hash(PACKAGE),
    );
    profile
        .package_sources
        .push(PackageSourcePolicy::HttpsRange {
            allowed_origins: vec![origin],
        });
    profile.limits.max_package_read_bytes = 8;
    let session = HeadlessPlatformFactory::new(temp.path().join("run"), temp.path())
        .with_https_root_certificate_pem(certificate)
        .start(profile.into())
        .await
        .unwrap();
    let source = session
        .client
        .open_package(PackageSourceRequest::HttpsRange {
            url,
            expected_hash: hash(PACKAGE),
        })
        .await
        .unwrap();
    assert_eq!(
        session
            .client
            .read_package_range(source, 14, 8)
            .await
            .unwrap(),
        b"TLS pack"
    );
    session.client.close_package(source).await.unwrap();
    session.client.shutdown().await.unwrap();
    server.abort();
}
