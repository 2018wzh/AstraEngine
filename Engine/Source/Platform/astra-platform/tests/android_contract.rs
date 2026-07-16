use astra_platform::{
    required_conformance_checks, validate_host_profile, PackageSourcePolicy, PlatformHostProfile,
    PlatformId,
};

#[test]
fn android_release_profile_is_explicit_and_hardware_bound() {
    let profile = PlatformHostProfile::android_release("nativevn-game", "com.astra.nativevn");
    validate_host_profile(&profile).unwrap();
    assert_eq!(profile.platform, PlatformId::Android);
    assert_eq!(profile.renderer.providers, ["wgpu_vulkan"]);
    assert_eq!(profile.decode.providers, ["mediacodec"]);
    assert_eq!(profile.audio.providers, ["oboe_aaudio", "oboe_opensl_es"]);
    assert_eq!(profile.save.providers, ["android_app_storage"]);
    assert_eq!(
        profile.package_sources,
        [
            PackageSourcePolicy::Bundled,
            PackageSourcePolicy::UserAuthorized
        ]
    );
}

#[test]
fn android_conformance_requires_real_mobile_evidence() {
    let checks = required_conformance_checks(PlatformId::Android);
    for required in [
        "surface.vulkan_present_readback",
        "input.android_consumption",
        "accessibility.talkback_semantics",
        "audio.android_focus_meter",
        "decode.mediacodec_audio_video",
        "package.bundled_saf_hash_range",
        "host.resume_recreate",
        "resource.zero_leaks",
    ] {
        assert!(checks.contains(&required), "missing {required}");
    }
}
