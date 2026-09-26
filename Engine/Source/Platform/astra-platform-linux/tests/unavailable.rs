#[test]
fn linux_probe_is_explicitly_unavailable_until_stage6() {
    let report = astra_platform_linux::probe(Some("nativevn-game"));
    assert!(report.renderer.available.is_empty());
    assert!(report.renderer.selected.is_none());
    #[cfg(target_os = "linux")]
    assert_eq!(
        report.diagnostics[0].code,
        "ASTRA_PLATFORM_RUNTIME_PROBE_REQUIRED"
    );
    #[cfg(not(target_os = "linux"))]
    assert_eq!(report.diagnostics[0].code, "ASTRA_PLATFORM_NOT_IMPLEMENTED");
    let _factory = astra_platform_linux::factory();
}

#[cfg(all(target_os = "linux", feature = "platform-test-driver"))]
#[tokio::test]
#[ignore = "requires an environment without a Wayland or X11 display"]
async fn missing_display_rejects_real_host_start() {
    use astra_platform::{HostLaunchProfile, PlatformHostFactory, PlatformHostProfile};
    assert!(std::env::var_os("DISPLAY").is_none());
    assert!(std::env::var_os("WAYLAND_DISPLAY").is_none());
    let root = tempfile::tempdir().unwrap();
    let factory = astra_platform_linux::factory_with_test_roots(root.path(), root.path());
    let profile = PlatformHostProfile::linux_steam_sniper_release(
        "nativevn-game",
        "com.astra.linux.missing-display-test",
    );
    let result = factory.start(HostLaunchProfile::platform(profile)).await;
    let error = match result {
        Ok(_) => panic!("a display-less environment cannot start a native host"),
        Err(error) => error,
    };
    assert_eq!(error.operation, "host.start");
    assert_eq!(error.message, "Linux event loop could not be created");
}
