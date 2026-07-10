use astra_platform::{
    build_fingerprint, PlatformCapabilityReport, PlatformErrorCode, PlatformHostFactory,
    PlatformHostProfile, PlatformId, SdkStatus, UnavailablePlatformFactory,
    PLATFORM_CAPABILITY_REPORT_SCHEMA,
};

#[test]
fn unavailable_report_does_not_claim_runtime_capabilities() {
    let report = PlatformCapabilityReport::unavailable(
        PlatformId::Macos,
        Some("nativevn-game"),
        SdkStatus::Missing,
        "sha256:build",
    );
    assert_eq!(report.schema, PLATFORM_CAPABILITY_REPORT_SCHEMA);
    assert!(report.renderer.declared.is_empty());
    assert!(report.renderer.available.is_empty());
    assert!(report.renderer.selected.is_none());
    assert_eq!(report.sdk_status, SdkStatus::Missing);
}

#[test]
fn build_fingerprint_is_hash_bound_to_name_version_and_features() {
    let first = build_fingerprint("astra-platform-linux", "0.1.0", ["default"]);
    let second = build_fingerprint("astra-platform-linux", "0.1.0", ["default", "test"]);
    assert!(first.starts_with("sha256:"));
    assert_ne!(first, second);
}

#[tokio::test]
async fn unavailable_factory_never_constructs_a_fake_host() {
    let mut profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    profile.platform = PlatformId::Linux;
    profile.id = "linux-stage6".to_string();
    let error = UnavailablePlatformFactory::new(PlatformId::Linux)
        .start(profile)
        .await
        .err()
        .expect("unavailable platform must not start");
    assert_eq!(error.code, PlatformErrorCode::PlatformNotImplemented);
    assert_eq!(
        error.fields.get("platform").map(String::as_str),
        Some("linux")
    );
}
