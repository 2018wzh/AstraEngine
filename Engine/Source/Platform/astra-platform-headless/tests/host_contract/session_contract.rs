use super::*;

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
pub(super) async fn gpu_atlas_repack_preserves_reserved_white_texel_for_solid_meshes() {
    let temp = tempfile::tempdir().unwrap();
    let mut profile = HeadlessHostProfile::reference(
        "headless-gpu-test",
        "com.example.gpu-solid-after-repack",
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
            title: "Solid after atlas repack".into(),
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
                clear_rgba: [0, 0, 0, 255],
                commands: vec![SceneCommand::UploadTexture {
                    resource_id: "wide.texture".into(),
                    frame: TextureFrame::from_vec(1200, 1, vec![255; 1200 * 4]).unwrap(),
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
                sequence: 2,
                width: 8,
                height: 8,
                clear_rgba: [0, 0, 0, 255],
                commands: vec![SceneCommand::MeshBatch2D {
                    vertices: vec![
                        MeshVertex2D {
                            position: [0.0, 0.0],
                            uv: [0.0, 0.0],
                            premultiplied_rgba: [128; 4],
                        },
                        MeshVertex2D {
                            position: [8.0, 0.0],
                            uv: [0.0, 0.0],
                            premultiplied_rgba: [128; 4],
                        },
                        MeshVertex2D {
                            position: [0.0, 8.0],
                            uv: [0.0, 0.0],
                            premultiplied_rgba: [128; 4],
                        },
                        MeshVertex2D {
                            position: [8.0, 8.0],
                            uv: [0.0, 0.0],
                            premultiplied_rgba: [128; 4],
                        },
                    ]
                    .into(),
                    indices: vec![0, 1, 2, 2, 1, 3].into(),
                    draws: vec![MeshDraw2D {
                        vertex_start: 0,
                        vertex_count: 4,
                        index_start: 0,
                        index_count: 6,
                        material: MeshMaterial2D::Solid,
                        texture_id: None,
                        texture_filter: TextureFilter2D::Linear,
                        opacity: 1.0,
                        blend: BlendMode::Alpha,
                        scissor: None,
                    }]
                    .into(),
                    compositing: SceneCompositing2D::EncodedSrgb,
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
        .all(|pixel| *pixel == [128, 128, 128, 255]));
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();
}

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
pub(super) async fn performance_gpu_submits_every_frame_but_reads_back_only_checkpoints() {
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

pub(super) fn expected_gpu_backend() -> &'static str {
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
pub(super) async fn rejected_skipped_scene_does_not_advance_submitted_evidence() {
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
