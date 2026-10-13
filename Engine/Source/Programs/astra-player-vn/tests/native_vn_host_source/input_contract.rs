use super::*;

#[test]
pub(super) fn native_vn_source_exposes_route_evidence_from_runtime_outputs() {
    let mut source = source_for(STORY);

    source.launch().unwrap();
    let evidence = source.last_step_evidence().expect("launch evidence");

    assert_eq!(evidence.schema, "astra.player_vn_step_evidence.v2");
    assert_eq!(evidence.fixed_step, 1);
    assert!(evidence.coverage_reached.contains("state.start"));
    assert_eq!(evidence.current_state_id.as_deref(), Some("state.start"));
    assert_eq!(
        evidence.pending_wait_command_id.as_deref(),
        Some("line.one")
    );
    assert!(
        evidence
            .pending_wait_await_id
            .as_deref()
            .is_some_and(|await_id| astra_core::StableId::parse(await_id).is_ok()),
        "runtime wait occurrence must expose its serialized AwaitToken identity"
    );

    for _ in 0..4 {
        advance(&mut source);
    }
    let terminal = source.last_step_evidence().expect("terminal evidence");
    assert!(
        !terminal.terminal_route_ids.is_empty(),
        "terminal routes: {:?}",
        terminal.terminal_route_ids,
    );
}

#[test]
pub(super) fn native_vn_source_save_restore_resumes_the_same_runtime_state() {
    let mut source = source_for(STORY);
    source.launch().unwrap();
    advance(&mut source);
    prepare_test_save_metadata(&mut source, "slot.01");
    let saved = source.save("slot.01").unwrap();

    advance(&mut source);
    let uninterrupted = source.last_step_evidence().unwrap().clone();

    source.restore(&saved).unwrap();
    assert_eq!(
        source
            .product_observation_evidence()
            .unwrap()
            .occupied_save_slot_count,
        1
    );
    advance(&mut source);
    let resumed = source.last_step_evidence().unwrap();
    assert_eq!(resumed.current_state_id, uninterrupted.current_state_id);
    assert_eq!(
        resumed.pending_wait_command_id,
        uninterrupted.pending_wait_command_id
    );
    assert_eq!(resumed.pending_choice_ids, uninterrupted.pending_choice_ids);
    assert_eq!(resumed.terminal_route_ids, uninterrupted.terminal_route_ids);
}

#[test]
pub(super) fn native_vn_source_rejects_tampered_save_before_restore() {
    let mut source = source_for(STORY);
    source.launch().unwrap();
    prepare_test_save_metadata(&mut source, "slot.01");
    let mut saved = source.save("slot.01").unwrap();
    saved.truncate(saved.len() - 1);

    let error = source.restore(&saved).unwrap_err();

    assert!(error.to_string().contains("ASTRA_PLAYER_SAVE_INTEGRITY"));
}

#[test]
pub(super) fn native_vn_source_hydrates_validated_save_metadata_before_launch() {
    let mut writer = source_for(STORY);
    writer.launch().unwrap();
    prepare_test_save_metadata(&mut writer, "slot.01");
    let saved = writer.save("slot.01").unwrap();

    let mut reopened = source_for(STORY);
    assert!(matches!(
        reopened.list_saves().unwrap().commands.as_slice(),
        [PlayerHostCommand::ListSaves { .. }]
    ));
    reopened
        .ingest_save_catalog_entry("slot.01", &saved)
        .unwrap();
    reopened.launch().unwrap();

    assert_eq!(
        reopened
            .product_observation_evidence()
            .unwrap()
            .occupied_save_slot_count,
        1
    );
}

#[test]
pub(super) fn native_vn_source_rejects_catalog_slot_identity_mismatch() {
    let mut writer = source_for(STORY);
    writer.launch().unwrap();
    prepare_test_save_metadata(&mut writer, "slot.01");
    let saved = writer.save("slot.01").unwrap();

    let mut reopened = source_for(STORY);
    assert!(reopened
        .ingest_save_catalog_entry("slot.02", &saved)
        .unwrap_err()
        .to_string()
        .contains("ASTRA_PLAYER_SAVE_CATALOG_IDENTITY"));
}

#[test]
pub(super) fn unavailable_catalog_slot_is_protected_while_other_slots_remain_playable() {
    let ui = TEST_UI.replace("save_slots:\"slot.01\"", "save_slots:\"slot.01,slot.02\"");
    let bytes = product_package_with_ui_and_request(STORY, &ui, test_compile_options(), |_| {});
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    let old_header = postcard::to_allocvec("astra.player_vn_save_envelope.v8").unwrap();
    assert!(source
        .ingest_save_catalog_entry("slot.01", &old_header)
        .is_err());
    source.reject_save_catalog_entry("slot.01");
    source.launch().unwrap();
    assert_eq!(
        source
            .product_observation_evidence()
            .unwrap()
            .occupied_save_slot_count,
        1
    );
    assert!(source
        .prepare_save_metadata("slot.01", "now".into(), 0)
        .unwrap_err()
        .to_string()
        .contains("SAVE_SLOT_PROTECTED"));
    assert!(source
        .save("slot.01")
        .unwrap_err()
        .to_string()
        .contains("SAVE_SLOT_PROTECTED"));
    advance(&mut source);
    prepare_test_save_metadata(&mut source, "slot.02");
    let saved = source.save("slot.02").unwrap();
    source.restore(&saved).unwrap();
    advance(&mut source);
    assert!(source
        .prepare_save_metadata("slot.01", "later".into(), 1)
        .unwrap_err()
        .to_string()
        .contains("SAVE_SLOT_PROTECTED"));
    source.release_resources().unwrap();
    source.shutdown().unwrap();
}

#[test]
pub(super) fn quick_keys_follow_declared_slot_and_reject_repeat_empty_and_protected_requests() {
    let ui = TEST_UI
        .replace("save_slots:\"slot.01\"", "save_slots:\"slot.01,slot.02\"")
        .replace("quick_slot:\"\"", "quick_slot:\"slot.02\"");
    let bytes = product_package_with_ui_and_request(STORY, &ui, test_compile_options(), |_| {});
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
    )
    .unwrap();
    source.launch().unwrap();
    let key = |name: &str, repeat| UiInputEventKind::Keyboard {
        logical_key: name.into(),
        physical_key: name.into(),
        state: UiButtonState::Pressed,
        repeat,
        modifiers: 0,
    };
    source.dispatch_ui_event(key("F9", false)).unwrap();
    assert!(source.take_ui_host_request().is_none());
    source.dispatch_ui_event(key("F5", true)).unwrap();
    assert!(source.take_ui_host_request().is_none());
    source.dispatch_ui_event(key("F5", false)).unwrap();
    assert!(
        matches!(source.take_ui_host_request(), Some(astra_player_vn::VnUiHostRequest::Save { slot_id, .. }) if slot_id == "slot.02")
    );
    prepare_test_save_metadata(&mut source, "slot.02");
    source.save("slot.02").unwrap();
    source.mark_save_committed("slot.02").unwrap();
    source.dispatch_ui_event(key("F9", false)).unwrap();
    assert!(
        matches!(source.take_ui_host_request(), Some(astra_player_vn::VnUiHostRequest::Load { slot_id }) if slot_id == "slot.02")
    );
    source.reject_save_catalog_entry("slot.02");
    for name in ["F5", "F9"] {
        source.dispatch_ui_event(key(name, false)).unwrap();
        assert!(source.take_ui_host_request().is_none());
    }
    let mut no_quick = source_for(STORY);
    no_quick.launch().unwrap();
    for name in ["F5", "F9"] {
        no_quick.dispatch_ui_event(key(name, false)).unwrap();
        assert!(no_quick.take_ui_host_request().is_none());
    }
    source.release_resources().unwrap();
    source.shutdown().unwrap();
    no_quick.release_resources().unwrap();
    no_quick.shutdown().unwrap();
}

#[test]
pub(super) fn native_vn_source_builds_atomic_platform_save_transaction() {
    let mut source = source_for(STORY);
    source.launch().unwrap();
    prepare_test_save_metadata(&mut source, "slot.01");

    let plan = source
        .prepare_save_transaction("slot.01", PlayerHostResourceId(20))
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
pub(super) fn native_vn_source_completes_wait_through_product_context() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    wait fence:voice.end #@id wait.voice
    text key:line.after #@id line.after
"#;
    let bytes = product_package_for(story);
    let package = PackageReader::open(&bytes).unwrap();
    let mut source = NativeVnHostCommandSource::from_package_with_execution(
        &package,
        VnRunConfig::classic("en"),
        320,
        180,
        PlayerHostResourceId(1),
        NativeVnRuntimeExecution::evidence_parallel(),
    )
    .unwrap();
    source.launch().unwrap();
    let before = source
        .last_step_evidence()
        .unwrap()
        .pending_wait_command_id
        .clone();

    source.complete_wait("voice.end").unwrap();

    assert_ne!(
        source.last_step_evidence().unwrap().pending_wait_command_id,
        before
    );
    advance(&mut source);
}

#[test]
pub(super) fn native_vn_source_exposes_validated_timeline_tasks_to_player() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    timeline id:intro target:camera property:x keyframes:0=0,120=1 join:block fence:timeline.intro.complete budget_ms:2 #@id timeline.intro
"#;
    let mut source = source_for(story);

    source.launch().unwrap();
    let tasks = source.take_timeline_tasks();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "intro");
    assert_eq!(tasks[0].target.as_deref(), Some("camera"));
    assert_eq!(tasks[0].duration_ms, Some(120));
    assert_eq!(tasks[0].fence.as_deref(), Some("timeline.intro.complete"));
}

#[test]
pub(super) fn packaged_native_vn_source_exposes_hash_validated_audio_requests() {
    let story = r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    bgm asset:asset:/voice/hero/0001 #@id bgm.hero.0001
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
    let preloads = source.take_audio_preload_requests();
    assert_eq!(preloads.len(), 1);
    assert_eq!(preloads[0].asset_id, "asset:/voice/hero/0001");
    assert_eq!(preloads[0].encoded_length, encoded.len() as u64);
    let audio = source.take_audio_requests();

    assert_eq!(audio.len(), 1);
    let NativeVnAudioOutput::Start(audio) = &audio[0] else {
        panic!("expected packaged audio start output");
    };
    assert_eq!(audio.asset_id, "asset:/voice/hero/0001");
    assert_eq!(audio.codec, "mp3");
    assert_eq!(audio.encoded_bytes.as_ref(), encoded.as_slice());
    assert_eq!(audio.encoded_length, encoded.len() as u64);
    assert_eq!(audio.attributes.get("asset"), Some(&audio.asset_id));
    let decode = source.prepare_audio_decode(audio).unwrap();
    assert!(matches!(
        decode.decode.commands.as_slice(),
        [PlayerHostCommand::Decode { codec, bytes, .. }] if codec == "mp3" && bytes.as_slice() == encoded
    ));
}

#[test]
pub(super) fn packaged_native_vn_source_routes_typed_audio_control_to_product_audio_owner() {
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

#[test]
pub(super) fn packaged_player_rejects_headless_presentation_binding() {
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
