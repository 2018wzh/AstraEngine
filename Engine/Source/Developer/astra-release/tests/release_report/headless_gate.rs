use super::*;

#[test]
pub(super) fn shipping_isolation_rejects_current_and_retired_test_payloads() {
    for (schema, expected) in [
        ("astra.headless_checkpoint_config.v2", CheckStatus::Blocked),
        ("astra.headless_checkpoint_config.v3", CheckStatus::Blocked),
        ("astra.headless_review.v2", CheckStatus::Blocked),
        ("example.config.v1", CheckStatus::Pass),
    ] {
        let blob = package_with_target_manifest(
            "classic",
            nativevn_target_manifest(),
            vec![SectionPayload::raw(
                "custom.config",
                "example.config.v1",
                serde_json::json!({"schema": schema})
                    .to_string()
                    .into_bytes(),
            )],
        );
        let report = ReleaseValidator
            .validate_package(PackageValidateRequest {
                package_bytes: blob.into_bytes(),
                profile: "classic".into(),
                require_ffmpeg: false,
                target: Some("nativevn-game".into()),
                require_platform_report: false,
                platform_report: None,
            })
            .unwrap();
        let check = report
            .checks
            .iter()
            .find(|check| check.id == "shipping.headless_isolation")
            .unwrap();
        assert_eq!(check.status, expected, "{schema}");
    }
}

#[test]
pub(super) fn release_gate_blocks_headless_profile_schema_in_package() {
    let blob = package_with_target_manifest(
        "classic",
        nativevn_target_manifest(),
        vec![SectionPayload::raw(
            "headless.profile",
            "astra.headless_host_profile.v3",
            br#"{"schema":"astra.headless_host_profile.v3"}"#.to_vec(),
        )],
    );

    assert_headless_release_boundary_blocked(
        blob.into_bytes(),
        "classic",
        "nativevn-game",
        "ASTRA_HEADLESS_RELEASE_SCHEMA",
    );
}

#[test]
pub(super) fn release_gate_blocks_headless_launch_profile_in_cooked_platform_profiles() {
    let blob = package_with_target_manifest(
        "classic",
        nativevn_target_manifest(),
        vec![SectionPayload::raw(
            "platform.profiles",
            "astra.platform_profiles.v3",
            serde_json::json!({
                "schema": "astra.platform_profiles.v3",
                "profiles": [{
                    "kind": "headless",
                    "profile": {"schema": "astra.headless_host_profile.v3"}
                }]
            })
            .to_string()
            .into_bytes(),
        )],
    );

    assert_headless_release_boundary_blocked(
        blob.into_bytes(),
        "classic",
        "nativevn-game",
        "ASTRA_HEADLESS_RELEASE_PROFILE",
    );
}

#[test]
pub(super) fn shipping_release_target_cannot_declare_headless_platform() {
    let target = serde_json::json!({
        "schema": "astra.target_manifest.v3",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "default_profile": "desktop-release",
            "platforms": ["headless", "windows"],
            "packaged": true
        }]
    });
    let blob = package_with_target_manifest(
        "desktop-release",
        target,
        vec![cooked_project_section("desktop-release", "nativevn-game")],
    );

    assert_headless_release_boundary_blocked(
        blob.into_bytes(),
        "desktop-release",
        "nativevn-game",
        "ASTRA_HEADLESS_RELEASE_TARGET",
    );
}

#[test]
pub(super) fn release_gate_blocks_nativevn_minimal_engine_test_profile() {
    let blob = package_with_target_manifest(
        "minimal",
        nativevn_target_manifest(),
        vec![cooked_project_section("minimal", "nativevn-game")],
    );
    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "minimal".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "profile.engine_test_isolation")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_ENGINE_TEST_PROFILE_RELEASE"
    );
}

pub(super) fn assert_headless_release_boundary_blocked(
    package_bytes: Vec<u8>,
    profile: &str,
    target: &str,
    diagnostic_code: &str,
) {
    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes,
            profile: profile.to_string(),
            require_ffmpeg: false,
            target: Some(target.to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "platform.headless_release_boundary")
        .expect("Headless release boundary check must be present");
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check
            .diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.code.as_str()),
        Some(diagnostic_code)
    );
}

#[test]
pub(super) fn release_report_covers_pass_warning_and_blocked_checks() {
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
    assert_eq!(report.schema, "astra.release_report.v1");
    assert!(report.checks.iter().any(|check| {
        check.domain == ReleaseDomain::Package && check.status == CheckStatus::Pass
    }));
    let cook_graph = report
        .checks
        .iter()
        .find(|check| check.id == "package.cook_graph")
        .unwrap();
    assert_eq!(cook_graph.status, CheckStatus::Pass);
    assert!(cook_graph
        .evidence
        .iter()
        .any(|entry| entry.key == "artifact_count" && entry.value == "1"));
    assert!(report.checks.iter().any(|check| {
        check.domain == ReleaseDomain::Media && check.status == CheckStatus::Warning
    }));
    assert!(report.checks.iter().any(|check| {
        check.domain == ReleaseDomain::Target && check.status == CheckStatus::Pass
    }));
    assert!(report.checks.iter().any(|check| {
        check.id == "plugin.extension_registry" && check.status == CheckStatus::Pass
    }));
    assert!(report.checks.iter().any(|check| {
        check.id == "plugin.dependency_graph" && check.status == CheckStatus::Pass
    }));
    assert!(report
        .checks
        .iter()
        .any(|check| { check.id == "package.binding" && check.status == CheckStatus::Pass }));
    assert!(report
        .checks
        .iter()
        .any(|check| { check.id == "vn.runtime" && check.status == CheckStatus::Blocked }));

    let blocked = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: b"not a package".to_vec(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: true,
            target: None,
            require_platform_report: true,
            platform_report: None,
        })
        .unwrap();
    assert_eq!(blocked.status, CheckStatus::Blocked);
    assert!(blocked
        .checks
        .iter()
        .any(|check| check.id == "package.integrity" && check.status == CheckStatus::Blocked));
}

pub(super) fn required_ffmpeg_check() -> astra_release::ReleaseCheckRecord {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.ffmpeg-gate",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.media.fixture",
            "astra.cooked_asset.v1",
            b"fixture".to_vec(),
        )],
    ))
    .unwrap();
    ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: true,
            target: Some("native-smoke-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap()
        .checks
        .into_iter()
        .find(|check| check.id == "media.decode.ffmpeg")
        .unwrap()
}

#[cfg(not(feature = "ffmpeg-vcpkg"))]
#[test]
pub(super) fn required_ffmpeg_gate_blocks_when_feature_is_absent() {
    let check = required_ffmpeg_check();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.unwrap().code,
        "ASTRA_FFMPEG_FEATURE_DISABLED"
    );
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[test]
pub(super) fn required_ffmpeg_gate_passes_only_after_native_probe() {
    let check = required_ffmpeg_check();
    assert_eq!(check.status, CheckStatus::Pass);
    assert!(check
        .evidence
        .iter()
        .any(|item| item.key == "provider_id" && item.value == "astra.decode.ffmpeg"));
}

#[test]
pub(super) fn release_gate_accepts_player_full_playable_only_with_matching_live_report() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "classic",
        vec![SectionPayload::raw(
            "asset.background.opening",
            "astra.cooked_asset.v1",
            b"opening".to_vec(),
        )],
    ))
    .unwrap();
    let package_bytes = blob.into_bytes();
    let package_hash = PackageReader::open(&package_bytes)
        .unwrap()
        .package_hash()
        .to_string();

    let base = ReleaseValidator
        .validate_package(package_request(package_bytes.clone()))
        .unwrap();
    assert!(!base
        .checks
        .iter()
        .any(|check| check.id == "player.full_playable"));

    let report = ReleaseValidator
        .validate_package_with_player_report(
            package_request(package_bytes.clone()),
            Some(player_report(&package_hash, "classic", "test-game")),
        )
        .unwrap();
    let player_check = report
        .checks
        .iter()
        .find(|check| check.id == "player.full_playable")
        .unwrap();
    assert_eq!(player_check.domain, ReleaseDomain::Player);
    assert_eq!(
        player_check.status,
        CheckStatus::Pass,
        "unexpected player check: {player_check:?}"
    );

    let blocked = ReleaseValidator
        .validate_package_with_player_report(
            package_request(package_bytes),
            Some(player_report(
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "classic",
                "test-game",
            )),
        )
        .unwrap();
    let blocked_check = blocked
        .checks
        .iter()
        .find(|check| check.id == "player.full_playable")
        .unwrap();
    assert_eq!(blocked_check.status, CheckStatus::Blocked);
}

#[test]
pub(super) fn release_gate_requires_capability_conformance_player_identity_continuity() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.nativevn",
        "classic",
        Vec::new(),
    ))
    .unwrap();
    let package_bytes = blob.into_bytes();
    let package_hash = PackageReader::open(&package_bytes)
        .unwrap()
        .package_hash()
        .to_string();
    let capability = platform_capability(
        PlatformId::Windows,
        "test-game",
        &["wgpu_hardware", "wmf", "wasapi", "saved_games"],
    );
    let session_id = "session.windows.release";
    let checks = astra_platform::required_conformance_checks(PlatformId::Windows)
        .iter()
        .map(|id| ConformanceCheck::pass(*id, [("hash", &package_hash)]))
        .collect();
    let conformance = PlatformHostConformanceReport {
        schema: PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA.to_string(),
        status: ConformanceStatus::Pass,
        platform: PlatformId::Windows,
        target: capability.target.clone(),
        profile_hash: capability.profile_hash.clone(),
        package_hash: package_hash.clone(),
        build_fingerprint: capability.build_fingerprint.clone(),
        session_id: session_id.to_string(),
        checks,
        diagnostics: Vec::new(),
    };
    let mut player = player_report(&package_hash, "classic", "test-game");
    let full = player
        .checks
        .iter_mut()
        .find(|check| check.id == "player.full_playable")
        .unwrap();
    full.evidence.extend([
        PlayerAutomationEvidence {
            key: "profile_hash".to_string(),
            value: capability.profile_hash.clone(),
        },
        PlayerAutomationEvidence {
            key: "build_fingerprint".to_string(),
            value: capability.build_fingerprint.clone(),
        },
        PlayerAutomationEvidence {
            key: "session_id".to_string(),
            value: session_id.to_string(),
        },
    ]);
    let mut request = package_request(package_bytes);
    request.platform_report = Some(capability);
    request.require_platform_report = true;
    let report = ReleaseValidator
        .validate_package_with_platform_evidence(request, Some(conformance), Some(player))
        .unwrap();
    assert!(report.checks.iter().any(|check| {
        check.id == "platform.evidence_continuity" && check.status == CheckStatus::Pass
    }));
}
