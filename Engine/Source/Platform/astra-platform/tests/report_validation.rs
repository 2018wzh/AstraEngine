use astra_platform::{
    validate_capability_report, validate_conformance_report, ConformanceCheck, ConformanceStatus,
    PlatformCapabilityReport, PlatformHostConformanceReport, PlatformHostProfile,
    PlatformValidationStatus, PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA,
};

#[test]
fn capability_validation_blocks_declared_provider_without_runtime_availability() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let report = PlatformCapabilityReport::from_profile(
        &profile,
        "sha256:build",
        ["wgpu_hardware", "wmf", "saved_games"],
    )
    .unwrap();
    let (status, diagnostics) = validate_capability_report(&report);
    assert_eq!(status, PlatformValidationStatus::Blocked);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_PLATFORM_PROVIDER_UNAVAILABLE"));
}

#[test]
fn conformance_validation_requires_identity_continuity_and_every_product_check() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let capability = PlatformCapabilityReport::from_profile(
        &profile,
        "sha256:build",
        ["wgpu_hardware", "wmf", "wasapi", "saved_games"],
    )
    .unwrap();
    let mut report = PlatformHostConformanceReport {
        schema: PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA.to_string(),
        status: ConformanceStatus::Pass,
        platform: profile.platform,
        target: profile.target.clone(),
        profile_hash: profile.hash().unwrap(),
        package_hash: "sha256:package".to_string(),
        build_fingerprint: "sha256:build".to_string(),
        session_id: "session-1".to_string(),
        checks: astra_platform::required_conformance_checks(profile.platform)
            .iter()
            .map(|id| ConformanceCheck::pass(*id, [("status", "verified")]))
            .collect(),
        diagnostics: Vec::new(),
    };
    assert_eq!(
        validate_conformance_report(&capability, &report).0,
        PlatformValidationStatus::Pass
    );
    report.build_fingerprint = "sha256:other".to_string();
    assert_eq!(
        validate_conformance_report(&capability, &report).0,
        PlatformValidationStatus::Blocked
    );
}
