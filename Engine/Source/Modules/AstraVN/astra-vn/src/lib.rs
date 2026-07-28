//! AstraVN compatibility facade.
//!
//! This is a compatibility facade, not an umbrella crate. A glob re-export of
//! every implementation crate made a Windows Rust `dylib` export every
//! transitive public symbol, exceeding the PE export-table limit before Player
//! packaging could start. Keep this in-process product surface deliberately
//! small; feature crates remain the owner of their public APIs.

pub use astra_vn_core::{VnPlayerCommand, VnRunConfig, VnRuntime, VnRuntimeViewState, VnWaitKind};
pub use astra_vn_package::{
    decode_compiled_project, package_sections_for_project,
    package_sections_for_project_with_components, VnAdvancedPresentationManifest,
    VnCommercialBaselineManifest, VnExtensionManifest, VnProfileManifest,
    VnStandardCommandManifest, VnUiComponentArtifactInput, VnUiComponentBundleManifest,
    VnUiComponentTarget, LoadedVnUiComponentArtifact, load_ui_component_artifact,
    load_player_locale_config, load_presentation_provider_manifest, PlayerLocaleConfig,
    PLAYER_LOCALE_CONFIG_SCHEMA,
};
pub use astra_vn_presentation::{
    StageModel, VnPresentationProviderManifest,
};
pub use astra_vn_runtime_provider::NativeVnRuntimeProvider;
pub use astra_vn_script::{
    compile_astra_project, format_astra_source, AstraSource, CompileAstraProjectOptions,
    FormatOptions, SystemStoryValidationStatus, VN_RUNTIME_VIEW_STATE_SCHEMA,
    VN_RUNTIME_VIEW_STATE_SCHEMA_MAJOR,
};
pub use astra_vn_system::{
    SystemStoryManifest, VnSystemUiProfileManifest,
};
pub use astra_vn_policy::{
    LuauUiControllerHost, VnPolicyBundleManifest, VnPolicyBundleSourceCache,
};
