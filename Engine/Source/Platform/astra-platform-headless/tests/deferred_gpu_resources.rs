use astra_media_core::{BlendMode, RectI, SceneCommand, TextureFrame};
use astra_platform::{
    HeadlessHostProfile, HeadlessRenderPolicy, PlatformHostFactory, SceneFrame, SurfaceRequest,
    WindowRequest,
};
use astra_platform_headless::HeadlessPlatformFactory;

#[tokio::test]
#[ignore = "requires a native hardware GPU runner"]
async fn skipped_frames_preserve_release_and_reupload_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let hash = format!("sha256:{}", "0".repeat(64));
    let mut profile = HeadlessHostProfile::reference(
        "deferred-textures",
        "com.example.deferred",
        hash.clone(),
        hash,
    );
    profile.providers.renderer = "wgpu_offscreen".into();
    profile.render_policy = HeadlessRenderPolicy::Checkpoints;
    let session = HeadlessPlatformFactory::new(temp.path().join("artifacts"), temp.path())
        .with_gpu(true)
        .start(profile.into())
        .await
        .unwrap();
    let client = session.client;
    let window = client
        .create_window(WindowRequest {
            title: "Deferred textures".into(),
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
    for sequence in 1..=7 {
        let commands = if sequence % 2 == 0 {
            vec![SceneCommand::ReleaseResource {
                resource_id: "image".into(),
            }]
        } else {
            vec![
                SceneCommand::UploadTexture {
                    resource_id: "image".into(),
                    frame: TextureFrame::from_vec(
                        1,
                        1,
                        if sequence == 1 {
                            vec![255, 0, 0, 255]
                        } else {
                            vec![0, 255, 0, 255]
                        },
                    )
                    .unwrap(),
                },
                SceneCommand::Sprite {
                    id: "sprite".into(),
                    texture_id: "image".into(),
                    source: None,
                    destination: RectI {
                        x: 0,
                        y: 0,
                        width: 8,
                        height: 8,
                    },
                    opacity: 1.0,
                    blend: BlendMode::Alpha,
                },
            ]
        };
        if sequence == 3 {
            let mut invalid_commands = commands.clone();
            invalid_commands.push(SceneCommand::PopClip);
            let error = client
                .present_scene(
                    surface,
                    SceneFrame {
                        sequence,
                        width: 8,
                        height: 8,
                        clear_rgba: [0, 0, 0, 255],
                        commands: invalid_commands,
                        semantics: None,
                    },
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, astra_platform::PlatformErrorCode::InvalidState);
        }
        client
            .present_scene(
                surface,
                SceneFrame {
                    sequence,
                    width: 8,
                    height: 8,
                    clear_rgba: [0, 0, 0, 255],
                    commands,
                    semantics: None,
                },
            )
            .await
            .unwrap();
    }
    let captured = client.capture_surface(surface).await;
    client.destroy_surface(surface).await.unwrap();
    client.destroy_window(window).await.unwrap();
    client.shutdown().await.unwrap();
    let captured = captured.unwrap();
    assert!(captured
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| *pixel == [0, 255, 0, 255]));
}
