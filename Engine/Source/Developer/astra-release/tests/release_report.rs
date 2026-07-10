use astra_core::Hash256;
use astra_package::{
    AstraContainerBuilder, ContainerKind, PackageBuildRequest, PackageBuilder, PackageManifest,
    SectionPayload, CURRENT_CONTAINER_VERSION,
};
use astra_platform::{
    ConformanceCheck, ConformanceStatus, PlatformCapabilityReport, PlatformHostConformanceReport,
    PlatformHostProfile, PlatformId, PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA,
};
use astra_player_core::{
    PlayerAutomationCheck, PlayerAutomationEvidence, PlayerAutomationReport,
    PlayerAutomationStatus, PlayerPlatform,
};
use astra_release::{CheckStatus, PackageValidateRequest, ReleaseDomain, ReleaseValidator};
use astra_vn::{compile_astra_sources, package_sections_for_story, AstraSource};

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
            require_platform_report: false,
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
    assert!(report.checks.iter().any(|check| {
        check.id == "plugin.extension_registry" && check.status == CheckStatus::Pass
    }));
    assert!(report.checks.iter().any(|check| {
        check.id == "plugin.dependency_graph" && check.status == CheckStatus::Pass
    }));
    assert!(report.checks.iter().any(|check| {
        check.id == "runtime_provider.binding" && check.status == CheckStatus::Pass
    }));
    assert!(report.checks.iter().any(|check| {
        check.id == "runtime_provider.native_vn" && check.status == CheckStatus::Blocked
    }));

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

#[test]
fn release_gate_accepts_player_full_playable_only_with_matching_live_report() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
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
    let package_hash = Hash256::from_sha256(&package_bytes).to_string();

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
            Some(player_report(
                &package_hash,
                "classic",
                "tsuinosora-internal-game",
            )),
        )
        .unwrap();
    let player_check = report
        .checks
        .iter()
        .find(|check| check.id == "player.full_playable")
        .unwrap();
    assert_eq!(player_check.domain, ReleaseDomain::Player);
    assert_eq!(player_check.status, CheckStatus::Pass);

    let blocked = ReleaseValidator
        .validate_package_with_player_report(
            package_request(package_bytes),
            Some(player_report(
                "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "classic",
                "tsuinosora-internal-game",
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
fn release_gate_requires_capability_conformance_player_identity_continuity() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
        "com.example.nativevn",
        "classic",
        Vec::new(),
    ))
    .unwrap();
    let package_bytes = blob.into_bytes();
    let package_hash = Hash256::from_sha256(&package_bytes).to_string();
    let capability = platform_capability(
        PlatformId::Windows,
        "tsuinosora-internal-game",
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
    let mut player = player_report(&package_hash, "classic", "tsuinosora-internal-game");
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

#[test]
fn release_gate_blocks_plugin_registry_conflict_and_invalid_binding() {
    let mut request = PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.plugin_extension_registry = serde_json::json!({
        "schema": "astra.plugin_extension_registry.v1",
        "providers": [{
            "slot": "presentation",
            "provider_id": "astra.provider.first",
            "capability": "presentation.headless",
            "phase": "runtime",
            "packaged": true
        }],
        "bindings": [{
            "slot": "presentation",
            "provider_id": "astra.provider.missing"
        }],
        "conflicts": [{
            "slot": "presentation",
            "selected_provider": "astra.provider.first",
            "conflicting_provider": "astra.provider.second",
            "reason": "provider slot already has an explicit binding"
        }]
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

    let plugin_check = report
        .checks
        .iter()
        .find(|check| check.id == "plugin.extension_registry")
        .unwrap();
    assert_eq!(plugin_check.status, CheckStatus::Blocked);
    assert_eq!(
        plugin_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PLUGIN_EXTENSION_CONFLICT"
    );
}

#[test]
fn runtime_provider_gate_blocks_missing_nativevn_binding() {
    let mut request = PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.provider_policy = serde_json::json!({
        "schema": "astra.provider_policy.v1",
        "profile": "desktop-release",
        "bindings": []
    })
    .to_string()
    .into_bytes();
    request.plugin_extension_registry = serde_json::json!({
        "schema": "astra.plugin_extension_registry.v1",
        "providers": [],
        "bindings": [],
        "conflicts": []
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

    let binding = report
        .checks
        .iter()
        .find(|check| check.id == "runtime_provider.binding")
        .unwrap();
    assert_eq!(binding.status, CheckStatus::Blocked);
    assert_eq!(
        binding.diagnostic.as_ref().unwrap().code,
        "ASTRA_RUNTIME_PROVIDER_BINDING_MISSING"
    );
}

#[test]
fn release_gate_blocks_unresolved_plugin_dependency() {
    let mut request = PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.plugin_dependency_graph = serde_json::json!({
        "schema": "astra.plugin_dependency_graph.v1",
        "dependencies": [{
            "plugin_id": "astra.provider.required",
            "version_req": ">=0.1.0",
            "required": true,
            "reason": "runtime provider binding",
            "resolved": false
        }]
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

    let dependency_check = report
        .checks
        .iter()
        .find(|check| check.id == "plugin.dependency_graph")
        .unwrap();
    assert_eq!(dependency_check.status, CheckStatus::Blocked);
    assert_eq!(
        dependency_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PLUGIN_DEPENDENCY_UNRESOLVED"
    );
}

#[test]
fn vfs_mount_gate_blocks_missing_vfs_manifest() {
    let manifest = PackageManifest {
        schema: "astra.package_manifest.v1".to_string(),
        package_id: "com.example.legacy".to_string(),
        profile: "desktop-release".to_string(),
        container_version: CURRENT_CONTAINER_VERSION,
    };
    let blob = AstraContainerBuilder::new(ContainerKind::Package)
        .add_section(
            SectionPayload::postcard("package.manifest", "astra.package_manifest.v1", &manifest)
                .unwrap(),
        )
        .add_section(SectionPayload::raw(
            "schema.registry",
            "astra.schema_registry.v1",
            b"{\"schema\":\"astra.schema_registry.v1\",\"schemas\":[]}".to_vec(),
        ))
        .write()
        .unwrap();

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: None,
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let vfs_check = report
        .checks
        .iter()
        .find(|check| check.id == "vfs.prefix_registry")
        .unwrap();
    assert_eq!(vfs_check.status, CheckStatus::Blocked);
    assert_eq!(
        vfs_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VFS_MANIFEST_MISSING"
    );
}

#[test]
fn vfs_mount_gate_blocks_asset_registry_compat_section() {
    let mut request = PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.extra_sections.push(SectionPayload::raw(
        "asset.registry",
        "astra.asset_registry.v1",
        b"{\"schema\":\"astra.asset_registry.v1\",\"assets\":[]}".to_vec(),
    ));
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

    let vfs_check = report
        .checks
        .iter()
        .find(|check| check.id == "vfs.uri_format")
        .unwrap();
    assert_eq!(vfs_check.status, CheckStatus::Blocked);
    assert_eq!(
        vfs_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VFS_ASSET_REGISTRY_REMOVED"
    );
}

#[test]
fn vfs_mount_gate_blocks_missing_provider_binding_for_prefix() {
    let mut request = PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.plugin_extension_registry = serde_json::json!({
        "schema": "astra.plugin_extension_registry.v1",
        "providers": [],
        "bindings": [],
        "conflicts": []
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

    let vfs_check = report
        .checks
        .iter()
        .find(|check| check.id == "vfs.prefix_registry")
        .unwrap();
    assert_eq!(vfs_check.status, CheckStatus::Blocked);
    assert_eq!(
        vfs_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VFS_PROVIDER_MISSING"
    );
}

#[test]
fn plugin_provider_gate_blocks_unpacked_vfs_prefix_provider() {
    let mut request = PackageBuildRequest::minimal(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    request.plugin_extension_registry = serde_json::json!({
        "schema": "astra.plugin_extension_registry.v1",
        "providers": [{
            "slot": "vfs_provider",
            "provider_id": "astra.vfs.package",
            "capability": "vfs.backend.package",
            "phase": "runtime",
            "packaged": false
        }],
        "bindings": [],
        "conflicts": []
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

    let vfs_check = report
        .checks
        .iter()
        .find(|check| check.id == "vfs.prefix_registry")
        .unwrap();
    assert_eq!(vfs_check.status, CheckStatus::Blocked);
    assert_eq!(
        vfs_check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VFS_PROVIDER_UNPACKAGED"
    );
}

#[test]
fn release_profile_blocks_missing_platform_report() {
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
fn dev_profile_warns_on_missing_platform_report() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
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
fn release_profile_blocks_fixture_package_without_cooked_project() {
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
fn release_profile_accepts_cooked_project_input_section() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
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
fn release_profile_blocks_package_profile_mismatch() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
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
fn release_report_blocks_windows_platform_report_without_required_provider() {
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
fn release_report_includes_windows_platform_provider_evidence() {
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
fn release_report_blocks_web_platform_report_without_required_provider() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
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
fn release_report_includes_web_platform_provider_evidence() {
    let blob = PackageBuilder::build(PackageBuildRequest::minimal(
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
        &["webgpu", "webcodecs", "webaudio", "opfs"],
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
        .any(|entry| entry.key == "provider.save.selected" && entry.value == "opfs"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "profile_hash" && entry.value.starts_with("sha256:")));
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

#[test]
fn release_gate_requires_nativevn_sections_for_classic_profile() {
    let blob = package_with_target_manifest(
        "classic",
        serde_json::json!({
            "schema": "astra.target_manifest.v1",
            "targets": [{
                "id": "nativevn-game",
                "kind": "game",
                "crate": "astra-vn",
                "default_profile": "classic",
                "platforms": ["windows", "web"],
                "packaged": true
            }]
        }),
        Vec::new(),
    );

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.compiled_story" && check.status == CheckStatus::Blocked));
}

#[test]
fn release_gate_accepts_nativevn_sections_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let blob = package_with_target_manifest(
        "classic",
        serde_json::json!({
            "schema": "astra.target_manifest.v1",
            "targets": [{
                "id": "nativevn-game",
                "kind": "game",
                "crate": "astra-vn",
                "default_profile": "classic",
                "platforms": ["windows", "web"],
                "packaged": true
            }]
        }),
        sections,
    );

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.compiled_story" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.profile_manifest" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.policy_bundle" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.extension_bindings" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.standard_commands" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.presentation_provider" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.commercial_baseline" && check.status == CheckStatus::Pass));
    assert!(report.checks.iter().any(|check| {
        check.id == "runtime_provider.native_vn"
            && check.status == CheckStatus::Pass
            && check
                .evidence
                .iter()
                .any(|evidence| evidence.key == "behavior_state_hash")
    }));
}

#[test]
fn release_gate_blocks_missing_policy_bundle_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.retain(|section| section.id != "vn.policy_bundle_manifest");
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.policy_bundle")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_POLICY_BUNDLE"
    );
}

#[test]
fn release_gate_blocks_missing_policy_bundle_source_cache_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.retain(|section| section.id != "vn.policy_bundle_source_cache");
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.policy_bundle")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_POLICY_CACHE"
    );
}

#[test]
fn release_gate_blocks_missing_standard_command_manifest_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.retain(|section| section.id != "vn.standard_command_manifest");
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.standard_commands")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_STANDARD_COMMAND_MANIFEST"
    );
}

#[test]
fn release_gate_blocks_unknown_standard_command_usage_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        format!(
            "{}\nstate extra #@id state.extra\n  scene extra #@id scene.extra\n    warp asset:native-assets/effect/warp.json #@id command.warp\n",
            nativevn_story_with_system_pages()
        ),
    )])
    .unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.standard_commands")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_STANDARD_COMMAND_UNKNOWN"
    );
}

#[test]
fn release_gate_blocks_missing_presentation_provider_manifest_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.retain(|section| section.id != "vn.presentation_provider_manifest");
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.presentation_provider")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_PRESENTATION_PROVIDER_MANIFEST"
    );
}

#[test]
fn release_gate_blocks_missing_commercial_baseline_manifest_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.retain(|section| section.id != "vn.commercial_baseline_manifest");
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.commercial_baseline")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_COMMERCIAL_BASELINE_MANIFEST"
    );
}

#[test]
fn release_gate_blocks_incomplete_commercial_baseline_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:hello speaker:narrator #@id line.hello
"#,
    )])
    .unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.commercial_baseline")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_COMMERCIAL_BASELINE_FEATURE"
    );
}

#[test]
fn release_gate_blocks_missing_vn_extension_bindings_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.retain(|section| section.id != "vn.extension_manifest");
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.extension_bindings")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_EXTENSION_MANIFEST"
    );
}

#[test]
fn release_gate_blocks_compiled_story_without_command_manifest() {
    let mut compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    compiled.command_manifest.commands.clear();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.compiled_story")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_COMPILED_STORY_MANIFEST"
    );
}

#[test]
fn release_gate_blocks_missing_system_story_manifest_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:hello speaker:narrator #@id line.hello
"#,
    )])
    .unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.system_ui_profile")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_SYSTEM_ENTRY_MISSING"
    );
}

#[test]
fn release_gate_blocks_system_story_entries_without_policy() {
    let source = nativevn_story_with_system_pages().replace(" policy:astra.policy.standard", "");
    let compiled = compile_astra_sources([AstraSource::new("main.astra", source)]).unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.system_ui_profile")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_SYSTEM_POLICY_MISSING"
    );
}

#[test]
fn release_gate_blocks_missing_system_ui_profile_manifest() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    sections.retain(|section| section.id != "vn.system_ui_profile_manifest");
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.system_ui_profile")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_SYSTEM_UI_PROFILE_MANIFEST"
    );
}

#[test]
fn release_gate_accepts_system_story_manifest_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let sections =
        package_sections_for_story(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let blob = package_with_target_manifest("classic", nativevn_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "vn.system_ui_profile")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Pass);
    assert!(check
        .evidence
        .iter()
        .any(|entry| entry.key == "page_count" && entry.value == "10"));
}

#[test]
fn release_gate_blocks_missing_tsuinosora_sections_for_tsuinosora_target() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.reference_evidence")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_SECTION_MISSING"
    );
}

#[test]
fn release_gate_accepts_tsuinosora_sections_for_classic_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    for id in [
        "tsuinosora.reference_evidence",
        "tsuinosora.asset_analysis",
        "tsuinosora.conversion_manifest",
        "tsuinosora.mount_policy",
    ] {
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.id == id && check.status == CheckStatus::Pass),
            "missing pass check {id}"
        );
    }
}

#[test]
fn release_gate_blocks_tsuinosora_reference_hash_mismatch() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    let mut tsui_sections = tsuinosora_sections("tsuinosora-internal-game", false, false);
    let reference = tsui_sections
        .iter_mut()
        .find(|section| section.id == "tsuinosora.reference_evidence")
        .unwrap();
    reference.payload = serde_json::json!({
        "schema": "tsuinosora.visual_reference_report.v1",
        "status": "pass",
        "references": [
            {
                "logical_id": "title",
                "file_name": "Title.png",
                "dimensions": {"width": 1386, "height": 1040},
                "hash": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                "allowed_regions": ["title_background", "title_menu_buttons"]
            },
            {
                "logical_id": "game",
                "file_name": "Game.png",
                "dimensions": {"width": 1403, "height": 1053},
                "hash": "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84",
                "allowed_regions": ["background_viewport", "text_window"]
            }
        ],
        "diagnostics": [],
        "prohibited_outputs": ["new_commercial_screenshot", "commercial_audio"]
    })
    .to_string()
    .into_bytes();
    sections.extend(tsui_sections);
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.reference_evidence")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_REFERENCE_HASH_MISMATCH"
    );
}

#[test]
fn release_gate_blocks_tsuinosora_asset_quarantine() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections("tsuinosora-internal-game", true, false));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.asset_analysis")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_ASSET_QUARANTINE"
    );
}

#[test]
fn release_gate_blocks_empty_tsuinosora_asset_analysis() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.retain(|section| section.id != "tsuinosora.asset_analysis");
    sections.push(json_section(
        "tsuinosora.asset_analysis",
        "tsuinosora.asset_analysis.v1",
        serde_json::json!({
            "schema": "tsuinosora.asset_analysis.v1",
            "status": "pass",
            "reference_hashes": [
                "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca",
                "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84"
            ],
            "assets": [],
            "quarantine": [],
            "diagnostics": [],
            "redaction": {"paths": "alias_or_report_relative_only", "payload": "omitted"}
        }),
    ));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.asset_analysis")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_ASSET_ANALYSIS_EMPTY"
    );
}

#[test]
fn release_gate_requires_tsuinosora_modern_profile_report() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string(), "modern".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    let blob = package_with_target_manifest("modern", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "modern".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    let missing = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.modern_profile_report")
        .unwrap();
    assert_eq!(missing.status, CheckStatus::Blocked);

    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string(), "modern".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections("tsuinosora-internal-game", false, true));
    let blob = package_with_target_manifest("modern", tsuinosora_target_manifest(), sections);
    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "modern".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    assert!(report.checks.iter().any(|check| {
        check.id == "tsuinosora.modern_profile_report" && check.status == CheckStatus::Pass
    }));
}

#[test]
fn release_gate_blocks_tsuinosora_conversion_route_gap() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.retain(|section| section.id != "tsuinosora.conversion_manifest");
    sections.push(json_section(
        "tsuinosora.conversion_manifest",
        "tsuinosora.conversion_report.v1",
        serde_json::json!({
            "schema": "tsuinosora.conversion_report.v1",
            "status": "pass",
            "counts": {"source_files": 2, "asset_count": 1, "quarantine_count": 0, "route_count": 1},
            "routes": [{"route_id": "classic.main", "coverage": "missing"}],
            "diagnostics": [],
            "redaction": {"paths": "alias_only", "payload": "omitted"}
        }),
    ));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.conversion_manifest")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_ROUTE_COVERAGE"
    );
}

#[test]
fn release_gate_blocks_tsuinosora_conversion_without_resource_evidence() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.retain(|section| section.id != "tsuinosora.conversion_manifest");
    sections.push(json_section(
        "tsuinosora.conversion_manifest",
        "tsuinosora.conversion_report.v1",
        serde_json::json!({
            "schema": "tsuinosora.conversion_report.v1",
            "status": "pass",
            "inputs": {"original_install_root": "original_install_root"},
            "counts": {
                "source_files": 2,
                "asset_count": 0,
                "quarantine_count": 0,
                "route_count": 1
            },
            "resources": [],
            "routes": [{
                "route_id": "classic.main",
                "coverage": "covered",
                "terminal": "ending.good"
            }],
            "diagnostics": [],
            "redaction": {"paths": "alias_only", "payload": "omitted"}
        }),
    ));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.conversion_manifest")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_CONVERSION_RESOURCE_EVIDENCE"
    );
    assert!(check
        .evidence
        .iter()
        .any(|evidence| evidence.key == "resource_count" && evidence.value == "0"));
}

#[test]
fn release_gate_blocks_tsuinosora_conversion_resource_without_hash_evidence() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.retain(|section| section.id != "tsuinosora.conversion_manifest");
    sections.push(json_section(
        "tsuinosora.conversion_manifest",
        "tsuinosora.conversion_report.v1",
        serde_json::json!({
            "schema": "tsuinosora.conversion_report.v1",
            "status": "pass",
            "inputs": {"original_install_root": "original_install_root"},
            "counts": {
                "source_files": 2,
                "asset_count": 1,
                "quarantine_count": 0,
                "route_count": 1
            },
            "resources": [{
                "source": "containers/ready/0001_png.png",
                "native_path": "native-assets/backgrounds/0001_png.png",
                "classification": "background",
                "byte_size": 68
            }],
            "routes": [{
                "route_id": "classic.main",
                "coverage": "covered",
                "terminal": "ending.good"
            }],
            "diagnostics": [],
            "redaction": {"paths": "alias_only", "payload": "omitted"}
        }),
    ));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.conversion_manifest")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_CONVERSION_RESOURCE_EVIDENCE"
    );
    assert!(check
        .evidence
        .iter()
        .any(|evidence| evidence.key == "field" && evidence.value == "resources.0.source_hash"));
}

#[test]
fn release_gate_blocks_tsuinosora_report_path_leak() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.retain(|section| section.id != "tsuinosora.mount_policy");
    let leaked_alias = ["C", ":/", "local-source"].concat();
    sections.push(json_section(
        "tsuinosora.mount_policy",
        "tsuinosora.mount_policy.v1",
        serde_json::json!({
            "schema": "tsuinosora.mount_policy.v1",
            "target": "tsuinosora-internal-game",
            "status": "pass",
            "aliases": [{
                "alias": "original",
                "value": leaked_alias,
                "hash_policy": "manifest_required",
                "fallback": "blocking"
            }],
            "diagnostics": []
        }),
    ));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.mount_policy")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_REPORT_PATH_LEAK"
    );
}

#[test]
fn release_gate_blocks_tsuinosora_report_payload_field_leak() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["classic".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.retain(|section| section.id != "tsuinosora.conversion_manifest");
    sections.push(json_section(
        "tsuinosora.conversion_manifest",
        "tsuinosora.conversion_report.v1",
        serde_json::json!({
            "schema": "tsuinosora.conversion_report.v1",
            "status": "pass",
            "counts": {
                "source_files": 2,
                "asset_count": 1,
                "quarantine_count": 0,
                "route_count": 1
            },
            "routes": [{
                "route_id": "classic.main",
                "coverage": "covered",
                "terminal": "ending.good",
                "script_text": "commercial text must never enter release reports"
            }],
            "diagnostics": [],
            "redaction": {"paths": "alias_only", "payload": "omitted"}
        }),
    ));
    let blob = package_with_target_manifest("classic", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "classic".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();

    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.conversion_manifest")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_REPORT_PAYLOAD_LEAK"
    );
    assert!(check
        .evidence
        .iter()
        .any(|evidence| evidence.key == "field" && evidence.value == "routes.0.script_text"));
}

#[test]
fn release_gate_requires_tsuinosora_manual_signoff_for_release_profile() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["desktop-release".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    let blob =
        package_with_target_manifest("desktop-release", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    let missing = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.manual_signoff")
        .unwrap();
    assert_eq!(missing.status, CheckStatus::Blocked);
    assert_eq!(
        missing.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_SECTION_MISSING"
    );

    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["desktop-release".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.push(tsuinosora_manual_signoff_section());
    let blob =
        package_with_target_manifest("desktop-release", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    assert!(report.checks.iter().any(|check| {
        check.id == "tsuinosora.manual_signoff" && check.status == CheckStatus::Pass
    }));
}

#[test]
fn release_gate_blocks_incomplete_tsuinosora_manual_signoff_check_set() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["desktop-release".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.push(json_section(
        "tsuinosora.manual_signoff",
        "tsuinosora.manual_signoff.v1",
        serde_json::json!({
            "schema": "tsuinosora.manual_signoff.v1",
            "status": "pass",
            "checks": [
                {"check_id": "manual.full_playthrough", "result": "pass"}
            ],
            "blockers": [],
            "redaction": {"paths": "alias_or_hash_only", "payload": "omitted"}
        }),
    ));
    let blob =
        package_with_target_manifest("desktop-release", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.manual_signoff")
        .unwrap();

    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_TSUI_MANUAL_SIGNOFF"
    );
    assert!(check
        .evidence
        .iter()
        .any(|evidence| evidence.key == "missing_required_count" && evidence.value == "3"));
}

#[test]
fn release_gate_requires_tsuinosora_manual_signoff_check_id_field() {
    let compiled = compile_astra_sources([AstraSource::new(
        "main.astra",
        nativevn_story_with_system_pages(),
    )])
    .unwrap();
    let mut sections = package_sections_for_story(
        &compiled,
        &["desktop-release".to_string()],
        "tsuinosora-internal-game",
    )
    .unwrap();
    sections.extend(tsuinosora_sections(
        "tsuinosora-internal-game",
        false,
        false,
    ));
    sections.push(json_section(
        "tsuinosora.manual_signoff",
        "tsuinosora.manual_signoff.v1",
        serde_json::json!({
            "schema": "tsuinosora.manual_signoff.v1",
            "status": "pass",
            "checks": [
                {"id": "manual.full_playthrough", "result": "pass"},
                {"id": "manual.audio_listening", "result": "pass"},
                {"id": "manual.visual_review", "result": "pass"},
                {"id": "manual.alias_replacement", "result": "pass"}
            ],
            "blockers": [],
            "redaction": {"paths": "alias_or_hash_only", "payload": "omitted"}
        }),
    ));
    let blob =
        package_with_target_manifest("desktop-release", tsuinosora_target_manifest(), sections);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "desktop-release".to_string(),
            require_ffmpeg: false,
            target: Some("tsuinosora-internal-game".to_string()),
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tsuinosora.manual_signoff")
        .unwrap();

    assert_eq!(check.status, CheckStatus::Blocked);
    assert!(check
        .evidence
        .iter()
        .any(|evidence| evidence.key == "missing_required_count" && evidence.value == "4"));
}

fn package_with_target_manifest(
    profile: &str,
    target_manifest: serde_json::Value,
    sections: Vec<SectionPayload>,
) -> astra_package::ContainerBlob {
    let mut request = PackageBuildRequest::minimal("com.example.nativevn", profile, sections);
    request.target_manifest = target_manifest.to_string().into_bytes();
    PackageBuilder::build(request).unwrap()
}

fn nativevn_target_manifest() -> serde_json::Value {
    serde_json::json!({
        "schema": "astra.target_manifest.v1",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "default_profile": "classic",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    })
}

fn tsuinosora_target_manifest() -> serde_json::Value {
    serde_json::json!({
        "schema": "astra.target_manifest.v1",
        "targets": [{
            "id": "tsuinosora-internal-game",
            "kind": "game",
            "crate": "astra-vn",
            "default_profile": "classic",
            "platforms": ["headless", "windows", "web"],
            "packaged": true
        }]
    })
}

fn tsuinosora_sections(
    target: &str,
    quarantine: bool,
    include_modern: bool,
) -> Vec<SectionPayload> {
    let mut sections = vec![
        json_section(
            "tsuinosora.reference_evidence",
            "tsuinosora.visual_reference_report.v1",
            serde_json::json!({
                "schema": "tsuinosora.visual_reference_report.v1",
                "status": "pass",
                "references": [
                    {
                        "logical_id": "title",
                        "dimensions": {"width": 1386, "height": 1040},
                        "hash": "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca",
                        "allowed_regions": ["title_background", "title_menu_buttons"]
                    },
                    {
                        "logical_id": "game",
                        "dimensions": {"width": 1403, "height": 1053},
                        "hash": "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84",
                        "allowed_regions": ["background_viewport", "text_window"]
                    }
                ],
                "prohibited_outputs": ["new_commercial_screenshot", "commercial_audio"]
            }),
        ),
        json_section(
            "tsuinosora.asset_analysis",
            "tsuinosora.asset_analysis.v1",
            serde_json::json!({
                "schema": "tsuinosora.asset_analysis.v1",
                "status": if quarantine { "blocked" } else { "pass" },
                "reference_hashes": [
                    "sha256:3799183a831bdbdc144e1bc9e06dffd831417d436338a1daf04b45bc35624bca",
                    "sha256:1c4ddf68fa15fd6a76db259b155366456198bd551c49de8a9ede9ca0f2be9d84"
                ],
                "assets": [{
                    "relative_path": "native-assets/bg/opening.png",
                    "classification": "background",
                    "confidence": 0.91,
                    "sha256": "sha256:bg"
                }],
                "quarantine": if quarantine {
                    serde_json::json!([{"relative_path": "native-assets/chara/unknown.png"}])
                } else {
                    serde_json::json!([])
                },
                "diagnostics": []
            }),
        ),
        json_section(
            "tsuinosora.conversion_manifest",
            "tsuinosora.conversion_report.v1",
            serde_json::json!({
                "schema": "tsuinosora.conversion_report.v1",
                "status": "pass",
                "inputs": {"original_install_root": "original_install_root"},
                "counts": {
                    "source_files": 2,
                    "asset_count": 1,
                    "quarantine_count": 0,
                    "route_count": 1
                },
                "resources": [{
                    "source": "containers/ready/0001_png.png",
                    "native_path": "native-assets/backgrounds/0001_png.png",
                    "classification": "background",
                    "source_hash": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
                    "converted_hash": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
                    "byte_size": 68
                }],
                "routes": [{
                    "route_id": "classic.main",
                    "coverage": "covered",
                    "terminal": "ending.good"
                }],
                "diagnostics": [],
                "redaction": {"paths": "alias_only", "payload": "omitted"}
            }),
        ),
        json_section(
            "tsuinosora.mount_policy",
            "tsuinosora.mount_policy.v1",
            serde_json::json!({
                "schema": "tsuinosora.mount_policy.v1",
                "target": target,
                "status": "pass",
                "aliases": [{
                    "alias": "original",
                    "value": "original_install_root",
                    "hash_policy": "manifest_required",
                    "fallback": "blocking"
                }],
                "diagnostics": []
            }),
        ),
    ];
    if include_modern {
        sections.push(json_section(
            "tsuinosora.modern_profile_report",
            "tsuinosora.modern_profile_report.v1",
            serde_json::json!({
                "schema": "tsuinosora.modern_profile_report.v1",
                "status": "pass",
                "base_conversion_status": "pass",
                "counts": {"feature_count": 1, "route_count": 1},
                "features": [{
                    "feature_id": "remake_overlay.hero",
                    "feature_kind": "portrait_overlay",
                    "input_hash": "sha256:input",
                    "output_hash": "sha256:output",
                    "fallback_hash": "sha256:fallback",
                    "independent_switch": true,
                    "affects_core_state": false
                }],
                "diagnostics": [],
                "redaction": {"paths": "alias_or_hash_only", "payload": "omitted"}
            }),
        ));
    }
    sections
}

fn json_section(id: &str, schema: &str, value: serde_json::Value) -> SectionPayload {
    SectionPayload::raw(id, schema, value.to_string().into_bytes())
}

fn tsuinosora_manual_signoff_section() -> SectionPayload {
    json_section(
        "tsuinosora.manual_signoff",
        "tsuinosora.manual_signoff.v1",
        serde_json::json!({
            "schema": "tsuinosora.manual_signoff.v1",
            "status": "pass",
            "checks": [
                {"check_id": "manual.full_playthrough", "result": "pass"},
                {"check_id": "manual.audio_listening", "result": "pass"},
                {"check_id": "manual.visual_review", "result": "pass_with_diagnostics"},
                {"check_id": "manual.alias_replacement", "result": "pass"}
            ],
            "blockers": [],
            "redaction": {"paths": "alias_or_hash_only", "payload": "omitted"}
        }),
    )
}

fn nativevn_story_with_system_pages() -> &'static str {
    r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    movie layer:video.opening asset:native-assets/movie/op.webm end:wait fallback:native-assets/movie/op_fallback.png #@id movie.opening
    voice asset:native-assets/voice/hero0001.ogg sync:text #@id voice.opening
    text key:hello speaker:narrator voice:voice.hero.0001 #@id line.hello
    choice key:where #@id choice.where
      option key:choice.library -> library #@id choice.library
      option key:choice.rooftop -> rooftop #@id choice.rooftop
state library #@id state.library
  scene library #@id scene.library
    bgm asset:native-assets/bgm/library.ogg loop:true #@id bgm.library
    se asset:native-assets/se/page.ogg #@id se.page
    wait fence:voice.opening.end #@id wait.voice
    jump ending.good #@id jump.good
state rooftop #@id state.rooftop
  scene rooftop #@id scene.rooftop
    text key:rooftop speaker:narrator #@id line.rooftop
    jump ending.rooftop #@id jump.rooftop

story system #@id story.system
state title #@id state.system.title
  scene title #@id scene.system.title
    system_page kind:title policy:astra.policy.standard #@id page.title
state save #@id state.system.save
  scene save #@id scene.system.save
    system_page kind:save policy:astra.policy.standard #@id page.save
state load #@id state.system.load
  scene load #@id scene.system.load
    system_page kind:load policy:astra.policy.standard #@id page.load
state config #@id state.system.config
  scene config #@id scene.system.config
    system_page kind:config policy:astra.policy.standard #@id page.config
state gallery #@id state.system.gallery
  scene gallery #@id scene.system.gallery
    system_page kind:gallery policy:astra.policy.standard #@id page.gallery
state replay #@id state.system.replay
  scene replay #@id scene.system.replay
    system_page kind:replay policy:astra.policy.standard #@id page.replay
state voice_replay #@id state.system.voice_replay
  scene voice_replay #@id scene.system.voice_replay
    system_page kind:voice_replay policy:astra.policy.standard #@id page.voice_replay
state route_chart #@id state.system.route_chart
  scene route_chart #@id scene.system.route_chart
    system_page kind:route_chart policy:astra.policy.standard #@id page.route_chart
state backlog #@id state.system.backlog
  scene backlog #@id scene.system.backlog
    system_page kind:backlog policy:astra.policy.standard #@id page.backlog
state localization_preview #@id state.system.localization_preview
  scene localization_preview #@id scene.system.localization_preview
    system_page kind:localization_preview policy:astra.policy.standard #@id page.localization_preview
"#
}

fn platform_capability(
    platform: PlatformId,
    target: &str,
    available: &[&str],
) -> PlatformCapabilityReport {
    let profile = match platform {
        PlatformId::Windows => PlatformHostProfile::windows_release(target, "com.example.nativevn"),
        PlatformId::Web => PlatformHostProfile::web_release(target, "com.example.nativevn"),
        _ => unreachable!("release fixture only covers migrated platforms"),
    };
    PlatformCapabilityReport::from_profile(
        &profile,
        Hash256::from_sha256(b"release-test-build").to_string(),
        available.iter().copied(),
    )
    .unwrap()
}

fn cooked_project_section(profile: &str, target: &str) -> SectionPayload {
    SectionPayload::raw(
        "compiled.project",
        "astra.cooked_project.v1",
        serde_json::json!({
            "schema": "astra.cooked_project.v1",
            "package_id": "com.example.nativevn",
            "profile": profile,
            "target": target,
            "project_hash": "sha256:synthetic-cook-fixture"
        })
        .to_string()
        .into_bytes(),
    )
}

fn package_request(package_bytes: Vec<u8>) -> PackageValidateRequest {
    PackageValidateRequest {
        package_bytes,
        profile: "classic".to_string(),
        require_ffmpeg: false,
        target: Some("tsuinosora-internal-game".to_string()),
        require_platform_report: false,
        platform_report: None,
    }
}

fn player_report(package_hash: &str, profile: &str, target: &str) -> PlayerAutomationReport {
    PlayerAutomationReport {
        schema: "astra.player_automation_report.v1".to_string(),
        status: PlayerAutomationStatus::Pass,
        target: target.to_string(),
        profile: profile.to_string(),
        platform: PlayerPlatform::Windows,
        package_hash: package_hash.to_string(),
        transcript_hash: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_string(),
        route_coverage: vec!["opening".to_string()],
        checks: vec![PlayerAutomationCheck {
            id: "player.full_playable".to_string(),
            status: PlayerAutomationStatus::Pass,
            summary: "live input, visual, audio and route coverage passed".to_string(),
            diagnostic: None,
            evidence: vec![PlayerAutomationEvidence {
                key: "route_count".to_string(),
                value: "1".to_string(),
            }],
        }],
    }
}
