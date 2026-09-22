use astra_plugin_abi::{ProductRuntimeDescriptor, NATIVE_VN_PROVIDER_ID, NATIVE_VN_RUNTIME_ID};
pub fn native_vn_descriptor() -> ProductRuntimeDescriptor {
    ProductRuntimeDescriptor {
        runtime_id: NATIVE_VN_RUNTIME_ID.to_string(),
        product_kind: "visual_novel".to_string(),
        provider_id: NATIVE_VN_PROVIDER_ID.to_string(),
        presentation_lane: astra_plugin_abi::RuntimePresentationLane::Scene2D,
        supported_targets: vec!["game".to_string()],
        capabilities: vec!["runtime.native_vn".to_string()],
        package_sections: native_vn_package_sections(),
        release_checks: native_vn_release_check_ids(),
    }
}

fn native_vn_package_sections() -> Vec<String> {
    [
        "vn.compiled_project",
        "vn.story",
        "vn.ui_blueprint_bundle",
        "vn.ui_binding_manifest",
        "vn.ui_source_map",
        "vn.ui_controller_manifest",
        "vn.ui_theme_manifest",
        "vn.ui_backend_manifest",
        "vn.ui_component_manifest",
        "vn.profile_manifest",
        "vn.policy_bundle_manifest",
        "vn.extension_manifest",
        "vn.standard_command_manifest",
        "vn.presentation_provider_manifest",
        "vn.commercial_baseline_manifest",
        "vn.system_story_manifest",
        "vn.system_ui_profile_manifest",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn native_vn_release_check_ids() -> Vec<String> {
    [
        "runtime_provider.native_vn",
        "vn.commercial_baseline",
        "vn.system_ui_profile",
        "vn.advanced_presentation",
        "player.full_playable",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
