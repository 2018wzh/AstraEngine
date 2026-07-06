use astra_platform::{PlatformCapabilityReport, PlatformId, ReportBackedPlatformHost, SdkStatus};

pub fn host(target: Option<&str>) -> ReportBackedPlatformHost {
    ReportBackedPlatformHost::new(probe(target))
}

pub fn probe(target: Option<&str>) -> PlatformCapabilityReport {
    PlatformCapabilityReport::new(
        PlatformId::Android,
        target.map(str::to_string),
        if cfg!(target_os = "android")
            || std::env::var_os("ANDROID_HOME").is_some()
            || std::env::var_os("ANDROID_SDK_ROOT").is_some()
        {
            SdkStatus::Present
        } else {
            SdkStatus::Missing
        },
        vec!["wgpu_vulkan".to_string()],
        vec!["mediacodec".to_string()],
        vec!["aaudio".to_string(), "opensl_es".to_string()],
        vec![
            "app_storage".to_string(),
            "storage_access_framework".to_string(),
        ],
        vec![
            "touch".to_string(),
            "safe_area".to_string(),
            "gamepad".to_string(),
        ],
        vec![
            "activity_resume".to_string(),
            "background_resume".to_string(),
            "rotation".to_string(),
        ],
        vec![
            "network_runtime_ai_profile_gated".to_string(),
            "luau_no_jit".to_string(),
        ],
    )
}
