use astra_core::Hash256;
use astra_player_core::{PlayerAction, PlayerHostCommand, PlayerHostResourceId};
use astra_player_vn::NativeVnHostCommandSource;
use astra_vn_core::{compile_astra_sources, AstraSource, VnRunConfig};
use astra_vn_package::package_sections_for_story;

const STORY: &str = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    text key:line.one speaker:hero #@id line.one
    text key:line.two speaker:hero #@id line.two
"#;

#[test]
fn native_vn_source_turns_real_runtime_steps_into_changing_frames() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    let first = source.launch().unwrap();
    let second = source.dispatch_action(PlayerAction::Advance).unwrap();
    let frame_hash = |command: &PlayerHostCommand| match command {
        PlayerHostCommand::PresentRgba { rgba8, .. } => Hash256::from_sha256(rgba8),
        _ => panic!("expected present command"),
    };
    assert_ne!(
        frame_hash(&first.commands[0]),
        frame_hash(&second.commands[0])
    );
}

#[test]
fn native_vn_source_exposes_route_evidence_from_runtime_outputs() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();

    source.launch().unwrap();
    let evidence = source.last_step_evidence().expect("launch evidence");

    assert_eq!(evidence.schema, "astra.player_vn_step_evidence.v1");
    assert_eq!(evidence.fixed_step, 1);
    assert!(evidence.coverage_reached.contains("state.start"));
    assert!(evidence.runtime_state_hash.starts_with("hash128:"));
    assert!(evidence.runtime_event_hash.starts_with("hash128:"));
    assert!(evidence.runtime_presentation_hash.starts_with("hash128:"));
    assert_eq!(evidence.current_state_id.as_deref(), Some("state.start"));

    source.dispatch_action(PlayerAction::Advance).unwrap();
    source.dispatch_action(PlayerAction::Advance).unwrap();
    let terminal = source.last_step_evidence().expect("terminal evidence");
    assert!(
        !terminal.terminal_route_ids.is_empty(),
        "terminal routes: {:?}",
        terminal.terminal_route_ids
    );
}

#[test]
fn native_vn_source_save_restore_resumes_the_same_runtime_state() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    source.dispatch_action(PlayerAction::Advance).unwrap();
    let saved = source.save("slot.main").unwrap();

    source.dispatch_action(PlayerAction::Advance).unwrap();
    let uninterrupted = source
        .last_step_evidence()
        .unwrap()
        .vn_state_hash_after
        .clone();

    source.restore(&saved).unwrap();
    source.dispatch_action(PlayerAction::Advance).unwrap();
    assert_eq!(
        source.last_step_evidence().unwrap().vn_state_hash_after,
        uninterrupted
    );
}

#[test]
fn native_vn_source_rejects_tampered_save_before_restore() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    let mut saved = source.save("slot.main").unwrap();
    let last = saved.len() - 1;
    saved[last] ^= 0x5a;

    let error = source.restore(&saved).unwrap_err();

    assert!(error.to_string().contains("ASTRA_PLAYER_SAVE_INTEGRITY"));
}

#[test]
fn native_vn_source_builds_atomic_platform_save_transaction() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
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

#[test]
fn native_vn_source_completes_wait_through_runtime_provider() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    wait fence:voice.end #@id wait.voice
    text key:line.after #@id line.after
"#;
    let compiled = compile_astra_sources([AstraSource::new("main.astra", story)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
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
    source.dispatch_action(PlayerAction::Advance).unwrap();
}

#[test]
fn native_vn_source_exposes_validated_timeline_tasks_to_player() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    timeline id:intro target:hero join:block fence:timeline.intro.complete duration:120 #@id timeline.intro
"#;
    let compiled = compile_astra_sources([AstraSource::new("main.astra", story)]).unwrap();
    let mut source = NativeVnHostCommandSource::new(
        compiled,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();

    source.launch().unwrap();
    let tasks = source.take_timeline_tasks();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "intro");
    assert_eq!(tasks[0].target.as_deref(), Some("hero"));
    assert_eq!(tasks[0].duration_ms, Some(120));
    assert_eq!(tasks[0].fence.as_deref(), Some("timeline.intro.complete"));
}

#[test]
fn packaged_native_vn_source_exposes_hash_validated_audio_requests() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    voice asset:asset:/voice/hero/0001 end:continue #@id voice.hero.0001
    text key:line.after #@id line.after
"#;
    let compiled = compile_astra_sources([AstraSource::new("main.astra", story)]).unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let encoded = b"ID3\x04\x00\x00\x00\x00\x00\x00fixture".to_vec();
    sections.push(astra_package::SectionPayload::raw(
        "asset.voice.hero.0001",
        "astra.cooked_audio.v1",
        encoded.clone(),
    ));
    let mut request = astra_package::PackageBuildRequest::minimal(
        "com.example.player-audio",
        "classic",
        sections,
    );
    apply_product_bindings(&mut request);
    let bytes = astra_package::PackageBuilder::build(request).unwrap();
    let package = astra_package::PackageReader::open(bytes.as_bytes()).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();

    source.launch().unwrap();
    let audio = source.take_audio_requests();

    assert_eq!(audio.len(), 1);
    assert_eq!(audio[0].asset_id, "asset:/voice/hero/0001");
    assert_eq!(audio[0].codec, "mp3");
    assert_eq!(audio[0].encoded_bytes, encoded);
    assert_eq!(audio[0].encoded_hash, Hash256::from_sha256(&encoded));
    assert_eq!(audio[0].attributes.get("asset"), Some(&audio[0].asset_id));
    let decode = source.prepare_audio_decode(&audio[0]).unwrap();
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

fn apply_product_bindings(request: &mut astra_package::PackageBuildRequest) {
    request.provider_policy = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.provider_policy.v1",
        "profile": "classic",
        "renderer": "astra.renderer2d.wgpu",
        "bindings": [{
            "slot": "presentation",
            "provider_id": "astra.vn.standard_presentation"
        }, {
            "slot": "renderer2d",
            "provider_id": "astra.renderer2d.wgpu"
        }, {
            "slot": "game_runtime_provider",
            "provider_id": "astra.runtime.native_vn"
        }]
    }))
    .unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.plugin_extension_registry.v1",
        "providers": [{
            "slot": "presentation",
            "provider_id": "astra.vn.standard_presentation",
            "capability": "presentation.vn.standard",
            "phase": "runtime",
            "packaged": true
        }, {
            "slot": "renderer2d",
            "provider_id": "astra.renderer2d.wgpu",
            "capability": "renderer2d.wgpu",
            "phase": "runtime",
            "packaged": true
        }, {
            "slot": "game_runtime_provider",
            "provider_id": "astra.runtime.native_vn",
            "capability": "runtime.native_vn",
            "phase": "runtime",
            "packaged": true
        }],
        "bindings": [{
            "slot": "presentation",
            "provider_id": "astra.vn.standard_presentation"
        }, {
            "slot": "renderer2d",
            "provider_id": "astra.renderer2d.wgpu"
        }, {
            "slot": "game_runtime_provider",
            "provider_id": "astra.runtime.native_vn"
        }],
        "conflicts": []
    }))
    .unwrap();
}

#[test]
fn packaged_player_rejects_headless_presentation_binding() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let request = astra_package::PackageBuildRequest::minimal(
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

#[test]
fn packaged_player_accepts_explicit_product_provider_bindings() {
    let compiled = compile_astra_sources([AstraSource::new("main.astra", STORY)]).unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let mut request = astra_package::PackageBuildRequest::minimal(
        "com.example.player-product-binding",
        "classic",
        sections,
    );
    request.provider_policy = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.provider_policy.v1",
        "profile": "classic",
        "renderer": "astra.renderer2d.wgpu",
        "bindings": [{
            "slot": "presentation",
            "provider_id": "astra.vn.standard_presentation"
        }, {
            "slot": "renderer2d",
            "provider_id": "astra.renderer2d.wgpu"
        }, {
            "slot": "game_runtime_provider",
            "provider_id": "astra.runtime.native_vn"
        }]
    }))
    .unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&serde_json::json!({
        "schema": "astra.plugin_extension_registry.v1",
        "providers": [{
            "slot": "presentation",
            "provider_id": "astra.vn.standard_presentation",
            "capability": "presentation.vn.standard",
            "phase": "runtime",
            "packaged": true
        }, {
            "slot": "renderer2d",
            "provider_id": "astra.renderer2d.wgpu",
            "capability": "renderer2d.wgpu",
            "phase": "runtime",
            "packaged": true
        }, {
            "slot": "game_runtime_provider",
            "provider_id": "astra.runtime.native_vn",
            "capability": "runtime.native_vn",
            "phase": "runtime",
            "packaged": true
        }],
        "bindings": [{
            "slot": "presentation",
            "provider_id": "astra.vn.standard_presentation"
        }, {
            "slot": "renderer2d",
            "provider_id": "astra.renderer2d.wgpu"
        }, {
            "slot": "game_runtime_provider",
            "provider_id": "astra.runtime.native_vn"
        }],
        "conflicts": []
    }))
    .unwrap();
    let bytes = astra_package::PackageBuilder::build(request).unwrap();
    let package = astra_package::PackageReader::open(bytes.as_bytes()).unwrap();

    NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("zh-Hans"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .expect("product-bound package should open");
}
