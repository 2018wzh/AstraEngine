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
    assert!(report.checks.iter().any(|check| {
        check.id == "plugin.extension_registry" && check.status == CheckStatus::Pass
    }));
    assert!(report.checks.iter().any(|check| {
        check.id == "plugin.dependency_graph" && check.status == CheckStatus::Pass
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
fn release_report_blocks_web_platform_report_without_required_smoke() {
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
            platform_report: Some(PlatformCapabilityReport::new(
                PlatformId::Web,
                Some("nativevn-web".to_string()),
                SdkStatus::Present,
                vec!["webgpu".to_string(), "webgl_fallback".to_string()],
                vec!["webcodecs".to_string()],
                vec!["webaudio".to_string()],
                vec!["opfs".to_string(), "indexeddb".to_string()],
                vec!["keyboard".to_string(), "touch".to_string()],
                vec!["browser_launch".to_string()],
                vec!["browser_sandbox".to_string()],
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
fn release_report_includes_web_platform_smoke_evidence() {
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
    let platform_report = PlatformCapabilityReport::new(
        PlatformId::Web,
        Some("nativevn-web".to_string()),
        SdkStatus::Present,
        vec!["webgpu".to_string(), "webgl_fallback".to_string()],
        vec!["webcodecs".to_string()],
        vec!["webaudio".to_string()],
        vec![
            "opfs".to_string(),
            "indexeddb".to_string(),
            "file_api".to_string(),
            "http_range".to_string(),
        ],
        vec![
            "keyboard".to_string(),
            "mouse".to_string(),
            "touch".to_string(),
        ],
        vec![
            "browser_launch".to_string(),
            "visibility_resume".to_string(),
            "worker".to_string(),
        ],
        vec!["browser_sandbox".to_string()],
    )
    .with_smoke(vec![
        smoke("browser_smoke", PlatformSmokeStatus::Pass),
        smoke("renderer.webgpu_or_webgl", PlatformSmokeStatus::Pass),
        smoke("decode.webcodecs", PlatformSmokeStatus::Pass),
        smoke("audio.webaudio_unlock", PlatformSmokeStatus::Pass),
        smoke("save.web_storage", PlatformSmokeStatus::Pass),
        smoke("package.web_source", PlatformSmokeStatus::Pass),
        smoke("input.browser", PlatformSmokeStatus::Pass),
        smoke("lifecycle.worker_visibility", PlatformSmokeStatus::Pass),
    ]);

    let report = ReleaseValidator
        .validate_package(PackageValidateRequest {
            package_bytes: blob.into_bytes(),
            profile: "web-release".to_string(),
            require_ffmpeg: false,
            target: Some("nativevn-web".to_string()),
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
        .any(|entry| entry.key == "smoke.decode.webcodecs.status" && entry.value == "pass"));
    assert!(platform_check
        .evidence
        .iter()
        .any(|entry| entry.key == "smoke.package.web_source.status" && entry.value == "pass"));
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
