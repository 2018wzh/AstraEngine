use super::*;

pub(super) async fn spawn_tls_range_server(
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

pub(super) fn hash(bytes: &[u8]) -> String {
    astra_core::Hash256::from_sha256(bytes).to_string()
}

#[tokio::test]
pub(super) async fn surface_without_a_submission_can_be_destroyed() {
    let temp = tempfile::tempdir().unwrap();
    let profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.empty-surface",
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
            title: "Empty Surface".into(),
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

    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();

    let manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(root.join("artifact-manifest.json")).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.submitted_frame_count, 0);
    assert_eq!(manifest.rasterized_frame_count, 0);
}

#[tokio::test]
pub(super) async fn checkpoint_policy_validates_every_scene_and_rasterizes_only_first_and_final() {
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
}

#[tokio::test]
pub(super) async fn checkpoint_retention_hashes_frames_without_persisting_every_frame() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-test",
        "com.example.checkpoint-retention",
        hash(b"build"),
        hash(b"package"),
    );
    profile.artifacts.retention = HeadlessArtifactRetention::Checkpoints;
    let root = temp.path().join("artifacts");
    let session = HeadlessPlatformFactory::new(&root, temp.path())
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let window = client
        .create_window(WindowRequest {
            title: "Checkpoint retention".into(),
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
        .present_rgba(
            surface,
            RgbaFrame {
                sequence: 1,
                width: 2,
                height: 2,
                rgba8: vec![255; 16],
            },
        )
        .await
        .unwrap();
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();

    let manifest: ArtifactManifest =
        serde_json::from_slice(&fs::read(root.join("artifact-manifest.json")).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.rasterized_frame_count, 1);
    assert!(manifest.artifacts.iter().all(|artifact| !matches!(
        artifact,
        astra_headless_protocol::ArtifactEntry::Frame { .. }
    )));
    assert!(!root.join("frames/frame-0000000001.png").exists());
}

#[tokio::test]
pub(super) async fn renderer_flag_and_profile_binding_must_match() {
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
pub(super) async fn gpu_profile_renders_captures_and_records_native_backend_identity() {
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
pub(super) async fn gpu_sparse_frames_defer_retained_resource_mutations_until_materialization() {
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
    let pixels = [255, 0, 0, 255].repeat(4);
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
                    frame: TextureFrame::from_vec(2, 2, pixels).unwrap(),
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
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| *pixel == [255, 0, 0, 255]));
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();
}
