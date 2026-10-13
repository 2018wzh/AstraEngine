use super::*;

#[test]
pub(super) fn product_source_persists_resize_scale_and_text_across_repaints() {
    let mut source = source_for(STORY);
    source.launch().unwrap();
    let resized = source
        .dispatch_ui_event(UiInputEventKind::Resize {
            viewport: UiViewport {
                physical_width: 640,
                physical_height: 360,
                scale_factor: 2.0,
                font_scale: 1.0,
                safe_area_points: UiInsets {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                },
            },
        })
        .unwrap();
    assert!(scene_commands(&resized)
        .iter()
        .any(|command| matches!(command, SceneCommand::GlyphRun { .. })));

    let repaint = source
        .dispatch_ui_event(UiInputEventKind::FixedTime {
            time_ns: 33_333_334,
        })
        .unwrap();
    assert!(scene_commands(&repaint)
        .iter()
        .any(|command| matches!(command, SceneCommand::GlyphRun { .. })));

    let semantics = source.ui_semantics().expect("semantic snapshot");
    semantics.validate().unwrap();
    let root = semantics
        .nodes
        .iter()
        .find(|node| node.id == semantics.root_id)
        .expect("semantic root");
    assert_eq!(root.bounds_points.min.x, 0.0);
    assert_eq!(root.bounds_points.min.y, 0.0);
    assert_eq!(root.bounds_points.max.x, 320.0);
    assert_eq!(root.bounds_points.max.y, 180.0);

    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

pub(super) fn scene_commands(batch: &astra_player_core::PlayerHostCommandBatch) -> &[SceneCommand] {
    match &batch.commands[0] {
        PlayerHostCommand::PresentScene { commands, .. } => commands,
        command => panic!("expected retained scene presentation, got {command:?}"),
    }
}

#[test]
pub(super) fn packaged_native_vn_source_shapes_localized_text_into_retained_scene_commands() {
    let bytes = product_package();
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    )
    .unwrap();

    let first = source.launch().unwrap();
    let second = advance(&mut source);
    assert!(scene_commands(&first)
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    for commands in [scene_commands(&first), scene_commands(&second)] {
        assert!(commands
            .iter()
            .any(|command| matches!(command, SceneCommand::GlyphRun { .. })));
        assert!(!commands.iter().any(|command| {
            matches!(
                command,
                SceneCommand::Glyph { .. }
                    | SceneCommand::Texture { .. }
                    | SceneCommand::VideoFrame { .. }
            )
        }));
    }
    assert_ne!(scene_commands(&first), scene_commands(&second));
    let shutdown = source.release_resources().unwrap();
    assert!(scene_commands(&shutdown)
        .iter()
        .any(|command| matches!(command, SceneCommand::ReleaseResource { .. })));
    source.shutdown().unwrap();
}

#[test]
pub(super) fn packaged_native_vn_stage_uses_product_director_and_package_texture() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    stage viewport:640x360 safe_area:16:9 #@id stage.main
    layer id:bg kind:background z:0 blend:normal clip:stage #@id layer.bg
    background asset:asset:/background/apartment-night layer:bg duration:0 interrupt:replace_from_current #@id background.main
    text key:line.one speaker:hero #@id line.one
"#,
    );
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    )
    .unwrap();

    assert_eq!(source.decoded_asset_cache_bytes(), 0);
    let launch = source.launch().unwrap();
    assert!(source.decoded_asset_cache_bytes() <= 128 * 1024 * 1024);
    let commands = scene_commands(&launch);
    assert!(commands.iter().any(|command| matches!(
        command,
        SceneCommand::UploadTexture { resource_id, .. }
            if resource_id == "asset:/background/apartment-night"
    )));
    assert!(commands.iter().any(|command| matches!(
        command,
        SceneCommand::Sprite { texture_id, destination, .. }
            if texture_id == "asset:/background/apartment-night"
                && destination.width == 640
                && destination.height == 360
    )));
    assert!(commands
        .iter()
        .any(|command| matches!(command, SceneCommand::PushClip { .. })));
    let stable = advance(&mut source);
    assert!(!source
        .decoded_image_cached("asset:/background/apartment-night")
        .unwrap());
    assert!(!scene_commands(&stable).iter().any(|command| matches!(
        command,
        SceneCommand::UploadTexture { resource_id, .. }
            if resource_id == "asset:/background/apartment-night"
    )));
    source.invalidate_gpu_resources();
    let rebuilt = source
        .dispatch_ui_event(UiInputEventKind::Resize {
            viewport: UiViewport {
                physical_width: 640,
                physical_height: 360,
                scale_factor: 1.0,
                font_scale: 1.0,
                safe_area_points: UiInsets {
                    left: 0.0,
                    top: 0.0,
                    right: 0.0,
                    bottom: 0.0,
                },
            },
        })
        .unwrap();
    assert!(scene_commands(&rebuilt).iter().any(|command| matches!(
        command,
        SceneCommand::UploadTexture { resource_id, .. }
            if resource_id == "asset:/background/apartment-night"
    )));
    assert!(scene_commands(&rebuilt)
        .iter()
        .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
pub(super) fn packaged_native_vn_stage_composes_a_typed_director_transition() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene first #@id scene.first
    stage viewport:640x360 safe_area:16:9 #@id stage.main
    layer id:bg kind:background z:0 blend:normal clip:stage #@id layer.bg
    background asset:asset:/background/apartment-night layer:bg duration:0 interrupt:replace_from_current #@id background.first
    text key:line.one speaker:hero #@id line.one
  scene second #@id scene.second
    transition preset:director_puppet_1 duration:250 descriptor:director.puppet.1 #@id transition.wipe
    background asset:asset:/background/apartment-night layer:bg duration:0 interrupt:replace_from_current #@id background.second
    text key:line.two speaker:hero #@id line.two
"#,
    );
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    )
    .unwrap();

    source.launch().unwrap();
    for _ in 0..4 {
        advance(&mut source);
    }
    let transitioned = source.tick_presentation(16_666_667).unwrap().unwrap();
    assert!(scene_commands(&transitioned).iter().any(|command| {
        matches!(command, SceneCommand::PushClip { rect } if rect.width > 0 && rect.width < 640)
    }));
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
pub(super) fn packaged_native_vn_stage_restores_a_director_transition_snapshot() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene first #@id scene.first
    stage viewport:640x360 safe_area:16:9 #@id stage.main
    layer id:bg kind:background z:0 blend:normal clip:stage #@id layer.bg
    background asset:asset:/background/apartment-night layer:bg duration:0 interrupt:replace_from_current #@id background.first
    text key:line.one speaker:hero #@id line.one
  scene second #@id scene.second
    transition preset:director_puppet_9 duration:250 descriptor:director.puppet.9 #@id transition.center
    background asset:asset:/background/apartment-night layer:bg duration:0 interrupt:replace_from_current #@id background.second
    text key:line.two speaker:hero #@id line.two
"#,
    );
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    )
    .unwrap();

    source.launch().unwrap();
    for _ in 0..4 {
        advance(&mut source);
    }
    source.tick_presentation(16_666_667).unwrap().unwrap();
    prepare_test_save_metadata(&mut source, "slot.01");
    let saved = source.save("slot.01").unwrap();

    source.restore(&saved).unwrap();
    let restored = source.tick_presentation(16_666_667).unwrap().unwrap();
    assert!(scene_commands(&restored).iter().any(|command| {
        matches!(command, SceneCommand::PushClip { rect } if rect.width > 0 && rect.height > 0 && rect.width < 640)
    }));
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
pub(super) fn packaged_native_vn_preload_does_not_create_an_unbounded_gpu_residency_lease() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    preload asset:asset:/background/apartment-night #@id preload.background
    text key:line.one speaker:hero #@id line.one
"#,
    );
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    )
    .unwrap();

    let frame = source.launch().unwrap();
    let uploaded = scene_commands(&frame)
        .iter()
        .find_map(|command| match command {
            SceneCommand::UploadTexture { resource_id, frame }
                if resource_id == "asset:/background/apartment-night" =>
            {
                Some(frame)
            }
            _ => None,
        });
    let uploaded = uploaded.expect("viewport-fitted background must be GPU-resident");
    assert!(uploaded.width <= 640 && uploaded.height <= 360);
    assert!(uploaded.rgba8.len() as u64 <= DEFAULT_NATIVE_VN_GPU_TEXTURE_CACHE_BYTES);
    let shutdown = source.release_resources().unwrap();
    assert!(scene_commands(&shutdown).iter().any(|command| {
        matches!(
            command,
            SceneCommand::ReleaseResource { resource_id }
                if resource_id == "asset:/background/apartment-night"
        )
    }));
    source.shutdown().unwrap();
}

#[test]
pub(super) fn package_open_blocks_undeclared_localization() {
    let bytes = product_package();
    let package = PackageReader::open(&bytes).unwrap();
    let missing_locale = match NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("zh-Hans"),
        640,
        360,
        PlayerHostResourceId(1),
    ) {
        Ok(_) => panic!("undeclared localization must block package open"),
        Err(error) => error.to_string(),
    };
    assert!(missing_locale.contains("ASTRA_PLAYER_LOCALE_UNDECLARED"));
}

#[test]
pub(super) fn package_open_accepts_movie_for_product_media_execution() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    movie layer:video asset:asset:/movie/unsupported end:continue interrupt:reject #@id movie.unsupported
    text key:line.one speaker:hero #@id line.one
"#,
    );
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    )
    .unwrap();
    assert!(
        source.launch().is_err(),
        "undeclared movie asset must block at execution"
    );
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
pub(super) fn package_open_blocks_undeclared_presentation_preset_before_provider_creation() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    background asset:asset:/background/missing layer:bg preset:not_packaged duration:100 interrupt:replace_from_current #@id background.policy
    text key:line.one speaker:hero #@id line.one
"#,
    );
    let package = PackageReader::open(&bytes).unwrap();
    let error = match NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    ) {
        Ok(_) => panic!("undeclared presentation preset must block package open"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("ASTRA_VN_PRESENTATION_PRESET_UNDECLARED"));
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
#[tokio::test]
pub(super) async fn packaged_native_vn_scene_reaches_live_windows_wgpu_and_releases_resources() {
    use astra_platform::{
        HostLaunchProfile, PlatformHostFactory, PlatformHostProfile, SurfaceRequest, WindowRequest,
    };
    use astra_player_core::{PlatformCommandSink, PlayerHostCommandExecutor};

    let bytes = product_package();
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    )
    .unwrap();
    let session = astra_platform_windows::factory()
        .start(HostLaunchProfile::platform(
            PlatformHostProfile::windows_release("nativevn-game", "com.example.player"),
        ))
        .await
        .unwrap();
    let window = session
        .client
        .create_window(WindowRequest {
            title: "AstraVN retained scene test".to_string(),
            width: 640,
            height: 360,
            visible: true,
        })
        .await
        .unwrap();
    let surface = session
        .client
        .create_surface(SurfaceRequest {
            window,
            width: 640,
            height: 360,
        })
        .await
        .unwrap();
    let mut sink = PlatformCommandSink::new(session.client.clone());
    sink.bind_surface(PlayerHostResourceId(1), surface).unwrap();
    let mut executor = PlayerHostCommandExecutor::new(sink);
    executor
        .execute_batch(source.launch().unwrap())
        .await
        .unwrap();
    let first = session.client.capture_surface(surface).await.unwrap();
    assert!(first
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel != [8, 10, 16, 255]));

    executor
        .execute_batch(source.release_resources().unwrap())
        .await
        .unwrap();
    let released = session.client.capture_surface(surface).await.unwrap();
    assert!(released
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel == [0, 0, 0, 255]));
    source.shutdown().unwrap();
    drop(executor);
    session.client.destroy_surface(surface).await.unwrap();
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
}
