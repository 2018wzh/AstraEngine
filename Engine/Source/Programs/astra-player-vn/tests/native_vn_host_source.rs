use astra_asset::VfsUri;
use astra_core::Hash256;
use astra_media::{
    FontPackageEntry, FontPackageManifest, UnicodeRange, FONT_PACKAGE_MANIFEST_SCHEMA,
};
use astra_media_core::SceneCommand;
use astra_package::{PackageBuildRequest, PackageBuilder, PackageReader, SectionPayload};
use astra_player_core::{PlayerHostCommand, PlayerHostResourceId};
use astra_player_vn::{NativeVnAudioOutput, NativeVnHostCommandSource};
use astra_plugin_abi::{
    LoadPhase, PluginExtensionRegistrySnapshot, ProviderBinding, ProviderBindingContext,
    ProviderExtensionRecord, ProviderPolicy, PLUGIN_EXTENSION_REGISTRY_SCHEMA,
    PROVIDER_POLICY_SCHEMA,
};
use astra_ui_core::{UiButtonState, UiInputEventKind};
use astra_vn_core::{compile_astra_project, AstraSource, CompileAstraProjectOptions, VnRunConfig};
use astra_vn_package::{package_sections_for_project, PLAYER_LOCALE_CONFIG_SCHEMA};
use astra_vn_runtime_provider::NativeVnRuntimeProvider;

const STORY: &str = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    text key:line.one speaker:hero #@id line.one
    text key:line.two speaker:hero #@id line.two
"#;

const TEST_UI: &str = r#"
ui_bind surface:message view:ui.test.message controller:test.message policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.message
ui_bind surface:choice view:ui.test.choice controller:test.choice policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.choice
ui_bind system_page:save view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.save
ui_bind system_page:load view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.load
ui_bind system_page:config view:ui.test.system controller:test.system policy:astra.policy.standard theme:astra.vn.theme.classic #@id bind.config
ui_view ui.test.message model:astra.vn.ui_model.message.v1 theme:astra.vn.theme.classic #@id ui.test.message
  screen id:root
    panel id:advance fill:true
      on activate -> vn.advance
    text id:body value:$model.text_key
ui_view ui.test.choice model:astra.vn.ui_model.choice.v1 theme:astra.vn.theme.classic #@id ui.test.choice
  screen id:root
    virtual_list id:options items:$model.options item_key:option_id overscan:2 item_extent:48
      button id:option min_height:44 value:$item.text_key
        on activate -> vn.choose option_id:$item.option_id
ui_view ui.test.system model:astra.vn.ui_model.system.v1 theme:astra.vn.theme.classic #@id ui.test.system
  screen id:root
    button id:back min_height:48
      on activate -> vn.return_system
"#;

fn product_package() -> Vec<u8> {
    product_package_for(STORY)
}

fn product_package_for(story: &str) -> Vec<u8> {
    product_package_with_request(story, |_| {})
}

fn source_for(story: &str) -> NativeVnHostCommandSource {
    let bytes = product_package_for(story);
    let package = PackageReader::open(&bytes).unwrap();
    NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap()
}

fn advance(source: &mut NativeVnHostCommandSource) -> astra_player_core::PlayerHostCommandBatch {
    source
        .dispatch_ui_event(UiInputEventKind::Keyboard {
            logical_key: "Enter".to_string(),
            physical_key: "Enter".to_string(),
            state: UiButtonState::Pressed,
            repeat: false,
            modifiers: 0,
        })
        .unwrap()
}

fn product_package_with_request(
    story: &str,
    mutate: impl FnOnce(&mut PackageBuildRequest),
) -> Vec<u8> {
    let compiled = compile_astra_project(
        [
            AstraSource::story("main.astra", story),
            AstraSource::ui("test-ui.astra", TEST_UI),
        ],
        test_compile_options(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let font = include_bytes!("../../../../../Examples/NativeVN/Assets/Fonts/Poppins-Regular.ttf")
        .to_vec();
    let font_hash = Hash256::from_sha256(&font);
    let background =
        include_bytes!("../../../../../Examples/NativeVN/Assets/Backgrounds/apartment-night.png")
            .to_vec();
    let background_hash = Hash256::from_sha256(&background);
    sections.push(SectionPayload::raw(
        "asset.font.ui",
        "astra.cooked_asset.v1",
        font.clone(),
    ));
    sections.push(SectionPayload::raw(
        "media.font_manifest",
        FONT_PACKAGE_MANIFEST_SCHEMA,
        serde_json::to_vec(&FontPackageManifest {
            schema: FONT_PACKAGE_MANIFEST_SCHEMA.to_string(),
            target: "nativevn-game".to_string(),
            profile: "classic".to_string(),
            provider_binding: "astra.vfs.package".to_string(),
            fonts: vec![FontPackageEntry {
                asset_id: "asset:/font/ui".to_string(),
                uri: VfsUri::parse("package:/fonts/ui.ttf").unwrap(),
                family: "Poppins".to_string(),
                face_index: 0,
                hash: font_hash,
                license_id: "OFL-1.1".to_string(),
                subset: Some("latin-basic".to_string()),
                coverage: vec![UnicodeRange {
                    start: 32,
                    end: 126,
                }],
                targets: vec!["nativevn-game".to_string()],
                profiles: vec!["classic".to_string()],
            }],
        })
        .unwrap(),
    ));
    sections.push(SectionPayload::raw(
        "asset.image.background",
        "astra.cooked_asset.v1",
        background.clone(),
    ));
    sections.push(SectionPayload::raw(
        "vn.localization.en",
        "astra.vn.localization_table.v1",
        br#"{
            "schema":"astra.vn.localization_table.v1",
            "locale":"en",
            "strings":{
                "line.one":"First production line.",
                "line.two":"Second production line.",
                "line.after":"Line after wait.",
                "speaker.hero":"Hero"
            }
        }"#
        .to_vec(),
    ));
    sections.push(SectionPayload::raw(
        "player.locale_config",
        PLAYER_LOCALE_CONFIG_SCHEMA,
        br#"{
            "schema":"astra.player_locale_config.v1",
            "default_locale":"en",
            "available_locales":["en"]
        }"#
        .to_vec(),
    ));

    let mut request = PackageBuildRequest::fixture("com.example.player", "classic", sections);
    bind_product_provider_authority(&mut request);
    request.asset_vfs_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_vfs_manifest.v1",
        "prefixes": [{
            "prefix": "package",
            "provider_id": "astra.vfs.package",
            "backend": "package",
            "case_policy": "case_sensitive",
            "mode": "read_only",
            "redaction": "shipping",
            "capabilities": ["vfs.backend.package"]
        }],
        "layers": [{
            "layer_id": "package.base",
            "prefix": "package",
            "priority": 0,
            "source": {"kind": "package_section", "section_id": "package.manifest"},
            "targets": ["nativevn-game"],
            "profiles": ["classic"]
        }],
        "entries": [
            {
                "vfs_uri": "package:/fonts/ui.ttf",
                "layer_id": "package.base",
                "source": {"kind": "package_section", "section_id": "asset.font.ui"},
                "offset": 0,
                "size": font.len(),
                "hash": font_hash,
                "codec": "raw",
                "media_kind": "font",
                "diagnostics": []
            },
            {
                "vfs_uri": "package:/background/apartment-night.png",
                "layer_id": "package.base",
                "source": {"kind": "package_section", "section_id": "asset.image.background"},
                "offset": 0,
                "size": background.len(),
                "hash": background_hash,
                "codec": "raw",
                "media_kind": "image/png",
                "diagnostics": []
            }
        ],
        "whiteouts": []
    }))
    .unwrap();
    request.asset_catalog = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.asset_catalog.v1",
        "assets": [
            {
                "asset_id": "asset:/font/ui",
                "vfs_uri": "package:/fonts/ui.ttf",
                "media_kind": "font",
                "tags": ["ui"],
                "bundle_id": "classic",
                "chunk_id": "base",
                "profiles": ["classic"]
            },
            {
                "asset_id": "asset:/background/apartment-night",
                "vfs_uri": "package:/background/apartment-night.png",
                "media_kind": "image/png",
                "tags": ["background"],
                "bundle_id": "classic",
                "chunk_id": "base",
                "profiles": ["classic"]
            }
        ]
    }))
    .unwrap();
    mutate(&mut request);
    PackageBuilder::build(request).unwrap().into_bytes()
}

fn test_compile_options() -> CompileAstraProjectOptions {
    let mut theme = astra_ui_core::UiThemeManifest {
        schema: "astra.ui_theme_manifest.v1".into(),
        id: "astra.vn.theme.classic".into(),
        parent: None,
        tokens: [(
            "surface.system".into(),
            astra_ui_core::UiThemeValue::Color([0, 0, 0, 255]),
        )]
        .into_iter()
        .collect(),
        high_contrast_tokens: Default::default(),
        content_hash: Hash256::from_sha256(&[]),
    };
    theme.content_hash = theme.compute_hash().unwrap();
    test_controller_options(CompileAstraProjectOptions::default().with_ui_theme(theme))
}

fn test_controller_options(mut options: CompileAstraProjectOptions) -> CompileAstraProjectOptions {
    let source = test_controller_source();
    for id in ["test.message", "test.choice", "test.system"] {
        options = options.with_ui_controller_source(id, source.clone());
    }
    options
}

fn test_controller_source() -> String {
    r#"
local controllers = {
  { "test.message", "ui.test.message", "astra.vn.ui_model.message.v1" },
  { "test.choice", "ui.test.choice", "astra.vn.ui_model.choice.v1" },
  { "test.system", "ui.test.system", "astra.vn.ui_model.system.v1" },
}
for _, definition in controllers do
  astra.ui.controller.register(definition[1], {
    schema = "astra.vn.ui_controller.v1", view = definition[2],
    model_schema = definition[3], snapshot = "none",
  }, { on_action = function(_, _, action)
    return { astra.ui.effect.forward(action) }
  end })
end
"#
    .to_string()
}

fn bind_product_provider_authority(request: &mut PackageBuildRequest) {
    let specs = [
        ("presentation", "astra.renderer.wgpu", "renderer2d.wgpu"),
        ("vfs_provider", "astra.vfs.package", "vfs.backend.package"),
        (
            "game_runtime_provider",
            "astra.runtime.native_vn",
            "runtime.native_vn",
        ),
    ];
    let bindings = specs
        .iter()
        .map(|(slot, provider_id, capability)| {
            ProviderBinding::new(
                *slot,
                *provider_id,
                ProviderBindingContext {
                    package_id: request.package_id.clone(),
                    target: "nativevn-game".to_string(),
                    profile: request.profile.clone(),
                    required_capability: capability.to_string(),
                    engine_version: env!("CARGO_PKG_VERSION").to_string(),
                    rustc_fingerprint: "rustc-stable".to_string(),
                    feature_fingerprint: "runtime-envelope-v2".to_string(),
                    abi_fingerprint: "astra-plugin-abi-v2".to_string(),
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    request.provider_policy = serde_json::to_vec(&ProviderPolicy {
        schema: PROVIDER_POLICY_SCHEMA.to_string(),
        profile: request.profile.clone(),
        renderer: "astra.renderer.wgpu".to_string(),
        decode_fallback: "profile_bound".to_string(),
        runtime_provider: NativeVnRuntimeProvider::descriptor(),
        bindings: bindings.clone(),
    })
    .unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&PluginExtensionRegistrySnapshot {
        schema: PLUGIN_EXTENSION_REGISTRY_SCHEMA.to_string(),
        providers: specs
            .iter()
            .map(|(slot, provider_id, capability)| ProviderExtensionRecord {
                slot: slot.to_string(),
                provider_id: provider_id.to_string(),
                capability: capability.to_string(),
                phase: LoadPhase::Runtime,
                packaged: true,
                engine_version: env!("CARGO_PKG_VERSION").to_string(),
                rustc_fingerprint: "rustc-stable".to_string(),
                feature_fingerprint: "runtime-envelope-v2".to_string(),
                abi_fingerprint: "astra-plugin-abi-v2".to_string(),
            })
            .collect(),
        bindings,
        conflicts: vec![],
    })
    .unwrap();
    request.target_manifest = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.target_manifest.v2",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "runtime_provider": "native_vn",
            "ui_provider": "astra.ui.yakui",
            "default_profile": "classic",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    }))
    .unwrap();
    request.platform_eligibility = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.platform_eligibility.v1",
        "target": "nativevn-game",
        "profiles": ["classic"],
        "platforms": ["windows", "web"]
    }))
    .unwrap();
}

fn scene_commands(batch: &astra_player_core::PlayerHostCommandBatch) -> &[SceneCommand] {
    match &batch.commands[0] {
        PlayerHostCommand::PresentScene { commands, .. } => commands,
        command => panic!("expected retained scene presentation, got {command:?}"),
    }
}

#[astra_headless_test::test]
fn packaged_native_vn_source_shapes_localized_text_into_retained_scene_commands() {
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
    for commands in [scene_commands(&first), scene_commands(&second)] {
        assert!(commands
            .iter()
            .any(|command| matches!(command, SceneCommand::UploadGlyph { .. })));
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
    assert_ne!(
        Hash256::from_sha256(&serde_json::to_vec(scene_commands(&first)).unwrap()),
        Hash256::from_sha256(&serde_json::to_vec(scene_commands(&second)).unwrap())
    );
    let shutdown = source.release_resources().unwrap();
    assert!(scene_commands(&shutdown)
        .iter()
        .any(|command| matches!(command, SceneCommand::ReleaseResource { .. })));
    source.shutdown().unwrap();
}

#[astra_headless_test::test]
fn packaged_native_vn_stage_uses_product_director_and_package_texture() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    stage viewport:640x360 safe_area:16:9 #@id stage.main
    layer id:bg kind:background z:0 blend:normal clip:stage #@id layer.bg
    background asset:asset:/background/apartment-night layer:bg duration:0 #@id background.main
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

    let launch = source.launch().unwrap();
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
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[astra_headless_test::test]
fn package_open_blocks_undeclared_localization() {
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

#[astra_headless_test::test]
fn package_open_accepts_movie_for_product_media_execution() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    movie layer:video asset:asset:/movie/unsupported end:continue #@id movie.unsupported
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

#[astra_headless_test::test]
fn package_open_blocks_undeclared_presentation_preset_before_provider_creation() {
    let bytes = product_package_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    background asset:asset:/background/missing layer:bg preset:not_packaged duration:100 #@id background.policy
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

#[astra_headless_test::test]
fn package_open_blocks_runtime_descriptor_drift_before_provider_creation() {
    let bytes = product_package_with_request(STORY, |request| {
        let mut policy: ProviderPolicy = serde_json::from_slice(&request.provider_policy).unwrap();
        policy.runtime_provider.output_schemas[0].schema =
            "astra.vn.runtime_step_effect.drift".to_string();
        request.provider_policy = serde_json::to_vec(&policy).unwrap();
    });
    let package = PackageReader::open(&bytes).unwrap();
    let error = match NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        640,
        360,
        PlayerHostResourceId(1),
    ) {
        Ok(_) => panic!("linked provider descriptor drift must block package open"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("ASTRA_RUNTIME_PROVIDER_LINKED_DESCRIPTOR_MISMATCH"));
}

#[cfg(all(target_os = "windows", feature = "platform-test-driver"))]
#[astra_headless_test::tokio_test]
async fn packaged_native_vn_scene_reaches_live_windows_wgpu_and_releases_resources() {
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
        .chunks_exact(4)
        .any(|pixel| pixel != [8, 10, 16, 255]));

    executor
        .execute_batch(source.release_resources().unwrap())
        .await
        .unwrap();
    let released = session.client.capture_surface(surface).await.unwrap();
    assert!(released
        .rgba8
        .chunks_exact(4)
        .all(|pixel| pixel == [0, 0, 0, 255]));
    source.shutdown().unwrap();
    drop(executor);
    session.client.destroy_surface(surface).await.unwrap();
    session.client.destroy_window(window).await.unwrap();
    session.client.shutdown().await.unwrap();
}

#[astra_headless_test::test]
fn native_vn_source_exposes_route_evidence_from_runtime_outputs() {
    let mut source = source_for(STORY);

    source.launch().unwrap();
    let evidence = source.last_step_evidence().expect("launch evidence");

    assert_eq!(evidence.schema, "astra.player_vn_step_evidence.v1");
    assert_eq!(evidence.fixed_step, 1);
    assert!(evidence.coverage_reached.contains("state.start"));
    assert!(evidence.runtime_state_hash.starts_with("hash128:"));
    assert!(evidence.runtime_event_hash.starts_with("hash128:"));
    assert!(evidence.runtime_presentation_hash.starts_with("hash128:"));
    assert_eq!(evidence.current_state_id.as_deref(), Some("state.start"));

    advance(&mut source);
    advance(&mut source);
    let terminal = source.last_step_evidence().expect("terminal evidence");
    assert!(
        !terminal.terminal_route_ids.is_empty(),
        "terminal routes: {:?}",
        terminal.terminal_route_ids
    );
}

#[astra_headless_test::test]
fn native_vn_source_save_restore_resumes_the_same_runtime_state() {
    let mut source = source_for(STORY);
    source.launch().unwrap();
    advance(&mut source);
    let saved = source.save("slot.main").unwrap();

    advance(&mut source);
    let uninterrupted = source
        .last_step_evidence()
        .unwrap()
        .vn_state_hash_after
        .clone();

    source.restore(&saved).unwrap();
    advance(&mut source);
    assert_eq!(
        source.last_step_evidence().unwrap().vn_state_hash_after,
        uninterrupted
    );
}

#[astra_headless_test::test]
fn native_vn_source_rejects_tampered_save_before_restore() {
    let mut source = source_for(STORY);
    source.launch().unwrap();
    let mut saved = source.save("slot.main").unwrap();
    let last = saved.len() - 1;
    saved[last] ^= 0x5a;

    let error = source.restore(&saved).unwrap_err();

    assert!(error.to_string().contains("ASTRA_PLAYER_SAVE_INTEGRITY"));
}

#[astra_headless_test::test]
fn native_vn_source_builds_atomic_platform_save_transaction() {
    let mut source = source_for(STORY);
    source.launch().unwrap();

    let plan = source
        .prepare_save_transaction("slot.main", PlayerHostResourceId(20))
        .unwrap();

    assert!(matches!(
        plan.begin.commands.as_slice(),
        [PlayerHostCommand::BeginSave {
            transaction: PlayerHostResourceId(20),
            ..
        }]
    ));
    assert!(matches!(
        plan.write.commands.as_slice(),
        [PlayerHostCommand::WriteSave { transaction: PlayerHostResourceId(20), bytes, .. }] if !bytes.is_empty()
    ));
    assert!(matches!(
        plan.commit.commands.as_slice(),
        [PlayerHostCommand::CommitSave {
            transaction: PlayerHostResourceId(20),
            ..
        }]
    ));
    assert!(matches!(
        plan.abort.commands.as_slice(),
        [PlayerHostCommand::AbortSave {
            transaction: PlayerHostResourceId(20),
            ..
        }]
    ));
}

#[astra_headless_test::test]
fn native_vn_source_completes_wait_through_runtime_provider() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    wait fence:voice.end #@id wait.voice
    text key:line.after #@id line.after
"#;
    let mut source = source_for(story);
    source.launch().unwrap();
    let before = source
        .last_step_evidence()
        .unwrap()
        .vn_state_hash_after
        .clone();

    source.complete_wait("voice.end").unwrap();

    assert_ne!(
        source.last_step_evidence().unwrap().vn_state_hash_after,
        before
    );
    advance(&mut source);
}

#[astra_headless_test::test]
fn native_vn_source_exposes_validated_timeline_tasks_to_player() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    timeline id:intro target:hero property:opacity keyframes:0=0,120=1 join:block fence:timeline.intro.complete budget_ms:2 #@id timeline.intro
"#;
    let mut source = source_for(story);

    source.launch().unwrap();
    let tasks = source.take_timeline_tasks();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "intro");
    assert_eq!(tasks[0].target.as_deref(), Some("hero"));
    assert_eq!(tasks[0].duration_ms, Some(120));
    assert_eq!(tasks[0].fence.as_deref(), Some("timeline.intro.complete"));
}

#[astra_headless_test::test]
fn packaged_native_vn_source_exposes_hash_validated_audio_requests() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    voice asset:asset:/voice/hero/0001 #@id voice.hero.0001
    text key:line.after #@id line.after
"#;
    let encoded = b"ID3\x04\x00\x00\x00\x00\x00\x00fixture".to_vec();
    let encoded_hash = Hash256::from_sha256(&encoded);
    let bytes = product_package_with_request(story, |request| {
        request
            .cooked_assets
            .push(astra_package::SectionPayload::raw(
                "asset.voice.hero.0001",
                "astra.cooked_audio.v1",
                encoded.clone(),
            ));
        let mut vfs: serde_json::Value =
            serde_json::from_slice(&request.asset_vfs_manifest).unwrap();
        vfs["entries"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "vfs_uri": "package:/voice/hero/0001.mp3",
                "layer_id": "package.base",
                "source": {"kind": "package_section", "section_id": "asset.voice.hero.0001"},
                "offset": 0,
                "size": encoded.len(),
                "hash": encoded_hash,
                "codec": "raw",
                "media_kind": "audio/mp3",
                "diagnostics": []
            }));
        request.asset_vfs_manifest = serde_json::to_vec(&vfs).unwrap();
        let mut catalog: serde_json::Value =
            serde_json::from_slice(&request.asset_catalog).unwrap();
        catalog["assets"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "asset_id": "asset:/voice/hero/0001",
                "vfs_uri": "package:/voice/hero/0001.mp3",
                "media_kind": "audio/mp3",
                "tags": ["voice"],
                "bundle_id": "classic",
                "chunk_id": "base",
                "profiles": ["classic"]
            }));
        request.asset_catalog = serde_json::to_vec(&catalog).unwrap();
    });
    let package = astra_package::PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();

    source.launch().unwrap();
    let audio = source.take_audio_requests();

    assert_eq!(audio.len(), 1);
    let NativeVnAudioOutput::Start(audio) = &audio[0] else {
        panic!("expected packaged audio start output");
    };
    assert_eq!(audio.asset_id, "asset:/voice/hero/0001");
    assert_eq!(audio.codec, "mp3");
    assert_eq!(audio.encoded_bytes, encoded);
    assert_eq!(audio.encoded_hash, encoded_hash);
    assert_eq!(audio.attributes.get("asset"), Some(&audio.asset_id));
    let decode = source.prepare_audio_decode(audio).unwrap();
    assert!(matches!(
        decode.decode.commands.as_slice(),
        [PlayerHostCommand::Decode { codec, bytes, .. }] if codec == "mp3" && bytes == &encoded
    ));
    let playback = source
        .prepare_audio_playback(&astra_player_core::PlayerDecodedAudio {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0.0; 10_000],
        })
        .unwrap();
    assert_eq!(playback.expected_sample_count, 10_000);
    assert_eq!(playback.submits.len(), 2);
    assert!(matches!(
        playback.drain.commands.as_slice(),
        [PlayerHostCommand::DrainAudio { .. }]
    ));
    let (output, open) = source
        .prepare_persistent_audio_open(48_000, 2, 8_192)
        .unwrap();
    assert!(matches!(
        open.commands.as_slice(),
        [PlayerHostCommand::OpenAudio { output: opened, max_buffered_frames: 8_192, .. }] if *opened == output
    ));
    let query = source.prepare_persistent_audio_query(output).unwrap();
    assert!(matches!(
        query.commands.as_slice(),
        [PlayerHostCommand::QueryAudio { output: queried, .. }] if *queried == output
    ));
}

#[astra_headless_test::test]
fn packaged_native_vn_source_routes_typed_audio_control_to_product_audio_owner() {
    let mut source = source_for(
        r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    audio action:pause target:bgm.main #@id audio.pause
"#,
    );

    source.launch().unwrap();
    let audio = source.take_audio_requests();

    assert!(matches!(
        audio.as_slice(),
        [NativeVnAudioOutput::Control(control)]
            if control.command_id == "audio.pause"
                && control.action == "pause"
                && control.target == "bgm.main"
    ));
}

#[astra_headless_test::test]
fn packaged_player_rejects_headless_presentation_binding() {
    let compiled = compile_astra_project(
        [AstraSource::story("main.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let request = astra_package::PackageBuildRequest::fixture(
        "com.example.player-binding",
        "classic",
        sections,
    );
    let bytes = astra_package::PackageBuilder::build(request).unwrap();
    let package = astra_package::PackageReader::open(bytes.as_bytes()).unwrap();

    let error = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .err()
    .expect("headless presentation binding must be rejected");

    assert!(error
        .to_string()
        .contains("ASTRA_PLAYER_PRESENTATION_PROVIDER_INELIGIBLE"));
}

#[astra_headless_test::test]
fn packaged_player_accepts_explicit_product_provider_bindings() {
    let bytes = product_package();
    let package = astra_package::PackageReader::open(&bytes).unwrap();

    NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .expect("product-bound package should open");
}
