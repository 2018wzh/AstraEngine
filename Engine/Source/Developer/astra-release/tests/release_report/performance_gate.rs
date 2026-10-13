use super::*;

#[test]
pub(super) fn release_gate_accepts_only_measured_performance_from_the_same_clean_product_run() {
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

#[test]
pub(super) fn release_gate_blocks_plugin_registry_conflict_and_invalid_binding() {
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

#[test]
pub(super) fn package_gate_blocks_missing_mechanism_bindings() {
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

#[test]
pub(super) fn release_gate_blocks_unresolved_plugin_dependency() {
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

#[test]
pub(super) fn vfs_mount_gate_blocks_missing_vfs_manifest() {
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

#[test]
pub(super) fn vfs_mount_gate_blocks_asset_registry_compat_section() {
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

#[test]
pub(super) fn vfs_mount_gate_blocks_missing_provider_binding_for_prefix() {
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

#[test]
pub(super) fn plugin_provider_gate_blocks_unpacked_vfs_prefix_provider() {
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
