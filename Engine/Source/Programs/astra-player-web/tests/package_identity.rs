use astra_package::{PackageBuildRequest, PackageBuilder, SectionPayload};
use astra_platform::PlatformHostProfile;
use astra_player_web::{validate_package, WebPlayerConfig};

#[test]
fn web_player_binds_profile_to_validated_package() {
    let package = PackageBuilder::build(PackageBuildRequest::minimal(
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
        package: "package/game.astrapkg".to_string(),
    };
    let profile = validate_package(&config, package.as_bytes()).unwrap();
    assert_eq!(profile.package_id, "com.example.game");
    assert_eq!(profile.target, "game-web");
}

#[test]
fn web_player_rejects_profile_mismatch_and_corrupt_package() {
    let package = PackageBuilder::build(PackageBuildRequest::minimal(
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
