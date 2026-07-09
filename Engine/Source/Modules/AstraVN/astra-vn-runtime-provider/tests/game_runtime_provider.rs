use astra_plugin_abi::{
    RuntimeOpenRequest, RuntimeStepInput, GAME_RUNTIME_PROVIDER_SLOT, NATIVE_VN_PROVIDER_ID,
    NATIVE_VN_RUNTIME_ID,
};
use astra_vn_runtime_provider::{
    compile_astra_sources, AstraSource, NativeVnRuntimeProvider, VnRunConfig,
};

const STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
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
                seed: 7,
                package_hash: "sha256:fixture".to_string(),
            },
        )
        .unwrap();

    let first = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 0,
            action: "launch_default".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    assert_eq!(first.status, "blocked");
    assert!(first.presentation.iter().any(|value| {
        value
            .get("Dialogue")
            .and_then(|dialogue| dialogue.get("key"))
            .and_then(|key| key.as_str())
            == Some("line.hello")
    }));

    let choice = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            action: "advance".to_string(),
            payload: serde_json::json!({}),
        })
        .unwrap();
    assert!(choice
        .presentation
        .iter()
        .any(|value| value.get("Choice").is_some()));

    let selected = provider
        .step(RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 2,
            action: "choose".to_string(),
            payload: serde_json::json!({ "option_id": "choice.library" }),
        })
        .unwrap();
    assert_eq!(selected.status, "blocked");
    assert!(selected
        .dirty_save_sections
        .contains(&"vn.runtime_state".to_string()));

    let save = provider
        .save(astra_plugin_abi::RuntimeSaveRequest {
            session_id: open.session_id.clone(),
            slot: "slot.auto".to_string(),
        })
        .unwrap();
    assert!(save.sections.iter().any(|section| {
        section.section_id == "vn.runtime_state" && section.hash.starts_with("sha256:")
    }));

    let shutdown = provider.shutdown(open.session_id).unwrap();
    assert_eq!(shutdown.status, "shutdown");
}
