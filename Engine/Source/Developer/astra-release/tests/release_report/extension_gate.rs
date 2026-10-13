use super::*;

#[test]
pub(super) fn release_gate_blocks_missing_vn_extension_bindings_for_classic_profile() {
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

#[test]
pub(super) fn release_gate_blocks_compiled_story_without_command_manifest() {
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

#[test]
pub(super) fn release_gate_blocks_missing_system_story_manifest_for_classic_profile() {
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

#[test]
pub(super) fn release_gate_blocks_system_story_entries_without_policy() {
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

#[test]
pub(super) fn release_gate_blocks_missing_system_ui_profile_manifest() {
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

#[test]
pub(super) fn release_gate_blocks_system_story_manifest_without_profile_policy() {
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
    assert_eq!(check.status, CheckStatus::Blocked);
    assert_eq!(
        check
            .diagnostic
            .as_ref()
            .map(|diagnostic| diagnostic.code.as_str()),
        Some("ASTRA_VN_SYSTEM_UI_PROFILE_POLICY_MISSING")
    );
}

pub(super) fn package_with_target_manifest(
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

pub(super) fn bind_request_to_target(
    request: &mut PackageBuildRequest,
    target_id: &str,
    profile: &str,
    _use_native_vn: bool,
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
    registry.bindings = bindings;
    request.provider_policy = serde_json::to_vec(&policy).unwrap();
    request.plugin_extension_registry = serde_json::to_vec(&registry).unwrap();
}

pub(super) fn nativevn_target_manifest() -> serde_json::Value {
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
    })
}

pub(super) fn nativevn_story_with_system_pages() -> &'static str {
    r#"
story main #@id story.main
state prologue #@id state.prologue
  scene room #@id scene.room
    movie layer:video.opening asset:asset:/movie/op end:wait fence:movie.opening.end fallback:asset:/movie/op_fallback interrupt:reject #@id movie.opening
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

pub(super) fn platform_capability(
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

pub(super) fn cooked_project_section(profile: &str, target: &str) -> SectionPayload {
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

pub(super) fn package_request(package_bytes: Vec<u8>) -> PackageValidateRequest {
    PackageValidateRequest {
        package_bytes,
        profile: "classic".to_string(),
        require_ffmpeg: false,
        target: Some("test-game".to_string()),
        require_platform_report: false,
        platform_report: None,
    }
}

pub(super) fn append_valid_locale_sections(sections: &mut Vec<SectionPayload>) {
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

pub(super) fn player_report(
    package_hash: &str,
    profile: &str,
    target: &str,
) -> PlayerAutomationReport {
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
