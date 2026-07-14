use astra_platform::{
    validate_headless_host_profile, HeadlessHostProfile, HostKind, HostLaunchProfile,
    PlatformErrorCode, PlatformHostFactory, PlatformHostProfile, PlatformId,
    UnavailablePlatformFactory, HEADLESS_HOST_PROFILE_SCHEMA, USER_INPUT_SEQUENCE_SCHEMA,
};

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[test]
fn headless_profile_is_identity_bound_and_separate_from_platform_id() {
    let profile =
        HeadlessHostProfile::reference("nativevn-game", "com.example.game", hash('a'), hash('b'));
    assert_eq!(profile.schema, HEADLESS_HOST_PROFILE_SCHEMA);
    assert_eq!(profile.input.protocol_schema, USER_INPUT_SEQUENCE_SCHEMA);
    assert!(validate_headless_host_profile(&profile).is_ok());

    let launch = HostLaunchProfile::headless(profile.clone());
    assert_eq!(launch.kind(), HostKind::Headless);
    assert_eq!(launch.target(), "nativevn-game");
    assert_eq!(launch.package_id(), "com.example.game");
    assert_eq!(launch.require_headless().unwrap(), &profile);
    assert_eq!(
        launch.require_platform().unwrap_err().operation,
        "host.start"
    );
    assert_eq!(PlatformId::all().len(), 6);
}

#[test]
fn malformed_headless_identity_limits_and_transport_fail_closed() {
    let valid =
        HeadlessHostProfile::reference("nativevn-game", "com.example.game", hash('a'), hash('b'));
    let mutations: [fn(&mut HeadlessHostProfile); 6] = [
        |profile| profile.package_hash = "sha256:short".to_string(),
        |profile| profile.input.max_messages = 0,
        |profile| {
            profile.input.allow_file = false;
            profile.input.allow_stdio = false;
        },
        |profile| profile.artifacts.max_total_bytes = 0,
        |profile| profile.artifacts.required_checkpoints = vec!["same".into(), "same".into()],
        |profile| profile.providers.renderer.clear(),
    ];
    for mutate in mutations {
        let mut profile = valid.clone();
        mutate(&mut profile);
        let error = validate_headless_host_profile(&profile).unwrap_err();
        assert_eq!(error.code, PlatformErrorCode::InvalidProfile);
        assert_eq!(error.operation, "headless.profile.validate");
    }
}

#[tokio::test]
async fn native_factory_rejects_headless_before_platform_availability() {
    let headless =
        HeadlessHostProfile::reference("nativevn-game", "com.example.game", hash('a'), hash('b'));
    let error = UnavailablePlatformFactory::new(PlatformId::Linux)
        .start(HostLaunchProfile::headless(headless))
        .await
        .err()
        .expect("native factory must reject Headless launch profiles");
    assert_eq!(error.code, PlatformErrorCode::InvalidProfile);
    assert_eq!(error.operation, "host.start");

    let mut linux = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    linux.platform = PlatformId::Linux;
    linux.id = "linux-stage6".to_string();
    let launch = HostLaunchProfile::platform(linux);
    assert_eq!(launch.kind(), HostKind::Platform(PlatformId::Linux));
    assert!(launch.require_platform().is_ok());
    assert!(launch.require_headless().is_err());
}
