use astra_core::SchemaVersion;
use astra_plugin_abi::{
    RuntimeOpenRequest, RuntimeOutputDomain, RuntimeStepInput, GAME_RUNTIME_PROVIDER_SLOT,
    NATIVE_VN_PROVIDER_ID, NATIVE_VN_RUNTIME_ID,
};
use astra_vn_runtime_provider::{
    compile_astra_sources, AstraSource, NativeVnRuntimeProvider, PresentationCommand, VnRunConfig,
    VnTimelineTask,
};

const STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    voice asset:voice.hero.0001 end:continue #@id voice.hello
    text key:line.hello speaker:hero #@id line.hello
    choice key:choice.next #@id choice.next
      option key:choice.library -> library #@id choice.library

state library #@id state.library
  scene library #@id scene.library
    text key:line.library speaker:hero #@id line.library
"#;

#[test]
fn native_vn_provider_descriptor_declares_game_runtime_slot_contract() {
    let descriptor = NativeVnRuntimeProvider::descriptor();

    assert_eq!(NativeVnRuntimeProvider::slot(), GAME_RUNTIME_PROVIDER_SLOT);
    assert_eq!(descriptor.runtime_id, NATIVE_VN_RUNTIME_ID);
    assert_eq!(descriptor.provider_id, NATIVE_VN_PROVIDER_ID);
    assert!(descriptor
        .package_sections
        .contains(&"vn.compiled_story".to_string()));
    assert!(descriptor
        .release_checks
        .contains(&"runtime_provider.native_vn".to_string()));
}

#[test]
fn native_vn_provider_steps_compiled_story_through_runtime_session() {
    let compiled = compile_astra_sources([AstraSource::new("story.astra", STORY)]).unwrap();
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

    let first = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            action: "launch_default".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    assert_eq!(first.status, "blocked");
    assert!(first.outputs.iter().any(|value| {
        matches!(
            value.decode_postcard::<PresentationCommand>(
                RuntimeOutputDomain::Presentation,
                "astra.vn.presentation_command.v1",
                SchemaVersion::new(1, 0, 0)
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
            action: "advance".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    assert!(choice.outputs.iter().any(|value| matches!(
        value.decode_postcard::<PresentationCommand>(
            RuntimeOutputDomain::Presentation,
            "astra.vn.presentation_command.v1",
            SchemaVersion::new(1, 0, 0)
        ),
        Ok(PresentationCommand::Choice { .. })
    )));

    let selected = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 3,
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
        .is_ok_and(|section| section == "vn.runtime_state")));

    let save = provider
        .save(astra_plugin_abi::RuntimeSaveRequest {
            session_id: open.session_id.clone(),
            slot: "slot.auto".to_string(),
        })
        .unwrap();
    assert!(save.sections.iter().any(|section| {
        section.section_id == "vn.runtime_state"
            && section.hash.to_string().starts_with("sha256:")
            && !section.bytes.is_empty()
    }));
    let saved_hash = astra_core::Hash128::from_blake3(
        &postcard::to_allocvec(&provider.state(&open.session_id).unwrap()).unwrap(),
    );
    provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 4,
            action: "advance".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    let restore = provider
        .restore(astra_plugin_abi::RuntimeRestoreRequest {
            session_id: open.session_id.clone(),
            sections: save.sections,
        })
        .unwrap();
    assert_eq!(restore.status, "restored");
    let restored_hash = astra_core::Hash128::from_blake3(
        &postcard::to_allocvec(&provider.state(&open.session_id).unwrap()).unwrap(),
    );
    assert_eq!(restored_hash, saved_hash);

    let shutdown = provider.shutdown(open.session_id).unwrap();
    assert_eq!(shutdown.status, "shutdown");
}

#[test]
fn native_vn_provider_returns_timeline_tasks_to_the_product_host() {
    let story = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    timeline id:intro target:hero join:block fence:timeline.intro.complete duration:120 #@id timeline.intro
"#;
    let compiled = compile_astra_sources([AstraSource::new("timeline.astra", story)]).unwrap();
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
            fixed_step: 0,
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
                task.command == "timeline"
                    && task.attributes.get("fence").map(String::as_str)
                        == Some("timeline.intro.complete")
            })
    }));
}
