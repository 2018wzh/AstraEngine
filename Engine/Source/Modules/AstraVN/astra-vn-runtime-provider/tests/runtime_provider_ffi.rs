use abi_stable::std_types::RVec;
use astra_core::{Hash256, SchemaVersion};
use astra_plugin_abi::{
    GameRuntimeSessionId, ProviderInstanceId, RuntimeEditorMetadata, RuntimeOpenReport,
    RuntimeOpenRequest, RuntimeOutputDomain, RuntimePrepareReport, RuntimePrepareRequest,
    RuntimeProviderCall, RuntimeProviderCreateRequest, RuntimeProviderDestroyRequest,
    RuntimeProviderInstanceReport, RuntimeRestoreReport, RuntimeRestoreRequest, RuntimeSaveRequest,
    RuntimeSaveSections, RuntimeSectionCodec, RuntimeSectionPayload, RuntimeShutdownReport,
    RuntimeStepInput, RuntimeStepMode, RuntimeStepOutput, PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA,
};
use astra_vn_runtime_provider::{compile_astra_project, AstraSource, NativeVnRuntimeProvider};
use serde::{de::DeserializeOwned, Serialize};

const STORY: &str = r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:line.hello speaker:hero #@id line.hello
"#;

#[astra_headless_test::test]
fn native_vn_runtime_provider_ffi_runs_a_real_session_lifecycle() {
    let registration = NativeVnRuntimeProvider::ffi_registration();
    assert_eq!(registration.provider_id.as_str(), "astra.runtime.native_vn");
    assert_eq!(registration.runtime_id.as_str(), "native_vn");
    assert_eq!(
        registration.descriptor_schema.as_str(),
        PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA
    );

    let prepare = invoke::<_, RuntimePrepareReport>(
        registration.prepare,
        &RuntimePrepareRequest {
            target_id: "nativevn-game".to_string(),
            profile: "classic".to_string(),
            package_hash: "sha256:fixture".to_string(),
            section_ids: vec!["vn.story".to_string()],
        },
    );
    assert_eq!(prepare.status, "pass");

    let instance_id = ProviderInstanceId("ffi.test.instance".to_string());
    let created = invoke::<_, RuntimeProviderInstanceReport>(
        registration.create_instance,
        &RuntimeProviderCreateRequest {
            instance_id: instance_id.clone(),
        },
    );
    assert_eq!(created.status, "created");

    let compiled = compile_astra_project(
        [AstraSource::story("ffi_story.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let compiled_bytes = postcard::to_allocvec(&compiled.story).unwrap();
    let open = invoke_call::<_, RuntimeOpenReport>(
        registration.open,
        &instance_id,
        &RuntimeOpenRequest {
            target_id: "nativevn-game".to_string(),
            profile: "classic".to_string(),
            locale: "zh-Hans".to_string(),
            seed: 41,
            package_hash: "sha256:fixture".to_string(),
            sections: vec![RuntimeSectionPayload {
                section_id: "vn.story".to_string(),
                schema: "astra.vn.story".to_string(),
                version: SchemaVersion::default(),
                codec: RuntimeSectionCodec::Postcard,
                hash: Hash256::from_sha256(&compiled_bytes),
                bytes: compiled_bytes,
            }],
        },
    );
    let step = invoke_call::<_, RuntimeStepOutput>(
        registration.step,
        &instance_id,
        &RuntimeStepInput {
            session_id: open.session_id.clone(),
            fixed_step: 1,
            delta_ns: 16_666_667,
            session_seed: 41,
            mode: RuntimeStepMode::Live,
            action: "launch_default".to_string(),
            payload: serde_json::json!({}),
        },
    );
    assert_eq!(step.status, "blocked");
    assert!(step
        .outputs
        .iter()
        .any(|output| output.domain == RuntimeOutputDomain::Presentation));

    let save = invoke_call::<_, RuntimeSaveSections>(
        registration.save,
        &instance_id,
        &RuntimeSaveRequest {
            session_id: open.session_id.clone(),
            slot: "slot.ffi".to_string(),
        },
    );
    assert_eq!(save.sections.len(), 1);
    assert_eq!(save.sections[0].section_id, "runtime.world");
    assert_eq!(save.sections[0].schema, "astra.runtime.save_blob.v2");
    assert!(save
        .sections
        .iter()
        .all(RuntimeSectionPayload::validate_hash));

    let restore = invoke_call::<_, RuntimeRestoreReport>(
        registration.restore,
        &instance_id,
        &RuntimeRestoreRequest {
            session_id: open.session_id.clone(),
            sections: save.sections,
        },
    );
    assert_eq!(restore.status, "restored");
    assert_eq!(restore.restored_fixed_step, 1);
    assert_eq!(restore.session_seed, 41);

    let shutdown = invoke_call::<_, RuntimeShutdownReport>(
        registration.shutdown,
        &instance_id,
        &GameRuntimeSessionId(open.session_id.0),
    );
    assert_eq!(shutdown.status, "shutdown");
    let destroyed = invoke::<_, RuntimeProviderInstanceReport>(
        registration.destroy_instance,
        &RuntimeProviderDestroyRequest { instance_id },
    );
    assert_eq!(destroyed.status, "destroyed");

    let metadata = invoke_empty::<RuntimeEditorMetadata>(registration.editor_metadata);
    assert!(metadata.authoring_surfaces.contains(&"graph".to_string()));
}

fn invoke<T: Serialize, R: DeserializeOwned>(
    callback: astra_plugin_abi::FfiRuntimeProviderInvoke,
    request: &T,
) -> R {
    decode(callback(RVec::from(serde_json::to_vec(request).unwrap())))
}

fn invoke_call<T: Serialize, R: DeserializeOwned>(
    callback: astra_plugin_abi::FfiRuntimeProviderInvoke,
    instance_id: &ProviderInstanceId,
    request: &T,
) -> R {
    invoke(
        callback,
        &RuntimeProviderCall {
            instance_id: instance_id.clone(),
            payload: serde_json::to_vec(request).unwrap(),
        },
    )
}

fn invoke_empty<R: DeserializeOwned>(callback: astra_plugin_abi::FfiRuntimeProviderInvoke) -> R {
    decode(callback(Vec::new().into()))
}

fn decode<R: DeserializeOwned>(result: astra_plugin_abi::FfiRuntimeProviderResult) -> R {
    assert!(result.ok, "{:?}", result.diagnostics);
    serde_json::from_slice(result.payload.as_slice()).unwrap()
}
