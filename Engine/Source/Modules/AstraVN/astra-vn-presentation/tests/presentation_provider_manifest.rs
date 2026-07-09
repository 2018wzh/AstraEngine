use astra_vn_presentation::{VnPresentationProviderManifest, VnWaitKind};

#[test]
fn presentation_provider_manifest_declares_filter_fallback_and_await_capabilities() {
    let report = VnPresentationProviderManifest::standard().validate_standard();

    assert!(report.passed, "{report:?}");
    assert!(report.filter_count >= 1);
    assert!(report.wait_capability_count >= 4);
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
