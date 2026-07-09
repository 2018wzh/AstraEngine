use astra_plugin_abi::{
    RuntimeEditorMetadata, RuntimePrepareReport, RuntimePrepareRequest,
    PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA,
};
use astra_vn_runtime_provider::NativeVnRuntimeProvider;

#[test]
fn native_vn_runtime_provider_ffi_registers_descriptor_and_invokes_entrypoints() {
    let registration = NativeVnRuntimeProvider::ffi_registration();
    assert_eq!(registration.provider_id.as_str(), "astra.runtime.native_vn");
    assert_eq!(registration.runtime_id.as_str(), "native_vn");
    assert_eq!(
        registration.descriptor_schema.as_str(),
        PRODUCT_RUNTIME_DESCRIPTOR_SCHEMA
    );

    let prepare_payload = serde_json::to_vec(&RuntimePrepareRequest {
        target_id: "nativevn-game".to_string(),
        profile: "classic".to_string(),
        package_hash: "sha256:fixture".to_string(),
        section_ids: vec!["vn.compiled_story".to_string()],
    })
    .unwrap();
    let prepare = (registration.prepare)(prepare_payload.into());
    assert!(prepare.ok, "{:?}", prepare.diagnostics);
    let report: RuntimePrepareReport = serde_json::from_slice(prepare.payload.as_slice()).unwrap();
    assert_eq!(report.status, "pass");

    let metadata = (registration.editor_metadata)(Vec::new().into());
    assert!(metadata.ok, "{:?}", metadata.diagnostics);
    let metadata: RuntimeEditorMetadata =
        serde_json::from_slice(metadata.payload.as_slice()).unwrap();
    assert!(metadata.authoring_surfaces.contains(&"graph".to_string()));
}
