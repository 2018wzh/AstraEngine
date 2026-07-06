use astra_package::{PackageBuildRequest, PackageBuilder, SectionPayload};
use astra_platform::{
    PlatformCapabilityReport, PlatformId, PlatformSmokeCheck, PlatformSmokeStatus, SdkStatus,
};
use astra_release::{CheckStatus, PackageValidateRequest, ReleaseDomain, ReleaseValidator};

#[test]
fn release_report_covers_pass_warning_and_blocked_checks() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    ))
    .unwrap();

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            platform_report: None,
        })
        .unwrap();
    assert_eq!(report.schema, "astra.release_report.v1");
    assert!(report.checks.iter().any(|check| {
        check.domain == ReleaseDomain::Package && check.status == CheckStatus::Pass
    }));
    assert!(report.checks.iter().any(|check| {
        check.domain == ReleaseDomain::Media && check.status == CheckStatus::Warning
    }));
    assert!(report.checks.iter().any(|check| {
        check.domain == ReleaseDomain::Target && check.status == CheckStatus::Pass
    }));

    let blocked = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: b"not a package".to_vec(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: true,
            target: None,
            platform_report: None,
        })
        .unwrap();
    assert_eq!(blocked.status, CheckStatus::Blocked);
    assert!(blocked
        .checks
        .iter()
        .any(|check| check.id == "package.integrity" && check.status == CheckStatus::Blocked));
}

#[test]
fn release_report_blocks_windows_platform_report_without_required_smoke() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    ))
    .unwrap();

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            platform_report: Some(PlatformCapabilityReport::new(
                PlatformId::Windows,
                Some("native-smoke-game".to_string()),
                SdkStatus::Present,
                vec!["wgpu".to_string()],
                vec!["wmf".to_string()],
                vec!["wasapi".to_string()],
                vec!["known_folder".to_string()],
                vec!["keyboard".to_string()],
                vec!["window".to_string()],
                vec!["network_runtime_ai_profile_gated".to_string()],
            )),
        })
        .unwrap();

    let platform_check = report
        .checks
        .iter()
        .find(|check| check.id == "platform.capability_report")
        .unwrap();

    assert_eq!(platform_check.status, CheckStatus::Blocked);
    assert_eq!(
        platform_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PLATFORM_SMOKE_MISSING"
    );
}

#[test]
fn release_report_includes_windows_platform_smoke_evidence() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    ))
    .unwrap();
    let platform_report = PlatformCapabilityReport::new(
        PlatformId::Windows,
        Some("native-smoke-game".to_string()),
        SdkStatus::Present,
        vec!["winit_window".to_string()],
        vec!["wmf".to_string()],
        vec!["wasapi".to_string()],
        vec!["known_folder".to_string()],
        vec!["keyboard".to_string()],
        vec!["window".to_string()],
        vec!["network_runtime_ai_profile_gated".to_string()],
    )
    .with_smoke(vec![
        smoke("windowed_smoke", PlatformSmokeStatus::Pass),
        smoke("decode.wmf", PlatformSmokeStatus::Pass),
        smoke("save.known_folder", PlatformSmokeStatus::Pass),
    ]);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            platform_report: Some(platform_report),
        })
        .unwrap();

    let platform_check = report
        .checks
        .iter()
        .find(|check| check.id == "platform.capability_report")
        .unwrap();
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| { entry.key == "smoke.windowed_smoke.status" && entry.value == "pass" }));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "smoke.decode.wmf.status" && entry.value == "pass"));
}

#[test]
fn release_gate_blocks_package_target_manifests_with_editor_descriptors() {
    let mut request = PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.target_manifest = serde_json::json!({
        "schema": "astra.target_manifest.v1",
        "targets": [
            {
                "id": "native-smoke-game",
                "kind": "game",
                "crate": "astra-runtime",
                "default_profile": "desktop-release",
                "platforms": ["windows"],
                "packaged": true
            },
            {
                "id": "native-editor",
                "kind": "editor",
                "binary": "astra-editor",
                "platforms": ["windows"],
                "packaged": false
            }
        ]
    })
    .to_string()
    .into_bytes();
    let blob = PackageBuilder::build(request).unwrap();

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            platform_report: None,
        })
        .unwrap();

    let target_check = report
        .checks
        .iter()
        .find(|check| check.id == "target.manifest")
        .unwrap();
    assert_eq!(target_check.status, CheckStatus::Blocked);
    assert_eq!(
        target_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TARGET_PACKAGE_SHAPE"
    );
}

fn smoke(id: &str, status: PlatformSmokeStatus) -> PlatformSmokeCheck {
    PlatformSmokeCheck {
        id: id.to_string(),
        status,
        summary: format!("{id} test evidence"),
    }
}
