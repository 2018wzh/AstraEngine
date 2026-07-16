use astra_platform::{PlatformHostProfile, PlatformId};
use astra_platform_android::{
    validate_interpreter_only_features, validate_selected_audio_backend, AndroidAudioBackend,
    AndroidLifecycle, AndroidLifecycleEvent, AndroidLifecycleState,
};

#[test]
fn factory_is_android_and_probe_requires_runtime_evidence() {
    let report = astra_platform_android::probe(Some("nativevn-game"));
    assert_eq!(report.platform, PlatformId::Android);
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "ASTRA_ANDROID_RUNTIME_PROBE_REQUIRED"));
    let _factory = astra_platform_android::factory();
}

#[test]
fn lifecycle_rejects_duplicate_window_and_invalid_transition() {
    let mut lifecycle = AndroidLifecycle::default();
    lifecycle.transition(AndroidLifecycleEvent::Start).unwrap();
    lifecycle.transition(AndroidLifecycleEvent::Resume).unwrap();
    lifecycle.set_native_window_available(true);
    lifecycle.create_main_window().unwrap();
    assert!(lifecycle.create_main_window().is_err());
    assert!(lifecycle.transition(AndroidLifecycleEvent::Start).is_err());
    assert_eq!(lifecycle.state(), AndroidLifecycleState::Resumed);
}

#[test]
fn audio_backend_and_luau_policy_are_fail_closed() {
    let profile = PlatformHostProfile::android_release("nativevn-game", "com.astra.game");
    validate_selected_audio_backend(&profile, AndroidAudioBackend::AAudio).unwrap();
    validate_selected_audio_backend(&profile, AndroidAudioBackend::OpenSlEs).unwrap();
    validate_interpreter_only_features(["lua54", "vendored"]).unwrap();
    assert!(validate_interpreter_only_features(["luau_jit"]).is_err());

    let mut invalid = profile;
    invalid.audio.providers = vec!["oboe_opensl_es".to_string()];
    assert!(validate_selected_audio_backend(&invalid, AndroidAudioBackend::OpenSlEs).is_err());
}
