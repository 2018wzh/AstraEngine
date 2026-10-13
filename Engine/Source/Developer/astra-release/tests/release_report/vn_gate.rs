use super::*;

#[test]
pub(super) fn release_gate_requires_nativevn_sections_for_classic_profile() {
    let blob = package_with_target_manifest(
        "classic",
        serde_json::json!({
            "schema": "astra.target_manifest.v3",
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
        .any(|check| check.id == "vn.compiled_project" && check.status == CheckStatus::Blocked));
}

#[test]
pub(super) fn package_rejects_legacy_gameplay_policy() {
    let mut request = PackageBuildRequest::fixture("com.example.nativevn", "classic", vec![]);
    let mut policy: serde_json::Value = serde_json::from_slice(&request.provider_policy).unwrap();
    policy["runtime_provider"] = serde_json::json!({"runtime_id": "native_vn"});
    request.provider_policy = serde_json::to_vec(&policy).unwrap();
    assert!(PackageBuilder::build(request).is_err());
}

#[test]
pub(super) fn release_gate_accepts_nativevn_sections_for_classic_profile() {
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
            "schema": "astra.target_manifest.v3",
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
        check.id == "vn.runtime"
            && check.status == CheckStatus::Pass
            && check
                .evidence
                .iter()
                .any(|evidence| evidence.key == "behavior_state_hash")
            && check
                .evidence
                .iter()
                .any(|evidence| evidence.key == "behavior_resumed_step" && evidence.value == "2")
    }));
}

#[test]
pub(super) fn release_gate_blocks_missing_or_duplicate_nativevn_localization() {
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

#[test]
pub(super) fn release_gate_blocks_missing_policy_bundle_for_classic_profile() {
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

#[test]
pub(super) fn release_gate_blocks_missing_policy_bundle_source_cache_for_classic_profile() {
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

#[test]
pub(super) fn release_gate_blocks_missing_standard_command_manifest_for_classic_profile() {
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

#[test]
pub(super) fn compiler_blocks_unknown_standard_command_before_release_packaging() {
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

#[test]
pub(super) fn release_gate_blocks_missing_presentation_provider_manifest_for_classic_profile() {
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

#[test]
pub(super) fn release_gate_blocks_missing_commercial_baseline_manifest_for_classic_profile() {
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

#[test]
pub(super) fn release_gate_blocks_incomplete_commercial_baseline_for_classic_profile() {
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
