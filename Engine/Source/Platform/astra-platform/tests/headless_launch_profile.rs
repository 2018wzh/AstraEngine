use astra_platform::{
    validate_headless_host_profile, validate_headless_performance_profile, GpuAdapterPolicy,
    GpuBackendPolicy, GpuDeviceTypePolicy, HeadlessHostProfile, HeadlessReadbackPolicy,
    HeadlessRenderPolicy, HostKind, HostLaunchProfile, PlatformErrorCode, PlatformHostFactory,
    PlatformHostProfile, PlatformId, UnavailablePlatformFactory, HEADLESS_HOST_PROFILE_SCHEMA,
    USER_INPUT_SEQUENCE_SCHEMA,
};

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[test]
fn performance_profile_requires_v3_hardware_policy_and_timestamp_queries() {
    let mut profile =
        HeadlessHostProfile::reference("nativevn-game", "com.example.game", hash('a'), hash('b'));
    profile.providers.renderer = "wgpu_offscreen".into();
    profile.render_policy = HeadlessRenderPolicy::All;
    profile.readback_policy = HeadlessReadbackPolicy::CheckpointsOnly;
    profile.gpu_adapter = Some(GpuAdapterPolicy {
        backend: GpuBackendPolicy::Dx12,
        device_type: GpuDeviceTypePolicy::Integrated,
        require_timestamp_query: true,
        adapter_identity_hash: Some(hash('c')),
    });
    validate_headless_performance_profile(&profile).unwrap();

    profile
        .gpu_adapter
        .as_mut()
        .unwrap()
        .require_timestamp_query = false;
    assert!(validate_headless_performance_profile(&profile).is_err());
    profile
        .gpu_adapter
        .as_mut()
        .unwrap()
        .require_timestamp_query = true;
    profile.schema = "astra.headless_host_profile.v2".into();
    assert!(validate_headless_performance_profile(&profile).is_err());
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
    let mutations: [fn(&mut HeadlessHostProfile); 8] = [
        |profile| profile.package_hash = "sha256:short".to_string(),
        |profile| profile.input.max_messages = 0,
        |profile| profile.input.transports.clear(),
        |profile| profile.artifacts.max_total_bytes = 0,
        |profile| profile.artifacts.required_checkpoints = vec!["same".into(), "same".into()],
        |profile| profile.providers.renderer.clear(),
        |profile| profile.max_decode_output_bytes = 0,
        |profile| profile.max_video_frames = 0,
    ];
    for mutate in mutations {
        let mut profile = valid.clone();
        mutate(&mut profile);
        let error = validate_headless_host_profile(&profile).unwrap_err();
        assert_eq!(error.code, PlatformErrorCode::InvalidProfile);
        assert_eq!(error.operation, "headless.profile.validate");
    }
}

#[test]
fn legacy_v1_provider_shape_is_not_deserialized() {
    let json = serde_json::json!({
        "schema": "astra.headless_host_profile.v1",
        "id": "legacy",
        "target": "nativevn-game",
        "package_id": "com.example.game",
        "build_fingerprint": hash('a'),
        "package_hash": hash('b'),
        "providers": { "renderer": "cpu_reference", "text": "cosmic_text_cpu", "audio": "old", "decode": "old", "save": "old", "package": "old" },
        "input": { "schema": "astra.headless_input_policy.v1", "protocol_schema": "astra.user_input_sequence.v1", "max_messages": 1, "max_tick": 1, "allow_file": true, "allow_stdio": true, "allow_realtime": false },
        "artifacts": { "namespace": "legacy", "retention": "all", "max_artifacts": 1, "max_total_bytes": 1, "max_frames": 1, "max_audio_frames": 1, "max_duration_ns": 1, "required_checkpoints": [] },
        "package_sources": ["bundled"],
        "limits": { "command_queue_capacity": 1, "event_queue_capacity": 1, "max_frame_bytes": 1, "max_audio_frames": 1, "max_package_read_bytes": 1 }
    });
    assert!(serde_json::from_value::<HeadlessHostProfile>(json).is_err());
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
