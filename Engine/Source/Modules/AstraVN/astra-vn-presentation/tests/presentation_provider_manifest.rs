use astra_vn_presentation::{VnPresentationProviderManifest, VnWaitKind};

#[test]
fn presentation_provider_manifest_declares_filter_fallback_and_await_capabilities() {
    let manifest = VnPresentationProviderManifest::standard();
    let report = manifest.validate_standard();

    assert!(report.passed, "{report:?}");
    assert!(report.filter_count >= 1);
    assert!(report.wait_capability_count >= 4);
    assert_eq!(report.profile_count, 3);
    assert_eq!(report.preset_count, 4);
    assert_eq!(
        manifest
            .resolve_preset("advanced-vn", "camera", "slow_push")
            .unwrap()
            .duration_ms,
        480
    );
}

#[test]
fn presentation_provider_manifest_blocks_unknown_and_wrong_command_presets() {
    let manifest = VnPresentationProviderManifest::standard();

    assert_eq!(
        manifest
            .resolve_preset("advanced-vn", "show", "missing")
            .unwrap_err()
            .code,
        "ASTRA_VN_PRESENTATION_PRESET_UNDECLARED"
    );
    assert_eq!(
        manifest
            .resolve_preset("advanced-vn", "show", "slow_push")
            .unwrap_err()
            .code,
        "ASTRA_VN_PRESENTATION_PRESET_COMMAND"
    );
}

#[test]
fn presentation_provider_manifest_blocks_duplicate_and_over_budget_policy() {
    let mut manifest = VnPresentationProviderManifest::standard();
    manifest.presets.push(manifest.presets[0].clone());
    manifest.presets[0]
        .command_kinds
        .push("background".to_string());
    manifest.profiles[0].max_effect_budget_us = 1;
    manifest.profiles[0]
        .allowed_filters
        .push("astra.filter.fade".to_string());

    let report = manifest.validate_standard();

    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_PRESENTATION_PRESET_ID"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| { diagnostic.code == "ASTRA_VN_PRESENTATION_PROFILE_PRESET_BUDGET" }));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_PRESENTATION_PRESET_COMMAND"));
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_PRESENTATION_PROFILE_FILTER"));
}

#[test]
fn presentation_provider_manifest_blocks_missing_movie_await_capability() {
    let mut manifest = VnPresentationProviderManifest::standard();
    manifest
        .wait_capabilities
        .retain(|kind| *kind != VnWaitKind::MovieEnd);

    let report = manifest.validate_standard();

    assert!(!report.passed);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_VN_PRESENTATION_WAIT_CAPABILITY"));
}
