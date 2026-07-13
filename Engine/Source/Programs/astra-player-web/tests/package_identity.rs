use astra_package::{PackageBuildRequest, PackageBuilder, SectionPayload};
use astra_platform::PlatformHostProfile;
use astra_player_web::{
    validate_package, WebPlayerConfig, WebPlayerLiveEvidence, WEB_PLAYER_LIVE_EVIDENCE_SCHEMA,
};

#[test]
fn web_live_evidence_roundtrips_stable_runtime_identity_without_payload() {
    let evidence = WebPlayerLiveEvidence {
        schema: WEB_PLAYER_LIVE_EVIDENCE_SCHEMA.to_string(),
        event: "runtime.input_consumed".to_string(),
        target: "nativevn-game".to_string(),
        profile: "classic".to_string(),
        package_hash: format!("sha256:{}", "1".repeat(64)),
        package_byte_size: None,
        provider_id: Some("astra.runtime.native_vn".to_string()),
        session_id: Some("session.web.1".to_string()),
        player_sequence: Some(7),
        input_kind: Some("pointer".to_string()),
        fixed_step: Some(3),
        runtime_state_hash: Some(format!("sha256:{}", "2".repeat(64))),
        runtime_event_hash: Some(format!("sha256:{}", "3".repeat(64))),
        runtime_presentation_hash: Some(format!("sha256:{}", "4".repeat(64))),
        coverage_reached: vec!["route.library".to_string()],
        current_state_id: Some("route.library".to_string()),
        terminal_route_ids: vec!["route.library".to_string()],
        pending_choice_ids: vec!["choice.library".to_string()],
        audio_meter: None,
    };
    let encoded = serde_json::to_vec(&evidence).unwrap();
    let decoded: WebPlayerLiveEvidence = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, evidence);
    let text = String::from_utf8(encoded).unwrap();
    assert!(!text.contains("D:\\"));
    assert!(!text.contains("payload"));
}

#[test]
fn web_player_binds_profile_to_validated_package() {
    let package = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.game",
        "web-release",
        vec![platform_profiles("game-web", "com.example.game")],
    ))
    .unwrap();
    let config = WebPlayerConfig {
        schema: "astra.player_config.v2".to_string(),
        target: "game-web".to_string(),
        profile: "web-release".to_string(),
        platform: "web".to_string(),
        locale: "en".to_string(),
        package: "package/game.astrapkg".to_string(),
    };
    let profile = validate_package(&config, package.as_bytes()).unwrap();
    assert_eq!(profile.package_id, "com.example.game");
    assert_eq!(profile.target, "game-web");
}

#[test]
fn web_player_rejects_profile_mismatch_and_corrupt_package() {
    let package = PackageBuilder::build(PackageBuildRequest::fixture(
        "com.example.game",
        "classic",
        vec![platform_profiles("game-web", "com.example.game")],
    ))
    .unwrap();
    let config = WebPlayerConfig {
        schema: "astra.player_config.v2".to_string(),
        target: "game-web".to_string(),
        profile: "web-release".to_string(),
        platform: "web".to_string(),
        locale: "en".to_string(),
        package: "package/game.astrapkg".to_string(),
    };
    assert!(validate_package(&config, package.as_bytes()).is_err());
    assert!(validate_package(&config, b"not-a-package").is_err());
}

fn platform_profiles(target: &str, package_id: &str) -> SectionPayload {
    SectionPayload::raw(
        "platform.profiles",
        "astra.platform_profiles.v1",
        serde_json::to_vec(&serde_json::json!({
            "schema": "astra.platform_profiles.v1",
            "profiles": [PlatformHostProfile::web_release(target, package_id)]
        }))
        .unwrap(),
    )
}
