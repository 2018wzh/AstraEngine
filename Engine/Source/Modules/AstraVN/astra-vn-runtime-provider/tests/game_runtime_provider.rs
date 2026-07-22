use astra_core::SchemaVersion;
use astra_plugin_abi::{
    RuntimeOpenRequest, RuntimeOutputDomain, RuntimeStepInput, RuntimeStepMode,
    GAME_RUNTIME_PROVIDER_SLOT, NATIVE_VN_PROVIDER_ID, NATIVE_VN_RUNTIME_ID,
};
use astra_vn_runtime_provider::{
    compile_astra_project, AstraSource, NativeVnRuntimeProvider, PresentationCommand, StageCommand,
    SystemPageKind, TimelineCommand, VnAudioCommand, VnAudioControlAction, VnPlayerCommand,
    VnRunConfig, VnTimelineTask,
};

const STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    voice asset:asset:/voice/hero0001 #@id voice.hello
    text key:line.hello speaker:hero #@id line.hello
    choice key:choice.next #@id choice.next
      option key:choice.library -> library #@id choice.library

state library #@id state.library
  scene library #@id scene.library
    text key:line.library speaker:hero #@id line.library
"#;

#[astra_headless_test::test]
fn native_vn_provider_descriptor_declares_game_runtime_slot_contract() {
    let descriptor = NativeVnRuntimeProvider::descriptor();

    assert_eq!(NativeVnRuntimeProvider::slot(), GAME_RUNTIME_PROVIDER_SLOT);
    assert_eq!(descriptor.runtime_id, NATIVE_VN_RUNTIME_ID);
    assert_eq!(descriptor.provider_id, NATIVE_VN_PROVIDER_ID);
    assert!(descriptor
        .package_sections
        .contains(&"vn.story".to_string()));
    assert!(descriptor
        .release_checks
        .contains(&"runtime_provider.native_vn".to_string()));
    assert!(descriptor.output_schemas.iter().any(|schema| {
        schema.domain == RuntimeOutputDomain::Trace
            && schema.schema == "astra.vn.runtime_view_state.v1"
            && schema.version == SchemaVersion::new(1, 0, 0)
    }));
    assert!(!descriptor
        .output_schemas
        .iter()
        .any(|schema| schema.schema == "astra.vn.runtime_state.v2"));
}

#[astra_headless_test::test]
fn native_vn_provider_routes_v2_system_switch_and_hidden_reading_without_losing_awaits() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "system-v2.astra",
            r#"
story main #@id story.main
state start #@id state.start
  scene room #@id scene.room
    text key:line.one #@id line.one
    text key:line.two #@id line.two

story system #@id story.system
state save #@id state.system.save
  scene save #@id scene.system.save
    system_page kind:save #@id page.save
state load #@id state.system.load
  scene load #@id scene.system.load
    system_page kind:load #@id page.load
"#,
        )],
        Default::default(),
    )
    .unwrap();
    let mut provider = NativeVnRuntimeProvider::default();
    let open = provider
        .open_compiled_story(
            compiled,
            VnRunConfig::classic("ja"),
            RuntimeOpenRequest {
                target_id: "system-v2".into(),
                profile: "classic".into(),
                locale: "ja".into(),
                seed: 3,
                package_hash: "sha256:fixture".into(),
                sections: vec![],
            },
        )
        .unwrap();
    let step = |provider: &mut NativeVnRuntimeProvider,
                fixed_step: u64,
                command: Option<VnPlayerCommand>| {
        provider
            .step(RuntimeStepInput {
                session_id: open.session_id.clone(),
                fixed_step,
                delta_ns: 16_666_667,
                session_seed: 3,
                mode: RuntimeStepMode::Live,
                action: if command.is_some() {
                    "command".into()
                } else {
                    "launch_default".into()
                },
                payload: command
                    .map(|command| serde_json::to_value(command).unwrap())
                    .unwrap_or_else(|| serde_json::json!({})),
            })
            .unwrap();
    };

    step(&mut provider, 1, None);
    step(
        &mut provider,
        2,
        Some(VnPlayerCommand::OpenSystem {
            page: SystemPageKind::Save,
        }),
    );
    step(
        &mut provider,
        3,
        Some(VnPlayerCommand::SwitchSystemPage {
            page: SystemPageKind::Load,
        }),
    );
    assert_eq!(
        provider
            .runtime_snapshot(&open.session_id)
            .unwrap()
            .awaits
            .pending()
            .len(),
        2
    );
    step(&mut provider, 4, Some(VnPlayerCommand::ReturnSystem));
    assert_eq!(
        provider
            .runtime_snapshot(&open.session_id)
            .unwrap()
            .awaits
            .pending()
            .len(),
        1
    );
    step(
        &mut provider,
        5,
        Some(VnPlayerCommand::SetReadingMode {
            mode: astra_vn_runtime_provider::ReadingMode::Hidden,
        }),
    );
    let hidden_wait = provider
        .state(&open.session_id)
        .unwrap()
        .pending_wait
        .clone();
    step(&mut provider, 6, Some(VnPlayerCommand::Advance));
    assert_eq!(
        provider.state(&open.session_id).unwrap().pending_wait,
        hidden_wait
    );
    assert_eq!(
        provider
            .runtime_snapshot(&open.session_id)
            .unwrap()
            .awaits
            .pending()
            .len(),
        1
    );
    step(&mut provider, 7, Some(VnPlayerCommand::Advance));
    assert_eq!(
        provider
            .state(&open.session_id)
            .unwrap()
            .pending_wait
            .as_ref()
            .map(|wait| wait.command_id.as_str()),
        Some("line.two")
    );
}

#[astra_headless_test::test]
fn native_vn_provider_preserves_audio_and_stage_control_order() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "audio-order.astra",
            r#"
story main #@id story.main
state start #@id state.start
  scene start #@id scene.start
    bgm id:bgm.main asset:asset:/bgm/main loop:true fade:500 #@id bgm.start
    audio action:fade_stop target:bgm.main duration:1000 fence:bgm.main.end #@id bgm.stop
    wait fence:bgm.main.end #@id wait.bgm
"#,
        )],
        Default::default(),
    )
    .unwrap();
    let mut provider = NativeVnRuntimeProvider::default();
    let open = provider
        .open_compiled_story(
            compiled,
            VnRunConfig::classic("ja"),
            RuntimeOpenRequest {
                target_id: "audio-order".into(),
                profile: "classic".into(),
                locale: "ja".into(),
                seed: 1,
                package_hash: "sha256:fixture".into(),
                sections: vec![],
            },
        )
        .unwrap();

    let output = provider
        .step(RuntimeStepInput {
            session_id: open.session_id,
            fixed_step: 1,
            delta_ns: 16_666_667,
            session_seed: 1,
            mode: RuntimeStepMode::Live,
            action: "launch_default".into(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    let audio_index = output
        .outputs
        .iter()
        .position(|envelope| {
            matches!(
                envelope.decode_postcard::<VnAudioCommand>(
                    RuntimeOutputDomain::Audio,
                    "astra.vn.audio_command.v2",
                    SchemaVersion::new(2, 0, 0),
                ),
                Ok(command) if command.command_id == "bgm.main"
            )
        })
        .unwrap();
    let stop_index = output
        .outputs
        .iter()
        .position(|envelope| {
            matches!(
                envelope.decode_postcard::<PresentationCommand>(
                    RuntimeOutputDomain::Presentation,
                    "astra.vn.presentation_command.v2",
                    SchemaVersion::new(2, 0, 0),
                ),
                Ok(PresentationCommand::Stage(StageCommand::AudioControl(control)))
                    if matches!(control.action, VnAudioControlAction::FadeStop { .. })
            )
        })
        .unwrap();
    assert!(audio_index < stop_index);
}

#[astra_headless_test::test]
fn native_vn_provider_steps_compiled_story_through_runtime_session() {
    let compiled = compile_astra_project(
        [AstraSource::story("story.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let mut provider = NativeVnRuntimeProvider::default();
    let open = provider
        .open_compiled_story(
            compiled,
            VnRunConfig::classic("zh-Hans"),
            RuntimeOpenRequest {
                target_id: "nativevn-game".to_string(),
                profile: "classic".to_string(),
                locale: "zh-Hans".to_string(),
                seed: 7,
                package_hash: "sha256:fixture".to_string(),
                sections: vec![],
            },
        )
        .unwrap();

    let replay_error = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            delta_ns: 16_666_667,
            session_seed: 7,
            mode: RuntimeStepMode::Replay,
            action: "launch_default".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap_err();
    assert!(replay_error
        .to_string()
        .contains("ASTRA_NATIVE_VN_LIVE_PROVIDER_REPLAY"));
    assert_eq!(provider.runtime_snapshot(&open.session_id).unwrap().step, 0);

    let first = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            delta_ns: 16_666_667,
            session_seed: 7,
            mode: RuntimeStepMode::Live,
            action: "launch_default".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    assert_eq!(first.status, "blocked");
    assert!(!first.outputs.iter().any(|value| {
        value.domain == RuntimeOutputDomain::Trace && value.schema == "astra.vn.runtime_state.v2"
    }));
    assert!(first.outputs.iter().any(|value| {
        value.domain == RuntimeOutputDomain::Trace
            && value.schema == "astra.vn.runtime_view_state.v1"
            && value.version == SchemaVersion::new(1, 0, 0)
    }));
    assert!(first.outputs.iter().any(|value| {
        matches!(
            value.decode_postcard::<PresentationCommand>(
                RuntimeOutputDomain::Presentation,
                "astra.vn.presentation_command.v2",
                SchemaVersion::new(2, 0, 0)
            ),
            Ok(PresentationCommand::Dialogue { key, .. }) if key == "line.hello"
        )
    }));
    let runtime = provider.runtime_snapshot(&open.session_id).unwrap();
    assert!(runtime
        .machines
        .trace()
        .iter()
        .any(|trace| trace.action_id == "astra.vn.step"));
    assert!(runtime
        .mutations
        .iter()
        .any(|mutation| mutation.source == "astra.vn.step"));
    assert!(runtime
        .events
        .pending()
        .iter()
        .any(|event| event.payload.kind == "vn.route.reached"));
    assert!(runtime
        .effects
        .iter()
        .any(|effect| effect.envelope.domain == "audio" && effect.envelope.validate_hash()));
    assert_eq!(runtime.awaits.pending().len(), 1);
    let await_id = runtime.awaits.pending()[0].token_id.0.to_string();
    assert_eq!(
        provider
            .state(&open.session_id)
            .unwrap()
            .pending_wait
            .as_ref()
            .and_then(|wait| wait.await_id.as_deref()),
        Some(await_id.as_str())
    );

    let choice = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 2,
            delta_ns: 16_666_667,
            session_seed: 7,
            mode: RuntimeStepMode::Live,
            action: "advance".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    assert!(choice.outputs.iter().any(|value| matches!(
        value.decode_postcard::<PresentationCommand>(
            RuntimeOutputDomain::Presentation,
            "astra.vn.presentation_command.v2",
            SchemaVersion::new(2, 0, 0)
        ),
        Ok(PresentationCommand::Choice { .. })
    )));

    let selected = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 3,
            delta_ns: 16_666_667,
            session_seed: 7,
            mode: RuntimeStepMode::Live,
            action: "choose".to_string(),
            payload: serde_json::json!({ "option_id": "choice.library" }),
        })
        .unwrap();
    assert_eq!(selected.status, "blocked");
    assert!(selected.outputs.iter().any(|section| section
        .decode_postcard::<String>(
            RuntimeOutputDomain::DirtySaveSection,
            "astra.runtime.dirty_save_section.v1",
            SchemaVersion::new(1, 0, 0)
        )
        .is_ok_and(|section| section == "runtime.world")));

    let save = provider
        .save(astra_plugin_abi::RuntimeSaveRequest {
            session_id: open.session_id.clone(),
            slot: "slot.auto".to_string(),
        })
        .unwrap();
    assert_eq!(save.sections.len(), 1);
    assert_eq!(save.sections[0].section_id, "runtime.world");
    assert_eq!(save.sections[0].schema, "astra.runtime.save_blob.v2");
    assert!(save.sections[0].validate_hash());
    assert!(!save.sections[0].bytes.is_empty());
    let saved_hash = provider.runtime_snapshot(&open.session_id).unwrap();
    provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 4,
            delta_ns: 16_666_667,
            session_seed: 7,
            mode: RuntimeStepMode::Live,
            action: "advance".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    let after_step_four = provider.runtime_snapshot(&open.session_id).unwrap();
    let mut corrupted = save.sections.clone();
    let last = corrupted[0].bytes.len() - 1;
    corrupted[0].bytes[last] ^= 0x80;
    corrupted[0].hash = astra_core::Hash256::from_sha256(&corrupted[0].bytes);
    provider
        .restore(astra_plugin_abi::RuntimeRestoreRequest {
            session_id: open.session_id.clone(),
            sections: corrupted,
        })
        .unwrap_err();
    assert_eq!(
        provider.runtime_snapshot(&open.session_id).unwrap(),
        after_step_four
    );
    let restore = provider
        .restore(astra_plugin_abi::RuntimeRestoreRequest {
            session_id: open.session_id.clone(),
            sections: save.sections,
        })
        .unwrap();
    assert_eq!(restore.status, "restored");
    assert_eq!(restore.restored_fixed_step, 3);
    assert_eq!(restore.session_seed, 7);
    let restored_hash = provider.runtime_snapshot(&open.session_id).unwrap();
    assert_eq!(restored_hash, saved_hash);

    provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 4,
            delta_ns: 16_666_667,
            session_seed: 7,
            mode: RuntimeStepMode::RestoreContinuation,
            action: "advance".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();

    let shutdown = provider.shutdown(open.session_id).unwrap();
    assert_eq!(shutdown.status, "shutdown");
}

#[astra_headless_test::test]
fn native_vn_provider_returns_timeline_tasks_to_the_product_host() {
    let story = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    timeline id:intro target:hero property:opacity keyframes:0=0,120=1 join:block fence:timeline.intro.complete budget_ms:2 #@id timeline.intro
"#;
    let compiled = compile_astra_project(
        [AstraSource::story("timeline.astra", story)],
        Default::default(),
    )
    .unwrap();
    let mut provider = NativeVnRuntimeProvider::default();
    let open = provider
        .open_compiled_story(
            compiled,
            VnRunConfig::classic("zh-Hans"),
            RuntimeOpenRequest {
                target_id: "nativevn-game".to_string(),
                profile: "classic".to_string(),
                locale: "zh-Hans".to_string(),
                seed: 9,
                package_hash: "sha256:fixture".to_string(),
                sections: vec![],
            },
        )
        .unwrap();

    let first = provider
        .step(RuntimeStepInput {
            session_id: open.session_id,
            fixed_step: 1,
            delta_ns: 16_666_667,
            session_seed: 9,
            mode: RuntimeStepMode::Live,
            action: "launch_default".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();

    assert!(first.outputs.iter().any(|value| {
        value
            .decode_postcard::<VnTimelineTask>(
                RuntimeOutputDomain::Effect,
                "astra.vn.timeline_task.v1",
                SchemaVersion::new(1, 0, 0),
            )
            .is_ok_and(|task| {
                matches!(
                    task.command,
                    TimelineCommand::Start(spec)
                        if spec.fence.as_deref() == Some("timeline.intro.complete")
                )
            })
    }));
}
