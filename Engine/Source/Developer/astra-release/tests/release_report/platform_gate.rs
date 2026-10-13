use super::*;

#[test]
pub(super) fn release_profile_blocks_missing_platform_report() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
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
            require_platform_report: true,
            platform_report: None,
        })
        .unwrap();

    let platform_check = report
        .checks
        .iter()
        .find(|check| check.id == "platform.capability_report")
        .unwrap();
    assert_eq!(report.status, CheckStatus::Blocked);
    assert_eq!(platform_check.status, CheckStatus::Blocked);
    assert_eq!(
        platform_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PLATFORM_REPORT_MISSING"
    );
}

#[test]
pub(super) fn dev_profile_warns_on_missing_platform_report() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "dev",
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
            profile: "dev".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let platform_check = report
        .checks
        .iter()
        .find(|check| check.id == "platform.capability_report")
        .unwrap();
    assert_eq!(platform_check.status, CheckStatus::Warning);
}

#[test]
pub(super) fn release_profile_blocks_fixture_package_without_cooked_project() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
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
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let cooked_project_check = report
        .checks
        .iter()
        .find(|check| check.id == "package.cooked_project")
        .unwrap();
    assert_eq!(report.status, CheckStatus::Blocked);
    assert_eq!(cooked_project_check.status, CheckStatus::Blocked);
    assert_eq!(
        cooked_project_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PACKAGE_COOKED_PROJECT_MISSING"
    );
}

#[test]
pub(super) fn release_profile_accepts_cooked_project_input_section() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![
            SectionPayload::raw(
                "asset.characters.hero",
                "astra.cooked_asset.v1",
                b"hero".to_vec(),
            ),
            cooked_project_section("desktop-release", "native-smoke-game"),
        ],
    ))
    .unwrap();

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let cooked_project_check = report
        .checks
        .iter()
        .find(|check| check.id == "package.cooked_project")
        .unwrap();
    assert_eq!(cooked_project_check.status, CheckStatus::Pass);
    assert!(cooked_project_check
        .evidence
        .iter()
        .any(|entry| { entry.key == "section" && entry.value == "compiled.project" }));
}

#[test]
pub(super) fn release_profile_blocks_package_profile_mismatch() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "dev",
        vec![
            SectionPayload::raw(
                "asset.characters.hero",
                "astra.cooked_asset.v1",
                b"hero".to_vec(),
            ),
            cooked_project_section("dev", "native-smoke-game"),
        ],
    ))
    .unwrap();

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let cooked_project_check = report
        .checks
        .iter()
        .find(|check| check.id == "package.cooked_project")
        .unwrap();
    assert_eq!(cooked_project_check.status, CheckStatus::Blocked);
    assert_eq!(
        cooked_project_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PACKAGE_PROFILE_MISMATCH"
    );
}

#[test]
pub(super) fn release_report_blocks_windows_platform_report_without_required_provider() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
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
            require_platform_report: true,
            platform_report: Some(platform_capability(
                PlatformId::Windows,
                "native-smoke-game",
                &["wgpu_hardware", "wmf", "wasapi"],
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
        "ASTRA_PLATFORM_PROVIDER_UNAVAILABLE"
    );
}

#[test]
pub(super) fn release_report_includes_windows_platform_provider_evidence() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    ))
    .unwrap();
    let platform_report = platform_capability(
        PlatformId::Windows,
        "native-smoke-game",
        &["wgpu_hardware", "wmf", "wasapi", "saved_games"],
    );

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("native-smoke-game".to_string()),
            require_platform_report: true,
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
        .any(|entry| entry.key == "provider.renderer.selected" && entry.value == "wgpu_hardware"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "provider.decode.selected" && entry.value == "wmf"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "build_fingerprint" && entry.value.starts_with("sha256:")));
}

#[test]
pub(super) fn release_report_blocks_web_platform_report_without_required_provider() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "web-release",
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
            profile: "web-release".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-web".to_string()),
            require_platform_report: true,
            platform_report: Some(platform_capability(
                PlatformId::Web,
                "nativevn-web",
                &["webgpu", "webcodecs", "webaudio"],
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
        "ASTRA_PLATFORM_PROVIDER_UNAVAILABLE"
    );
}

#[test]
pub(super) fn release_report_includes_web_platform_provider_evidence() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "web-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    ))
    .unwrap();
    let platform_report = platform_capability(
        PlatformId::Web,
        "nativevn-web",
        &["webgpu", "webcodecs", "kira", "webaudio", "opfs"],
    );

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "web-release".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-web".to_string()),
            require_platform_report: true,
            platform_report: Some(platform_report),
        })
        .unwrap();

    let platform_check = report
        .checks
        .iter()
        .find(|check| check.id == "platform.capability_report")
        .unwrap();
    assert_eq!(platform_check.status, CheckStatus::Pass);
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "provider.decode.selected" && entry.value == "webcodecs"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "provider.audio_mixer.selected" && entry.value == "kira"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "provider.audio_output.selected" && entry.value == "webaudio"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "provider.save.selected" && entry.value == "opfs"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "profile_hash" && entry.value.starts_with("sha256:")));
}

#[test]
pub(super) fn release_gate_blocks_package_target_manifests_with_editor_descriptors() {
    let mut request = PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.target_manifest = serde_json::json!({
        "schema": "astra.target_manifest.v3",
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
            require_platform_report: false,
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
