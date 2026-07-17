use astra_core::{
    Hash256, PerformanceBudget, PerformanceMetricBudget, PerformanceRecorder,
    PerformanceRunIdentity, PerformanceThresholds, PerformanceUnit, PERFORMANCE_BUDGET_SCHEMA,
};
use astra_emu_manager_core::EmuReleaseManifestV1;
use astra_package::{
    AstraContainerBuilder, ContainerKind, PackageBuildRequest, PackageBuilder, PackageManifest,
    PackageReader, SectionPayload, CURRENT_CONTAINER_VERSION,
};
use astra_platform::{
    ConformanceCheck, ConformanceStatus, PlatformCapabilityReport, PlatformHostConformanceReport,
    PlatformHostProfile, PlatformId, PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA,
};
use astra_player_core::{
    PlayerAutomationCheck, PlayerAutomationEvidence, PlayerAutomationReport,
    PlayerAutomationStatus, PlayerPlatform, PlayerPresentationReport,
    PLAYER_PRESENTATION_REPORT_SCHEMA,
};
use astra_release::{
    CheckStatus, PackageValidateRequest, ProductPerformanceEvidence, ReleaseDomain,
    ReleaseValidator,
};
use astra_vn::{
    compile_astra_project, package_sections_for_project, AstraSource, PLAYER_LOCALE_CONFIG_SCHEMA,
    VN_LOCALIZATION_TABLE_SCHEMA,
};

#[astra_headless_test::test]
fn release_gate_blocks_headless_profile_schema_in_package() {
    let blob = package_with_target_manifest(
        "classic",
        nativevn_target_manifest(),
        vec![SectionPayload::raw(
            "headless.profile",
            "astra.headless_host_profile.v2",
            br#"{"schema":"astra.headless_host_profile.v2"}"#.to_vec(),
        )],
    );

    assert_headless_release_boundary_blocked(
        blob.into_bytes(),
        "classic",
        "nativevn-game",
        "ASTRA_HEADLESS_RELEASE_SCHEMA",
    );
}

#[astra_headless_test::test]
fn astra_emu_release_manifest_cannot_omit_bound_evidence_sections() {
    let manifest = EmuReleaseManifestV1 {
        schema: "astra.emu.release_manifest.v1".into(),
        runtime_provider_id: "astra.runtime.astraemu".into(),
        family_id: "fvp".into(),
        family_provider_id: "astra.emu.family.fvp".into(),
        ui_provider_id: "astra.emu.ui.slint".into(),
        required_platforms: vec!["windows".into(), "android-x86_64".into()],
        evidence_sections: Default::default(),
    };
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.astraemu",
        "fvp-v1",
        vec![SectionPayload::postcard(
            "emu.release_manifest",
            "astra.emu.release_manifest.v1",
            &manifest,
        )
        .unwrap()],
    ))
    .unwrap();
    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "fvp-v1".into(),
            require_ffmpeg: false,
            target: None,
            require_platform_report: false,
            platform_report: None,
        })
        .unwrap();
    let manifest_check = report
        .checks
        .iter()
        .find(|check| check.id == "emu.release_manifest")
        .expect("AstraEMU manifest check must be emitted");
    assert_eq!(manifest_check.status, CheckStatus::Blocked);
    assert_eq!(manifest_check.domain, ReleaseDomain::Emu);
}

#[astra_headless_test::test]
fn release_gate_blocks_headless_launch_profile_in_cooked_platform_profiles() {
    let blob = package_with_target_manifest(
        "classic",
        nativevn_target_manifest(),
        vec![SectionPayload::raw(
            "platform.profiles",
            "astra.platform_profiles.v2",
            serde_json::json!({
                "schema": "astra.platform_profiles.v2",
                "profiles": [{
                    "kind": "headless",
                    "profile": {"schema": "astra.headless_host_profile.v2"}
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

#[astra_headless_test::test]
fn shipping_release_target_cannot_declare_headless_platform() {
    let target = serde_json::json!({
        "schema": "astra.target_manifest.v2",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "runtime_provider": "native_vn",
            "ui_provider": "astra.ui.yakui",
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

#[astra_headless_test::test]
fn release_gate_blocks_nativevn_minimal_engine_test_profile() {
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

fn assert_headless_release_boundary_blocked(
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

#[astra_headless_test::test]
fn release_report_covers_pass_warning_and_blocked_checks() {
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

fn required_ffmpeg_check() -> astra_release::ReleaseCheckRecord {
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
#[astra_headless_test::test]
fn required_ffmpeg_gate_blocks_when_feature_is_absent() {
    let check = required_ffmpeg_check();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.unwrap().code,
        "ASTRA_FFMPEG_FEATURE_DISABLED"
    );
}

#[cfg(feature = "ffmpeg-vcpkg")]
#[astra_headless_test::test]
fn required_ffmpeg_gate_passes_only_after_native_probe() {
    let check = required_ffmpeg_check();
    assert_eq!(check.status, CheckStatus::Pass);
    assert!(check
        .evidence
        .iter()
        .any(|item| item.key == "provider_id" && item.value == "astra.decode.ffmpeg"));
}

#[astra_headless_test::test]
fn release_gate_accepts_player_full_playable_only_with_matching_live_report() {
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

#[astra_headless_test::test]
fn release_gate_requires_capability_conformance_player_identity_continuity() {
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

#[astra_headless_test::test]
fn release_gate_accepts_only_measured_performance_from_the_same_clean_product_run() {
    let blob = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.performance",
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
    let session_id = "session.windows.performance";
    let conformance = PlatformHostConformanceReport {
        schema: PLATFORM_HOST_CONFORMANCE_REPORT_SCHEMA.to_string(),
        status: ConformanceStatus::Pass,
        platform: PlatformId::Windows,
        target: capability.target.clone(),
        profile_hash: capability.profile_hash.clone(),
        package_hash: package_hash.clone(),
        build_fingerprint: capability.build_fingerprint.clone(),
        session_id: session_id.to_string(),
        checks: astra_platform::required_conformance_checks(PlatformId::Windows)
            .iter()
            .map(|id| ConformanceCheck::pass(*id, [("hash", &package_hash)]))
            .collect(),
        diagnostics: Vec::new(),
    };
    let mut player = player_report(&package_hash, "classic", "test-game");
    player.checks[0].evidence.extend([
        PlayerAutomationEvidence {
            key: "profile_hash".into(),
            value: capability.profile_hash.clone(),
        },
        PlayerAutomationEvidence {
            key: "build_fingerprint".into(),
            value: capability.build_fingerprint.clone(),
        },
        PlayerAutomationEvidence {
            key: "session_id".into(),
            value: session_id.into(),
        },
    ]);
    let budget = PerformanceBudget {
        schema: PERFORMANCE_BUDGET_SCHEMA.into(),
        budget_id: "windows.product.performance.v1".into(),
        target: capability.target.clone(),
        profile: "classic".into(),
        profile_hash: capability.profile_hash.clone(),
        min_run_duration_us: 1,
        metrics: vec![PerformanceMetricBudget {
            id: "frame.total_us".into(),
            unit: PerformanceUnit::Microseconds,
            min_samples: 1,
            max_samples: 2,
            thresholds: PerformanceThresholds {
                min_p50: None,
                min_p95: None,
                max_p50: Some(20_000),
                max_p95: Some(20_000),
                max_p99: Some(20_000),
                max: Some(20_000),
            },
        }],
    };
    let identity = PerformanceRunIdentity {
        source_revision: "0123456789abcdef0123456789abcdef01234567".into(),
        dirty: false,
        target: capability.target.clone(),
        profile: "classic".into(),
        profile_hash: capability.profile_hash.clone(),
        package_hash: package_hash.clone(),
        build_fingerprint: capability.build_fingerprint.clone(),
        session_id: session_id.into(),
    };
    let mut recorder = PerformanceRecorder::new(budget.clone()).unwrap();
    recorder.record("frame.total_us", 16_000).unwrap();
    let performance = recorder.finalize(identity, 1).unwrap();
    let presentation = PlayerPresentationReport {
        schema: PLAYER_PRESENTATION_REPORT_SCHEMA.into(),
        status: PlayerAutomationStatus::Pass,
        target: capability.target.clone(),
        profile: "classic".into(),
        platform: PlayerPlatform::Windows,
        package_hash: package_hash.clone(),
        profile_hash: capability.profile_hash.clone(),
        build_fingerprint: capability.build_fingerprint.clone(),
        session_id: session_id.into(),
        renderer_provider: "wgpu_hardware".into(),
        presentation_path: "glyph_atlas".into(),
        font_provider_hash: Hash256::from_sha256(b"font-provider").to_string(),
        layout_hash: Hash256::from_sha256(b"layout").to_string(),
        command_hash: Hash256::from_sha256(b"commands").to_string(),
        capture_hash: Hash256::from_sha256(b"capture").to_string(),
        sequence: 1,
        width: 1280,
        height: 720,
        changed_pixels: 4096,
        diagnostics: Vec::new(),
    };
    let mut request = package_request(package_bytes.clone());
    request.platform_report = Some(capability.clone());
    request.require_platform_report = true;
    let report = ReleaseValidator
        .validate_package_with_product_evidence(
            request,
            Some(conformance.clone()),
            Some(player.clone()),
            Some(presentation.clone()),
            vec![ProductPerformanceEvidence {
                budget: budget.clone(),
                report: performance.clone(),
            }],
        )
        .unwrap();
    assert!(
        report.checks.iter().any(|check| {
            check.id == "performance.product_run" && check.status == CheckStatus::Pass
        }),
        "unexpected performance checks: {:?}",
        report
            .checks
            .iter()
            .filter(|check| check.id == "performance.product_run")
            .collect::<Vec<_>>()
    );
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "player.presentation" && check.status == CheckStatus::Pass));

    let mut request = package_request(package_bytes.clone());
    request.platform_report = Some(capability.clone());
    request.require_platform_report = true;
    let missing = ReleaseValidator
        .validate_package_with_product_evidence(
            request,
            Some(conformance.clone()),
            Some(player.clone()),
            None,
            vec![ProductPerformanceEvidence {
                budget: budget.clone(),
                report: performance.clone(),
            }],
        )
        .unwrap();
    assert!(missing.checks.iter().any(|check| {
        check.id == "player.presentation" && check.status == CheckStatus::Blocked
    }));

    let mut dirty = performance.clone();
    dirty.identity.dirty = true;
    let mut request = package_request(package_bytes.clone());
    request.platform_report = Some(capability.clone());
    request.require_platform_report = true;
    let blocked = ReleaseValidator
        .validate_package_with_product_evidence(
            request,
            Some(conformance.clone()),
            Some(player.clone()),
            Some(presentation.clone()),
            vec![ProductPerformanceEvidence {
                budget: budget.clone(),
                report: dirty,
            }],
        )
        .unwrap();
    let check = blocked
        .checks
        .iter()
        .find(|check| check.id == "performance.product_run")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PERFORMANCE_EVIDENCE_IDENTITY"
    );

    let mut presentation_drift = presentation;
    presentation_drift.session_id = "session.other".into();
    let mut request = package_request(package_bytes);
    request.platform_report = Some(capability);
    request.require_platform_report = true;
    let blocked = ReleaseValidator
        .validate_package_with_product_evidence(
            request,
            Some(conformance),
            Some(player),
            Some(presentation_drift),
            vec![ProductPerformanceEvidence {
                budget,
                report: performance,
            }],
        )
        .unwrap();
    let check = blocked
        .checks
        .iter()
        .find(|check| check.id == "player.presentation")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_PLAYER_PRESENTATION_EVIDENCE"
    );
}

#[astra_headless_test::test]
fn release_gate_blocks_plugin_registry_conflict_and_invalid_binding() {
    let mut request = PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    let mut registry: astra_plugin_abi::PluginExtensionRegistrySnapshot =
        serde_json::from_slice(&request.plugin_extension_registry).unwrap();
    registry
        .conflicts
        .push(astra_plugin_abi::ExtensionConflict {
            slot: "presentation".into(),
            selected_provider: "astra.fixture.headless_presentation".into(),
            conflicting_provider: "astra.provider.second".into(),
            reason: "provider slot already has an explicit binding".into(),
        });
    request.plugin_extension_registry = serde_json::to_vec(&registry).unwrap();
    let error = PackageBuilder::build(request).unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_PLUGIN_EXTENSION_CONFLICT"));
}

#[astra_headless_test::test]
fn runtime_provider_gate_blocks_missing_nativevn_binding() {
    let mut request = PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    let mut policy: astra_plugin_abi::ProviderPolicy =
        serde_json::from_slice(&request.provider_policy).unwrap();
    let mut registry: astra_plugin_abi::PluginExtensionRegistrySnapshot =
        serde_json::from_slice(&request.plugin_extension_registry).unwrap();
    policy.bindings.clear();
    registry.bindings.clear();
    request.provider_policy = serde_json::to_vec(&policy).unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&registry).unwrap();
    let error = PackageBuilder::build(request).unwrap_err();
    assert!(error.to_string().contains("ASTRA_PLUGIN_BINDING_MISSING"));
}

#[astra_headless_test::test]
fn release_gate_blocks_unresolved_plugin_dependency() {
    let mut request = PackageBuildRequest::fixture(
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

#[astra_headless_test::test]
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

    let integrity = report
        .checks
        .iter()
        .find(|check| check.id == "package.integrity")
        .unwrap();
    assert_eq!(integrity.status, CheckStatus::Blocked);
    assert_eq!(
        integrity.diagnostic.as_ref().unwrap().code,
        "ASTRA_PACKAGE_INTEGRITY"
    );
}

#[astra_headless_test::test]
fn vfs_mount_gate_blocks_asset_registry_compat_section() {
    let mut request = PackageBuildRequest::fixture(
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

#[astra_headless_test::test]
fn vfs_mount_gate_blocks_missing_provider_binding_for_prefix() {
    let mut request = PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    let mut policy: astra_plugin_abi::ProviderPolicy =
        serde_json::from_slice(&request.provider_policy).unwrap();
    let mut registry: astra_plugin_abi::PluginExtensionRegistrySnapshot =
        serde_json::from_slice(&request.plugin_extension_registry).unwrap();
    policy
        .bindings
        .retain(|binding| binding.slot != "vfs_provider");
    registry
        .bindings
        .retain(|binding| binding.slot != "vfs_provider");
    registry
        .providers
        .retain(|provider| provider.slot != "vfs_provider");
    request.provider_policy = serde_json::to_vec(&policy).unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&registry).unwrap();
    let error = PackageBuilder::build(request).unwrap_err();
    assert!(error.to_string().contains("ASTRA_VFS_PROVIDER_MISSING"));
}

#[astra_headless_test::test]
fn plugin_provider_gate_blocks_unpacked_vfs_prefix_provider() {
    let mut request = PackageBuildRequest::fixture(
        "com.example.nativevn",
        "desktop-release",
        vec![SectionPayload::raw(
            "asset.characters.hero",
            "astra.cooked_asset.v1",
            b"hero".to_vec(),
        )],
    );
    let mut registry: astra_plugin_abi::PluginExtensionRegistrySnapshot =
        serde_json::from_slice(&request.plugin_extension_registry).unwrap();
    registry
        .providers
        .iter_mut()
        .find(|provider| provider.slot == "vfs_provider")
        .unwrap()
        .packaged = false;
    request.plugin_extension_registry = serde_json::to_vec(&registry).unwrap();
    let error = PackageBuilder::build(request).unwrap_err();
    assert!(error
        .to_string()
        .contains("ASTRA_PLUGIN_PACKAGED_INELIGIBLE"));
}

#[astra_headless_test::test]
fn release_profile_blocks_missing_platform_report() {
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

#[astra_headless_test::test]
fn dev_profile_warns_on_missing_platform_report() {
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

#[astra_headless_test::test]
fn release_profile_blocks_fixture_package_without_cooked_project() {
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

#[astra_headless_test::test]
fn release_profile_accepts_cooked_project_input_section() {
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

#[astra_headless_test::test]
fn release_profile_blocks_package_profile_mismatch() {
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

#[astra_headless_test::test]
fn release_report_blocks_windows_platform_report_without_required_provider() {
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

#[astra_headless_test::test]
fn release_report_includes_windows_platform_provider_evidence() {
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

#[astra_headless_test::test]
fn release_report_blocks_web_platform_report_without_required_provider() {
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

#[astra_headless_test::test]
fn release_report_includes_web_platform_provider_evidence() {
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

#[astra_headless_test::test]
fn release_gate_blocks_package_target_manifests_with_editor_descriptors() {
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
        "schema": "astra.target_manifest.v2",
        "targets": [
            {
                "id": "native-smoke-game",
                "kind": "game",
                "crate": "astra-runtime",
                "runtime_provider": "native_vn",
                "ui_provider": "astra.ui.yakui",
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

#[astra_headless_test::test]
fn release_gate_requires_nativevn_sections_for_classic_profile() {
    let blob = package_with_target_manifest(
        "classic",
        serde_json::json!({
            "schema": "astra.target_manifest.v2",
            "targets": [{
                "id": "nativevn-game",
                "kind": "game",
                "crate": "astra-vn",
                "runtime_provider": "native_vn",
                "ui_provider": "astra.ui.yakui",
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
        .any(|check| check.id == "vn.compiled_project" && check.status == CheckStatus::Blocked));
}

#[astra_headless_test::test]
fn release_gate_accepts_nativevn_sections_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    append_valid_locale_sections(&mut sections);
    let blob = package_with_target_manifest(
        "classic",
        serde_json::json!({
            "schema": "astra.target_manifest.v2",
            "targets": [{
                "id": "nativevn-game",
                "kind": "game",
                "crate": "astra-vn",
                "runtime_provider": "native_vn",
                "ui_provider": "astra.ui.yakui",
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
        .any(|check| check.id == "vn.compiled_project" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "vn.profile_manifest" && check.status == CheckStatus::Pass));
    assert!(report
        .checks
        .iter()
        .any(|check| check.id == "player.locale_config" && check.status == CheckStatus::Pass));
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

#[astra_headless_test::test]
fn release_gate_blocks_missing_or_duplicate_nativevn_localization() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
    let missing =
        package_with_target_manifest("classic", nativevn_target_manifest(), sections.clone());
    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: missing.into_bytes(),
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
        .find(|check| check.id == "player.locale_config")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert!(check
        .diagnostic
        .as_ref()
        .unwrap()
        .message
        .contains("ASTRA_PLAYER_LOCALE_CONFIG_MISSING"));

    let mut duplicate = sections;
    duplicate.push(SectionPayload::raw(
        "vn.localization.en",
        VN_LOCALIZATION_TABLE_SCHEMA,
        br#"{"schema":"astra.vn.localization_table.v1","locale":"en","strings":{"line.one":"A","line.one":"B"}}"#.to_vec(),
    ));
    duplicate.push(SectionPayload::raw(
        "player.locale_config",
        PLAYER_LOCALE_CONFIG_SCHEMA,
        br#"{"schema":"astra.player_locale_config.v1","default_locale":"en","available_locales":["en"]}"#.to_vec(),
    ));
    let duplicate = package_with_target_manifest("classic", nativevn_target_manifest(), duplicate);
    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: duplicate.into_bytes(),
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
        .find(|check| check.id == "player.locale_config")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert!(check
        .diagnostic
        .as_ref()
        .unwrap()
        .message
        .contains("duplicate localization key"));
}

#[astra_headless_test::test]
fn release_gate_blocks_missing_policy_bundle_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_missing_policy_bundle_source_cache_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_missing_standard_command_manifest_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn compiler_blocks_unknown_standard_command_before_release_packaging() {
    let error = compile_astra_project([AstraSource::story(
        "main.astra",
        format!(
            "{}\nstate extra #@id state.extra\n  scene extra #@id scene.extra\n    warp asset:native-assets/effect/warp.json #@id command.warp\n",
            nativevn_story_with_system_pages()
        ),
    )], Default::default())
    .unwrap_err();

    assert_eq!(error.code(), "ASTRA_VN_COMMAND_UNBOUND");
}

#[astra_headless_test::test]
fn release_gate_blocks_missing_presentation_provider_manifest_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_missing_commercial_baseline_manifest_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_incomplete_commercial_baseline_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:hello speaker:narrator #@id line.hello
"#,
        )],
        Default::default(),
    )
    .unwrap();
    let sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_missing_vn_extension_bindings_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_compiled_story_without_command_manifest() {
    let mut compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    compiled.command_manifest.commands.clear();
    let sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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
        .find(|check| check.id == "vn.compiled_project")
        .unwrap();
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check.diagnostic.as_ref().unwrap().code,
        "ASTRA_VN_COMPILED_STORY_MANIFEST"
    );
}

#[astra_headless_test::test]
fn release_gate_blocks_missing_system_story_manifest_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    text key:hello speaker:narrator #@id line.hello
"#,
        )],
        Default::default(),
    )
    .unwrap();
    let sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_system_story_entries_without_policy() {
    let source = nativevn_story_with_system_pages().replace(" policy:astra.policy.standard", "");
    let compiled = compile_astra_project(
        [AstraSource::story("main.astra", source)],
        Default::default(),
    )
    .unwrap();
    let sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_blocks_missing_system_ui_profile_manifest() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let mut sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

#[astra_headless_test::test]
fn release_gate_accepts_system_story_manifest_for_classic_profile() {
    let compiled = compile_astra_project(
        [AstraSource::story(
            "main.astra",
            nativevn_story_with_system_pages(),
        )],
        Default::default(),
    )
    .unwrap();
    let sections =
        package_sections_for_project(&compiled, &["classic".to_string()], "nativevn-game").unwrap();
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

fn package_with_target_manifest(
    profile: &str,
    target_manifest: serde_json::Value,
    sections: Vec<SectionPayload>,
) -> astra_package::ContainerBlob {
    let mut request = PackageBuildRequest::fixture("com.example.nativevn", profile, vec![]);
    let has_compiled_story = sections
        .iter()
        .any(|section| section.id == "vn.compiled_project");
    for section in sections {
        match section.id.as_str() {
            "asset.vfs_manifest" => request.asset_vfs_manifest = section.payload,
            "asset.catalog" => request.asset_catalog = section.payload,
            "media.manifest" => request.media_manifest = section.payload,
            "provider.policy" => request.provider_policy = section.payload,
            "plugin.extension_registry" => request.plugin_extension_registry = section.payload,
            "plugin.dependency_graph" => request.plugin_dependency_graph = section.payload,
            "module.fingerprint" => request.module_fingerprint = section.payload,
            "release.summary" => request.release_summary = section.payload,
            "scenario.refs" => request.scenario_refs = section.payload,
            "platform.eligibility" => request.platform_eligibility = section.payload,
            _ if section.id.starts_with("asset.") => request.cooked_assets.push(section),
            _ => request.extra_sections.push(section),
        }
    }
    let target_id = target_manifest["targets"]
        .as_array()
        .and_then(|targets| targets.iter().find(|target| target["kind"] == "game"))
        .and_then(|target| target["id"].as_str())
        .expect("test target manifest must contain a game target");
    bind_request_to_target(&mut request, target_id, profile, has_compiled_story);
    request.target_manifest = target_manifest.to_string().into_bytes();
    PackageBuilder::build(request).unwrap()
}

fn bind_request_to_target(
    request: &mut PackageBuildRequest,
    target_id: &str,
    profile: &str,
    use_native_vn: bool,
) {
    let mut policy: astra_plugin_abi::ProviderPolicy =
        serde_json::from_slice(&request.provider_policy).unwrap();
    let mut registry: astra_plugin_abi::PluginExtensionRegistrySnapshot =
        serde_json::from_slice(&request.plugin_extension_registry).unwrap();
    let bindings = registry
        .bindings
        .iter()
        .map(|binding| {
            let mut context = binding.context.clone();
            context.target = target_id.to_string();
            context.profile = profile.to_string();
            astra_plugin_abi::ProviderBinding::new(
                binding.slot.clone(),
                binding.provider_id.clone(),
                context,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    policy.profile = profile.to_string();
    policy.bindings = bindings.clone();
    if use_native_vn {
        policy.runtime_provider = astra_vn::NativeVnRuntimeProvider::descriptor();
    }
    registry.bindings = bindings;
    request.provider_policy = serde_json::to_vec(&policy).unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&registry).unwrap();
}

fn nativevn_target_manifest() -> serde_json::Value {
    serde_json::json!({
        "schema": "astra.target_manifest.v2",
        "targets": [{
            "id": "nativevn-game",
            "kind": "game",
            "crate": "astra-vn",
            "runtime_provider": "native_vn",
            "ui_provider": "astra.ui.yakui",
            "default_profile": "classic",
            "platforms": ["windows", "web"],
            "packaged": true
        }]
    })
}

fn nativevn_story_with_system_pages() -> &'static str {
    r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    movie layer:video.opening asset:asset:/movie/op end:wait fence:movie.opening.end fallback:asset:/movie/op_fallback #@id movie.opening
    voice asset:asset:/voice/hero0001 sync:text #@id voice.opening
    text key:hello speaker:narrator voice:voice.hero.0001 #@id line.hello
    choice key:where #@id choice.where
      option key:choice.library -> library #@id choice.library
      option key:choice.rooftop -> rooftop #@id choice.rooftop
state library #@id state.library
  scene library #@id scene.library
    bgm asset:asset:/bgm/library loop:true #@id bgm.library
    se asset:asset:/se/page #@id se.page
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
        target: Some("test-game".to_string()),
        require_platform_report: false,
        platform_report: None,
    }
}

fn append_valid_locale_sections(sections: &mut Vec<SectionPayload>) {
    sections.push(SectionPayload::raw(
        "vn.localization.en",
        VN_LOCALIZATION_TABLE_SCHEMA,
        br#"{"schema":"astra.vn.localization_table.v1","locale":"en","strings":{"line.one":"Hello"}}"#.to_vec(),
    ));
    sections.push(SectionPayload::raw(
        "player.locale_config",
        PLAYER_LOCALE_CONFIG_SCHEMA,
        br#"{"schema":"astra.player_locale_config.v1","default_locale":"en","available_locales":["en"]}"#.to_vec(),
    ));
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
