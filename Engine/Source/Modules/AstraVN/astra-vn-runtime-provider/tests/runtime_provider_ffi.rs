use abi_stable::std_types::{ROption, RVec};
use astra_plugin_abi::{
    FfiRuntimeInstanceRequest, FfiRuntimeIntegrityMode, FfiRuntimeOpenRequest,
    FfiRuntimePrepareRequest, FfiRuntimeRestoreRequest, FfiRuntimeSaveRequest, FfiRuntimeSection,
    FfiRuntimeSectionCodec, FfiRuntimeShutdownRequest, FfiRuntimeStepMode, FfiRuntimeStepRequest,
    PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA, PRODUCT_RUNTIME_PROVIDER_ABI_VERSION,
};
use astra_vn_runtime_provider::{compile_astra_project, AstraSource, NativeVnRuntimeProvider};

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
        registration.abi_version,
        PRODUCT_RUNTIME_PROVIDER_ABI_VERSION
    );
    assert_eq!(
        registration.descriptor_schema.as_str(),
        PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA
    );

    let prepare = (registration.prepare)(FfiRuntimePrepareRequest {
        target_id: "nativevn-game".into(),
        profile: "classic".into(),
        package_id: "package.fixture".into(),
        section_ids: RVec::from(vec!["vn.story".into()]),
    });
    assert!(prepare.ok);
    assert_eq!(prepare.status.as_str(), "pass");

    let instance_id = "ffi.test.instance";
    let created = (registration.create_instance)(FfiRuntimeInstanceRequest {
        instance_id: instance_id.into(),
    });
    assert!(created.ok);
    assert_eq!(created.status.as_str(), "created");

    let compiled = compile_astra_project(
        [AstraSource::story("ffi_story.astra", STORY)],
        Default::default(),
    )
    .unwrap();
    let compiled_bytes = postcard::to_allocvec(&compiled.story).unwrap();
    let opened = (registration.open_session)(FfiRuntimeOpenRequest {
        instance_id: instance_id.into(),
        target_id: "nativevn-game".into(),
        profile: "classic".into(),
        locale: "zh-Hans".into(),
        seed: 41,
        integrity_mode: FfiRuntimeIntegrityMode::Evidence,
        worker_count: 4,
        package_id: "package.fixture".into(),
        sections: RVec::from(vec![FfiRuntimeSection {
            section_id: "vn.story".into(),
            schema: "astra.vn.story".into(),
            version_major: 1,
            version_minor: 0,
            version_patch: 0,
            codec: FfiRuntimeSectionCodec::Postcard,
            bytes: RVec::from(compiled_bytes),
        }]),
    });
    assert!(opened.ok, "{:?}", opened.diagnostics);
    let step = (registration.step)(FfiRuntimeStepRequest {
        instance_id: instance_id.into(),
        session_handle: opened.session_handle,
        session_id: opened.session_id.clone(),
        fixed_step: 1,
        delta_ns: 16_666_667,
        session_seed: 41,
        mode: FfiRuntimeStepMode::Live,
        action: "launch_default".into(),
        argument: ROption::RNone,
        auxiliary: ROption::RNone,
        flag: ROption::RNone,
        input_edges: RVec::new(),
        await_results: RVec::new(),
        provider_results: RVec::new(),
        max_instructions: 100_000,
        max_effects: 256,
        max_trace_entries: 256,
    });
    assert!(step.ok, "{:?}", step.diagnostics);
    assert_eq!(step.status.as_str(), "blocked");
    assert!(step.persisted.iter().any(|output| output.domain == 1));

    let save = (registration.save)(FfiRuntimeSaveRequest {
        instance_id: instance_id.into(),
        session_handle: opened.session_handle,
        session_id: opened.session_id.clone(),
        slot: "slot.ffi".into(),
    });
    assert!(save.ok, "{:?}", save.diagnostics);
    assert_eq!(save.sections.len(), 1);
    assert_eq!(save.sections[0].section_id.as_str(), "runtime.world");
    assert_eq!(
        save.sections[0].schema.as_str(),
        "astra.runtime.save_blob.v4"
    );

    let restore = (registration.restore)(FfiRuntimeRestoreRequest {
        instance_id: instance_id.into(),
        session_handle: opened.session_handle,
        session_id: opened.session_id.clone(),
        sections: RVec::from(
            save.sections
                .into_iter()
                .map(|section| FfiRuntimeSection {
                    section_id: section.section_id,
                    schema: section.schema,
                    version_major: section.version_major,
                    version_minor: section.version_minor,
                    version_patch: section.version_patch,
                    codec: section.codec,
                    bytes: section.bytes,
                })
                .collect::<Vec<_>>(),
        ),
    });
    assert!(restore.ok, "{:?}", restore.diagnostics);
    assert_eq!(restore.status.as_str(), "restored");
    assert_eq!(restore.restored_fixed_step, 1);
    assert_eq!(restore.session_seed, 41);

    let shutdown = (registration.shutdown)(FfiRuntimeShutdownRequest {
        instance_id: instance_id.into(),
        session_handle: opened.session_handle,
        session_id: opened.session_id,
    });
    assert!(shutdown.ok, "{:?}", shutdown.diagnostics);
    assert_eq!(shutdown.status.as_str(), "shutdown");
    let destroyed = (registration.destroy_instance)(FfiRuntimeInstanceRequest {
        instance_id: instance_id.into(),
    });
    assert!(destroyed.ok, "{:?}", destroyed.diagnostics);
    assert_eq!(destroyed.status.as_str(), "destroyed");

    let metadata = (registration.editor_metadata)();
    assert!(metadata.ok, "{:?}", metadata.diagnostics);
    assert!(metadata
        .authoring_surfaces
        .iter()
        .any(|surface| surface.as_str() == "graph"));
}
