use astra_platform::{
    validate_host_profile, AudioOutputHandle, DecodeSessionHandle, MediaFrameHandle,
    PackageSourceHandle, PlatformCapabilityReport, PlatformErrorCode, PlatformHostProfile,
    PlatformId, SaveTransactionHandle, SurfaceHandle, WindowHandle,
    PLATFORM_CAPABILITY_REPORT_SCHEMA, PLATFORM_HOST_PROFILE_SCHEMA,
};

#[test]
fn typed_handles_preserve_slot_and_generation_without_provider_strings() {
    let window = WindowHandle::from_parts(7, 3).expect("valid window handle");
    let surface = SurfaceHandle::from_parts(8, 4).expect("valid surface handle");
    let audio = AudioOutputHandle::from_parts(9, 5).expect("valid audio handle");
    let decode = DecodeSessionHandle::from_parts(10, 6).expect("valid decode handle");
    let media = MediaFrameHandle::from_parts(11, 7).expect("valid media handle");
    let save = SaveTransactionHandle::from_parts(12, 8).expect("valid save handle");
    let package = PackageSourceHandle::from_parts(13, 9).expect("valid package handle");

    assert_eq!(window.parts(), (7, 3));
    assert_eq!(surface.parts(), (8, 4));
    assert_eq!(audio.parts(), (9, 5));
    assert_eq!(decode.parts(), (10, 6));
    assert_eq!(media.parts(), (11, 7));
    assert_eq!(save.parts(), (12, 8));
    assert_eq!(package.parts(), (13, 9));
    assert_eq!(
        WindowHandle::from_parts(0, 1).unwrap_err().code,
        PlatformErrorCode::InvalidHandle
    );
}

#[test]
fn release_profiles_lock_selected_providers_without_hidden_fallbacks() {
    let windows = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    assert_eq!(windows.schema, PLATFORM_HOST_PROFILE_SCHEMA);
    assert_eq!(windows.platform, PlatformId::Windows);
    assert_eq!(windows.renderer.providers, ["wgpu_hardware"]);
    assert_eq!(windows.decode.providers, ["wmf"]);
    assert_eq!(windows.audio.providers, ["wasapi"]);
    assert_eq!(windows.save.providers, ["saved_games"]);
    assert!(!windows.renderer.allow_software);
    assert!(validate_host_profile(&windows).is_ok());

    let web = PlatformHostProfile::web_release("nativevn-web", "com.example.game");
    assert_eq!(web.platform, PlatformId::Web);
    assert_eq!(web.renderer.providers, ["webgpu"]);
    assert_eq!(web.decode.providers, ["webcodecs"]);
    assert_eq!(web.audio.providers, ["webaudio"]);
    assert_eq!(web.save.providers, ["opfs"]);
    assert!(validate_host_profile(&web).is_ok());
}

#[test]
fn capability_report_v2_separates_declared_available_and_selected() {
    let profile = PlatformHostProfile::windows_release("nativevn-game", "com.example.game");
    let report = PlatformCapabilityReport::from_profile(
        &profile,
        "sha256:build",
        ["wgpu_hardware", "wmf", "wasapi", "saved_games"],
    )
    .expect("capability report");

    assert_eq!(report.schema, PLATFORM_CAPABILITY_REPORT_SCHEMA);
    assert_eq!(report.renderer.declared, ["wgpu_hardware"]);
    assert_eq!(report.renderer.available, ["wgpu_hardware"]);
    assert_eq!(report.renderer.selected.as_deref(), Some("wgpu_hardware"));
    assert!(report.profile_hash.starts_with("sha256:"));
}
